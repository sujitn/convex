//! # Convex Ext Redb
//!
//! Embedded storage implementation using redb for the Convex pricing engine.
//!
//! This crate provides default storage implementations for:
//! - Bond reference data
//! - Curve configs and snapshots
//! - Pricing configurations
//! - Price overrides
//! - Audit logs

#![warn(missing_docs)]
#![warn(clippy::all)]

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};

use convex_traits::error::TraitError;
use convex_traits::ids::{CurveId, InstrumentId};
use convex_traits::reference_data::BondReferenceData;
use convex_traits::storage::{
    AuditEntry, AuditFilter, AuditStore, BondFilter, BondPricingConfig, BondStore, ConfigStore,
    ConfigVersion, CurveConfig, CurveSnapshot, CurveStore, OverrideAudit, OverrideStore, Page,
    Pagination, PriceOverride, StorageAdapter,
};

// Table definitions
const BONDS: TableDefinition<&str, &[u8]> = TableDefinition::new("bonds");
const CURVE_CONFIGS: TableDefinition<&str, &[u8]> = TableDefinition::new("curve_configs");
const CURVE_SNAPSHOTS: TableDefinition<(&str, i64), &[u8]> = TableDefinition::new("curve_snapshots");
const PRICING_CONFIGS: TableDefinition<&str, &[u8]> = TableDefinition::new("pricing_configs");
const OVERRIDES: TableDefinition<&str, &[u8]> = TableDefinition::new("overrides");
const AUDIT: TableDefinition<u64, &[u8]> = TableDefinition::new("audit");

/// Redb-based bond store.
pub struct RedbBondStore {
    db: Arc<Database>,
}

impl RedbBondStore {
    /// Create a new redb bond store.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl BondStore for RedbBondStore {
    async fn get(&self, id: &InstrumentId) -> Result<Option<BondReferenceData>, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(BONDS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        match table.get(id.as_str()) {
            Ok(Some(data)) => {
                let bond: BondReferenceData = serde_json::from_slice(data.value())
                    .map_err(|e| TraitError::ParseError(e.to_string()))?;
                Ok(Some(bond))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(TraitError::DatabaseError(e.to_string())),
        }
    }

    async fn get_many(&self, ids: &[InstrumentId]) -> Result<Vec<BondReferenceData>, TraitError> {
        let mut results = Vec::new();
        for id in ids {
            if let Some(bond) = self.get(id).await? {
                results.push(bond);
            }
        }
        Ok(results)
    }

    async fn save(&self, bond: &BondReferenceData) -> Result<(), TraitError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(BONDS)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

            let bytes = serde_json::to_vec(bond)
                .map_err(|e| TraitError::SerializationError(e.to_string()))?;

            table
                .insert(bond.instrument_id.as_str(), bytes.as_slice())
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    async fn save_batch(&self, bonds: &[BondReferenceData]) -> Result<(), TraitError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(BONDS)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

            for bond in bonds {
                let bytes = serde_json::to_vec(bond)
                    .map_err(|e| TraitError::SerializationError(e.to_string()))?;
                table
                    .insert(bond.instrument_id.as_str(), bytes.as_slice())
                    .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            }
        }
        write_txn
            .commit()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &InstrumentId) -> Result<bool, TraitError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        let deleted = {
            let mut table = write_txn
                .open_table(BONDS)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            let result = table
                .remove(id.as_str())
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            result.is_some()
        };
        write_txn
            .commit()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(deleted)
    }

    async fn list(
        &self,
        _filter: &BondFilter,
        pagination: &Pagination,
    ) -> Result<Page<BondReferenceData>, TraitError> {
        // Simple implementation - would need index for filtering
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(BONDS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(Page {
                    items: vec![],
                    total: 0,
                    offset: pagination.offset,
                    limit: pagination.limit,
                })
            }
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        let mut items = Vec::new();
        let mut total = 0u64;

        for result in table.iter().map_err(|e| TraitError::DatabaseError(e.to_string()))? {
            let (_, value) = result.map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            total += 1;

            if total > pagination.offset as u64
                && items.len() < pagination.limit
            {
                let bond: BondReferenceData = serde_json::from_slice(value.value())
                    .map_err(|e| TraitError::ParseError(e.to_string()))?;
                items.push(bond);
            }
        }

        Ok(Page {
            items,
            total,
            offset: pagination.offset,
            limit: pagination.limit,
        })
    }

    async fn count(&self, _filter: &BondFilter) -> Result<u64, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(BONDS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        let count = table
            .len()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(count)
    }

    async fn search(
        &self,
        _query: &str,
        limit: usize,
    ) -> Result<Vec<BondReferenceData>, TraitError> {
        // Simple implementation - would need full-text index
        let page = self
            .list(&BondFilter::default(), &Pagination::new(0, limit))
            .await?;
        Ok(page.items)
    }
}

/// Redb-based curve store.
pub struct RedbCurveStore {
    db: Arc<Database>,
}

impl RedbCurveStore {
    /// Create a new redb curve store.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CurveStore for RedbCurveStore {
    async fn get_config(&self, id: &CurveId) -> Result<Option<CurveConfig>, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(CURVE_CONFIGS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        match table.get(id.as_str()) {
            Ok(Some(data)) => {
                let config: CurveConfig = serde_json::from_slice(data.value())
                    .map_err(|e| TraitError::ParseError(e.to_string()))?;
                Ok(Some(config))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(TraitError::DatabaseError(e.to_string())),
        }
    }

    async fn save_config(&self, config: &CurveConfig) -> Result<(), TraitError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(CURVE_CONFIGS)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

            let bytes = serde_json::to_vec(config)
                .map_err(|e| TraitError::SerializationError(e.to_string()))?;

            table
                .insert(config.curve_id.as_str(), bytes.as_slice())
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    async fn list_configs(&self) -> Result<Vec<CurveConfig>, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(CURVE_CONFIGS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(vec![]),
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        let mut configs = Vec::new();
        for result in table.iter().map_err(|e| TraitError::DatabaseError(e.to_string()))? {
            let (_, value) = result.map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            let config: CurveConfig = serde_json::from_slice(value.value())
                .map_err(|e| TraitError::ParseError(e.to_string()))?;
            configs.push(config);
        }
        Ok(configs)
    }

    async fn delete_config(&self, id: &CurveId) -> Result<bool, TraitError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        let deleted = {
            let mut table = write_txn
                .open_table(CURVE_CONFIGS)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            let result = table
                .remove(id.as_str())
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            result.is_some()
        };
        write_txn
            .commit()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(deleted)
    }

    async fn save_snapshot(&self, snapshot: &CurveSnapshot) -> Result<(), TraitError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(CURVE_SNAPSHOTS)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

            let bytes = serde_json::to_vec(snapshot)
                .map_err(|e| TraitError::SerializationError(e.to_string()))?;

            table
                .insert((snapshot.curve_id.as_str(), snapshot.as_of), bytes.as_slice())
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    async fn get_snapshot(
        &self,
        id: &CurveId,
        as_of: i64,
    ) -> Result<Option<CurveSnapshot>, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(CURVE_SNAPSHOTS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        match table.get((id.as_str(), as_of)) {
            Ok(Some(data)) => {
                let snapshot: CurveSnapshot = serde_json::from_slice(data.value())
                    .map_err(|e| TraitError::ParseError(e.to_string()))?;
                Ok(Some(snapshot))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(TraitError::DatabaseError(e.to_string())),
        }
    }

    async fn get_latest_snapshot(&self, id: &CurveId) -> Result<Option<CurveSnapshot>, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(CURVE_SNAPSHOTS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        // Find the latest snapshot for this curve
        let mut latest: Option<CurveSnapshot> = None;
        for result in table.iter().map_err(|e| TraitError::DatabaseError(e.to_string()))? {
            let (key, value) = result.map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            let (curve_id_str, as_of) = key.value();
            if curve_id_str == id.as_str() {
                let snapshot: CurveSnapshot = serde_json::from_slice(value.value())
                    .map_err(|e| TraitError::ParseError(e.to_string()))?;
                if latest.as_ref().map(|l| l.as_of < as_of).unwrap_or(true) {
                    latest = Some(snapshot);
                }
            }
        }
        Ok(latest)
    }

    async fn list_snapshots(
        &self,
        id: &CurveId,
        from: i64,
        to: i64,
    ) -> Result<Vec<CurveSnapshot>, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(CURVE_SNAPSHOTS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(vec![]),
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        let mut snapshots = Vec::new();
        for result in table.iter().map_err(|e| TraitError::DatabaseError(e.to_string()))? {
            let (key, value) = result.map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            let (curve_id_str, as_of) = key.value();
            if curve_id_str == id.as_str() && as_of >= from && as_of <= to {
                let snapshot: CurveSnapshot = serde_json::from_slice(value.value())
                    .map_err(|e| TraitError::ParseError(e.to_string()))?;
                snapshots.push(snapshot);
            }
        }
        snapshots.sort_by_key(|s| s.as_of);
        Ok(snapshots)
    }

    async fn delete_snapshots_before(&self, id: &CurveId, before: i64) -> Result<u64, TraitError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        // First, collect keys to delete (need owned values)
        let to_delete: Vec<(String, i64)> = {
            let table = write_txn
                .open_table(CURVE_SNAPSHOTS)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

            table
                .iter()
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?
                .filter_map(|r| r.ok())
                .filter_map(|(key, _)| {
                    let (curve_id_str, as_of) = key.value();
                    if curve_id_str == id.as_str() && as_of < before {
                        Some((curve_id_str.to_string(), as_of))
                    } else {
                        None
                    }
                })
                .collect()
        };

        let count = to_delete.len() as u64;

        // Now delete the entries
        {
            let mut table = write_txn
                .open_table(CURVE_SNAPSHOTS)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

            for (curve_id_str, as_of) in &to_delete {
                table
                    .remove((curve_id_str.as_str(), *as_of))
                    .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            }
        }

        write_txn
            .commit()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(count)
    }
}

/// Redb-based config store.
pub struct RedbConfigStore {
    db: Arc<Database>,
}

impl RedbConfigStore {
    /// Create a new redb config store.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ConfigStore for RedbConfigStore {
    async fn get(&self, id: &str) -> Result<Option<BondPricingConfig>, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(PRICING_CONFIGS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        match table.get(id) {
            Ok(Some(data)) => {
                let config: BondPricingConfig = serde_json::from_slice(data.value())
                    .map_err(|e| TraitError::ParseError(e.to_string()))?;
                Ok(Some(config))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(TraitError::DatabaseError(e.to_string())),
        }
    }

    async fn save(&self, config: &BondPricingConfig) -> Result<(), TraitError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(PRICING_CONFIGS)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

            let bytes = serde_json::to_vec(config)
                .map_err(|e| TraitError::SerializationError(e.to_string()))?;

            table
                .insert(config.config_id.as_str(), bytes.as_slice())
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<BondPricingConfig>, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(PRICING_CONFIGS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(vec![]),
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        let mut configs = Vec::new();
        for result in table.iter().map_err(|e| TraitError::DatabaseError(e.to_string()))? {
            let (_, value) = result.map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            let config: BondPricingConfig = serde_json::from_slice(value.value())
                .map_err(|e| TraitError::ParseError(e.to_string()))?;
            configs.push(config);
        }
        Ok(configs)
    }

    async fn delete(&self, id: &str) -> Result<bool, TraitError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        let deleted = {
            let mut table = write_txn
                .open_table(PRICING_CONFIGS)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            let result = table
                .remove(id)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            result.is_some()
        };
        write_txn
            .commit()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(deleted)
    }

    async fn get_version(
        &self,
        id: &str,
        _version: u64,
    ) -> Result<Option<BondPricingConfig>, TraitError> {
        // Simple implementation - no versioning
        self.get(id).await
    }

    async fn list_versions(&self, _id: &str) -> Result<Vec<ConfigVersion>, TraitError> {
        // Simple implementation - no versioning
        Ok(vec![])
    }
}

/// Redb-based override store.
pub struct RedbOverrideStore {
    db: Arc<Database>,
}

impl RedbOverrideStore {
    /// Create a new redb override store.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl OverrideStore for RedbOverrideStore {
    async fn get(&self, id: &InstrumentId) -> Result<Option<PriceOverride>, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(OVERRIDES) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        match table.get(id.as_str()) {
            Ok(Some(data)) => {
                let override_: PriceOverride = serde_json::from_slice(data.value())
                    .map_err(|e| TraitError::ParseError(e.to_string()))?;
                Ok(Some(override_))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(TraitError::DatabaseError(e.to_string())),
        }
    }

    async fn get_active(&self) -> Result<Vec<PriceOverride>, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(OVERRIDES) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(vec![]),
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let mut overrides = Vec::new();
        for result in table.iter().map_err(|e| TraitError::DatabaseError(e.to_string()))? {
            let (_, value) = result.map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            let override_: PriceOverride = serde_json::from_slice(value.value())
                .map_err(|e| TraitError::ParseError(e.to_string()))?;

            // Check if active (approved and not expired)
            if override_.is_approved && override_.expires_at.map(|e| e > now).unwrap_or(true) {
                overrides.push(override_);
            }
        }
        Ok(overrides)
    }

    async fn save(&self, override_: &PriceOverride) -> Result<(), TraitError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(OVERRIDES)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

            let bytes = serde_json::to_vec(override_)
                .map_err(|e| TraitError::SerializationError(e.to_string()))?;

            table
                .insert(override_.instrument_id.as_str(), bytes.as_slice())
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &InstrumentId) -> Result<bool, TraitError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        let deleted = {
            let mut table = write_txn
                .open_table(OVERRIDES)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            let result = table
                .remove(id.as_str())
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            result.is_some()
        };
        write_txn
            .commit()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(deleted)
    }

    async fn get_pending_approval(&self) -> Result<Vec<PriceOverride>, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(OVERRIDES) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(vec![]),
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        let mut overrides = Vec::new();
        for result in table.iter().map_err(|e| TraitError::DatabaseError(e.to_string()))? {
            let (_, value) = result.map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            let override_: PriceOverride = serde_json::from_slice(value.value())
                .map_err(|e| TraitError::ParseError(e.to_string()))?;
            if !override_.is_approved {
                overrides.push(override_);
            }
        }
        Ok(overrides)
    }

    async fn get_history(&self, _id: &InstrumentId) -> Result<Vec<OverrideAudit>, TraitError> {
        // Would need separate audit table
        Ok(vec![])
    }
}

/// Redb-based audit store.
pub struct RedbAuditStore {
    db: Arc<Database>,
}

impl RedbAuditStore {
    /// Create a new redb audit store.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AuditStore for RedbAuditStore {
    async fn append(&self, entry: &AuditEntry) -> Result<(), TraitError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(AUDIT)
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

            let bytes = serde_json::to_vec(entry)
                .map_err(|e| TraitError::SerializationError(e.to_string()))?;

            table
                .insert(entry.id, bytes.as_slice())
                .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    async fn query(
        &self,
        _filter: &AuditFilter,
        pagination: &Pagination,
    ) -> Result<Page<AuditEntry>, TraitError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TraitError::DatabaseError(e.to_string()))?;

        let table = match read_txn.open_table(AUDIT) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(Page {
                    items: vec![],
                    total: 0,
                    offset: pagination.offset,
                    limit: pagination.limit,
                })
            }
            Err(e) => return Err(TraitError::DatabaseError(e.to_string())),
        };

        let mut items = Vec::new();
        let mut total = 0u64;

        for result in table.iter().map_err(|e| TraitError::DatabaseError(e.to_string()))? {
            let (_, value) = result.map_err(|e| TraitError::DatabaseError(e.to_string()))?;
            total += 1;

            if total > pagination.offset as u64 && items.len() < pagination.limit {
                let entry: AuditEntry = serde_json::from_slice(value.value())
                    .map_err(|e| TraitError::ParseError(e.to_string()))?;
                items.push(entry);
            }
        }

        Ok(Page {
            items,
            total,
            offset: pagination.offset,
            limit: pagination.limit,
        })
    }
}

/// Create a full storage adapter with redb backend.
pub fn create_redb_storage(path: impl AsRef<Path>) -> Result<StorageAdapter, TraitError> {
    let db = Arc::new(
        Database::create(path).map_err(|e| TraitError::DatabaseError(e.to_string()))?,
    );

    Ok(StorageAdapter {
        bonds: Arc::new(RedbBondStore::new(db.clone())),
        curves: Arc::new(RedbCurveStore::new(db.clone())),
        configs: Arc::new(RedbConfigStore::new(db.clone())),
        overrides: Arc::new(RedbOverrideStore::new(db.clone())),
        audit: Arc::new(RedbAuditStore::new(db)),
    })
}

/// Create an in-memory storage adapter for testing.
///
/// Uses a temporary file that will be cleaned up when the process exits.
pub fn create_memory_storage() -> Result<StorageAdapter, TraitError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let temp_dir = std::env::temp_dir();
    let db_path = temp_dir.join(format!("convex_test_{}.redb", id));

    create_redb_storage(&db_path)
}
