//! RedbStorage implementation.
//!
//! Implements the StorageAdapter trait using redb as the underlying database.

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};

use crate::adapter::{SecurityFilter, StorageAdapter, StorageStats};
use crate::error::StorageResult;
use crate::types::{
    ConfigRecord, CurveSnapshot, QuoteRecord, SecurityMaster, TimeRange, Versioned,
};

// Table definitions
const SECURITIES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("securities");
const CURVES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("curves");
const CURVE_HISTORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("curve_history");
const QUOTES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("quotes");
const CONFIGS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("configs");
const CONFIG_HISTORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("config_history");
const VERSIONED_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("versioned");

/// Redb-based storage adapter.
///
/// This adapter uses redb, a pure-Rust embedded database, for persistent storage.
/// It provides ACID transactions and is suitable for single-process applications.
///
/// # Example
///
/// ```rust,ignore
/// use convex_storage::{RedbStorage, StorageAdapter};
///
/// let storage = RedbStorage::open("./data.redb")?;
/// assert!(storage.is_healthy());
/// ```
pub struct RedbStorage {
    db: Arc<Database>,
}

impl RedbStorage {
    /// Opens or creates a database at the given path.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the database file
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or created.
    pub fn open<P: AsRef<Path>>(path: P) -> StorageResult<Self> {
        let db = Database::create(path)?;
        let storage = Self { db: Arc::new(db) };
        storage.initialize_tables()?;
        Ok(storage)
    }

    /// Creates a temporary in-memory database (for testing).
    ///
    /// Note: This still creates a file but uses a temp path.
    /// For true in-memory storage, use `InMemoryStorage` instead.
    #[cfg(test)]
    pub fn temp() -> StorageResult<Self> {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join(format!("convex_test_{}.redb", uuid::Uuid::new_v4()));
        Self::open(path)
    }

    /// Initializes all required tables.
    fn initialize_tables(&self) -> StorageResult<()> {
        let write_txn = self.db.begin_write()?;
        {
            // Create tables if they don't exist
            let _ = write_txn.open_table(SECURITIES_TABLE)?;
            let _ = write_txn.open_table(CURVES_TABLE)?;
            let _ = write_txn.open_table(CURVE_HISTORY_TABLE)?;
            let _ = write_txn.open_table(QUOTES_TABLE)?;
            let _ = write_txn.open_table(CONFIGS_TABLE)?;
            let _ = write_txn.open_table(CONFIG_HISTORY_TABLE)?;
            let _ = write_txn.open_table(VERSIONED_TABLE)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Creates a composite key for curve history.
    fn curve_history_key(name: &str, timestamp: DateTime<Utc>) -> String {
        format!("{}:{}", name, timestamp.timestamp_millis())
    }

    /// Creates a composite key for quote storage.
    fn quote_key(security_id: &str, timestamp: DateTime<Utc>) -> String {
        format!("{}:{}", security_id, timestamp.timestamp_millis())
    }

    /// Creates a composite key for config history.
    fn config_history_key(key: &str, version: u64) -> String {
        format!("{}:v{:010}", key, version)
    }

    /// Creates a composite key for versioned storage.
    fn versioned_key(table: &str, key: &str, version: u64) -> String {
        format!("{}:{}:v{:010}", table, key, version)
    }

    /// Creates a prefix for versioned storage queries.
    fn versioned_prefix(table: &str, key: &str) -> String {
        format!("{}:{}:", table, key)
    }
}

impl StorageAdapter for RedbStorage {
    fn backend_name(&self) -> &'static str {
        "redb"
    }

    fn is_healthy(&self) -> bool {
        // Try a simple read transaction to verify database is accessible
        self.db.begin_read().is_ok()
    }

    // =========================================================================
    // SECURITY MASTER OPERATIONS
    // =========================================================================

    fn store_security(&self, security: &SecurityMaster) -> StorageResult<()> {
        let data = serde_json::to_vec(security)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SECURITIES_TABLE)?;
            table.insert(security.id.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    fn get_security(&self, id: &str) -> StorageResult<Option<SecurityMaster>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SECURITIES_TABLE)?;
        match table.get(id)? {
            Some(data) => {
                let security: SecurityMaster = serde_json::from_slice(data.value())?;
                Ok(Some(security))
            }
            None => Ok(None),
        }
    }

    fn delete_security(&self, id: &str) -> StorageResult<bool> {
        let write_txn = self.db.begin_write()?;
        let deleted = {
            let mut table = write_txn.open_table(SECURITIES_TABLE)?;
            let result = table.remove(id)?;
            result.is_some()
        };
        write_txn.commit()?;
        Ok(deleted)
    }

    fn list_securities(
        &self,
        filter: Option<&SecurityFilter>,
    ) -> StorageResult<Vec<SecurityMaster>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SECURITIES_TABLE)?;

        let mut results = Vec::new();
        let offset = filter.and_then(|f| f.offset).unwrap_or(0);
        let limit = filter.and_then(|f| f.limit).unwrap_or(usize::MAX);

        for entry in table.iter()? {
            let (_, value) = entry?;
            let security: SecurityMaster = serde_json::from_slice(value.value())?;

            // Apply filters
            if let Some(f) = filter {
                if let Some(ref currency) = f.currency {
                    if security.currency.to_string() != *currency {
                        continue;
                    }
                }
                if let Some(ref issuer) = f.issuer {
                    if !security.issuer.to_lowercase().contains(&issuer.to_lowercase()) {
                        continue;
                    }
                }
                if let Some(ref sector) = f.sector {
                    if security.sector.as_deref() != Some(sector.as_str()) {
                        continue;
                    }
                }
                if let Some(ref rating) = f.rating {
                    if security.rating.as_deref() != Some(rating.as_str()) {
                        continue;
                    }
                }
                if let Some(ref security_type) = f.security_type {
                    if format!("{:?}", security.security_type) != *security_type {
                        continue;
                    }
                }
                if let Some(ref status) = f.status {
                    if format!("{:?}", security.status) != *status {
                        continue;
                    }
                }
            }

            results.push(security);
        }

        // Apply offset and limit
        let results: Vec<_> = results.into_iter().skip(offset).take(limit).collect();

        Ok(results)
    }

    fn count_securities(&self, filter: Option<&SecurityFilter>) -> StorageResult<usize> {
        // For now, use list_securities and count
        // Could be optimized with a dedicated counter
        Ok(self.list_securities(filter)?.len())
    }

    // =========================================================================
    // CURVE SNAPSHOT OPERATIONS
    // =========================================================================

    fn store_curve_snapshot(&self, snapshot: &CurveSnapshot) -> StorageResult<()> {
        let data = serde_json::to_vec(snapshot)?;
        let history_key = Self::curve_history_key(&snapshot.name, snapshot.build_time);

        let write_txn = self.db.begin_write()?;
        {
            // Store as latest
            let mut curves = write_txn.open_table(CURVES_TABLE)?;
            curves.insert(snapshot.name.as_str(), data.as_slice())?;

            // Store in history
            let mut history = write_txn.open_table(CURVE_HISTORY_TABLE)?;
            history.insert(history_key.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    fn get_curve_snapshot(&self, name: &str) -> StorageResult<Option<CurveSnapshot>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(CURVES_TABLE)?;
        match table.get(name)? {
            Some(data) => {
                let snapshot: CurveSnapshot = serde_json::from_slice(data.value())?;
                Ok(Some(snapshot))
            }
            None => Ok(None),
        }
    }

    fn get_curve_snapshot_at(
        &self,
        name: &str,
        as_of: DateTime<Utc>,
    ) -> StorageResult<Option<CurveSnapshot>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(CURVE_HISTORY_TABLE)?;

        let prefix = format!("{}:", name);
        let target_ts = as_of.timestamp_millis();

        let mut best_match: Option<CurveSnapshot> = None;
        let mut best_ts: i64 = i64::MIN;

        for entry in table.iter()? {
            let (key, value) = entry?;
            let key_str = key.value();

            if key_str.starts_with(&prefix) {
                if let Some(ts_str) = key_str.strip_prefix(&prefix) {
                    if let Ok(ts) = ts_str.parse::<i64>() {
                        // Find the latest snapshot before or at as_of
                        if ts <= target_ts && ts > best_ts {
                            best_ts = ts;
                            best_match = Some(serde_json::from_slice(value.value())?);
                        }
                    }
                }
            }
        }

        Ok(best_match)
    }

    fn list_curve_snapshots(&self, name: &str, limit: usize) -> StorageResult<Vec<CurveSnapshot>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(CURVE_HISTORY_TABLE)?;

        let prefix = format!("{}:", name);
        let mut snapshots = Vec::new();

        for entry in table.iter()? {
            let (key, value) = entry?;
            if key.value().starts_with(&prefix) {
                let snapshot: CurveSnapshot = serde_json::from_slice(value.value())?;
                snapshots.push(snapshot);
            }
        }

        // Sort by build_time descending and take limit
        snapshots.sort_by(|a, b| b.build_time.cmp(&a.build_time));
        snapshots.truncate(limit);

        Ok(snapshots)
    }

    fn cleanup_curve_snapshots(&self, name: &str, keep_count: usize) -> StorageResult<usize> {
        // Get all snapshots for this curve
        let snapshots = self.list_curve_snapshots(name, usize::MAX)?;

        if snapshots.len() <= keep_count {
            return Ok(0);
        }

        let to_delete: Vec<_> = snapshots.into_iter().skip(keep_count).collect();
        let delete_count = to_delete.len();

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(CURVE_HISTORY_TABLE)?;
            for snapshot in to_delete {
                let key = Self::curve_history_key(&snapshot.name, snapshot.build_time);
                table.remove(key.as_str())?;
            }
        }
        write_txn.commit()?;

        Ok(delete_count)
    }

    // =========================================================================
    // QUOTE HISTORY OPERATIONS
    // =========================================================================

    fn append_quote(&self, quote: &QuoteRecord) -> StorageResult<()> {
        let data = serde_json::to_vec(quote)?;
        let key = Self::quote_key(&quote.security_id, quote.timestamp);

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(QUOTES_TABLE)?;
            table.insert(key.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    fn append_quotes(&self, quotes: &[QuoteRecord]) -> StorageResult<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(QUOTES_TABLE)?;
            for quote in quotes {
                let data = serde_json::to_vec(quote)?;
                let key = Self::quote_key(&quote.security_id, quote.timestamp);
                table.insert(key.as_str(), data.as_slice())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    fn get_quotes(&self, security_id: &str, range: &TimeRange) -> StorageResult<Vec<QuoteRecord>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(QUOTES_TABLE)?;

        let prefix = format!("{}:", security_id);
        let from_ts = range.from.timestamp_millis();
        let to_ts = range.to.timestamp_millis();

        let mut quotes = Vec::new();

        for entry in table.iter()? {
            let (key, value) = entry?;
            let key_str = key.value();

            if key_str.starts_with(&prefix) {
                if let Some(ts_str) = key_str.strip_prefix(&prefix) {
                    if let Ok(ts) = ts_str.parse::<i64>() {
                        if ts >= from_ts && ts <= to_ts {
                            let quote: QuoteRecord = serde_json::from_slice(value.value())?;
                            quotes.push(quote);
                        }
                    }
                }
            }
        }

        // Sort by timestamp
        quotes.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        Ok(quotes)
    }

    fn get_latest_quote(&self, security_id: &str) -> StorageResult<Option<QuoteRecord>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(QUOTES_TABLE)?;

        let prefix = format!("{}:", security_id);
        let mut latest: Option<(i64, QuoteRecord)> = None;

        for entry in table.iter()? {
            let (key, value) = entry?;
            let key_str = key.value();

            if key_str.starts_with(&prefix) {
                if let Some(ts_str) = key_str.strip_prefix(&prefix) {
                    if let Ok(ts) = ts_str.parse::<i64>() {
                        if latest.is_none() || ts > latest.as_ref().unwrap().0 {
                            let quote: QuoteRecord = serde_json::from_slice(value.value())?;
                            latest = Some((ts, quote));
                        }
                    }
                }
            }
        }

        Ok(latest.map(|(_, q)| q))
    }

    fn cleanup_quotes(&self, older_than: DateTime<Utc>) -> StorageResult<usize> {
        let cutoff_ts = older_than.timestamp_millis();
        let mut to_delete = Vec::new();

        // First, collect keys to delete
        {
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(QUOTES_TABLE)?;

            for entry in table.iter()? {
                let (key, _) = entry?;
                let key_str = key.value();

                // Extract timestamp from key (format: "security_id:timestamp")
                if let Some(pos) = key_str.rfind(':') {
                    if let Ok(ts) = key_str[pos + 1..].parse::<i64>() {
                        if ts < cutoff_ts {
                            to_delete.push(key_str.to_string());
                        }
                    }
                }
            }
        }

        let count = to_delete.len();

        // Delete collected keys
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(QUOTES_TABLE)?;
            for key in to_delete {
                table.remove(key.as_str())?;
            }
        }
        write_txn.commit()?;

        Ok(count)
    }

    // =========================================================================
    // CONFIGURATION OPERATIONS
    // =========================================================================

    fn store_config(&self, record: &ConfigRecord) -> StorageResult<()> {
        let data = serde_json::to_vec(record)?;
        let history_key = Self::config_history_key(&record.key, record.version);

        let write_txn = self.db.begin_write()?;
        {
            // Store as current
            let mut configs = write_txn.open_table(CONFIGS_TABLE)?;
            configs.insert(record.key.as_str(), data.as_slice())?;

            // Store in history
            let mut history = write_txn.open_table(CONFIG_HISTORY_TABLE)?;
            history.insert(history_key.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    fn get_config(&self, key: &str) -> StorageResult<Option<ConfigRecord>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(CONFIGS_TABLE)?;
        match table.get(key)? {
            Some(data) => {
                let config: ConfigRecord = serde_json::from_slice(data.value())?;
                if config.is_active {
                    Ok(Some(config))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    fn get_config_version(&self, key: &str, version: u64) -> StorageResult<Option<ConfigRecord>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(CONFIG_HISTORY_TABLE)?;
        let history_key = Self::config_history_key(key, version);
        match table.get(history_key.as_str())? {
            Some(data) => {
                let config: ConfigRecord = serde_json::from_slice(data.value())?;
                Ok(Some(config))
            }
            None => Ok(None),
        }
    }

    fn list_config_history(&self, key: &str, limit: usize) -> StorageResult<Vec<ConfigRecord>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(CONFIG_HISTORY_TABLE)?;

        let prefix = format!("{}:", key);
        let mut configs = Vec::new();

        for entry in table.iter()? {
            let (k, value) = entry?;
            if k.value().starts_with(&prefix) {
                let config: ConfigRecord = serde_json::from_slice(value.value())?;
                configs.push(config);
            }
        }

        // Sort by version descending and take limit
        configs.sort_by(|a, b| b.version.cmp(&a.version));
        configs.truncate(limit);

        Ok(configs)
    }

    fn list_configs_by_type(&self, config_type: &str) -> StorageResult<Vec<ConfigRecord>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(CONFIGS_TABLE)?;

        let mut configs = Vec::new();

        for entry in table.iter()? {
            let (_, value) = entry?;
            let config: ConfigRecord = serde_json::from_slice(value.value())?;
            if config.is_active && config.config_type == config_type {
                configs.push(config);
            }
        }

        Ok(configs)
    }

    fn delete_config(&self, key: &str) -> StorageResult<bool> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(CONFIGS_TABLE)?;

        match table.get(key)? {
            Some(data) => {
                let mut config: ConfigRecord = serde_json::from_slice(data.value())?;
                if !config.is_active {
                    return Ok(false);
                }

                // Mark as inactive
                config.is_active = false;
                config.version += 1;
                config.updated_at = Utc::now();
                drop(read_txn);

                self.store_config(&config)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    // =========================================================================
    // GENERIC VERSIONED STORAGE
    // =========================================================================

    fn store_versioned<T: serde::Serialize>(
        &self,
        table: &str,
        key: &str,
        record: &Versioned<T>,
    ) -> StorageResult<()> {
        let data = serde_json::to_vec(record)?;
        let storage_key = Self::versioned_key(table, key, record.version);

        let write_txn = self.db.begin_write()?;
        {
            let mut tbl = write_txn.open_table(VERSIONED_TABLE)?;
            tbl.insert(storage_key.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    fn get_versioned<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        key: &str,
    ) -> StorageResult<Option<Versioned<T>>> {
        let read_txn = self.db.begin_read()?;
        let tbl = read_txn.open_table(VERSIONED_TABLE)?;

        let prefix = Self::versioned_prefix(table, key);
        let mut latest: Option<(u64, Versioned<T>)> = None;

        for entry in tbl.iter()? {
            let (k, value) = entry?;
            let key_str = k.value();

            if key_str.starts_with(&prefix) {
                let record: Versioned<T> = serde_json::from_slice(value.value())?;
                if latest.is_none() || record.version > latest.as_ref().unwrap().0 {
                    latest = Some((record.version, record));
                }
            }
        }

        Ok(latest.map(|(_, r)| r))
    }

    fn get_versioned_at<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        key: &str,
        version: u64,
    ) -> StorageResult<Option<Versioned<T>>> {
        let read_txn = self.db.begin_read()?;
        let tbl = read_txn.open_table(VERSIONED_TABLE)?;
        let storage_key = Self::versioned_key(table, key, version);

        match tbl.get(storage_key.as_str())? {
            Some(data) => {
                let record: Versioned<T> = serde_json::from_slice(data.value())?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }

    // =========================================================================
    // MAINTENANCE OPERATIONS
    // =========================================================================

    fn compact(&self) -> StorageResult<()> {
        // Note: redb's compact() requires &mut self, which conflicts with Arc.
        // Compaction is performed automatically by redb during normal operations.
        // For manual compaction, create a new RedbStorage without sharing.
        Ok(())
    }

    fn stats(&self) -> StorageResult<StorageStats> {
        let read_txn = self.db.begin_read()?;

        let security_count = {
            let table = read_txn.open_table(SECURITIES_TABLE)?;
            table.len()? as usize
        };

        let curve_snapshot_count = {
            let table = read_txn.open_table(CURVE_HISTORY_TABLE)?;
            table.len()? as usize
        };

        let quote_count = {
            let table = read_txn.open_table(QUOTES_TABLE)?;
            table.len()? as usize
        };

        let config_count = {
            let table = read_txn.open_table(CONFIGS_TABLE)?;
            table.len()? as usize
        };

        Ok(StorageStats {
            security_count,
            curve_snapshot_count,
            quote_count,
            config_count,
            file_size_bytes: None, // Would need file system access
            free_space_bytes: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CurvePoint, QuoteCondition};
    use convex_core::types::Currency;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn create_test_storage() -> RedbStorage {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.redb");
        // Keep the tempdir alive by leaking it (for tests only)
        std::mem::forget(dir);
        RedbStorage::open(path).unwrap()
    }

    #[test]
    fn test_backend_name() {
        let storage = create_test_storage();
        assert_eq!(storage.backend_name(), "redb");
    }

    #[test]
    fn test_is_healthy() {
        let storage = create_test_storage();
        assert!(storage.is_healthy());
    }

    #[test]
    fn test_security_crud() {
        let storage = create_test_storage();

        // Create
        let security = SecurityMaster::builder("TEST001", "Test Issuer")
            .isin("US1234567890")
            .currency(Currency::USD)
            .coupon_rate(dec!(0.05))
            .build();

        storage.store_security(&security).unwrap();

        // Read
        let retrieved = storage.get_security("TEST001").unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, "TEST001");
        assert_eq!(retrieved.issuer, "Test Issuer");

        // Delete
        let deleted = storage.delete_security("TEST001").unwrap();
        assert!(deleted);

        // Verify deleted
        let retrieved = storage.get_security("TEST001").unwrap();
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_list_securities() {
        let storage = create_test_storage();

        // Add multiple securities
        for i in 1..=5 {
            let security = SecurityMaster::builder(format!("SEC{:03}", i), "Test Issuer")
                .currency(Currency::USD)
                .sector("Technology")
                .build();
            storage.store_security(&security).unwrap();
        }

        // List all
        let all = storage.list_securities(None).unwrap();
        assert_eq!(all.len(), 5);

        // List with sector filter
        let filter = SecurityFilter::new().sector("Technology");
        let filtered = storage.list_securities(Some(&filter)).unwrap();
        assert_eq!(filtered.len(), 5);

        // List with limit
        let filter = SecurityFilter::new().limit(3);
        let limited = storage.list_securities(Some(&filter)).unwrap();
        assert_eq!(limited.len(), 3);
    }

    #[test]
    fn test_curve_snapshot_crud() {
        let storage = create_test_storage();

        let snapshot = CurveSnapshot {
            name: "USD.SOFR".to_string(),
            reference_date: "2024-01-15".to_string(),
            build_time: Utc::now(),
            build_duration_us: 1234,
            points: vec![
                CurvePoint {
                    tenor: "1Y".to_string(),
                    years: 1.0,
                    zero_rate: 0.045,
                    discount_factor: 0.956,
                    forward_rate: Some(0.046),
                },
            ],
            inputs: vec![],
            interpolation: "MonotoneConvex".to_string(),
            day_count: "ACT/360".to_string(),
            compounding: convex_core::types::Compounding::Continuous,
            checksum: "abc123".to_string(),
            metadata: HashMap::new(),
        };

        storage.store_curve_snapshot(&snapshot).unwrap();

        // Retrieve latest
        let retrieved = storage.get_curve_snapshot("USD.SOFR").unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.name, "USD.SOFR");
        assert_eq!(retrieved.points.len(), 1);
    }

    #[test]
    fn test_quote_operations() {
        let storage = create_test_storage();

        let quote = QuoteRecord {
            security_id: "SEC001".to_string(),
            timestamp: Utc::now(),
            source: "BLOOMBERG".to_string(),
            bid: Some(dec!(99.50)),
            ask: Some(dec!(99.75)),
            mid: Some(dec!(99.625)),
            last: Some(dec!(99.60)),
            bid_size: None,
            ask_size: None,
            ytm: Some(0.045),
            z_spread: Some(0.0025),
            condition: QuoteCondition::Firm,
        };

        storage.append_quote(&quote).unwrap();

        let latest = storage.get_latest_quote("SEC001").unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().source, "BLOOMBERG");
    }

    #[test]
    fn test_stats() {
        let storage = create_test_storage();

        let stats = storage.stats().unwrap();
        assert_eq!(stats.security_count, 0);
        assert_eq!(stats.quote_count, 0);
    }
}
