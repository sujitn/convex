//! Storage adapter trait definition.
//!
//! This module defines the core `StorageAdapter` trait that all storage
//! backends must implement.

use crate::error::StorageResult;
use crate::types::{
    ConfigRecord, CurveSnapshot, QuoteRecord, SecurityMaster, TimeRange, Versioned,
};
use chrono::{DateTime, Utc};

/// Core storage adapter trait.
///
/// All storage backends (redb, in-memory, etc.) implement this trait.
/// The trait is designed to be async-ready but uses synchronous methods
/// for simplicity in the embedded database case.
///
/// # Example
///
/// ```rust,ignore
/// use convex_storage::{StorageAdapter, RedbStorage, SecurityMaster};
///
/// let storage = RedbStorage::open("./data.redb")?;
///
/// // Store a security
/// let security = SecurityMaster::builder("TEST001", "Test Corp").build();
/// storage.store_security(&security)?;
///
/// // Retrieve it
/// let retrieved = storage.get_security("TEST001")?;
/// ```
pub trait StorageAdapter: Send + Sync {
    /// Returns the backend name for logging/metrics.
    fn backend_name(&self) -> &'static str;

    /// Checks if the storage is healthy and accessible.
    fn is_healthy(&self) -> bool;

    // =========================================================================
    // SECURITY MASTER OPERATIONS
    // =========================================================================

    /// Stores a security master record.
    ///
    /// If a record with the same ID exists, it will be updated.
    fn store_security(&self, security: &SecurityMaster) -> StorageResult<()>;

    /// Retrieves a security by ID.
    fn get_security(&self, id: &str) -> StorageResult<Option<SecurityMaster>>;

    /// Deletes a security by ID.
    fn delete_security(&self, id: &str) -> StorageResult<bool>;

    /// Lists all securities, optionally filtered.
    fn list_securities(&self, filter: Option<&SecurityFilter>) -> StorageResult<Vec<SecurityMaster>>;

    /// Counts securities matching the filter.
    fn count_securities(&self, filter: Option<&SecurityFilter>) -> StorageResult<usize>;

    // =========================================================================
    // CURVE SNAPSHOT OPERATIONS
    // =========================================================================

    /// Stores a curve snapshot.
    fn store_curve_snapshot(&self, snapshot: &CurveSnapshot) -> StorageResult<()>;

    /// Retrieves the latest curve snapshot by name.
    fn get_curve_snapshot(&self, name: &str) -> StorageResult<Option<CurveSnapshot>>;

    /// Retrieves a curve snapshot at a specific time.
    fn get_curve_snapshot_at(
        &self,
        name: &str,
        as_of: DateTime<Utc>,
    ) -> StorageResult<Option<CurveSnapshot>>;

    /// Lists curve snapshot history for a curve.
    fn list_curve_snapshots(
        &self,
        name: &str,
        limit: usize,
    ) -> StorageResult<Vec<CurveSnapshot>>;

    /// Deletes old curve snapshots (retention policy).
    fn cleanup_curve_snapshots(
        &self,
        name: &str,
        keep_count: usize,
    ) -> StorageResult<usize>;

    // =========================================================================
    // QUOTE HISTORY OPERATIONS
    // =========================================================================

    /// Appends a quote record.
    fn append_quote(&self, quote: &QuoteRecord) -> StorageResult<()>;

    /// Appends multiple quote records in a batch.
    fn append_quotes(&self, quotes: &[QuoteRecord]) -> StorageResult<()>;

    /// Retrieves quotes for a security within a time range.
    fn get_quotes(
        &self,
        security_id: &str,
        range: &TimeRange,
    ) -> StorageResult<Vec<QuoteRecord>>;

    /// Retrieves the latest quote for a security.
    fn get_latest_quote(&self, security_id: &str) -> StorageResult<Option<QuoteRecord>>;

    /// Deletes old quotes (retention policy).
    fn cleanup_quotes(&self, older_than: DateTime<Utc>) -> StorageResult<usize>;

    // =========================================================================
    // CONFIGURATION OPERATIONS (WITH VERSIONING)
    // =========================================================================

    /// Stores a configuration record with versioning.
    fn store_config(&self, record: &ConfigRecord) -> StorageResult<()>;

    /// Retrieves the active configuration by key.
    fn get_config(&self, key: &str) -> StorageResult<Option<ConfigRecord>>;

    /// Retrieves a specific version of a configuration.
    fn get_config_version(&self, key: &str, version: u64) -> StorageResult<Option<ConfigRecord>>;

    /// Lists configuration history for a key.
    fn list_config_history(&self, key: &str, limit: usize) -> StorageResult<Vec<ConfigRecord>>;

    /// Lists all active configurations of a type.
    fn list_configs_by_type(&self, config_type: &str) -> StorageResult<Vec<ConfigRecord>>;

    /// Deletes a configuration (soft delete by marking inactive).
    fn delete_config(&self, key: &str) -> StorageResult<bool>;

    // =========================================================================
    // GENERIC VERSIONED STORAGE
    // =========================================================================

    /// Stores a versioned record with a given key.
    fn store_versioned<T: serde::Serialize>(
        &self,
        table: &str,
        key: &str,
        record: &Versioned<T>,
    ) -> StorageResult<()>;

    /// Retrieves the latest version of a record.
    fn get_versioned<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        key: &str,
    ) -> StorageResult<Option<Versioned<T>>>;

    /// Retrieves a specific version of a record.
    fn get_versioned_at<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        key: &str,
        version: u64,
    ) -> StorageResult<Option<Versioned<T>>>;

    // =========================================================================
    // MAINTENANCE OPERATIONS
    // =========================================================================

    /// Compacts the database (if supported).
    fn compact(&self) -> StorageResult<()>;

    /// Returns storage statistics.
    fn stats(&self) -> StorageResult<StorageStats>;
}

/// Filter for security queries.
#[derive(Debug, Clone, Default)]
pub struct SecurityFilter {
    /// Filter by currency.
    pub currency: Option<String>,
    /// Filter by issuer (partial match).
    pub issuer: Option<String>,
    /// Filter by sector.
    pub sector: Option<String>,
    /// Filter by rating.
    pub rating: Option<String>,
    /// Filter by security type.
    pub security_type: Option<String>,
    /// Filter by status.
    pub status: Option<String>,
    /// Maximum number of results.
    pub limit: Option<usize>,
    /// Offset for pagination.
    pub offset: Option<usize>,
}

impl SecurityFilter {
    /// Creates a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the currency filter.
    pub fn currency(mut self, currency: impl Into<String>) -> Self {
        self.currency = Some(currency.into());
        self
    }

    /// Sets the issuer filter.
    pub fn issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = Some(issuer.into());
        self
    }

    /// Sets the sector filter.
    pub fn sector(mut self, sector: impl Into<String>) -> Self {
        self.sector = Some(sector.into());
        self
    }

    /// Sets the limit.
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Sets the offset.
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }
}

/// Storage statistics.
#[derive(Debug, Clone, Default)]
pub struct StorageStats {
    /// Number of securities stored.
    pub security_count: usize,
    /// Number of curve snapshots stored.
    pub curve_snapshot_count: usize,
    /// Number of quote records stored.
    pub quote_count: usize,
    /// Number of configuration records stored.
    pub config_count: usize,
    /// Database file size in bytes (if applicable).
    pub file_size_bytes: Option<u64>,
    /// Free space in bytes (if applicable).
    pub free_space_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_filter_builder() {
        let filter = SecurityFilter::new()
            .currency("USD")
            .issuer("Apple")
            .sector("Technology")
            .limit(100);

        assert_eq!(filter.currency, Some("USD".to_string()));
        assert_eq!(filter.issuer, Some("Apple".to_string()));
        assert_eq!(filter.sector, Some("Technology".to_string()));
        assert_eq!(filter.limit, Some(100));
    }
}
