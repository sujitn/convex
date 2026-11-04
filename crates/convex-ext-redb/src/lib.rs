//! Single-process local redb storage for references, curves, pricing configuration,
//! activation, scoped policy overrides, audit, snapshots and archived results. See the
//! crate README for transaction and recovery limits.

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
mod configuration_repository;
mod curve_store;
mod database;
mod fixing_repository;
mod recovery;
mod reference_repository;
mod result_publisher;
mod runtime_store;
mod snapshot_repository;

pub use audit_store::RedbAuditStore;
pub use config_store::RedbConfigStore;
pub use configuration_repository::RedbConfigurationRepository;
pub use curve_store::RedbCurveStore;
pub use database::{migrate_database, APPLICATION_SCHEMA_VERSION};
pub use fixing_repository::RedbFixingRepository;
pub use recovery::{backup_database, restore_database, BackupManifest};
pub use reference_repository::RedbReferenceRepository;
pub use result_publisher::RedbResultPublisher;
pub use runtime_store::RedbRuntimeStore;
pub use snapshot_repository::RedbSnapshotRepository;

/// Every durable local port, on one database file under one backup.
pub struct RedbStorage {
    /// Authoritative repositories.
    pub storage: StorageAdapter,
    /// Pricing checkpoint/outbox and core results.
    pub runtime: Arc<RedbRuntimeStore>,
    /// Published fixing history.
    pub fixings: Arc<RedbFixingRepository>,
    /// Immutable market and core snapshots.
    pub snapshots: Arc<RedbSnapshotRepository>,
    /// Archived runtime results.
    pub results: Arc<RedbResultPublisher>,
}

/// Create a full storage adapter with redb backend.
pub fn create_redb_storage(path: impl AsRef<Path>) -> Result<StorageAdapter, TraitError> {
    create_redb_storage_with_runtime(path).map(|(storage, _)| storage)
}

/// Open the authoritative repositories and pricing checkpoint/outbox on one local database.
pub fn create_redb_storage_with_runtime(
    path: impl AsRef<Path>,
) -> Result<(StorageAdapter, Arc<RedbRuntimeStore>), TraitError> {
    create_redb_storage_with_core(path).map(|(storage, runtime, _)| (storage, runtime))
}

/// Compose existing storage/runtime ports with durable published fixing history.
pub fn create_redb_storage_with_core(
    path: impl AsRef<Path>,
) -> Result<
    (
        StorageAdapter,
        Arc<RedbRuntimeStore>,
        Arc<RedbFixingRepository>,
    ),
    TraitError,
> {
    open_redb_storage(path).map(|s| (s.storage, s.runtime, s.fixings))
}

/// Open every durable local port on one database.
pub fn open_redb_storage(path: impl AsRef<Path>) -> Result<RedbStorage, TraitError> {
    let db =
        Arc::new(Database::create(path).map_err(|e| TraitError::DatabaseError(e.to_string()))?);
    let configs = Arc::new(RedbConfigStore::new(db.clone())?);
    let runtime = Arc::new(RedbRuntimeStore::new(db.clone())?);
    let fixings = Arc::new(RedbFixingRepository::new(db.clone()));
    let snapshots = Arc::new(RedbSnapshotRepository::new(db.clone())?);
    let results = Arc::new(RedbResultPublisher::new(db.clone())?);
    let storage = StorageAdapter {
        bonds: Arc::new(
            RedbReferenceRepository::new(db.clone())
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?,
        ),
        curves: Arc::new(RedbCurveStore::new(db.clone())),
        configs,
        configuration: Arc::new(RedbConfigurationRepository::new(db.clone())?),
        audit: Arc::new(RedbAuditStore::new(db)),
    };
    Ok(RedbStorage {
        storage,
        runtime,
        fixings,
        snapshots,
        results,
    })
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
