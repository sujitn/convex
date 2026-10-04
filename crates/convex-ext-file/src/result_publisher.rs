//! Single-owner local result archive. Each delivery key is reconciled before acknowledgment.
use async_trait::async_trait;
use convex_core::contracts::SourceId;
use convex_ports::{
    runtime::{PublishedValuation, ResultPublisher},
    snapshot::content_digest,
    TraitError,
};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    sync::Mutex,
};

/// Maximum archive bytes before an operator rotates the file; 256 MiB.
pub const MAX_RESULT_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 512 * 1024;
const MAX_KEYS: usize = 100_000;
struct Archive {
    failed: bool,
    file: File,
    keys: HashMap<(SourceId, u64), String>,
}
/// Single-process append-only JSONL output, with (runtime_id, revision) deduplication.
/// Reopening verifies bounded complete records. A torn/corrupt tail requires explicit repair.
pub struct JsonlResultPublisher {
    archive: Mutex<Archive>,
}
impl JsonlResultPublisher {
    /// Create/open a regular non-symlink file. Parent directories must already exist.
    /// The caller owns this archive exclusively; concurrent processes/network filesystems are unsupported.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, TraitError> {
        if std::fs::symlink_metadata(path.as_ref())
            .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
        {
            return Err(TraitError::InvalidInput(
                "result archive must be a regular non-symlink file",
            ));
        }
        let newly_created = !path.as_ref().exists();
        let parent = path
            .as_ref()
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_ref())
            .map_err(|_| TraitError::IoError("cannot open result archive".into()))?;
        let metadata = file
            .metadata()
            .map_err(|_| TraitError::IoError("cannot inspect result archive".into()))?;
        if !metadata.is_file() {
            return Err(TraitError::InvalidInput(
                "result archive must be a regular file",
            ));
        }
        if metadata.len() > MAX_RESULT_ARCHIVE_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        if newly_created {
            file.sync_all().map_err(|_| TraitError::UnknownOutcome)?;
            #[cfg(unix)]
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|_| TraitError::UnknownOutcome)?;
            #[cfg(not(unix))]
            {
                let _ = parent;
                return Err(TraitError::Unsupported(
                    "result archive directory durability requires Unix certification",
                ));
            }
        }
        let mut keys = HashMap::new();
        let mut reader = BufReader::new(
            File::open(path.as_ref())
                .map_err(|_| TraitError::IoError("cannot verify result archive".into()))?,
        );
        loop {
            let mut record = Vec::with_capacity(4096);
            let read = reader
                .by_ref()
                .take((MAX_RECORD_BYTES + 1) as u64)
                .read_until(b'\n', &mut record)
                .map_err(|_| TraitError::IoError("cannot read result archive".into()))?;
            if read == 0 {
                break;
            }
            if read > MAX_RECORD_BYTES || !record.ends_with(b"\n") {
                return Err(TraitError::InvalidInput(
                    "torn/oversized result archive record",
                ));
            }
            let value: PublishedValuation = serde_json::from_slice(&record)
                .map_err(|_| TraitError::InvalidInput("corrupt result archive record"))?;
            let bytes = serde_json::to_vec(&value)
                .map_err(|_| TraitError::InvalidInput("invalid result archive record"))?;
            let key = (value.runtime_id, value.revision);
            let digest = content_digest(b"convex.published-result.v1\0", &bytes);
            if let Some(previous) = keys.get(&key) {
                if *previous != digest {
                    return Err(TraitError::Conflict(
                        "result delivery key has different content",
                    ));
                }
            } else {
                if keys.len() == MAX_KEYS {
                    return Err(TraitError::CapacityExceeded);
                }
                keys.insert(key, digest);
            }
        }
        Ok(Self {
            archive: Mutex::new(Archive {
                file,
                keys,
                failed: false,
            }),
        })
    }
}
#[async_trait]
impl ResultPublisher for JsonlResultPublisher {
    async fn publish(&self, result: &PublishedValuation) -> Result<(), TraitError> {
        let mut bytes = serde_json::to_vec(result)
            .map_err(|_| TraitError::InvalidInput("result serialization failed"))?;
        if bytes.len() >= MAX_RECORD_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        let digest = content_digest(b"convex.published-result.v1\0", &bytes);
        let key = (result.runtime_id.clone(), result.revision);
        bytes.push(b'\n');
        let mut archive = self
            .archive
            .lock()
            .map_err(|_| TraitError::SourceNotAvailable("result archive lock failed".into()))?;
        if archive.failed {
            return Err(TraitError::SourceNotAvailable(
                "result archive requires reopen and reconciliation".into(),
            ));
        }
        if let Some(previous) = archive.keys.get(&key) {
            if previous != &digest {
                return Err(TraitError::Conflict(
                    "result delivery key has different content",
                ));
            }
            if archive.file.sync_all().is_err() {
                archive.failed = true;
                return Err(TraitError::UnknownOutcome);
            }
            return Ok(());
        }
        if archive.keys.len() == MAX_KEYS
            || archive
                .file
                .metadata()
                .map_err(|_| TraitError::IoError("cannot inspect result archive".into()))?
                .len()
                .saturating_add(bytes.len() as u64)
                > MAX_RESULT_ARCHIVE_BYTES
        {
            return Err(TraitError::CapacityExceeded);
        }
        if archive
            .file
            .write_all(&bytes)
            .and_then(|()| archive.file.sync_all())
            .is_err()
        {
            archive.failed = true;
            return Err(TraitError::UnknownOutcome);
        }
        archive.keys.insert(key, digest);
        Ok(())
    }
}
