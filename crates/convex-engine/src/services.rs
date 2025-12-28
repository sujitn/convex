//! Service traits for the pricing engine.
//!
//! Services provide the business logic layer between the calculation graph
//! and the external world. They define interfaces for:
//!
//! - **BondService**: CRUD operations for bonds (security master)
//! - **CurveService**: Curve building and management
//! - **PricingService**: Bond pricing and analytics
//! - **OverrideService**: Manual price/yield overrides
//!
//! # Design
//!
//! Services are defined as traits to enable:
//! - Mock implementations for testing
//! - Multiple backend implementations (in-memory, database, external API)
//! - Dependency injection in the pricing engine

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_curves::CurveRef;

use crate::error::EngineResult;

// =============================================================================
// BOND SERVICE
// =============================================================================

/// Filter criteria for querying bonds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BondFilter {
    /// Filter by issuer.
    pub issuer: Option<String>,
    /// Filter by currency.
    pub currency: Option<String>,
    /// Filter by bond type.
    pub bond_type: Option<String>,
    /// Filter by maturity range (min, max).
    pub maturity_range: Option<(NaiveDate, NaiveDate)>,
    /// Filter by coupon range (min, max).
    pub coupon_range: Option<(Decimal, Decimal)>,
    /// Maximum number of results.
    pub limit: Option<usize>,
    /// Offset for pagination.
    pub offset: Option<usize>,
}

impl BondFilter {
    /// Creates a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filters by issuer.
    pub fn issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = Some(issuer.into());
        self
    }

    /// Filters by currency.
    pub fn currency(mut self, currency: impl Into<String>) -> Self {
        self.currency = Some(currency.into());
        self
    }

    /// Filters by bond type.
    pub fn bond_type(mut self, bond_type: impl Into<String>) -> Self {
        self.bond_type = Some(bond_type.into());
        self
    }

    /// Filters by maturity range.
    pub fn maturity_between(mut self, min: NaiveDate, max: NaiveDate) -> Self {
        self.maturity_range = Some((min, max));
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

/// Bond information for the security master.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondInfo {
    /// Instrument ID (CUSIP, ISIN, etc.).
    pub instrument_id: String,
    /// Issuer name.
    pub issuer: String,
    /// Currency.
    pub currency: String,
    /// Bond type (e.g., "fixed", "floating", "callable").
    pub bond_type: String,
    /// Face value.
    pub face_value: Decimal,
    /// Coupon rate (for fixed-rate bonds).
    pub coupon_rate: Option<Decimal>,
    /// Payment frequency (e.g., 1, 2, 4 for annual, semi-annual, quarterly).
    pub payment_frequency: Option<u32>,
    /// Issue date.
    pub issue_date: NaiveDate,
    /// Maturity date.
    pub maturity_date: NaiveDate,
    /// First coupon date.
    pub first_coupon_date: Option<NaiveDate>,
    /// Day count convention.
    pub day_count: String,
    /// Settlement days.
    pub settlement_days: u32,
    /// Whether the bond is callable.
    pub is_callable: bool,
    /// Additional metadata.
    pub metadata: HashMap<String, String>,
}

/// Service for managing bond data (security master).
#[async_trait]
pub trait BondService: Send + Sync {
    /// Gets a bond by its instrument ID.
    async fn get_bond(&self, instrument_id: &str) -> EngineResult<Option<BondInfo>>;

    /// Gets multiple bonds by their IDs.
    async fn get_bonds(&self, instrument_ids: &[&str]) -> EngineResult<Vec<BondInfo>>;

    /// Searches for bonds matching the filter criteria.
    async fn search_bonds(&self, filter: &BondFilter) -> EngineResult<Vec<BondInfo>>;

    /// Stores or updates a bond.
    async fn put_bond(&self, bond: BondInfo) -> EngineResult<()>;

    /// Deletes a bond.
    async fn delete_bond(&self, instrument_id: &str) -> EngineResult<bool>;

    /// Returns the total number of bonds.
    async fn count(&self) -> EngineResult<usize>;
}

// =============================================================================
// CURVE SERVICE
// =============================================================================

/// Filter criteria for querying curves.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CurveFilter {
    /// Filter by currency.
    pub currency: Option<String>,
    /// Filter by curve type.
    pub curve_type: Option<String>,
    /// Filter by reference date.
    pub reference_date: Option<NaiveDate>,
    /// Only return curves built after this time.
    pub built_after: Option<DateTime<Utc>>,
}

impl CurveFilter {
    /// Creates a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filters by currency.
    pub fn currency(mut self, currency: impl Into<String>) -> Self {
        self.currency = Some(currency.into());
        self
    }

    /// Filters by curve type.
    pub fn curve_type(mut self, curve_type: impl Into<String>) -> Self {
        self.curve_type = Some(curve_type.into());
        self
    }

    /// Filters by reference date.
    pub fn reference_date(mut self, date: NaiveDate) -> Self {
        self.reference_date = Some(date);
        self
    }
}

/// Curve building parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveBuildParams {
    /// Curve identifier.
    pub curve_id: String,
    /// Reference date.
    pub reference_date: NaiveDate,
    /// Instrument tenors (in years).
    pub tenors: Vec<f64>,
    /// Instrument rates.
    pub rates: Vec<f64>,
    /// Instrument types (e.g., "deposit", "swap", "ois").
    pub instrument_types: Vec<String>,
    /// Interpolation method.
    pub interpolation: String,
    /// Calibration method (e.g., "bootstrap", "global_fit").
    pub calibration_method: String,
}

/// Curve metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveInfo {
    /// Curve identifier.
    pub curve_id: String,
    /// Currency.
    pub currency: String,
    /// Curve type.
    pub curve_type: String,
    /// Reference date.
    pub reference_date: NaiveDate,
    /// When the curve was built.
    pub build_time: DateTime<Utc>,
    /// Version number.
    pub version: u64,
    /// Tenor bounds (min, max).
    pub tenor_bounds: (f64, f64),
    /// Build parameters.
    pub build_params: Option<CurveBuildParams>,
}

/// Service for curve building and management.
#[async_trait]
pub trait CurveService: Send + Sync {
    /// Gets a curve by ID.
    async fn get_curve(&self, curve_id: &str) -> EngineResult<Option<CurveRef>>;

    /// Gets curve metadata without the full curve.
    async fn get_curve_info(&self, curve_id: &str) -> EngineResult<Option<CurveInfo>>;

    /// Builds a new curve from parameters.
    async fn build_curve(&self, params: CurveBuildParams) -> EngineResult<CurveRef>;

    /// Stores a pre-built curve.
    async fn put_curve(&self, curve_id: &str, curve: CurveRef) -> EngineResult<()>;

    /// Rebuilds a curve with updated market data.
    async fn rebuild_curve(&self, curve_id: &str, rates: &[f64]) -> EngineResult<CurveRef>;

    /// Lists all available curves.
    async fn list_curves(&self, filter: &CurveFilter) -> EngineResult<Vec<CurveInfo>>;

    /// Deletes a curve.
    async fn delete_curve(&self, curve_id: &str) -> EngineResult<bool>;

    /// Gets zero rate at a specific tenor.
    async fn zero_rate(&self, curve_id: &str, tenor: f64) -> EngineResult<Option<f64>>;

    /// Gets discount factor at a specific tenor.
    async fn discount_factor(&self, curve_id: &str, tenor: f64) -> EngineResult<Option<f64>>;

    /// Gets forward rate between two tenors.
    async fn forward_rate(
        &self,
        curve_id: &str,
        start_tenor: f64,
        end_tenor: f64,
    ) -> EngineResult<Option<f64>>;
}

// =============================================================================
// PRICING SERVICE
// =============================================================================

/// Pricing request for a single bond.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingRequest {
    /// Instrument ID.
    pub instrument_id: String,
    /// Settlement date (uses T+settlement_days if not specified).
    pub settlement_date: Option<NaiveDate>,
    /// Pricing curve ID.
    pub pricing_curve: Option<String>,
    /// Discount curve ID.
    pub discount_curve: Option<String>,
    /// Spread curve ID for spread calculations.
    pub spread_curve: Option<String>,
    /// Whether to calculate risk metrics.
    pub calculate_risk: bool,
    /// Whether to calculate spread metrics.
    pub calculate_spreads: bool,
    /// Input price (for yield calculation).
    pub price: Option<Decimal>,
    /// Input yield (for price calculation).
    pub yield_value: Option<Decimal>,
}

impl PricingRequest {
    /// Creates a new pricing request.
    pub fn new(instrument_id: impl Into<String>) -> Self {
        Self {
            instrument_id: instrument_id.into(),
            settlement_date: None,
            pricing_curve: None,
            discount_curve: None,
            spread_curve: None,
            calculate_risk: true,
            calculate_spreads: false,
            price: None,
            yield_value: None,
        }
    }

    /// Sets the settlement date.
    pub fn settlement_date(mut self, date: NaiveDate) -> Self {
        self.settlement_date = Some(date);
        self
    }

    /// Sets the pricing curve.
    pub fn pricing_curve(mut self, curve_id: impl Into<String>) -> Self {
        self.pricing_curve = Some(curve_id.into());
        self
    }

    /// Sets the spread curve.
    pub fn spread_curve(mut self, curve_id: impl Into<String>) -> Self {
        self.spread_curve = Some(curve_id.into());
        self
    }

    /// Sets the input price.
    pub fn with_price(mut self, price: Decimal) -> Self {
        self.price = Some(price);
        self
    }

    /// Sets the input yield.
    pub fn with_yield(mut self, yield_value: Decimal) -> Self {
        self.yield_value = Some(yield_value);
        self
    }

    /// Enables spread calculation.
    pub fn with_spreads(mut self) -> Self {
        self.calculate_spreads = true;
        self
    }
}

/// Pricing result for a single bond.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingResult {
    /// Instrument ID.
    pub instrument_id: String,
    /// Settlement date used.
    pub settlement_date: NaiveDate,
    /// Clean price.
    pub clean_price: Option<Decimal>,
    /// Dirty price.
    pub dirty_price: Option<Decimal>,
    /// Accrued interest.
    pub accrued_interest: Option<Decimal>,
    /// Yield to maturity.
    pub ytm: Option<Decimal>,
    /// Yield to worst (for callable bonds).
    pub ytw: Option<Decimal>,
    /// Modified duration.
    pub modified_duration: Option<Decimal>,
    /// Macaulay duration.
    pub macaulay_duration: Option<Decimal>,
    /// Convexity.
    pub convexity: Option<Decimal>,
    /// DV01 (dollar value of 1bp).
    pub dv01: Option<Decimal>,
    /// Z-spread in basis points.
    pub z_spread: Option<Decimal>,
    /// OAS in basis points.
    pub oas: Option<Decimal>,
    /// Pricing timestamp.
    pub timestamp: DateTime<Utc>,
    /// Warning messages.
    pub warnings: Vec<String>,
}

impl PricingResult {
    /// Creates a new pricing result.
    pub fn new(instrument_id: impl Into<String>, settlement_date: NaiveDate) -> Self {
        Self {
            instrument_id: instrument_id.into(),
            settlement_date,
            clean_price: None,
            dirty_price: None,
            accrued_interest: None,
            ytm: None,
            ytw: None,
            modified_duration: None,
            macaulay_duration: None,
            convexity: None,
            dv01: None,
            z_spread: None,
            oas: None,
            timestamp: Utc::now(),
            warnings: Vec::new(),
        }
    }

    /// Adds a warning message.
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }
}

/// Batch pricing request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchPricingRequest {
    /// Individual requests.
    pub requests: Vec<PricingRequest>,
    /// Use parallel processing.
    pub parallel: bool,
    /// Timeout in milliseconds.
    pub timeout_ms: Option<u64>,
}

/// Service for bond pricing and analytics.
#[async_trait]
pub trait PricingService: Send + Sync {
    /// Prices a single bond.
    async fn price_bond(&self, request: PricingRequest) -> EngineResult<PricingResult>;

    /// Prices multiple bonds.
    async fn price_bonds(&self, request: BatchPricingRequest) -> EngineResult<Vec<PricingResult>>;

    /// Calculates yield from price.
    async fn calculate_yield(
        &self,
        instrument_id: &str,
        price: Decimal,
        settlement_date: NaiveDate,
    ) -> EngineResult<Decimal>;

    /// Calculates price from yield.
    async fn calculate_price(
        &self,
        instrument_id: &str,
        yield_value: Decimal,
        settlement_date: NaiveDate,
    ) -> EngineResult<Decimal>;

    /// Calculates risk metrics.
    async fn calculate_risk(
        &self,
        instrument_id: &str,
        yield_value: Decimal,
        settlement_date: NaiveDate,
    ) -> EngineResult<RiskMetrics>;

    /// Calculates spread metrics.
    async fn calculate_spreads(
        &self,
        instrument_id: &str,
        price: Decimal,
        settlement_date: NaiveDate,
        spread_curve: &str,
    ) -> EngineResult<SpreadMetrics>;
}

/// Risk metrics for a bond.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskMetrics {
    /// Modified duration.
    pub modified_duration: Decimal,
    /// Macaulay duration.
    pub macaulay_duration: Decimal,
    /// Convexity.
    pub convexity: Decimal,
    /// DV01 (dollar value of 1bp).
    pub dv01: Decimal,
    /// PV01 (present value of 1bp).
    pub pv01: Decimal,
}

/// Spread metrics for a bond.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadMetrics {
    /// Z-spread in basis points.
    pub z_spread: Decimal,
    /// I-spread in basis points.
    pub i_spread: Option<Decimal>,
    /// G-spread in basis points.
    pub g_spread: Option<Decimal>,
    /// Asset swap spread in basis points.
    pub asw: Option<Decimal>,
    /// OAS in basis points (for callable bonds).
    pub oas: Option<Decimal>,
}

// =============================================================================
// OVERRIDE SERVICE
// =============================================================================

/// A manual price or yield override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Override {
    /// Instrument ID.
    pub instrument_id: String,
    /// Override type.
    pub override_type: OverrideType,
    /// Override value.
    pub value: Decimal,
    /// When the override was set.
    pub set_at: DateTime<Utc>,
    /// When the override expires.
    pub expires_at: Option<DateTime<Utc>>,
    /// User who set the override.
    pub set_by: Option<String>,
    /// Reason for the override.
    pub reason: Option<String>,
}

/// Type of override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverrideType {
    /// Price override.
    Price,
    /// Yield override.
    Yield,
    /// Spread override.
    Spread,
}

/// Service for managing manual price/yield overrides.
#[async_trait]
pub trait OverrideService: Send + Sync {
    /// Gets the override for an instrument.
    async fn get_override(&self, instrument_id: &str) -> EngineResult<Option<Override>>;

    /// Sets an override.
    async fn set_override(&self, override_value: Override) -> EngineResult<()>;

    /// Removes an override.
    async fn remove_override(&self, instrument_id: &str) -> EngineResult<bool>;

    /// Lists all active overrides.
    async fn list_overrides(&self) -> EngineResult<Vec<Override>>;

    /// Clears expired overrides.
    async fn clear_expired(&self) -> EngineResult<usize>;
}

// =============================================================================
// IN-MEMORY IMPLEMENTATIONS
// =============================================================================

/// In-memory bond service for testing.
pub struct InMemoryBondService {
    bonds: dashmap::DashMap<String, BondInfo>,
}

impl InMemoryBondService {
    /// Creates a new in-memory bond service.
    pub fn new() -> Self {
        Self {
            bonds: dashmap::DashMap::new(),
        }
    }
}

impl Default for InMemoryBondService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BondService for InMemoryBondService {
    async fn get_bond(&self, instrument_id: &str) -> EngineResult<Option<BondInfo>> {
        Ok(self.bonds.get(instrument_id).map(|r| r.value().clone()))
    }

    async fn get_bonds(&self, instrument_ids: &[&str]) -> EngineResult<Vec<BondInfo>> {
        Ok(instrument_ids
            .iter()
            .filter_map(|id| self.bonds.get(*id).map(|r| r.value().clone()))
            .collect())
    }

    async fn search_bonds(&self, filter: &BondFilter) -> EngineResult<Vec<BondInfo>> {
        let mut results: Vec<BondInfo> = self
            .bonds
            .iter()
            .filter(|entry| {
                let bond = entry.value();

                // Apply filters
                if let Some(ref issuer) = filter.issuer {
                    if &bond.issuer != issuer {
                        return false;
                    }
                }
                if let Some(ref currency) = filter.currency {
                    if &bond.currency != currency {
                        return false;
                    }
                }
                if let Some(ref bond_type) = filter.bond_type {
                    if &bond.bond_type != bond_type {
                        return false;
                    }
                }
                if let Some((min, max)) = filter.maturity_range {
                    if bond.maturity_date < min || bond.maturity_date > max {
                        return false;
                    }
                }
                if let Some((min, max)) = filter.coupon_range {
                    if let Some(coupon) = bond.coupon_rate {
                        if coupon < min || coupon > max {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }

                true
            })
            .map(|entry| entry.value().clone())
            .collect();

        // Apply pagination
        if let Some(offset) = filter.offset {
            results = results.into_iter().skip(offset).collect();
        }
        if let Some(limit) = filter.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    async fn put_bond(&self, bond: BondInfo) -> EngineResult<()> {
        self.bonds.insert(bond.instrument_id.clone(), bond);
        Ok(())
    }

    async fn delete_bond(&self, instrument_id: &str) -> EngineResult<bool> {
        Ok(self.bonds.remove(instrument_id).is_some())
    }

    async fn count(&self) -> EngineResult<usize> {
        Ok(self.bonds.len())
    }
}

/// In-memory override service for testing.
pub struct InMemoryOverrideService {
    overrides: dashmap::DashMap<String, Override>,
}

impl InMemoryOverrideService {
    /// Creates a new in-memory override service.
    pub fn new() -> Self {
        Self {
            overrides: dashmap::DashMap::new(),
        }
    }
}

impl Default for InMemoryOverrideService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OverrideService for InMemoryOverrideService {
    async fn get_override(&self, instrument_id: &str) -> EngineResult<Option<Override>> {
        let entry = self.overrides.get(instrument_id);
        match entry {
            Some(r) => {
                let override_value = r.value().clone();
                // Check expiration
                if let Some(expires_at) = override_value.expires_at {
                    if Utc::now() > expires_at {
                        drop(r);
                        self.overrides.remove(instrument_id);
                        return Ok(None);
                    }
                }
                Ok(Some(override_value))
            }
            None => Ok(None),
        }
    }

    async fn set_override(&self, override_value: Override) -> EngineResult<()> {
        self.overrides
            .insert(override_value.instrument_id.clone(), override_value);
        Ok(())
    }

    async fn remove_override(&self, instrument_id: &str) -> EngineResult<bool> {
        Ok(self.overrides.remove(instrument_id).is_some())
    }

    async fn list_overrides(&self) -> EngineResult<Vec<Override>> {
        Ok(self
            .overrides
            .iter()
            .map(|r| r.value().clone())
            .collect())
    }

    async fn clear_expired(&self) -> EngineResult<usize> {
        let now = Utc::now();
        let expired: Vec<_> = self
            .overrides
            .iter()
            .filter_map(|r| {
                if let Some(expires_at) = r.value().expires_at {
                    if now > expires_at {
                        return Some(r.key().clone());
                    }
                }
                None
            })
            .collect();

        let count = expired.len();
        for id in expired {
            self.overrides.remove(&id);
        }

        Ok(count)
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sample_bond() -> BondInfo {
        BondInfo {
            instrument_id: "US912828Z229".into(),
            issuer: "US Treasury".into(),
            currency: "USD".into(),
            bond_type: "fixed".into(),
            face_value: dec!(100),
            coupon_rate: Some(dec!(0.025)),
            payment_frequency: Some(2),
            issue_date: NaiveDate::from_ymd_opt(2020, 1, 15).unwrap(),
            maturity_date: NaiveDate::from_ymd_opt(2030, 1, 15).unwrap(),
            first_coupon_date: Some(NaiveDate::from_ymd_opt(2020, 7, 15).unwrap()),
            day_count: "ACT/ACT".into(),
            settlement_days: 1,
            is_callable: false,
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_bond_service_crud() {
        let service = InMemoryBondService::new();
        let bond = sample_bond();

        // Create
        service.put_bond(bond.clone()).await.unwrap();
        assert_eq!(service.count().await.unwrap(), 1);

        // Read
        let retrieved = service.get_bond("US912828Z229").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().issuer, "US Treasury");

        // Delete
        let deleted = service.delete_bond("US912828Z229").await.unwrap();
        assert!(deleted);
        assert_eq!(service.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_bond_service_search() {
        let service = InMemoryBondService::new();
        service.put_bond(sample_bond()).await.unwrap();

        // Search by currency
        let filter = BondFilter::new().currency("USD");
        let results = service.search_bonds(&filter).await.unwrap();
        assert_eq!(results.len(), 1);

        // Search by issuer (no match)
        let filter = BondFilter::new().issuer("Apple Inc.");
        let results = service.search_bonds(&filter).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_override_service() {
        let service = InMemoryOverrideService::new();

        let override_value = Override {
            instrument_id: "TEST001".into(),
            override_type: OverrideType::Price,
            value: dec!(99.50),
            set_at: Utc::now(),
            expires_at: None,
            set_by: Some("trader1".into()),
            reason: Some("Manual adjustment".into()),
        };

        // Set
        service.set_override(override_value).await.unwrap();

        // Get
        let retrieved = service.get_override("TEST001").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().value, dec!(99.50));

        // List
        let all = service.list_overrides().await.unwrap();
        assert_eq!(all.len(), 1);

        // Remove
        let removed = service.remove_override("TEST001").await.unwrap();
        assert!(removed);
        assert!(service.get_override("TEST001").await.unwrap().is_none());
    }

    #[test]
    fn test_pricing_request_builder() {
        let request = PricingRequest::new("US912828Z229")
            .settlement_date(NaiveDate::from_ymd_opt(2024, 6, 15).unwrap())
            .pricing_curve("USD.GOVT")
            .with_price(dec!(98.50))
            .with_spreads();

        assert_eq!(request.instrument_id, "US912828Z229");
        assert!(request.settlement_date.is_some());
        assert!(request.pricing_curve.is_some());
        assert!(request.price.is_some());
        assert!(request.calculate_spreads);
    }
}
