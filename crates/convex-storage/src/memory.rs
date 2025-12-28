//! In-memory storage adapter.
//!
//! Provides a simple in-memory implementation of the StorageAdapter trait.
//! Useful for testing and development. Data is not persisted across restarts.

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{DateTime, Utc};

use crate::adapter::{SecurityFilter, StorageAdapter, StorageStats};
use crate::error::{StorageError, StorageResult};
use crate::types::{
    ConfigRecord, CurveSnapshot, QuoteRecord, SecurityMaster, TimeRange, Versioned,
};

/// In-memory storage adapter.
///
/// This adapter stores all data in memory using standard collections.
/// It's thread-safe through the use of RwLock.
///
/// # Example
///
/// ```rust
/// use convex_storage::{InMemoryStorage, StorageAdapter};
///
/// let storage = InMemoryStorage::new();
/// assert!(storage.is_healthy());
/// ```
pub struct InMemoryStorage {
    securities: RwLock<HashMap<String, SecurityMaster>>,
    curves: RwLock<HashMap<String, CurveSnapshot>>,
    curve_history: RwLock<HashMap<String, Vec<CurveSnapshot>>>,
    quotes: RwLock<HashMap<String, Vec<QuoteRecord>>>,
    configs: RwLock<HashMap<String, ConfigRecord>>,
    config_history: RwLock<HashMap<String, Vec<ConfigRecord>>>,
    versioned: RwLock<HashMap<String, Vec<u8>>>,
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryStorage {
    /// Creates a new empty in-memory storage.
    pub fn new() -> Self {
        Self {
            securities: RwLock::new(HashMap::new()),
            curves: RwLock::new(HashMap::new()),
            curve_history: RwLock::new(HashMap::new()),
            quotes: RwLock::new(HashMap::new()),
            configs: RwLock::new(HashMap::new()),
            config_history: RwLock::new(HashMap::new()),
            versioned: RwLock::new(HashMap::new()),
        }
    }

    /// Clears all data from storage.
    pub fn clear(&self) {
        self.securities.write().unwrap().clear();
        self.curves.write().unwrap().clear();
        self.curve_history.write().unwrap().clear();
        self.quotes.write().unwrap().clear();
        self.configs.write().unwrap().clear();
        self.config_history.write().unwrap().clear();
        self.versioned.write().unwrap().clear();
    }

    /// Creates a versioned storage key.
    fn versioned_key(table: &str, key: &str) -> String {
        format!("{}:{}", table, key)
    }
}

impl StorageAdapter for InMemoryStorage {
    fn backend_name(&self) -> &'static str {
        "memory"
    }

    fn is_healthy(&self) -> bool {
        true
    }

    // =========================================================================
    // SECURITY MASTER OPERATIONS
    // =========================================================================

    fn store_security(&self, security: &SecurityMaster) -> StorageResult<()> {
        self.securities
            .write()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .insert(security.id.clone(), security.clone());
        Ok(())
    }

    fn get_security(&self, id: &str) -> StorageResult<Option<SecurityMaster>> {
        Ok(self
            .securities
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .get(id)
            .cloned())
    }

    fn delete_security(&self, id: &str) -> StorageResult<bool> {
        Ok(self
            .securities
            .write()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .remove(id)
            .is_some())
    }

    fn list_securities(
        &self,
        filter: Option<&SecurityFilter>,
    ) -> StorageResult<Vec<SecurityMaster>> {
        let securities = self
            .securities
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        let offset = filter.and_then(|f| f.offset).unwrap_or(0);
        let limit = filter.and_then(|f| f.limit).unwrap_or(usize::MAX);

        let results: Vec<_> = securities
            .values()
            .filter(|s| {
                if let Some(f) = filter {
                    if let Some(ref currency) = f.currency {
                        if s.currency.to_string() != *currency {
                            return false;
                        }
                    }
                    if let Some(ref issuer) = f.issuer {
                        if !s.issuer.to_lowercase().contains(&issuer.to_lowercase()) {
                            return false;
                        }
                    }
                    if let Some(ref sector) = f.sector {
                        if s.sector.as_deref() != Some(sector.as_str()) {
                            return false;
                        }
                    }
                    if let Some(ref rating) = f.rating {
                        if s.rating.as_deref() != Some(rating.as_str()) {
                            return false;
                        }
                    }
                    if let Some(ref security_type) = f.security_type {
                        if format!("{:?}", s.security_type) != *security_type {
                            return false;
                        }
                    }
                    if let Some(ref status) = f.status {
                        if format!("{:?}", s.status) != *status {
                            return false;
                        }
                    }
                }
                true
            })
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();

        Ok(results)
    }

    fn count_securities(&self, filter: Option<&SecurityFilter>) -> StorageResult<usize> {
        Ok(self.list_securities(filter)?.len())
    }

    // =========================================================================
    // CURVE SNAPSHOT OPERATIONS
    // =========================================================================

    fn store_curve_snapshot(&self, snapshot: &CurveSnapshot) -> StorageResult<()> {
        // Store as latest
        self.curves
            .write()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .insert(snapshot.name.clone(), snapshot.clone());

        // Store in history
        self.curve_history
            .write()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .entry(snapshot.name.clone())
            .or_default()
            .push(snapshot.clone());

        Ok(())
    }

    fn get_curve_snapshot(&self, name: &str) -> StorageResult<Option<CurveSnapshot>> {
        Ok(self
            .curves
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .get(name)
            .cloned())
    }

    fn get_curve_snapshot_at(
        &self,
        name: &str,
        as_of: DateTime<Utc>,
    ) -> StorageResult<Option<CurveSnapshot>> {
        let history = self
            .curve_history
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        if let Some(snapshots) = history.get(name) {
            // Find the latest snapshot before or at as_of
            let result = snapshots
                .iter()
                .filter(|s| s.build_time <= as_of)
                .max_by_key(|s| s.build_time)
                .cloned();
            Ok(result)
        } else {
            Ok(None)
        }
    }

    fn list_curve_snapshots(&self, name: &str, limit: usize) -> StorageResult<Vec<CurveSnapshot>> {
        let history = self
            .curve_history
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        if let Some(snapshots) = history.get(name) {
            let mut sorted: Vec<_> = snapshots.clone();
            sorted.sort_by(|a, b| b.build_time.cmp(&a.build_time));
            sorted.truncate(limit);
            Ok(sorted)
        } else {
            Ok(vec![])
        }
    }

    fn cleanup_curve_snapshots(&self, name: &str, keep_count: usize) -> StorageResult<usize> {
        let mut history = self
            .curve_history
            .write()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        if let Some(snapshots) = history.get_mut(name) {
            if snapshots.len() <= keep_count {
                return Ok(0);
            }

            // Sort by build_time descending
            snapshots.sort_by(|a, b| b.build_time.cmp(&a.build_time));

            let delete_count = snapshots.len() - keep_count;
            snapshots.truncate(keep_count);

            Ok(delete_count)
        } else {
            Ok(0)
        }
    }

    // =========================================================================
    // QUOTE HISTORY OPERATIONS
    // =========================================================================

    fn append_quote(&self, quote: &QuoteRecord) -> StorageResult<()> {
        self.quotes
            .write()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .entry(quote.security_id.clone())
            .or_default()
            .push(quote.clone());
        Ok(())
    }

    fn append_quotes(&self, quotes: &[QuoteRecord]) -> StorageResult<()> {
        let mut storage = self
            .quotes
            .write()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        for quote in quotes {
            storage
                .entry(quote.security_id.clone())
                .or_default()
                .push(quote.clone());
        }
        Ok(())
    }

    fn get_quotes(&self, security_id: &str, range: &TimeRange) -> StorageResult<Vec<QuoteRecord>> {
        let quotes = self
            .quotes
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        if let Some(security_quotes) = quotes.get(security_id) {
            let mut filtered: Vec<_> = security_quotes
                .iter()
                .filter(|q| range.contains(q.timestamp))
                .cloned()
                .collect();
            filtered.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
            Ok(filtered)
        } else {
            Ok(vec![])
        }
    }

    fn get_latest_quote(&self, security_id: &str) -> StorageResult<Option<QuoteRecord>> {
        let quotes = self
            .quotes
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        if let Some(security_quotes) = quotes.get(security_id) {
            Ok(security_quotes.iter().max_by_key(|q| q.timestamp).cloned())
        } else {
            Ok(None)
        }
    }

    fn cleanup_quotes(&self, older_than: DateTime<Utc>) -> StorageResult<usize> {
        let mut quotes = self
            .quotes
            .write()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        let mut total_deleted = 0;

        for security_quotes in quotes.values_mut() {
            let original_len = security_quotes.len();
            security_quotes.retain(|q| q.timestamp >= older_than);
            total_deleted += original_len - security_quotes.len();
        }

        Ok(total_deleted)
    }

    // =========================================================================
    // CONFIGURATION OPERATIONS
    // =========================================================================

    fn store_config(&self, record: &ConfigRecord) -> StorageResult<()> {
        // Store as current
        self.configs
            .write()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .insert(record.key.clone(), record.clone());

        // Store in history
        self.config_history
            .write()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .entry(record.key.clone())
            .or_default()
            .push(record.clone());

        Ok(())
    }

    fn get_config(&self, key: &str) -> StorageResult<Option<ConfigRecord>> {
        let configs = self
            .configs
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        if let Some(config) = configs.get(key) {
            if config.is_active {
                Ok(Some(config.clone()))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    fn get_config_version(&self, key: &str, version: u64) -> StorageResult<Option<ConfigRecord>> {
        let history = self
            .config_history
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        if let Some(configs) = history.get(key) {
            Ok(configs.iter().find(|c| c.version == version).cloned())
        } else {
            Ok(None)
        }
    }

    fn list_config_history(&self, key: &str, limit: usize) -> StorageResult<Vec<ConfigRecord>> {
        let history = self
            .config_history
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        if let Some(configs) = history.get(key) {
            let mut sorted: Vec<_> = configs.clone();
            sorted.sort_by(|a, b| b.version.cmp(&a.version));
            sorted.truncate(limit);
            Ok(sorted)
        } else {
            Ok(vec![])
        }
    }

    fn list_configs_by_type(&self, config_type: &str) -> StorageResult<Vec<ConfigRecord>> {
        let configs = self
            .configs
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        Ok(configs
            .values()
            .filter(|c| c.is_active && c.config_type == config_type)
            .cloned()
            .collect())
    }

    fn delete_config(&self, key: &str) -> StorageResult<bool> {
        let mut configs = self
            .configs
            .write()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        if let Some(config) = configs.get_mut(key) {
            if !config.is_active {
                return Ok(false);
            }

            let mut updated = config.clone();
            updated.is_active = false;
            updated.version += 1;
            updated.updated_at = Utc::now();

            *config = updated.clone();
            drop(configs);

            // Store in history
            self.config_history
                .write()
                .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
                .entry(key.to_string())
                .or_default()
                .push(updated);

            Ok(true)
        } else {
            Ok(false)
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
        let storage_key = format!("{}:{}:v{}", table, key, record.version);

        self.versioned
            .write()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .insert(storage_key, data);

        Ok(())
    }

    fn get_versioned<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        key: &str,
    ) -> StorageResult<Option<Versioned<T>>> {
        let storage = self
            .versioned
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        let prefix = Self::versioned_key(table, key);
        let mut latest: Option<Versioned<T>> = None;

        for (k, data) in storage.iter() {
            if k.starts_with(&prefix) {
                let record: Versioned<T> = serde_json::from_slice(data)?;
                if latest.is_none() || record.version > latest.as_ref().unwrap().version {
                    latest = Some(record);
                }
            }
        }

        Ok(latest)
    }

    fn get_versioned_at<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        key: &str,
        version: u64,
    ) -> StorageResult<Option<Versioned<T>>> {
        let storage = self
            .versioned
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?;

        let storage_key = format!("{}:{}:v{}", table, key, version);

        if let Some(data) = storage.get(&storage_key) {
            let record: Versioned<T> = serde_json::from_slice(data)?;
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    // =========================================================================
    // MAINTENANCE OPERATIONS
    // =========================================================================

    fn compact(&self) -> StorageResult<()> {
        // No-op for in-memory storage
        Ok(())
    }

    fn stats(&self) -> StorageResult<StorageStats> {
        let security_count = self
            .securities
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .len();

        let curve_snapshot_count: usize = self
            .curve_history
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .values()
            .map(|v| v.len())
            .sum();

        let quote_count: usize = self
            .quotes
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .values()
            .map(|v| v.len())
            .sum();

        let config_count = self
            .configs
            .read()
            .map_err(|e| StorageError::Database(format!("Lock error: {}", e)))?
            .len();

        Ok(StorageStats {
            security_count,
            curve_snapshot_count,
            quote_count,
            config_count,
            file_size_bytes: None,
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

    #[test]
    fn test_in_memory_backend_name() {
        let storage = InMemoryStorage::new();
        assert_eq!(storage.backend_name(), "memory");
    }

    #[test]
    fn test_in_memory_is_healthy() {
        let storage = InMemoryStorage::new();
        assert!(storage.is_healthy());
    }

    #[test]
    fn test_in_memory_security_crud() {
        let storage = InMemoryStorage::new();

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
        assert_eq!(retrieved.unwrap().id, "TEST001");

        // Delete
        let deleted = storage.delete_security("TEST001").unwrap();
        assert!(deleted);

        // Verify deleted
        assert!(storage.get_security("TEST001").unwrap().is_none());
    }

    #[test]
    fn test_in_memory_curve_snapshot() {
        let storage = InMemoryStorage::new();

        let snapshot = CurveSnapshot {
            name: "USD.SOFR".to_string(),
            reference_date: "2024-01-15".to_string(),
            build_time: Utc::now(),
            build_duration_us: 1234,
            points: vec![CurvePoint {
                tenor: "1Y".to_string(),
                years: 1.0,
                zero_rate: 0.045,
                discount_factor: 0.956,
                forward_rate: Some(0.046),
            }],
            inputs: vec![],
            interpolation: "MonotoneConvex".to_string(),
            day_count: "ACT/360".to_string(),
            compounding: convex_core::types::Compounding::Continuous,
            checksum: "abc123".to_string(),
            metadata: HashMap::new(),
        };

        storage.store_curve_snapshot(&snapshot).unwrap();

        let retrieved = storage.get_curve_snapshot("USD.SOFR").unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "USD.SOFR");
    }

    #[test]
    fn test_in_memory_quotes() {
        let storage = InMemoryStorage::new();

        let quote = QuoteRecord {
            security_id: "SEC001".to_string(),
            timestamp: Utc::now(),
            source: "TEST".to_string(),
            bid: Some(dec!(99.50)),
            ask: Some(dec!(99.75)),
            mid: Some(dec!(99.625)),
            last: None,
            bid_size: None,
            ask_size: None,
            ytm: None,
            z_spread: None,
            condition: QuoteCondition::Firm,
        };

        storage.append_quote(&quote).unwrap();

        let latest = storage.get_latest_quote("SEC001").unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().source, "TEST");
    }

    #[test]
    fn test_in_memory_clear() {
        let storage = InMemoryStorage::new();

        let security = SecurityMaster::builder("TEST001", "Test Issuer").build();
        storage.store_security(&security).unwrap();

        assert!(storage.get_security("TEST001").unwrap().is_some());

        storage.clear();

        assert!(storage.get_security("TEST001").unwrap().is_none());
    }

    #[test]
    fn test_in_memory_stats() {
        let storage = InMemoryStorage::new();

        let stats = storage.stats().unwrap();
        assert_eq!(stats.security_count, 0);
        assert_eq!(stats.quote_count, 0);
    }
}
