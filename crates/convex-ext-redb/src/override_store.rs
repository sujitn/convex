//! Local overrides; audited approval and mutation history are explicitly unsupported.
use crate::database::*;
use async_trait::async_trait;
use convex_core::ids::InstrumentId;
use convex_ports::error::TraitError;
use convex_ports::storage::{OverrideAudit, OverrideStore, PriceOverride};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::sync::Arc;
const OVERRIDES: TableDefinition<&str, &[u8]> = TableDefinition::new("overrides");

/// Local mutable override projection. History/actor-aware mutations are not provided.
pub struct RedbOverrideStore {
    db: Arc<Database>,
}
impl RedbOverrideStore {
    /// Construct an override store on the shared database handle.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn list(&self) -> Result<Vec<PriceOverride>, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(OVERRIDES) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(database_error(e)),
        };
        if table.len().map_err(database_error)? > MAX_RECORDS {
            return Err(TraitError::CapacityExceeded);
        }
        table
            .iter()
            .map_err(database_error)?
            .map(|r| {
                let (_, data) = r.map_err(database_error)?;
                decode(data.value())
            })
            .collect()
    }
}

fn now_millis() -> Result<i64, TraitError> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| TraitError::InvalidInput("system clock precedes Unix epoch"))?;
    i64::try_from(elapsed.as_millis())
        .map_err(|_| TraitError::InvalidInput("system clock exceeds timestamp range"))
}

#[async_trait]
impl OverrideStore for RedbOverrideStore {
    async fn get(&self, id: &InstrumentId) -> Result<Option<PriceOverride>, TraitError> {
        validate_id(id.as_str())?;
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(OVERRIDES) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(database_error(e)),
        };
        table
            .get(id.as_str())
            .map_err(database_error)?
            .map(|v| decode(v.value()))
            .transpose()
    }
    async fn get_active(&self) -> Result<Vec<PriceOverride>, TraitError> {
        let now = now_millis()?;
        Ok(self
            .list()?
            .into_iter()
            .filter(|v| v.is_approved && v.expires_at.is_none_or(|e| e > now))
            .collect())
    }
    async fn save(&self, value: &PriceOverride) -> Result<(), TraitError> {
        validate_id(value.instrument_id.as_str())?;
        if value
            .approved_by
            .as_ref()
            .is_some_and(|v| v.is_empty() || v.len() > 1024 || v.chars().any(char::is_control))
            || value.created_by.chars().any(char::is_control)
            || value.is_approved && (value.approved_by.is_none() || value.approved_at.is_none())
            || value.approved_at.is_some_and(|v| v < value.created_at)
        {
            return Err(TraitError::InvalidInput(
                "invalid override approval metadata",
            ));
        }
        if [value.price, value.yield_value, value.spread]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite())
            || value.reason.is_empty()
            || value.reason.len() > 4096
            || value.created_by.is_empty()
            || value.created_by.len() > 1024
            || value.expires_at.is_some_and(|e| e < value.created_at)
        {
            return Err(TraitError::InvalidInput(
                "invalid override values, actor, reason or expiry",
            ));
        }
        let bytes = encode(value)?;
        let write = begin_write(&self.db)?;
        {
            let mut table = write.open_table(OVERRIDES).map_err(database_error)?;
            if table
                .get(value.instrument_id.as_str())
                .map_err(database_error)?
                .is_none()
                && table.len().map_err(database_error)? >= MAX_RECORDS
            {
                return Err(TraitError::CapacityExceeded);
            }
            table
                .insert(value.instrument_id.as_str(), bytes.as_slice())
                .map_err(database_error)?;
        }
        commit(write)
    }
    async fn delete(&self, id: &InstrumentId) -> Result<bool, TraitError> {
        validate_id(id.as_str())?;
        let write = begin_write(&self.db)?;
        let deleted = write
            .open_table(OVERRIDES)
            .map_err(database_error)?
            .remove(id.as_str())
            .map_err(database_error)?
            .is_some();
        commit(write)?;
        Ok(deleted)
    }
    async fn get_pending_approval(&self) -> Result<Vec<PriceOverride>, TraitError> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|v| !v.is_approved)
            .collect())
    }
    async fn get_history(&self, _id: &InstrumentId) -> Result<Vec<OverrideAudit>, TraitError> {
        Err(TraitError::Unsupported(
            "override mutation history and actor-aware deletion",
        ))
    }
}
