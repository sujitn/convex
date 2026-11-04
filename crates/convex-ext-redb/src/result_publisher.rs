//! Durable archive of published runtime results. Additive schema 1 table.
use crate::database::{begin_write, commit, database_error, MAX_HISTORY};
use async_trait::async_trait;
use convex_core::contracts::SourceId;
use convex_ports::{
    runtime::{PublishedValuation, ResultPublisher},
    TraitError,
};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::sync::Arc;

const RESULTS: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("runtime_results_v1");
const MAX_RESULT_BYTES: usize = 512 * 1024;

/// Each result is one redb transaction keyed by (runtime_id, revision). An identical
/// redelivery succeeds; different content under a retained key is a conflict.
pub struct RedbResultPublisher {
    db: Arc<Database>,
}
impl RedbResultPublisher {
    /// Open/create the additive table after validating the existing application schema.
    pub fn new(db: Arc<Database>) -> Result<Self, TraitError> {
        crate::migrate_database(&db)?;
        let write = begin_write(&db)?;
        write.open_table(RESULTS).map_err(database_error)?;
        commit(write)?;
        Ok(Self { db })
    }
    /// The archived result for one delivery key.
    pub fn get(
        &self,
        runtime_id: &SourceId,
        revision: u64,
    ) -> Result<Option<PublishedValuation>, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        let table = read.open_table(RESULTS).map_err(database_error)?;
        let Some(value) = table
            .get((runtime_id.as_str(), revision))
            .map_err(database_error)?
        else {
            return Ok(None);
        };
        if value.value().len() > MAX_RESULT_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        let result: PublishedValuation = serde_json::from_slice(value.value())
            .map_err(|_| TraitError::InvalidInput("corrupt result archive record"))?;
        if result.runtime_id != *runtime_id || result.revision != revision {
            return Err(TraitError::Conflict("result archive key/content mismatch"));
        }
        Ok(Some(result))
    }
    /// The number of archived results.
    pub fn len(&self) -> Result<u64, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        let table = read.open_table(RESULTS).map_err(database_error)?;
        table.len().map_err(database_error)
    }
    /// True when nothing is archived.
    pub fn is_empty(&self) -> Result<bool, TraitError> {
        self.len().map(|len| len == 0)
    }
}

#[async_trait]
impl ResultPublisher for RedbResultPublisher {
    async fn publish(&self, result: &PublishedValuation) -> Result<(), TraitError> {
        let bytes = serde_json::to_vec(result)
            .map_err(|_| TraitError::InvalidInput("result serialization failed"))?;
        if bytes.len() > MAX_RESULT_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        let key = (result.runtime_id.as_str(), result.revision);
        let write = begin_write(&self.db)?;
        {
            let mut table = write.open_table(RESULTS).map_err(database_error)?;
            if let Some(retained) = table.get(key).map_err(database_error)? {
                return if retained.value() == bytes.as_slice() {
                    Ok(())
                } else {
                    Err(TraitError::Conflict(
                        "result delivery key has different content",
                    ))
                };
            }
            if table.len().map_err(database_error)? >= MAX_HISTORY {
                return Err(TraitError::CapacityExceeded);
            }
            table
                .insert(key, bytes.as_slice())
                .map_err(database_error)?;
        }
        commit(write)
    }
}
