//! Offline, checksummed physical backup and isolated no-clobber restore.

use crate::database::{database_error, schema_version, APPLICATION_SCHEMA_VERSION};
use convex_ports::error::TraitError;
use redb::Database;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

const MAX_BACKUP_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MANIFEST_LIMIT: u64 = 8192;

/// Physical backup manifest. SHA-256 detects corruption; it does not authenticate a supplier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    /// Backup envelope version, independent of the database application schema.
    pub format_version: u64,
    /// Observed Convex schema; zero denotes a database predating schema markers.
    pub application_schema_version: u64,
    /// Supported physical backend file family.
    pub backend: String,
    /// Exact byte count in data.redb.
    pub bytes: u64,
    /// Lowercase SHA-256 of data.redb as backed up.
    pub sha256: String,
}

fn regular_file(path: &Path) -> Result<File, TraitError> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(TraitError::InvalidInput(
            "backup input must be a regular file",
        ));
    }
    Ok(File::open(path)?)
}

fn new_private_file(path: &Path) -> Result<File, TraitError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

fn sync_directory(path: &Path) -> Result<(), TraitError> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(TraitError::Unsupported(
            "offline recovery requires directory synchronization support",
        ))
    }
}

fn copy_with_hash(mut input: File, output: &mut File, expected: u64) -> Result<String, TraitError> {
    if expected == 0 || expected > MAX_BACKUP_BYTES {
        return Err(TraitError::CapacityExceeded);
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut copied = 0u64;
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        copied = copied
            .checked_add(count as u64)
            .ok_or(TraitError::CapacityExceeded)?;
        if copied > expected {
            return Err(TraitError::InvalidInput("backup byte count changed"));
        }
        hash.update(&buffer[..count]);
        output.write_all(&buffer[..count])?;
    }
    if copied != expected {
        return Err(TraitError::InvalidInput(
            "backup byte count differs from manifest",
        ));
    }
    output.sync_all()?;
    Ok(format!("{:x}", hash.finalize()))
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|v| !v.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn check_schema(version: u64) -> Result<(), TraitError> {
    if version > APPLICATION_SCHEMA_VERSION {
        return Err(TraitError::Unsupported("newer backup application schema"));
    }
    Ok(())
}

/// Back up a closed database into a new directory containing data.redb and manifest.json.
///
/// Opening the database takes redb's exclusive file lock; an active server makes this
/// fail. The lock is retained throughout copying. A manifest is published only after
/// the data file is synchronized. Incomplete directories are rejected during restore.
/// This intentionally supports Unix local filesystems, not online/shared backups.
pub fn backup_database(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<BackupManifest, TraitError> {
    let source = source.as_ref();
    let destination = destination.as_ref();
    let input = regular_file(source)?;
    let initial_bytes = input.metadata()?.len();
    if initial_bytes == 0 || initial_bytes > MAX_BACKUP_BYTES {
        return Err(TraitError::CapacityExceeded);
    }
    let mut db = Database::open(source).map_err(database_error)?;
    if !db.check_integrity().map_err(database_error)? {
        return Err(TraitError::Unsupported(
            "database required repair; reconcile before backup",
        ));
    }
    let version = schema_version(&db)?;
    check_schema(version)?;
    sync_directory(parent(destination))?;
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(destination)?;
    let mut output = new_private_file(&destination.join("data.redb"))?;
    let bytes = input.metadata()?.len();
    let sha256 = copy_with_hash(input, &mut output, bytes)?;
    let manifest = BackupManifest {
        format_version: 1,
        application_schema_version: version,
        backend: "redb-4".into(),
        bytes,
        sha256,
    };
    let mut file = new_private_file(&destination.join("manifest.json"))?;
    serde_json::to_writer_pretty(&mut file, &manifest)
        .map_err(|e| TraitError::SerializationError(e.to_string()))?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    sync_directory(destination)?;
    sync_directory(parent(destination))?;
    drop(db);
    Ok(manifest)
}

/// Verify a backup and atomically publish its database to a new destination file.
///
/// Copies into an isolated temporary directory, checks byte count, SHA-256, redb
/// integrity and application schema, then uses a same-filesystem hard link that
/// cannot overwrite an existing destination. A failure after publication is
/// UnknownOutcome; inspect the destination before retrying. Migration is a separate
/// operation. The original backup is never opened by redb or modified.
pub fn restore_database(
    backup: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<BackupManifest, TraitError> {
    let backup = backup.as_ref();
    let destination = destination.as_ref();
    let mut bytes = Vec::new();
    regular_file(&backup.join("manifest.json"))?
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(TraitError::CapacityExceeded);
    }
    let manifest: BackupManifest = serde_json::from_slice(&bytes)
        .map_err(|_| TraitError::InvalidInput("invalid backup manifest"))?;
    if manifest.format_version != 1 || manifest.backend != "redb-4" {
        return Err(TraitError::Unsupported("backup format"));
    }
    check_schema(manifest.application_schema_version)?;
    if manifest.sha256.len() != 64
        || !manifest
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(TraitError::InvalidInput("invalid backup checksum"));
    }
    if destination.try_exists()? {
        return Err(TraitError::Conflict("restore destination already exists"));
    }
    sync_directory(parent(destination))?;
    let staging = tempfile::Builder::new()
        .prefix(".convex-restore-")
        .tempdir_in(parent(destination))?;
    let data_path = staging.path().join("data.redb");
    let mut output = new_private_file(&data_path)?;
    let hash = copy_with_hash(
        regular_file(&backup.join("data.redb"))?,
        &mut output,
        manifest.bytes,
    )?;
    if hash != manifest.sha256 {
        return Err(TraitError::InvalidInput("backup checksum mismatch"));
    }
    drop(output);
    let mut db = Database::open(&data_path).map_err(database_error)?;
    if !db.check_integrity().map_err(database_error)? {
        return Err(TraitError::InvalidInput(
            "restored database required repair",
        ));
    }
    if schema_version(&db)? != manifest.application_schema_version {
        return Err(TraitError::InvalidInput(
            "backup schema differs from manifest",
        ));
    }
    drop(db);
    File::open(&data_path)?.sync_all()?;
    fs::hard_link(&data_path, destination).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            TraitError::Conflict("restore destination already exists")
        } else {
            TraitError::from(e)
        }
    })?;
    sync_directory(parent(destination)).map_err(|_| TraitError::UnknownOutcome)?;
    staging.close().map_err(|_| TraitError::UnknownOutcome)?;
    Ok(manifest)
}
