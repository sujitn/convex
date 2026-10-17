//! Local immutable content-addressed snapshots. Administrative I/O, not a certified hot path.
#![forbid(unsafe_code)]

use async_trait::async_trait;
use convex_core::contracts::SnapshotId;
use convex_ports::{
    snapshot::{MarketSnapshot, SnapshotRepository, MAX_SNAPSHOT_BYTES},
    TraitError,
};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Single-host filesystem adapter with atomic no-clobber publication and verified exact reads.
/// Unix durability acknowledges file and directory sync. Shared/network filesystems are uncertified.
pub struct FileSnapshotRepository {
    directory: PathBuf,
}
impl FileSnapshotRepository {
    fn read_core(
        &self,
        id: &SnapshotId,
    ) -> Result<Option<convex_ports::core_workflow::CoreSnapshot>, TraitError> {
        let path = self.path(id)?.with_extension("core.json");
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_error(e)),
            Ok(_) => {}
        }
        let bytes = read_regular_bounded(&path, MAX_SNAPSHOT_BYTES)?;
        let snapshot: convex_ports::core_workflow::CoreSnapshot = serde_json::from_slice(&bytes)
            .map_err(|_| TraitError::InvalidInput("corrupt core snapshot"))?;
        snapshot.verify()?;
        if snapshot.id != *id
            || serde_json::to_vec(&snapshot)
                .map_err(|_| TraitError::InvalidInput("core snapshot JSON"))?
                != bytes
        {
            return Err(TraitError::Conflict(
                "core snapshot identity/canonical content mismatch",
            ));
        }
        Ok(Some(snapshot))
    }
    /// Create/open an explicit directory. Reject a symlink/non-directory root.
    pub fn new(directory: impl AsRef<Path>) -> Result<Self, TraitError> {
        if !cfg!(unix) {
            return Err(TraitError::Unsupported(
                "snapshot directory sync is certified only for local Unix filesystems",
            ));
        }
        let created = match std::fs::create_dir(directory.as_ref()) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(io_error(error)),
        };
        if created {
            let parent = directory
                .as_ref()
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            std::fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(io_error)?;
        }
        let metadata = std::fs::symlink_metadata(directory.as_ref()).map_err(io_error)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(TraitError::InvalidInput(
                "snapshot root must be a real directory",
            ));
        }
        let directory = directory.as_ref().canonicalize().map_err(io_error)?;
        Ok(Self { directory })
    }
    fn path(&self, id: &SnapshotId) -> Result<PathBuf, TraitError> {
        let digest = id
            .as_str()
            .strip_prefix("sha256:")
            .ok_or(TraitError::InvalidInput("unsupported snapshot identity"))?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(TraitError::InvalidInput("invalid snapshot content digest"));
        }
        Ok(self.directory.join(format!("{digest}.json")))
    }
    fn read(&self, id: &SnapshotId) -> Result<Option<MarketSnapshot>, TraitError> {
        let path = self.path(id)?;
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(error)),
            Ok(_) => {}
        }
        let bytes = read_regular_bounded(&path, MAX_SNAPSHOT_BYTES)?;
        let snapshot: MarketSnapshot = serde_json::from_slice(&bytes)
            .map_err(|_| TraitError::InvalidInput("invalid/corrupt immutable snapshot"))?;
        if snapshot.id() != id || snapshot.canonical_bytes()? != bytes {
            return Err(TraitError::Conflict(
                "snapshot file identity/canonical content mismatch",
            ));
        }
        Ok(Some(snapshot))
    }
    fn write(&self, snapshot: &MarketSnapshot) -> Result<(), TraitError> {
        let bytes = snapshot.canonical_bytes()?;
        let path = self.path(snapshot.id())?;
        let mut staging = tempfile::NamedTempFile::new_in(&self.directory).map_err(io_error)?;
        staging.write_all(&bytes).map_err(io_error)?;
        staging.as_file().sync_all().map_err(io_error)?;
        // A same-directory hard link publishes complete synced bytes without replacing an existing ID.
        match std::fs::hard_link(staging.path(), &path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let retained = self
                    .read(snapshot.id())?
                    .ok_or(TraitError::UnknownOutcome)?;
                if retained != *snapshot {
                    return Err(TraitError::Conflict("immutable snapshot identity conflict"));
                }
            }
            Err(error) => return Err(io_error(error)),
        }
        // After publication a sync failure is uncertain: get(id) and reconcile before retry.
        std::fs::File::open(&self.directory)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| TraitError::UnknownOutcome)?;
        Ok(())
    }
}
#[async_trait]
impl SnapshotRepository for FileSnapshotRepository {
    async fn publish_core(
        &self,
        snapshot: &convex_ports::core_workflow::CoreSnapshot,
    ) -> Result<(), TraitError> {
        snapshot.verify()?;
        let bytes = serde_json::to_vec(snapshot)
            .map_err(|_| TraitError::InvalidInput("core snapshot JSON"))?;
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        let path = self.path(&snapshot.id)?.with_extension("core.json");
        let mut staging = tempfile::NamedTempFile::new_in(&self.directory).map_err(io_error)?;
        staging.write_all(&bytes).map_err(io_error)?;
        staging.as_file().sync_all().map_err(io_error)?;
        match std::fs::hard_link(staging.path(), &path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if self.read_core(&snapshot.id)?.as_ref() != Some(snapshot) {
                    return Err(TraitError::Conflict("immutable core snapshot changed"));
                }
            }
            Err(e) => return Err(io_error(e)),
        }
        std::fs::File::open(&self.directory)
            .and_then(|d| d.sync_all())
            .map_err(|_| TraitError::UnknownOutcome)?;
        Ok(())
    }
    async fn get_core(
        &self,
        id: &SnapshotId,
    ) -> Result<Option<convex_ports::core_workflow::CoreSnapshot>, TraitError> {
        self.read_core(id)
    }
    async fn publish(&self, snapshot: &MarketSnapshot) -> Result<(), TraitError> {
        self.write(snapshot)
    }
    async fn get(&self, id: &SnapshotId) -> Result<Option<MarketSnapshot>, TraitError> {
        self.read(id)
    }
}

pub(crate) fn read_regular_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, TraitError> {
    let metadata = std::fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TraitError::InvalidInput(
            "snapshot/history must be a regular non-symlink file",
        ));
    }
    if metadata.len() > maximum as u64 {
        return Err(TraitError::CapacityExceeded);
    }
    let mut file = std::fs::File::open(path).map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(TraitError::InvalidInput(
            "snapshot/history must be a regular file",
        ));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > maximum {
        return Err(TraitError::CapacityExceeded);
    }
    Ok(bytes)
}
fn io_error(_: std::io::Error) -> TraitError {
    TraitError::IoError("snapshot/history filesystem operation failed".into())
}
