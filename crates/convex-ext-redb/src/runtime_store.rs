//! Local durable checkpoint/outbox CAS. Additive schema 1 table; no shared-host ownership claim.
use crate::database::{begin_write, commit, database_error};
use async_trait::async_trait;
use convex_core::contracts::SourceId;
use convex_ports::{
    runtime::{RuntimeCheckpoint, RuntimeStore, MAX_RUNTIME_BYTES},
    TraitError,
};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::sync::Arc;
const CHECKPOINTS: TableDefinition<&str, &[u8]> = TableDefinition::new("pricing_runtime_v1");

/// One local redb transaction commits accepted work, result state and publication outbox.
pub struct RedbRuntimeStore {
    db: Arc<Database>,
}
impl RedbRuntimeStore {
    /// Open/create the additive table after validating the existing application schema.
    pub fn new(db: Arc<Database>) -> Result<Self, TraitError> {
        crate::migrate_database(&db)?;
        let write = begin_write(&db)?;
        write.open_table(CHECKPOINTS).map_err(database_error)?;
        commit(write)?;
        Ok(Self { db })
    }
}
fn encode(image: &RuntimeCheckpoint) -> Result<Vec<u8>, TraitError> {
    image.validate()?;
    let bytes = serde_json::to_vec(image)
        .map_err(|_| TraitError::InvalidInput("runtime serialization failed"))?;
    if bytes.len() > MAX_RUNTIME_BYTES {
        return Err(TraitError::CapacityExceeded);
    }
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> Result<RuntimeCheckpoint, TraitError> {
    if bytes.len() > MAX_RUNTIME_BYTES {
        return Err(TraitError::CapacityExceeded);
    }
    let image: RuntimeCheckpoint = serde_json::from_slice(bytes)
        .map_err(|_| TraitError::InvalidInput("corrupt runtime checkpoint"))?;
    image.validate()?;
    Ok(image)
}
#[async_trait]
impl RuntimeStore for RedbRuntimeStore {
    async fn load(&self, id: &SourceId) -> Result<Option<RuntimeCheckpoint>, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        read.open_table(CHECKPOINTS)
            .map_err(database_error)?
            .get(id.as_str())
            .map_err(database_error)?
            .map(|value| {
                let image = decode(value.value())?;
                if image.runtime_id != *id {
                    return Err(TraitError::Conflict("runtime key/content mismatch"));
                }
                Ok(image)
            })
            .transpose()
    }
    async fn checkpoint(&self, expected: u64, image: &RuntimeCheckpoint) -> Result<(), TraitError> {
        let bytes = encode(image)?;
        if expected.checked_add(1) != Some(image.revision) {
            return Err(TraitError::InvalidInput(
                "runtime requires next checkpoint revision",
            ));
        }
        let write = begin_write(&self.db)?;
        {
            let mut table = write.open_table(CHECKPOINTS).map_err(database_error)?;
            let previous = table
                .get(image.runtime_id.as_str())
                .map_err(database_error)?
                .map(|v| decode(v.value()))
                .transpose()?;
            if previous.as_ref() == Some(image) {
                return Ok(());
            }
            if previous.as_ref().map_or(0, |v| v.revision) != expected {
                return Err(TraitError::Conflict("runtime checkpoint changed"));
            }
            if previous.is_none() && table.len().map_err(database_error)? >= 64 {
                return Err(TraitError::CapacityExceeded);
            }
            table
                .insert(image.runtime_id.as_str(), bytes.as_slice())
                .map_err(database_error)?;
        }
        commit(write)
    }
}
