//! Reference data source traits.
//!
//! These traits define interfaces for reference data providers:
//! - [`BondReferenceSource`]: Bond terms and attributes
//! - [`IssuerReferenceSource`]: Issuer information
//! - [`RatingSource`]: Credit ratings
//! - [`EtfHoldingsSource`]: ETF holdings and constituents
//!
//! Reference data is static/semi-static (updated daily, not real-time).

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::TraitError;
use crate::ids::*;
use convex_core::{Currency, Date};

// =============================================================================
// BOND REFERENCE DATA
// =============================================================================

/// Bond type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BondType {
    /// Fixed rate bullet bond
    FixedBullet,
    /// Fixed rate callable bond
    FixedCallable,
    /// Fixed rate putable bond
    FixedPutable,
    /// Floating rate note
    FloatingRate,
    /// Zero coupon bond
    ZeroCoupon,
    /// Inflation-linked bond
    InflationLinked,
    /// Amortizing bond
    Amortizing,
    /// Convertible bond
    Convertible,
}

/// Issuer type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssuerType {
    /// Sovereign (government)
    Sovereign,
    /// Government agency
    Agency,
    /// Supranational
    Supranational,
    /// Corporate - investment grade
    CorporateIG,
    /// Corporate - high yield
    CorporateHY,
    /// Financial institution
    Financial,
    /// Municipal/local government
    Municipal,
}

/// Call schedule entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallScheduleEntry {
    /// Call date
    pub call_date: Date,
    /// Call price (typically par or premium)
    pub call_price: Decimal,
    /// Is make-whole call
    pub is_make_whole: bool,
}

/// Floating rate terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FloatingRateTerms {
    /// Rate index
    pub index: FloatingRateIndex,
    /// Spread over index (in bps)
    pub spread: Decimal,
    /// Reset frequency
    pub reset_frequency: u32,
    /// Rate cap (if any)
    pub cap: Option<Decimal>,
    /// Rate floor (if any)
    pub floor: Option<Decimal>,
}

/// Bond reference data (static attributes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondReferenceData {
    /// Internal identifier
    pub instrument_id: InstrumentId,
    /// ISIN
    pub isin: Option<String>,
    /// CUSIP
    pub cusip: Option<String>,
    /// SEDOL
    pub sedol: Option<String>,
    /// Bloomberg ID
    pub bbgid: Option<String>,
    /// Description/name
    pub description: String,
    /// Currency
    pub currency: Currency,
    /// Issue date
    pub issue_date: Date,
    /// Maturity date
    pub maturity_date: Date,
    /// Coupon rate (for fixed rate bonds, as decimal)
    pub coupon_rate: Option<Decimal>,
    /// Payment frequency (times per year)
    pub frequency: u32,
    /// Day count convention code
    pub day_count: String,
    /// Face value
    pub face_value: Decimal,
    /// Bond type
    pub bond_type: BondType,
    /// Issuer type
    pub issuer_type: IssuerType,
    /// Issuer ID
    pub issuer_id: String,
    /// Issuer name
    pub issuer_name: String,
    /// Seniority
    pub seniority: String,
    /// Is callable
    pub is_callable: bool,
    /// Call schedule (if callable)
    pub call_schedule: Vec<CallScheduleEntry>,
    /// Is putable
    pub is_putable: bool,
    /// Is sinkable
    pub is_sinkable: bool,
    /// Floating rate terms (if FRN)
    pub floating_terms: Option<FloatingRateTerms>,
    /// Inflation index (if inflation-linked)
    pub inflation_index: Option<InflationIndex>,
    /// Inflation base index value (if inflation-linked)
    pub inflation_base_index: Option<Decimal>,
    /// Has deflation floor (TIPS)
    pub has_deflation_floor: bool,
    /// Country of risk
    pub country_of_risk: String,
    /// Sector
    pub sector: String,
    /// Amount outstanding (in currency)
    pub amount_outstanding: Option<Decimal>,
    /// First coupon date
    pub first_coupon_date: Option<Date>,
    /// Last update timestamp
    pub last_updated: i64,
    /// Source
    pub source: String,
}

/// Filter for bond queries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BondFilter {
    /// Currency filter
    pub currency: Option<Currency>,
    /// Issuer type filter
    pub issuer_type: Option<IssuerType>,
    /// Bond type filter
    pub bond_type: Option<BondType>,
    /// Maturity from
    pub maturity_from: Option<Date>,
    /// Maturity to
    pub maturity_to: Option<Date>,
    /// Country of risk
    pub country: Option<String>,
    /// Sector
    pub sector: Option<String>,
    /// Is callable
    pub is_callable: Option<bool>,
    /// Is floating rate
    pub is_floating: Option<bool>,
    /// Is inflation-linked
    pub is_inflation_linked: Option<bool>,
    /// Issuer ID
    pub issuer_id: Option<String>,
    /// Text search query
    pub text_search: Option<String>,
}

/// Trait for bond reference data providers.
#[async_trait]
pub trait BondReferenceSource: Send + Sync {
    /// Get bond by ISIN.
    async fn get_by_isin(&self, isin: &str) -> Result<Option<BondReferenceData>, TraitError>;

    /// Get bond by CUSIP.
    async fn get_by_cusip(&self, cusip: &str) -> Result<Option<BondReferenceData>, TraitError>;

    /// Get bond by internal ID.
    async fn get_by_id(
        &self,
        instrument_id: &InstrumentId,
    ) -> Result<Option<BondReferenceData>, TraitError>;

    /// Get multiple bonds by ISIN.
    async fn get_many_by_isin(&self, isins: &[&str]) -> Result<Vec<BondReferenceData>, TraitError>;

    /// Search bonds by filter.
    async fn search(
        &self,
        filter: &BondFilter,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<BondReferenceData>, TraitError>;

    /// Count bonds matching filter.
    async fn count(&self, filter: &BondFilter) -> Result<u64, TraitError>;

    /// Subscribe to bond reference data changes.
    async fn subscribe(&self, filter: &BondFilter) -> Result<BondRefDataReceiver, TraitError>;
}

/// Receiver for bond reference data updates.
pub struct BondRefDataReceiver {
    rx: tokio::sync::broadcast::Receiver<BondReferenceData>,
}

impl BondRefDataReceiver {
    /// Create a new bond ref data receiver.
    pub fn new(rx: tokio::sync::broadcast::Receiver<BondReferenceData>) -> Self {
        Self { rx }
    }

    /// Receive the next update.
    pub async fn recv(&mut self) -> Option<BondReferenceData> {
        self.rx.recv().await.ok()
    }
}

// =============================================================================
// ISSUER REFERENCE DATA
// =============================================================================

/// Issuer information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuerInfo {
    /// Issuer identifier
    pub issuer_id: String,
    /// Legal entity identifier (LEI)
    pub lei: Option<String>,
    /// Issuer name
    pub name: String,
    /// Short name/ticker
    pub short_name: Option<String>,
    /// Parent company ID
    pub parent_id: Option<String>,
    /// Ultimate parent ID
    pub ultimate_parent_id: Option<String>,
    /// Country of incorporation
    pub country: String,
    /// Sector
    pub sector: String,
    /// Industry group
    pub industry: String,
    /// Is sovereign
    pub is_sovereign: bool,
    /// Is financial
    pub is_financial: bool,
    /// Last updated
    pub last_updated: i64,
    /// Source
    pub source: String,
}

/// Trait for issuer reference data providers.
#[async_trait]
pub trait IssuerReferenceSource: Send + Sync {
    /// Get issuer by ID.
    async fn get_issuer(&self, issuer_id: &str) -> Result<Option<IssuerInfo>, TraitError>;

    /// Get issuer by LEI.
    async fn get_by_lei(&self, lei: &str) -> Result<Option<IssuerInfo>, TraitError>;

    /// Search issuers.
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<IssuerInfo>, TraitError>;

    /// Get all issuers in a sector.
    async fn get_by_sector(&self, sector: &str) -> Result<Vec<IssuerInfo>, TraitError>;
}

// =============================================================================
// CREDIT RATINGS
// =============================================================================

/// Rating agency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RatingAgency {
    /// Moody's
    Moodys,
    /// S&P Global
    SP,
    /// Fitch
    Fitch,
}

/// Credit rating.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditRating {
    /// Issuer ID
    pub issuer_id: String,
    /// Rating agency
    pub agency: RatingAgency,
    /// Long-term rating
    pub long_term_rating: Option<String>,
    /// Short-term rating
    pub short_term_rating: Option<String>,
    /// Outlook
    pub outlook: Option<String>,
    /// Watch status
    pub watch: Option<String>,
    /// Rating date
    pub rating_date: Date,
    /// Last action
    pub last_action: Option<String>,
    /// Is investment grade
    pub is_investment_grade: bool,
    /// Numeric score (1-21, 1=AAA)
    pub numeric_score: Option<u32>,
}

/// Trait for credit rating providers.
#[async_trait]
pub trait RatingSource: Send + Sync {
    /// Get rating for issuer from specific agency.
    async fn get_rating(
        &self,
        issuer_id: &str,
        agency: RatingAgency,
    ) -> Result<Option<CreditRating>, TraitError>;

    /// Get all ratings for an issuer.
    async fn get_all_ratings(&self, issuer_id: &str) -> Result<Vec<CreditRating>, TraitError>;

    /// Get composite rating (lowest of all agencies).
    async fn get_composite_rating(
        &self,
        issuer_id: &str,
    ) -> Result<Option<CreditRating>, TraitError>;

    /// Get rating history.
    async fn get_rating_history(
        &self,
        issuer_id: &str,
        agency: RatingAgency,
        limit: usize,
    ) -> Result<Vec<CreditRating>, TraitError>;
}

// =============================================================================
// ETF HOLDINGS
// =============================================================================

/// ETF holding entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EtfHoldingEntry {
    /// Bond instrument ID
    pub instrument_id: InstrumentId,
    /// Weight in portfolio (as decimal, e.g., 0.05 = 5%)
    pub weight: Decimal,
    /// Shares/units held
    pub shares: Decimal,
    /// Market value
    pub market_value: Decimal,
    /// Notional value
    pub notional_value: Decimal,
    /// Accrued interest
    pub accrued_interest: Option<Decimal>,
}

/// ETF holdings data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EtfHoldings {
    /// ETF identifier
    pub etf_id: EtfId,
    /// ETF name
    pub name: String,
    /// As-of date
    pub as_of_date: Date,
    /// Holdings
    pub holdings: Vec<EtfHoldingEntry>,
    /// Total market value
    pub total_market_value: Decimal,
    /// Shares outstanding
    pub shares_outstanding: Decimal,
    /// NAV per share
    pub nav_per_share: Option<Decimal>,
    /// Last updated
    pub last_updated: i64,
    /// Source
    pub source: String,
}

/// Trait for ETF holdings providers.
#[async_trait]
pub trait EtfHoldingsSource: Send + Sync {
    /// Get current holdings for an ETF.
    async fn get_holdings(&self, etf_id: &EtfId) -> Result<Option<EtfHoldings>, TraitError>;

    /// Get historical holdings.
    async fn get_holdings_as_of(
        &self,
        etf_id: &EtfId,
        as_of_date: Date,
    ) -> Result<Option<EtfHoldings>, TraitError>;

    /// List all available ETFs.
    async fn list_etfs(&self) -> Result<Vec<EtfId>, TraitError>;

    /// Subscribe to holdings updates.
    async fn subscribe(&self, etf_ids: &[EtfId]) -> Result<EtfHoldingsReceiver, TraitError>;
}

/// Receiver for ETF holdings updates.
pub struct EtfHoldingsReceiver {
    rx: tokio::sync::broadcast::Receiver<EtfHoldings>,
}

impl EtfHoldingsReceiver {
    /// Create a new ETF holdings receiver.
    pub fn new(rx: tokio::sync::broadcast::Receiver<EtfHoldings>) -> Self {
        Self { rx }
    }

    /// Receive the next update.
    pub async fn recv(&mut self) -> Option<EtfHoldings> {
        self.rx.recv().await.ok()
    }
}

// =============================================================================
// COMPOSITE REFERENCE DATA PROVIDER
// =============================================================================

use std::sync::Arc;

/// Combined reference data provider.
pub struct ReferenceDataProvider {
    /// Bond reference source
    pub bonds: Arc<dyn BondReferenceSource>,
    /// Issuer reference source
    pub issuers: Arc<dyn IssuerReferenceSource>,
    /// Rating source
    pub ratings: Arc<dyn RatingSource>,
    /// ETF holdings source
    pub etf_holdings: Arc<dyn EtfHoldingsSource>,
}
