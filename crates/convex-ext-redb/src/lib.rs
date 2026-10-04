//! Single-process local redb storage for references, curves, pricing configuration,
//! overrides, and audit. See the crate README for transaction and recovery limits.

#![warn(missing_docs)]
#![warn(clippy::all)]
#![forbid(unsafe_code)]

use convex_ports::error::TraitError;
use convex_ports::storage::StorageAdapter;
use redb::Database;
use std::path::Path;
use std::sync::Arc;

mod audit_store;
mod config_store;
mod curve_store;
mod database;
mod override_store;
mod recovery;
mod reference_repository;
mod runtime_store;

pub use audit_store::RedbAuditStore;
pub use config_store::RedbConfigStore;
pub use curve_store::RedbCurveStore;
pub use database::{migrate_database, APPLICATION_SCHEMA_VERSION};
pub use override_store::RedbOverrideStore;
pub use recovery::{backup_database, restore_database, BackupManifest};
pub use reference_repository::RedbReferenceRepository;
pub use runtime_store::RedbRuntimeStore;

/// Create a full storage adapter with redb backend.
pub fn create_redb_storage(path: impl AsRef<Path>) -> Result<StorageAdapter, TraitError> {
    create_redb_storage_with_runtime(path).map(|(storage, _)| storage)
}

/// Open the authoritative repositories and pricing checkpoint/outbox on one local database.
pub fn create_redb_storage_with_runtime(
    path: impl AsRef<Path>,
) -> Result<(StorageAdapter, Arc<RedbRuntimeStore>), TraitError> {
    let db =
        Arc::new(Database::create(path).map_err(|e| TraitError::DatabaseError(e.to_string()))?);
    let configs = Arc::new(RedbConfigStore::new(db.clone())?);
    let runtime = Arc::new(RedbRuntimeStore::new(db.clone())?);
    let storage = StorageAdapter {
        bonds: Arc::new(
            RedbReferenceRepository::new(db.clone())
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?,
        ),
        curves: Arc::new(RedbCurveStore::new(db.clone())),
        configs,
        overrides: Arc::new(RedbOverrideStore::new(db.clone())),
        audit: Arc::new(RedbAuditStore::new(db)),
    };
    Ok((storage, runtime))
}

/// Create a file-backed storage adapter for testing.
///
/// Uses a uniquely named redb file in the system temporary directory.
/// Callers needing deterministic cleanup should use create_redb_storage with an owned temporary directory.
pub fn create_test_storage() -> Result<StorageAdapter, TraitError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let temp_dir = std::env::temp_dir();
    let db_path = temp_dir.join(format!("convex_test_{}_{}.redb", pid, id));
    // Stale leftover from a prior run (possibly older redb format) would fail to open.
    let _ = std::fs::remove_file(&db_path);

    create_redb_storage(&db_path)
}
