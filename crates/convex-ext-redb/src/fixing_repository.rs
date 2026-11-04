//! Immutable published fixing history on the existing local database.
use crate::database::{begin_write, commit, database_error, MAX_HISTORY};
use async_trait::async_trait;
use convex_core::contracts::UtcTimestamp;
use convex_ports::{fixing::*, TraitError};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::sync::Arc;
const FIXINGS: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("published_fixings_v1");

/// Source-separated full fixing revisions; no current-value substitute.
pub struct RedbFixingRepository {
    db: Arc<Database>,
}
impl RedbFixingRepository {
    /// Share the same application database as references, curves and results.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}
fn key(key: &FixingKey) -> Result<String, TraitError> {
    serde_json::to_string(key).map_err(|_| TraitError::InvalidInput("fixing key JSON"))
}
fn decode(bytes: &[u8]) -> Result<PublishedFixing, TraitError> {
    if bytes.len() > 8192 {
        return Err(TraitError::CapacityExceeded);
    }
    let r: PublishedFixing = serde_json::from_slice(bytes)
        .map_err(|_| TraitError::InvalidInput("corrupt fixing history"))?;
    r.validate()?;
    Ok(r)
}
#[async_trait]
impl FixingRepository for RedbFixingRepository {
    async fn publish(&self, values: &[PublishedFixing]) -> Result<(), TraitError> {
        if values.is_empty() || values.len() > 100_000 {
            return Err(TraitError::CapacityExceeded);
        }
        let mut batch = std::collections::BTreeMap::new();
        for value in values {
            value.validate()?;
            let k = (key(&value.key)?, value.version.get());
            if batch
                .insert(
                    k,
                    serde_json::to_vec(value)
                        .map_err(|_| TraitError::InvalidInput("fixing JSON"))?,
                )
                .is_some()
            {
                return Err(TraitError::Conflict("duplicate fixing revision in batch"));
            }
        }
        let write = begin_write(&self.db)?;
        let changed: std::collections::BTreeSet<_> = batch.keys().map(|(k, _)| k.clone()).collect();
        {
            let mut table = write.open_table(FIXINGS).map_err(database_error)?;
            for ((k, revision), bytes) in batch {
                if bytes.len() > 8192 {
                    return Err(TraitError::CapacityExceeded);
                }
                if let Some(prior) = table.get((k.as_str(), revision)).map_err(database_error)? {
                    if prior.value() != bytes.as_slice() {
                        return Err(TraitError::Conflict("immutable fixing revision changed"));
                    }
                    continue;
                }
                if table.len().map_err(database_error)? >= MAX_HISTORY {
                    return Err(TraitError::CapacityExceeded);
                }
                table
                    .insert((k.as_str(), revision), bytes.as_slice())
                    .map_err(database_error)?;
            }
            let mut visited = 0u64;
            for k in changed {
                let mut previous = None;
                for entry in table
                    .range((k.as_str(), 0)..=(k.as_str(), u64::MAX))
                    .map_err(database_error)?
                {
                    visited += 1;
                    if visited > MAX_HISTORY {
                        return Err(TraitError::CapacityExceeded);
                    }
                    let (_, bytes) = entry.map_err(database_error)?;
                    let current = decode(bytes.value())?;
                    if previous.is_some_and(|at| at > current.recorded_at) {
                        return Err(TraitError::Conflict("temporally reversed fixing revision"));
                    }
                    previous = Some(current.recorded_at);
                }
            }
        }
        commit(write)
    }
    async fn history_at(
        &self,
        keys: &[FixingKey],
        known_at: UtcTimestamp,
    ) -> Result<Vec<PublishedFixing>, TraitError> {
        if keys.len() > 1024 {
            return Err(TraitError::CapacityExceeded);
        }
        let selected: std::collections::BTreeSet<_> = keys.iter().collect();
        if selected.len() != keys.len() {
            return Err(TraitError::InvalidInput("duplicate fixing history key"));
        }
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(FIXINGS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(vec![]),
            Err(e) => return Err(database_error(e)),
        };
        let mut result = Vec::new();
        for selected in keys {
            let k = key(selected)?;
            for entry in table
                .range((k.as_str(), 0)..=(k.as_str(), u64::MAX))
                .map_err(database_error)?
            {
                let (stored, bytes) = entry.map_err(database_error)?;
                let value = decode(bytes.value())?;
                if value.key != *selected || stored.value().1 != value.version.get() {
                    return Err(TraitError::Conflict("fixing history key mismatch"));
                }
                if value.published_at <= known_at && value.recorded_at <= known_at {
                    if result.len() >= 100_000 {
                        return Err(TraitError::CapacityExceeded);
                    }
                    result.push(value);
                }
            }
        }
        Ok(result)
    }
    async fn get_at(
        &self,
        key: &FixingKey,
        known_at: UtcTimestamp,
    ) -> Result<Option<PublishedFixing>, TraitError> {
        select_fixing_at(
            &self.history_at(std::slice::from_ref(key), known_at).await?,
            key,
            known_at,
        )
    }
}
