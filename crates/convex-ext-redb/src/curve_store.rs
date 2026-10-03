//! Curve projections and immutable, ordered curve snapshots.
use crate::database::*;
use async_trait::async_trait;
use convex_core::ids::CurveId;
use convex_ports::error::TraitError;
use convex_ports::storage::{CurveConfig, CurveSnapshot, CurveStore};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::sync::Arc;
const CURVE_CONFIGS: TableDefinition<&str, &[u8]> = TableDefinition::new("curve_configs");
const CURVE_SNAPSHOTS: TableDefinition<(&str, i64), &[u8]> =
    TableDefinition::new("curve_snapshots");

/// Local curve config projection and immutable snapshots; no model calibration is performed here.
pub struct RedbCurveStore {
    db: Arc<Database>,
}
impl RedbCurveStore {
    /// Construct a curve store on the shared database handle.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CurveStore for RedbCurveStore {
    async fn get_config(&self, id: &CurveId) -> Result<Option<CurveConfig>, TraitError> {
        validate_id(id.as_str())?;
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(CURVE_CONFIGS) {
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
    async fn save_config(&self, config: &CurveConfig) -> Result<(), TraitError> {
        validate_id(config.curve_id.as_str())?;
        if config.tenors.len() > 4096
            || config.tenors.iter().any(|v| v.is_empty() || v.len() > 128)
            || [
                &config.name,
                &config.interpolation,
                &config.day_count,
                &config.build_schedule,
            ]
            .into_iter()
            .any(|v| v.is_empty() || v.len() > 1024 || v.chars().any(char::is_control))
        {
            return Err(TraitError::InvalidInput("invalid curve configuration"));
        }
        let bytes = encode(config)?;
        let write = begin_write(&self.db)?;
        {
            let mut table = write.open_table(CURVE_CONFIGS).map_err(database_error)?;
            if table
                .get(config.curve_id.as_str())
                .map_err(database_error)?
                .is_none()
                && table.len().map_err(database_error)? >= MAX_RECORDS
            {
                return Err(TraitError::CapacityExceeded);
            }
            table
                .insert(config.curve_id.as_str(), bytes.as_slice())
                .map_err(database_error)?;
        }
        commit(write)
    }
    async fn list_configs(&self) -> Result<Vec<CurveConfig>, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(CURVE_CONFIGS) {
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
                let (_, v) = r.map_err(database_error)?;
                decode(v.value())
            })
            .collect()
    }
    async fn delete_config(&self, id: &CurveId) -> Result<bool, TraitError> {
        validate_id(id.as_str())?;
        let write = begin_write(&self.db)?;
        let deleted = write
            .open_table(CURVE_CONFIGS)
            .map_err(database_error)?
            .remove(id.as_str())
            .map_err(database_error)?
            .is_some();
        commit(write)?;
        Ok(deleted)
    }
    async fn save_snapshot(&self, snapshot: &CurveSnapshot) -> Result<(), TraitError> {
        validate_id(snapshot.curve_id.as_str())?;
        if snapshot.points.is_empty()
            || snapshot.points.len() > 4096
            || snapshot.inputs_hash.is_empty()
            || snapshot.inputs_hash.len() > 1024
            || snapshot
                .points
                .iter()
                .any(|(tenor, rate)| *tenor == 0 || !rate.is_finite())
            || snapshot.points.windows(2).any(|v| v[0].0 >= v[1].0)
        {
            return Err(TraitError::InvalidInput("invalid curve snapshot"));
        }
        let bytes = encode(snapshot)?;
        let write = begin_write(&self.db)?;
        {
            let mut table = write.open_table(CURVE_SNAPSHOTS).map_err(database_error)?;
            if let Some(previous) = table
                .get((snapshot.curve_id.as_str(), snapshot.as_of))
                .map_err(database_error)?
            {
                return if decode::<CurveSnapshot>(previous.value())? == *snapshot {
                    Ok(())
                } else {
                    Err(TraitError::Conflict("curve snapshot has different content"))
                };
            }
            if table.len().map_err(database_error)? >= MAX_HISTORY {
                return Err(TraitError::CapacityExceeded);
            }
            table
                .insert(
                    (snapshot.curve_id.as_str(), snapshot.as_of),
                    bytes.as_slice(),
                )
                .map_err(database_error)?;
        }
        commit(write)
    }
    async fn get_snapshot(
        &self,
        id: &CurveId,
        as_of: i64,
    ) -> Result<Option<CurveSnapshot>, TraitError> {
        validate_id(id.as_str())?;
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(CURVE_SNAPSHOTS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(database_error(e)),
        };
        table
            .get((id.as_str(), as_of))
            .map_err(database_error)?
            .map(|v| decode(v.value()))
            .transpose()
    }
    async fn get_latest_snapshot(&self, id: &CurveId) -> Result<Option<CurveSnapshot>, TraitError> {
        validate_id(id.as_str())?;
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(CURVE_SNAPSHOTS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(database_error(e)),
        };
        table
            .range((id.as_str(), i64::MIN)..=(id.as_str(), i64::MAX))
            .map_err(database_error)?
            .next_back()
            .map(|r| {
                let (_, v) = r.map_err(database_error)?;
                decode(v.value())
            })
            .transpose()
    }
    async fn list_snapshots(
        &self,
        id: &CurveId,
        from: i64,
        to: i64,
    ) -> Result<Vec<CurveSnapshot>, TraitError> {
        validate_id(id.as_str())?;
        if from > to {
            return Err(TraitError::InvalidInput("inverted curve snapshot range"));
        }
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(CURVE_SNAPSHOTS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(database_error(e)),
        };
        if table.len().map_err(database_error)? > MAX_HISTORY {
            return Err(TraitError::CapacityExceeded);
        }
        table
            .range((id.as_str(), from)..=(id.as_str(), to))
            .map_err(database_error)?
            .map(|r| {
                let (_, v) = r.map_err(database_error)?;
                decode(v.value())
            })
            .collect()
    }
    async fn delete_snapshots_before(&self, id: &CurveId, before: i64) -> Result<u64, TraitError> {
        validate_id(id.as_str())?;
        if before == i64::MIN {
            return Ok(0);
        }
        let write = begin_write(&self.db)?;
        let count;
        {
            let mut table = write.open_table(CURVE_SNAPSHOTS).map_err(database_error)?;
            if table.len().map_err(database_error)? > MAX_HISTORY {
                return Err(TraitError::CapacityExceeded);
            }
            let keys: Vec<i64> = table
                .range((id.as_str(), i64::MIN)..(id.as_str(), before))
                .map_err(database_error)?
                .map(|r| r.map(|(key, _)| key.value().1).map_err(database_error))
                .collect::<Result<_, _>>()?;
            count = keys.len() as u64;
            for as_of in keys {
                table.remove((id.as_str(), as_of)).map_err(database_error)?;
            }
        }
        commit(write)?;
        Ok(count)
    }
}
