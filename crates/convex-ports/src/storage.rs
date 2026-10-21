//! Storage traits for persistence.
//!
//! These traits define interfaces for storage backends:
//! Bond transactions use [`crate::reference_repository::ReferenceRepository`].
//! - [`CurveStore`]: Curve config and snapshot storage
//! - [`ConfigStore`]: Pricing configuration storage
//! - [`crate::configuration::ConfigurationRepository`]: Audited staged settings and scoped overrides
//! - [`AuditStore`]: Audit log storage
//!
//! Storage implementations are EXTENSIONS (e.g., redb, PostgreSQL, Redis).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::TraitError;
use crate::reference_data::BondReferenceData;
use convex_core::ids::*;

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

    /// Validate the local query bound. A zero limit requests only the filtered total.
    pub fn validate(&self) -> Result<(), TraitError> {
        if self.limit > 1000 {
            return Err(TraitError::InvalidInput("page limit exceeds 1000"));
        }
        Ok(())
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
        (self.offset as u64).saturating_add(self.items.len() as u64) < self.total
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum BenchmarkReference {
    /// Interpolated yield from government curve at bond's maturity.
    GovernmentCurve {
        /// Curve identifier.
        curve: CurveId,
    },

    /// Specific on-the-run tenor (e.g., 10Y OTR Treasury).
    OnTheRun {
        /// Curve identifier.
        curve: CurveId,
        /// Tenor for OTR lookup.
        tenor: Tenor,
    },

    /// Specific benchmark bond by identifier.
    SpecificBond {
        /// Security identifier.
        security_id: InstrumentId,
        /// Quote side to use.
        side: QuoteSide,
    },

    /// Interpolated swap rate from swap curve.
    SwapCurve {
        /// Curve identifier.
        curve: CurveId,
    },

    /// Explicit yield value (manual override).
    ExplicitYield {
        /// Yield as a percentage.
        yield_pct: f64,
    },
}

/// Specification for how to derive a bond's price.
///
/// This is the core configuration that determines the pricing methodology.
/// The actual market data is fetched via `MarketDataProvider` based on this spec.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PricingSpec {
    /// Use direct market quote.
    MarketQuote {
        /// Which side of the quote to use.
        side: QuoteSide,
    },

    /// Benchmark yield + spread → Price.
    /// Industry terms: "Matrix pricing", "Spread off benchmark".
    BenchmarkSpread {
        /// Reference benchmark for yield.
        benchmark: BenchmarkReference,
        /// Spread over benchmark in basis points.
        spread_bps: f64,
    },

    /// FRN: Reference rate + Discount Margin → Price.
    DiscountMargin {
        /// Reference rate curve.
        reference_curve: CurveId,
        /// Quoted margin in basis points.
        quoted_margin_bps: f64,
        /// Discount margin in basis points.
        discount_margin_bps: f64,
    },

    /// Z-spread over discount curve → Price.
    ZSpread {
        /// Discount curve for Z-spread calculation.
        discount_curve: CurveId,
        /// Z-spread in basis points.
        z_spread_bps: f64,
    },

    /// OAS for callable bonds → Price.
    OptionAdjusted {
        /// Discount curve for OAS calculation.
        discount_curve: CurveId,
        /// Volatility surface for option pricing.
        vol_surface: VolSurfaceId,
        /// OAS in basis points.
        oas_bps: f64,
        /// Mean reversion parameter for rate model.
        mean_reversion: Option<f64>,
        /// Number of tree steps for lattice model.
        tree_steps: Option<u32>,
    },

    /// Real yield for inflation-linked bonds (TIPS) → Price.
    RealYield {
        /// Real yield as a percentage.
        real_yield_pct: f64,
        /// Inflation curve for projections.
        inflation_curve: CurveId,
        /// Index ratio for accrual.
        index_ratio: rust_decimal::Decimal,
    },

    /// Auto-derive from market (system chooses based on available data).
    #[default]
    Auto,
}

/// Bid-ask spread configuration for model-priced bonds.
///
/// When pricing from a model (BenchmarkSpread, ZSpread, etc.),
/// this config determines how to generate bid/ask from the mid price.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

    /// Floating-rate classification filter.
    pub is_floating: Option<bool>,

    /// Inflation-linked classification filter.
    pub is_inflation_linked: Option<bool>,

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

        if self
            .issuer_id
            .as_ref()
            .is_some_and(|id| id != &bond.issuer_id)
            || self.is_floating.is_some_and(|value| {
                value != (bond.bond_type == crate::reference_data::BondType::FloatingRate)
            })
            || self.is_inflation_linked.is_some_and(|value| {
                value != (bond.bond_type == crate::reference_data::BondType::InflationLinked)
            })
        {
            return false;
        }
        if let Some(query) = &self.text_search {
            let query = query.to_lowercase();
            if ![
                Some(bond.description.as_str()),
                Some(bond.issuer_name.as_str()),
                Some(bond.instrument_id.as_str()),
                bond.isin.as_deref(),
                bond.cusip.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|value| value.to_lowercase().contains(&query))
            {
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

    /// Validate bounded query fields without silently dropping a supplied constraint.
    pub fn validate(&self) -> Result<(), &'static str> {
        if [
            &self.isin,
            &self.cusip,
            &self.issuer_id,
            &self.sector,
            &self.country,
            &self.text_search,
        ]
        .into_iter()
        .flatten()
        .any(|value| value.len() > 1024 || value.chars().any(char::is_control))
        {
            return Err("invalid or oversized reference filter");
        }
        if self
            .maturity_from
            .zip(self.maturity_to)
            .is_some_and(|(a, b)| a > b)
        {
            return Err("inverted maturity range");
        }
        Ok(())
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
        if self.is_floating.is_some() {
            score += 10;
        }
        if self.is_inflation_linked.is_some() {
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
        if self.text_search.is_some() {
            score += 5;
        }

        score
    }
}

// =============================================================================
// CURVE STORE
// =============================================================================

/// Bound on `CurveStore::recipe_heads` results, the curve listing page size.
pub const MAX_CURVE_LISTING: usize = 256;

/// Latest stored revision of one curve, for bounded management listings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurveRecipeHead {
    /// Curve identity.
    pub curve_id: CurveId,
    /// Highest stored record revision.
    pub version: convex_core::contracts::Version,
    /// Recording time of that revision.
    pub recorded_at: convex_core::contracts::UtcTimestamp,
}

/// The single durable curve owner: immutable, versioned native curve recipes.
#[async_trait]
pub trait CurveStore: Send + Sync {
    /// Save one immutable complete native curve recipe. Same revision/different content conflicts.
    async fn save_recipe(
        &self,
        _recipe: &crate::snapshot::SnapshotCurveRecipe,
    ) -> Result<(), TraitError> {
        Err(TraitError::Unsupported("complete curve recipe persistence"))
    }
    /// Retrieve this exact recipe revision, without a latest-version substitute.
    async fn get_recipe(
        &self,
        _id: &CurveId,
        _version: convex_core::contracts::Version,
    ) -> Result<Option<crate::snapshot::SnapshotCurveRecipe>, TraitError> {
        Err(TraitError::Unsupported("complete curve recipe persistence"))
    }
    /// Highest revision recorded at or before `known_at`. The caller supplies the
    /// cutoff and reports the returned version; there is no implicit latest.
    async fn recipe_at(
        &self,
        _id: &CurveId,
        _known_at: convex_core::contracts::UtcTimestamp,
    ) -> Result<Option<crate::snapshot::SnapshotCurveRecipe>, TraitError> {
        Err(TraitError::Unsupported("complete curve recipe persistence"))
    }
    /// Up to `limit` (at most [`MAX_CURVE_LISTING`]) curve heads in identity order,
    /// starting after `after` when given.
    async fn recipe_heads(
        &self,
        _after: Option<&CurveId>,
        _limit: usize,
    ) -> Result<Vec<CurveRecipeHead>, TraitError> {
        Err(TraitError::Unsupported("complete curve recipe persistence"))
    }
}

// =============================================================================
// CONFIG STORE
// =============================================================================

/// Bond pricing configuration.
///
/// Defines how to price bonds matching a specific filter.
/// Multiple configs can exist; the most specific filter wins (based on `specificity()`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Positive immutable version. Save accepts current + 1, or an identical retained retry.
    pub version: u64,
    /// Created timestamp
    pub created_at: i64,
    /// Last updated timestamp
    pub updated_at: i64,
}

impl BondPricingConfig {
    /// Calculate effective priority (explicit priority or filter specificity).
    pub fn effective_priority(&self) -> i32 {
        self.priority
            .unwrap_or_else(|| self.applies_to.specificity() as i32)
    }
}

/// The active configuration that applies to `bond` with the highest effective
/// priority. `Ok(None)` when none applies; a tie between distinct configurations
/// is refused rather than broken by an arbitrary order.
pub fn select_pricing_config<'a>(
    configs: &'a [BondPricingConfig],
    bond: &BondReferenceData,
) -> Result<Option<&'a BondPricingConfig>, TraitError> {
    let mut best: Option<&BondPricingConfig> = None;
    let mut tied = false;
    for config in configs
        .iter()
        .filter(|c| c.active && c.applies_to.matches(bond))
    {
        match best.map(|b| config.effective_priority().cmp(&b.effective_priority())) {
            None | Some(std::cmp::Ordering::Greater) => {
                best = Some(config);
                tied = false;
            }
            Some(std::cmp::Ordering::Equal) => tied = true,
            Some(std::cmp::Ordering::Less) => {}
        }
    }
    if tied {
        return Err(TraitError::Conflict(
            "ambiguous applicable pricing configuration",
        ));
    }
    Ok(best)
}

/// Config version info.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

    /// Atomically save immutable content and make it current.
    ///
    /// New IDs start at version 1; updates use current version + 1. Identical retries
    /// of a retained version succeed without moving the current pointer backwards.
    /// A reused version with different content or a skipped version returns Conflict.
    async fn save(&self, config: &BondPricingConfig) -> Result<(), TraitError>;

    /// List all configs.
    async fn list(&self) -> Result<Vec<BondPricingConfig>, TraitError>;

    /// Delete config if the adapter supports a versioned deletion contract.
    /// The local redb adapter returns Unsupported; disable via a new inactive version.
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
// AUDIT STORE
// =============================================================================

/// Audit entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

impl AuditFilter {
    /// Reject oversized constraints and inverted inclusive time ranges.
    pub fn validate(&self) -> Result<(), TraitError> {
        if [
            &self.event_type,
            &self.entity_type,
            &self.entity_id,
            &self.user,
        ]
        .into_iter()
        .flatten()
        .any(|v| v.len() > 1024 || v.chars().any(char::is_control))
        {
            return Err(TraitError::InvalidInput("invalid audit filter"));
        }
        if self.from.zip(self.to).is_some_and(|(from, to)| from > to) {
            return Err(TraitError::InvalidInput("inverted audit time range"));
        }
        Ok(())
    }

    /// Apply all supplied constraints with exact text matches and inclusive timestamps.
    pub fn matches(&self, entry: &AuditEntry) -> bool {
        self.event_type
            .as_ref()
            .is_none_or(|v| v == &entry.event_type)
            && self
                .entity_type
                .as_ref()
                .is_none_or(|v| v == &entry.entity_type)
            && self
                .entity_id
                .as_ref()
                .is_none_or(|v| v == &entry.entity_id)
            && self
                .user
                .as_ref()
                .is_none_or(|v| entry.user.as_ref() == Some(v))
            && self.from.is_none_or(|v| entry.timestamp >= v)
            && self.to.is_none_or(|v| entry.timestamp <= v)
    }
}

/// Audit log storage (append-only).
#[async_trait]
pub trait AuditStore: Send + Sync {
    /// Append an immutable entry; identical ID/content retries succeed, different content conflicts.
    async fn append(&self, entry: &AuditEntry) -> Result<(), TraitError>;

    /// Query matches and their total in one read snapshot, ordered by entry ID.
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
    pub bonds: Arc<dyn crate::reference_repository::ReferenceRepository>,
    /// Curve store
    pub curves: Arc<dyn CurveStore>,
    /// Config store
    pub configs: Arc<dyn ConfigStore>,
    /// Staged application settings, atomic active pointer and append-only activation audit.
    pub configuration: Arc<dyn crate::configuration::ConfigurationRepository>,
    /// Audit store
    pub audit: Arc<dyn AuditStore>,
}

/// Validate persisted/replayed quantitative configuration without fetching current state.
pub fn validate_pricing_config(config: &BondPricingConfig) -> Result<(), TraitError> {
    let validate_id = |value: &str| {
        if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
            Err(TraitError::InvalidInput("invalid storage identity"))
        } else {
            Ok(())
        }
    };
    validate_id(&config.config_id)?;
    if config.version == 0
        || config.updated_at < config.created_at
        || config.description.len() > 4096
    {
        return Err(TraitError::InvalidInput(
            "invalid configuration version, time or description",
        ));
    }
    config
        .applies_to
        .validate()
        .map_err(TraitError::InvalidInput)?;
    validate_id(config.analytics_curves.discount_curve.as_str())?;
    for id in [
        &config.analytics_curves.government_curve,
        &config.analytics_curves.swap_curve,
        &config.analytics_curves.frn_reference_curve,
    ]
    .into_iter()
    .flatten()
    {
        validate_id(id.as_str())?;
    }
    let valid = match &config.pricing_spec {
        PricingSpec::Auto | PricingSpec::MarketQuote { .. } => true,
        PricingSpec::BenchmarkSpread {
            benchmark,
            spread_bps,
        } => {
            let valid_benchmark = match benchmark {
                BenchmarkReference::ExplicitYield { yield_pct } => yield_pct.is_finite(),
                BenchmarkReference::GovernmentCurve { curve }
                | BenchmarkReference::SwapCurve { curve }
                | BenchmarkReference::OnTheRun { curve, .. } => validate_id(curve.as_str()).is_ok(),
                BenchmarkReference::SpecificBond { security_id, .. } => {
                    validate_id(security_id.as_str()).is_ok()
                }
            };
            valid_benchmark && spread_bps.is_finite()
        }
        PricingSpec::DiscountMargin {
            reference_curve,
            quoted_margin_bps,
            discount_margin_bps,
        } => {
            validate_id(reference_curve.as_str()).is_ok()
                && quoted_margin_bps.is_finite()
                && discount_margin_bps.is_finite()
        }
        PricingSpec::ZSpread {
            discount_curve,
            z_spread_bps,
        } => validate_id(discount_curve.as_str()).is_ok() && z_spread_bps.is_finite(),
        PricingSpec::OptionAdjusted {
            discount_curve,
            vol_surface,
            oas_bps,
            mean_reversion,
            tree_steps,
        } => {
            validate_id(discount_curve.as_str()).is_ok()
                && validate_id(vol_surface.as_str()).is_ok()
                && oas_bps.is_finite()
                && mean_reversion.is_none_or(|v| v.is_finite() && v >= 0.0)
                && tree_steps.is_none_or(|v| (1..=10000).contains(&v))
        }
        PricingSpec::RealYield {
            real_yield_pct,
            inflation_curve,
            index_ratio,
        } => {
            real_yield_pct.is_finite()
                && validate_id(inflation_curve.as_str()).is_ok()
                && index_ratio.is_sign_positive()
                && !index_ratio.is_zero()
        }
    };
    let valid_spread = config.bid_ask_spread.as_ref().is_none_or(|s| {
        [Some(s.half_spread_bps), s.bid_spread_bps, s.ask_spread_bps]
            .into_iter()
            .flatten()
            .all(|v| v.is_finite() && v >= 0.0)
    });
    if !valid || !valid_spread {
        return Err(TraitError::InvalidInput(
            "invalid configuration model inputs",
        ));
    }
    Ok(())
}
