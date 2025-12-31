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
// PRICING SPEC TYPES
// =============================================================================

/// Which side of a market quote to use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuoteSide {
    /// Bid price/yield (conservative for selling)
    Bid,
    /// Mid price/yield (standard mark-to-market)
    #[default]
    Mid,
    /// Ask price/yield (conservative for buying)
    Ask,
}

/// Reference for benchmark-based pricing.
///
/// Note: Curves don't have bid/ask internally - use curve ID convention
/// (e.g., "USD_GOVT_BID", "USD_GOVT_MID") to reference specific side.
/// Only `SpecificBond` has `side` since bond quotes have bid/ask.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BenchmarkReference {
    /// Interpolated yield from government curve at bond's maturity.
    GovernmentCurve { curve: CurveId },

    /// Specific on-the-run tenor (e.g., 10Y OTR Treasury).
    OnTheRun { curve: CurveId, tenor: Tenor },

    /// Specific benchmark bond by identifier.
    SpecificBond {
        security_id: InstrumentId,
        side: QuoteSide,
    },

    /// Interpolated swap rate from swap curve.
    SwapCurve { curve: CurveId },

    /// Explicit yield value (manual override).
    ExplicitYield { yield_pct: f64 },
}

/// Specification for how to derive a bond's price.
///
/// This is the core configuration that determines the pricing methodology.
/// The actual market data is fetched via `MarketDataProvider` based on this spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PricingSpec {
    /// Use direct market quote.
    MarketQuote { side: QuoteSide },

    /// Benchmark yield + spread → Price.
    /// Industry terms: "Matrix pricing", "Spread off benchmark".
    BenchmarkSpread {
        benchmark: BenchmarkReference,
        spread_bps: f64,
    },

    /// FRN: Reference rate + Discount Margin → Price.
    DiscountMargin {
        reference_curve: CurveId,
        quoted_margin_bps: f64,
        discount_margin_bps: f64,
    },

    /// Z-spread over discount curve → Price.
    ZSpread {
        discount_curve: CurveId,
        z_spread_bps: f64,
    },

    /// OAS for callable bonds → Price.
    OptionAdjusted {
        discount_curve: CurveId,
        vol_surface: VolSurfaceId,
        oas_bps: f64,
        mean_reversion: Option<f64>,
        tree_steps: Option<u32>,
    },

    /// Real yield for inflation-linked bonds (TIPS) → Price.
    RealYield {
        real_yield_pct: f64,
        inflation_curve: CurveId,
        index_ratio: rust_decimal::Decimal,
    },

    /// Auto-derive from market (system chooses based on available data).
    Auto,
}

impl Default for PricingSpec {
    fn default() -> Self {
        PricingSpec::Auto
    }
}

/// Bid-ask spread configuration for model-priced bonds.
///
/// When pricing from a model (BenchmarkSpread, ZSpread, etc.),
/// this config determines how to generate bid/ask from the mid price.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BidAskSpreadConfig {
    /// Half-spread in basis points.
    /// bid = mid * (1 - half_spread/10000), ask = mid * (1 + half_spread/10000)
    pub half_spread_bps: f64,

    /// Asymmetric bid spread (overrides half_spread for bid side).
    pub bid_spread_bps: Option<f64>,

    /// Asymmetric ask spread (overrides half_spread for ask side).
    pub ask_spread_bps: Option<f64>,

    /// If true, use market bid-ask when available, fall back to config.
    pub prefer_market_spread: bool,
}

impl BidAskSpreadConfig {
    /// Create symmetric spread config.
    pub fn symmetric(half_spread_bps: f64) -> Self {
        Self {
            half_spread_bps,
            bid_spread_bps: None,
            ask_spread_bps: None,
            prefer_market_spread: true,
        }
    }

    /// Create asymmetric spread config.
    pub fn asymmetric(bid_spread_bps: f64, ask_spread_bps: f64) -> Self {
        Self {
            half_spread_bps: 0.0,
            bid_spread_bps: Some(bid_spread_bps),
            ask_spread_bps: Some(ask_spread_bps),
            prefer_market_spread: true,
        }
    }

    /// Get effective bid spread in bps.
    pub fn effective_bid_spread_bps(&self) -> f64 {
        self.bid_spread_bps.unwrap_or(self.half_spread_bps)
    }

    /// Get effective ask spread in bps.
    pub fn effective_ask_spread_bps(&self) -> f64 {
        self.ask_spread_bps.unwrap_or(self.half_spread_bps)
    }
}

/// Curves available for analytics calculations (spread calculations, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsCurves {
    /// Discount curve for Z-spread calculation.
    pub discount_curve: CurveId,

    /// Government curve for G-spread calculation.
    pub government_curve: Option<CurveId>,

    /// Swap curve for I-spread / ASW calculation.
    pub swap_curve: Option<CurveId>,

    /// FRN reference rate curve (if different from discount).
    pub frn_reference_curve: Option<CurveId>,
}

impl Default for AnalyticsCurves {
    fn default() -> Self {
        Self {
            discount_curve: CurveId::new("USD_OIS"),
            government_curve: Some(CurveId::new("USD_GOVT")),
            swap_curve: Some(CurveId::new("USD_SWAP")),
            frn_reference_curve: None,
        }
    }
}

impl AnalyticsCurves {
    /// Create for USD.
    pub fn usd() -> Self {
        Self::default()
    }

    /// Create for EUR.
    pub fn eur() -> Self {
        Self {
            discount_curve: CurveId::new("EUR_ESTR"),
            government_curve: Some(CurveId::new("EUR_GOVT")),
            swap_curve: Some(CurveId::new("EUR_SWAP")),
            frn_reference_curve: None,
        }
    }

    /// Create for GBP.
    pub fn gbp() -> Self {
        Self {
            discount_curve: CurveId::new("GBP_SONIA"),
            government_curve: Some(CurveId::new("GBP_GILT")),
            swap_curve: Some(CurveId::new("GBP_SWAP")),
            frn_reference_curve: None,
        }
    }
}

// =============================================================================
// BOND STORE
// =============================================================================

/// Bond filter for storage queries and config matching.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BondFilter {
    // === Per-bond identifiers (highest priority) ===

    /// Match specific ISIN.
    pub isin: Option<String>,

    /// Match specific CUSIP.
    pub cusip: Option<String>,

    /// Match specific internal instrument ID.
    pub instrument_id: Option<InstrumentId>,

    // === Issuer/sector filters ===

    /// Issuer ID filter.
    pub issuer_id: Option<String>,

    /// Sector filter (e.g., "Financials", "Technology").
    pub sector: Option<String>,

    /// Country filter (ISO 3166-1 alpha-2).
    pub country: Option<String>,

    // === Bond characteristics ===

    /// Currency filter.
    pub currency: Option<convex_core::Currency>,

    /// Bond type filter.
    pub bond_type: Option<crate::reference_data::BondType>,

    /// Issuer type filter.
    pub issuer_type: Option<crate::reference_data::IssuerType>,

    /// Is callable filter.
    pub is_callable: Option<bool>,

    // === Maturity filters ===

    /// Maturity from (inclusive).
    pub maturity_from: Option<convex_core::Date>,

    /// Maturity to (inclusive).
    pub maturity_to: Option<convex_core::Date>,

    // === Text search ===

    /// Text search (matches against name, ticker, etc.).
    pub text_search: Option<String>,
}

impl BondFilter {
    /// Create filter for a specific ISIN.
    pub fn by_isin(isin: impl Into<String>) -> Self {
        Self {
            isin: Some(isin.into()),
            ..Default::default()
        }
    }

    /// Create filter for a specific CUSIP.
    pub fn by_cusip(cusip: impl Into<String>) -> Self {
        Self {
            cusip: Some(cusip.into()),
            ..Default::default()
        }
    }

    /// Create filter for a specific instrument ID.
    pub fn by_instrument_id(id: InstrumentId) -> Self {
        Self {
            instrument_id: Some(id),
            ..Default::default()
        }
    }

    /// Create filter for currency.
    pub fn by_currency(currency: convex_core::Currency) -> Self {
        Self {
            currency: Some(currency),
            ..Default::default()
        }
    }

    /// Create filter for issuer type.
    pub fn by_issuer_type(issuer_type: crate::reference_data::IssuerType) -> Self {
        Self {
            issuer_type: Some(issuer_type),
            ..Default::default()
        }
    }

    /// Check if this filter matches a bond.
    pub fn matches(&self, bond: &BondReferenceData) -> bool {
        // ISIN match (exact)
        if let Some(ref isin) = self.isin {
            if bond.isin.as_ref() != Some(isin) {
                return false;
            }
        }

        // CUSIP match (exact)
        if let Some(ref cusip) = self.cusip {
            if bond.cusip.as_ref() != Some(cusip) {
                return false;
            }
        }

        // Instrument ID match
        if let Some(ref id) = self.instrument_id {
            if &bond.instrument_id != id {
                return false;
            }
        }

        // Currency match
        if let Some(currency) = self.currency {
            if bond.currency != currency {
                return false;
            }
        }

        // Issuer type match
        if let Some(issuer_type) = self.issuer_type {
            if bond.issuer_type != issuer_type {
                return false;
            }
        }

        // Bond type match
        if let Some(bond_type) = self.bond_type {
            if bond.bond_type != bond_type {
                return false;
            }
        }

        // Is callable match
        if let Some(is_callable) = self.is_callable {
            if bond.is_callable != is_callable {
                return false;
            }
        }

        // Sector match
        if let Some(ref sector) = self.sector {
            if &bond.sector != sector {
                return false;
            }
        }

        // Country match
        if let Some(ref country) = self.country {
            if &bond.country_of_risk != country {
                return false;
            }
        }

        // Maturity range match
        if let Some(from) = self.maturity_from {
            if bond.maturity_date < from {
                return false;
            }
        }
        if let Some(to) = self.maturity_to {
            if bond.maturity_date > to {
                return false;
            }
        }

        true
    }

    /// Calculate specificity score (higher = more specific).
    /// Used to determine config priority.
    pub fn specificity(&self) -> u32 {
        let mut score = 0;

        // Per-bond identifiers (highest priority)
        if self.isin.is_some() {
            score += 1000;
        }
        if self.cusip.is_some() {
            score += 1000;
        }
        if self.instrument_id.is_some() {
            score += 1000;
        }

        // Issuer-level
        if self.issuer_id.is_some() {
            score += 100;
        }

        // Sector/country
        if self.sector.is_some() {
            score += 50;
        }
        if self.country.is_some() {
            score += 30;
        }

        // Bond characteristics
        if self.bond_type.is_some() {
            score += 20;
        }
        if self.issuer_type.is_some() {
            score += 20;
        }
        if self.is_callable.is_some() {
            score += 10;
        }

        // Currency
        if self.currency.is_some() {
            score += 10;
        }

        // Maturity range
        if self.maturity_from.is_some() {
            score += 5;
        }
        if self.maturity_to.is_some() {
            score += 5;
        }

        score
    }
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
///
/// Defines how to price bonds matching a specific filter.
/// Multiple configs can exist; the most specific filter wins (based on `specificity()`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondPricingConfig {
    /// Config identifier
    pub config_id: String,
    /// Human-readable description
    pub description: String,
    /// Which bonds this config applies to
    pub applies_to: BondFilter,
    /// How to derive the price
    pub pricing_spec: PricingSpec,
    /// Bid/ask spread config for model-priced bonds
    pub bid_ask_spread: Option<BidAskSpreadConfig>,
    /// Curves for analytics calculations (spreads, duration, etc.)
    pub analytics_curves: AnalyticsCurves,
    /// Explicit priority (higher wins); if None, uses filter specificity
    pub priority: Option<i32>,
    /// Is this config active
    pub active: bool,
    /// Version number (for optimistic locking)
    pub version: u64,
    /// Created timestamp
    pub created_at: i64,
    /// Last updated timestamp
    pub updated_at: i64,
}

impl BondPricingConfig {
    /// Calculate effective priority (explicit priority or filter specificity).
    pub fn effective_priority(&self) -> i32 {
        self.priority.unwrap_or_else(|| self.applies_to.specificity() as i32)
    }
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
