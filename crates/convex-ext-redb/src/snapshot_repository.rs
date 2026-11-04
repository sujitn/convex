//! Immutable content-addressed market and core snapshots. Additive schema 1 tables.
use crate::database::{begin_write, commit, database_error, MAX_RECORDS};
use async_trait::async_trait;
use convex_core::contracts::SnapshotId;
use convex_ports::{
    core_workflow::CoreSnapshot,
    snapshot::{MarketSnapshot, SnapshotRepository, MAX_SNAPSHOT_BYTES},
    TraitError,
};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::sync::Arc;

const MARKET_SNAPSHOTS: TableDefinition<&str, &[u8]> = TableDefinition::new("market_snapshots_v1");
const CORE_SNAPSHOTS: TableDefinition<&str, &[u8]> = TableDefinition::new("core_snapshots_v1");

/// Each publication is one redb transaction: absent ids insert canonical bytes, exact
/// retries succeed, and different content under a retained id is a conflict.
pub struct RedbSnapshotRepository {
    db: Arc<Database>,
}
impl RedbSnapshotRepository {
    /// Open/create the additive tables after validating the existing application schema.
    pub fn new(db: Arc<Database>) -> Result<Self, TraitError> {
        crate::migrate_database(&db)?;
        let write = begin_write(&db)?;
        write.open_table(MARKET_SNAPSHOTS).map_err(database_error)?;
        write.open_table(CORE_SNAPSHOTS).map_err(database_error)?;
        commit(write)?;
        Ok(Self { db })
    }
    fn insert(
        &self,
        table: TableDefinition<&str, &[u8]>,
        id: &SnapshotId,
        bytes: &[u8],
    ) -> Result<(), TraitError> {
        let key = key(id)?;
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        let write = begin_write(&self.db)?;
        {
            let mut table = write.open_table(table).map_err(database_error)?;
            if let Some(retained) = table.get(key).map_err(database_error)? {
                return if retained.value() == bytes {
                    Ok(())
                } else {
                    Err(TraitError::Conflict("immutable snapshot identity conflict"))
                };
            }
            if table.len().map_err(database_error)? >= MAX_RECORDS {
                return Err(TraitError::CapacityExceeded);
            }
            table.insert(key, bytes).map_err(database_error)?;
        }
        commit(write)
    }
    fn read(
        &self,
        table: TableDefinition<&str, &[u8]>,
        id: &SnapshotId,
    ) -> Result<Option<Vec<u8>>, TraitError> {
        let key = key(id)?;
        let read = self.db.begin_read().map_err(database_error)?;
        let table = read.open_table(table).map_err(database_error)?;
        let Some(value) = table.get(key).map_err(database_error)? else {
            return Ok(None);
        };
        if value.value().len() > MAX_SNAPSHOT_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        Ok(Some(value.value().to_vec()))
    }
    /// The number of stored market snapshots.
    pub fn market_snapshot_count(&self) -> Result<u64, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        let table = read.open_table(MARKET_SNAPSHOTS).map_err(database_error)?;
        table.len().map_err(database_error)
    }
}

/// Only content digests are keys, as in the file adapter.
fn key(id: &SnapshotId) -> Result<&str, TraitError> {
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
    Ok(id.as_str())
}

#[async_trait]
impl SnapshotRepository for RedbSnapshotRepository {
    async fn publish_core(&self, snapshot: &CoreSnapshot) -> Result<(), TraitError> {
        let bytes = snapshot.canonical_bytes()?;
        self.insert(CORE_SNAPSHOTS, &snapshot.id, &bytes)
    }
    async fn get_core(&self, id: &SnapshotId) -> Result<Option<CoreSnapshot>, TraitError> {
        let Some(bytes) = self.read(CORE_SNAPSHOTS, id)? else {
            return Ok(None);
        };
        let snapshot: CoreSnapshot = serde_json::from_slice(&bytes)
            .map_err(|_| TraitError::InvalidInput("corrupt core snapshot"))?;
        if snapshot.id != *id || snapshot.canonical_bytes()? != bytes {
            return Err(TraitError::Conflict(
                "core snapshot identity/canonical content mismatch",
            ));
        }
        Ok(Some(snapshot))
    }
    async fn publish(&self, snapshot: &MarketSnapshot) -> Result<(), TraitError> {
        let bytes = snapshot.canonical_bytes()?;
        self.insert(MARKET_SNAPSHOTS, snapshot.id(), &bytes)
    }
    async fn get(&self, id: &SnapshotId) -> Result<Option<MarketSnapshot>, TraitError> {
        let Some(bytes) = self.read(MARKET_SNAPSHOTS, id)? else {
            return Ok(None);
        };
        let snapshot: MarketSnapshot = serde_json::from_slice(&bytes)
            .map_err(|_| TraitError::InvalidInput("invalid/corrupt immutable snapshot"))?;
        if snapshot.id() != id || snapshot.canonical_bytes()? != bytes {
            return Err(TraitError::Conflict(
                "snapshot identity/canonical content mismatch",
            ));
        }
        Ok(Some(snapshot))
    }
}
