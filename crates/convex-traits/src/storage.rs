//! Storage traits for persistence.
//!
//! These traits define interfaces for storage backends:
//! - [`BondStore`]: Bond reference data storage
//! - [`CurveStore`]: Curve config and snapshot storage
//! - [`ConfigStore`]: Pricing configuration storage
//! - [`OverrideStore`]: Price override storage with audit
//! - [`AuditStore`]: Audit log storage
//!
//! Storage implementations are EXTENSIONS (e.g., redb, PostgreSQL, Redis).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::TraitError;
use crate::ids::*;
use crate::reference_data::BondReferenceData;

// =============================================================================
// PAGINATION
// =============================================================================

/// Pagination parameters.
#[derive(Debug, Clone, Default)]
pub struct Pagination {
    /// Number of items to skip
    pub offset: usize,
    /// Maximum items to return
    pub limit: usize,
}

impl Pagination {
    /// Create new pagination.
    pub fn new(offset: usize, limit: usize) -> Self {
        Self { offset, limit }
    }
}

/// Paginated result.
#[derive(Debug, Clone)]
pub struct Page<T> {
    /// Items in this page
    pub items: Vec<T>,
    /// Total number of items (across all pages)
    pub total: u64,
    /// Current offset
    pub offset: usize,
    /// Page size limit
    pub limit: usize,
}

impl<T> Page<T> {
    /// Check if there are more pages.
    pub fn has_more(&self) -> bool {
        (self.offset + self.items.len()) < self.total as usize
    }
}

// =============================================================================
// BOND STORE
// =============================================================================

/// Bond filter for storage queries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BondFilter {
    /// Currency filter
    pub currency: Option<convex_core::Currency>,
    /// Maturity from
    pub maturity_from: Option<convex_core::Date>,
    /// Maturity to
    pub maturity_to: Option<convex_core::Date>,
    /// Issuer ID
    pub issuer_id: Option<String>,
    /// Is callable
    pub is_callable: Option<bool>,
    /// Text search
    pub text_search: Option<String>,
}

/// Bond reference data storage.
#[async_trait]
pub trait BondStore: Send + Sync {
    /// Get bond by ID.
    async fn get(&self, id: &InstrumentId) -> Result<Option<BondReferenceData>, TraitError>;

    /// Get multiple bonds by ID.
    async fn get_many(&self, ids: &[InstrumentId]) -> Result<Vec<BondReferenceData>, TraitError>;

    /// Save a bond.
    async fn save(&self, bond: &BondReferenceData) -> Result<(), TraitError>;

    /// Save multiple bonds.
    async fn save_batch(&self, bonds: &[BondReferenceData]) -> Result<(), TraitError>;

    /// Delete a bond.
    async fn delete(&self, id: &InstrumentId) -> Result<bool, TraitError>;

    /// List bonds with filter and pagination.
    async fn list(
        &self,
        filter: &BondFilter,
        pagination: &Pagination,
    ) -> Result<Page<BondReferenceData>, TraitError>;

    /// Count bonds matching filter.
    async fn count(&self, filter: &BondFilter) -> Result<u64, TraitError>;

    /// Search bonds by text query.
    async fn search(&self, query: &str, limit: usize)
        -> Result<Vec<BondReferenceData>, TraitError>;
}

// =============================================================================
// CURVE STORE
// =============================================================================

/// Curve configuration (what instruments to use for building).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveConfig {
    /// Curve identifier
    pub curve_id: CurveId,
    /// Curve name
    pub name: String,
    /// Currency
    pub currency: convex_core::Currency,
    /// Interpolation method
    pub interpolation: String,
    /// Day count convention
    pub day_count: String,
    /// Instrument tenors (e.g., ["1M", "3M", "1Y", "5Y", "10Y"])
    pub tenors: Vec<String>,
    /// Build schedule
    pub build_schedule: String,
    /// Last updated
    pub last_updated: i64,
}

/// Curve snapshot (built curve at a point in time).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveSnapshot {
    /// Curve identifier
    pub curve_id: CurveId,
    /// As-of timestamp
    pub as_of: i64,
    /// Curve points (tenor days -> zero rate)
    pub points: Vec<(u32, f64)>,
    /// Inputs hash (for change detection)
    pub inputs_hash: String,
    /// Build duration (ms)
    pub build_duration_ms: u64,
}

/// Curve storage (configs and snapshots).
#[async_trait]
pub trait CurveStore: Send + Sync {
    // Config methods

    /// Get curve config by ID.
    async fn get_config(&self, id: &CurveId) -> Result<Option<CurveConfig>, TraitError>;

    /// Save curve config.
    async fn save_config(&self, config: &CurveConfig) -> Result<(), TraitError>;

    /// List all curve configs.
    async fn list_configs(&self) -> Result<Vec<CurveConfig>, TraitError>;

    /// Delete curve config.
    async fn delete_config(&self, id: &CurveId) -> Result<bool, TraitError>;

    // Snapshot methods

    /// Save curve snapshot.
    async fn save_snapshot(&self, snapshot: &CurveSnapshot) -> Result<(), TraitError>;

    /// Get curve snapshot at a specific time.
    async fn get_snapshot(
        &self,
        id: &CurveId,
        as_of: i64,
    ) -> Result<Option<CurveSnapshot>, TraitError>;

    /// Get latest curve snapshot.
    async fn get_latest_snapshot(&self, id: &CurveId) -> Result<Option<CurveSnapshot>, TraitError>;

    /// List snapshots in time range.
    async fn list_snapshots(
        &self,
        id: &CurveId,
        from: i64,
        to: i64,
    ) -> Result<Vec<CurveSnapshot>, TraitError>;

    /// Delete snapshots before a timestamp.
    async fn delete_snapshots_before(&self, id: &CurveId, before: i64) -> Result<u64, TraitError>;
}

// =============================================================================
// CONFIG STORE
// =============================================================================

/// Bond pricing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondPricingConfig {
    /// Config identifier
    pub config_id: String,
    /// Description
    pub description: String,
    /// Applies to (filter)
    pub applies_to: BondFilter,
    /// Benchmark curve ID
    pub benchmark_curve: CurveId,
    /// Discount curve ID
    pub discount_curve: CurveId,
    /// Z-spread curve ID
    pub z_spread_curve: Option<CurveId>,
    /// OAS curve ID
    pub oas_curve: Option<CurveId>,
    /// ASW swap curve ID
    pub asw_swap_curve: Option<CurveId>,
    /// FRN projection curve ID
    pub frn_projection_curve: Option<CurveId>,
    /// OAS vol surface ID
    pub oas_vol_surface: Option<String>,
    /// OAS mean reversion parameter
    pub oas_mean_reversion: Option<f64>,
    /// OAS tree steps
    pub oas_tree_steps: Option<u32>,
    /// Default spread type
    pub default_spread_type: String,
    /// Version
    pub version: u64,
    /// Created at
    pub created_at: i64,
    /// Updated at
    pub updated_at: i64,
}

/// Config version info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigVersion {
    /// Version number
    pub version: u64,
    /// Timestamp
    pub timestamp: i64,
    /// Changed by
    pub changed_by: Option<String>,
    /// Change description
    pub change_description: Option<String>,
}

/// Pricing configuration storage.
#[async_trait]
pub trait ConfigStore: Send + Sync {
    /// Get config by ID.
    async fn get(&self, id: &str) -> Result<Option<BondPricingConfig>, TraitError>;

    /// Save config.
    async fn save(&self, config: &BondPricingConfig) -> Result<(), TraitError>;

    /// List all configs.
    async fn list(&self) -> Result<Vec<BondPricingConfig>, TraitError>;

    /// Delete config.
    async fn delete(&self, id: &str) -> Result<bool, TraitError>;

    /// Get specific version of config.
    async fn get_version(
        &self,
        id: &str,
        version: u64,
    ) -> Result<Option<BondPricingConfig>, TraitError>;

    /// List all versions of a config.
    async fn list_versions(&self, id: &str) -> Result<Vec<ConfigVersion>, TraitError>;
}

// =============================================================================
// OVERRIDE STORE
// =============================================================================

/// Price override entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceOverride {
    /// Instrument ID
    pub instrument_id: InstrumentId,
    /// Override price
    pub price: Option<f64>,
    /// Override yield
    pub yield_value: Option<f64>,
    /// Override spread
    pub spread: Option<f64>,
    /// Reason
    pub reason: String,
    /// Created by
    pub created_by: String,
    /// Created at
    pub created_at: i64,
    /// Expires at
    pub expires_at: Option<i64>,
    /// Is approved
    pub is_approved: bool,
    /// Approved by
    pub approved_by: Option<String>,
    /// Approved at
    pub approved_at: Option<i64>,
}

/// Override audit entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideAudit {
    /// Instrument ID
    pub instrument_id: InstrumentId,
    /// Action (create, update, delete, approve, reject)
    pub action: String,
    /// User who performed action
    pub user: String,
    /// Timestamp
    pub timestamp: i64,
    /// Previous value (JSON)
    pub previous_value: Option<String>,
    /// New value (JSON)
    pub new_value: Option<String>,
}

/// Override storage with audit.
#[async_trait]
pub trait OverrideStore: Send + Sync {
    /// Get override for instrument.
    async fn get(&self, id: &InstrumentId) -> Result<Option<PriceOverride>, TraitError>;

    /// Get all active overrides.
    async fn get_active(&self) -> Result<Vec<PriceOverride>, TraitError>;

    /// Save override.
    async fn save(&self, override_: &PriceOverride) -> Result<(), TraitError>;

    /// Delete override.
    async fn delete(&self, id: &InstrumentId) -> Result<bool, TraitError>;

    /// Get overrides pending approval.
    async fn get_pending_approval(&self) -> Result<Vec<PriceOverride>, TraitError>;

    /// Get override history for instrument.
    async fn get_history(&self, id: &InstrumentId) -> Result<Vec<OverrideAudit>, TraitError>;
}

// =============================================================================
// AUDIT STORE
// =============================================================================

/// Audit entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Entry ID
    pub id: u64,
    /// Timestamp
    pub timestamp: i64,
    /// Event type
    pub event_type: String,
    /// Entity type
    pub entity_type: String,
    /// Entity ID
    pub entity_id: String,
    /// User
    pub user: Option<String>,
    /// Action
    pub action: String,
    /// Details (JSON)
    pub details: Option<String>,
}

/// Audit filter.
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    /// Event type filter
    pub event_type: Option<String>,
    /// Entity type filter
    pub entity_type: Option<String>,
    /// Entity ID filter
    pub entity_id: Option<String>,
    /// User filter
    pub user: Option<String>,
    /// From timestamp
    pub from: Option<i64>,
    /// To timestamp
    pub to: Option<i64>,
}

/// Audit log storage (append-only).
#[async_trait]
pub trait AuditStore: Send + Sync {
    /// Append audit entry.
    async fn append(&self, entry: &AuditEntry) -> Result<(), TraitError>;

    /// Query audit log.
    async fn query(
        &self,
        filter: &AuditFilter,
        pagination: &Pagination,
    ) -> Result<Page<AuditEntry>, TraitError>;
}

// =============================================================================
// COMBINED STORAGE ADAPTER
// =============================================================================

use std::sync::Arc;

/// Combined storage adapter.
pub struct StorageAdapter {
    /// Bond store
    pub bonds: Arc<dyn BondStore>,
    /// Curve store
    pub curves: Arc<dyn CurveStore>,
    /// Config store
    pub configs: Arc<dyn ConfigStore>,
    /// Override store
    pub overrides: Arc<dyn OverrideStore>,
    /// Audit store
    pub audit: Arc<dyn AuditStore>,
}
