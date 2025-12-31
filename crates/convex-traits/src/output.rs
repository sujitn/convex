//! Output publishing traits.
//!
//! These traits define interfaces for publishing pricing outputs:
//! - [`QuotePublisher`]: Bond quote output
//! - [`CurvePublisher`]: Curve output
//! - [`EtfPublisher`]: ETF NAV/iNAV output
//! - [`AnalyticsPublisher`]: Portfolio analytics output
//! - [`AlertPublisher`]: Pricing alerts
//!
//! Output publishers can send to WebSocket, gRPC, Kafka, REST, etc.

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::TraitError;
use crate::ids::*;
use convex_core::{Currency, Date};

// =============================================================================
// BOND QUOTE OUTPUT
// =============================================================================

/// Complete bond quote output with all analytics.
///
/// Note: Dirty price is not included - calculate as clean_price_mid + accrued_interest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondQuoteOutput {
    /// Instrument identifier
    pub instrument_id: InstrumentId,
    /// ISIN
    pub isin: Option<String>,
    /// Currency
    pub currency: Currency,
    /// Settlement date
    pub settlement_date: Date,

    // Prices (bid/mid/ask)
    /// Clean price (bid)
    pub clean_price_bid: Option<Decimal>,
    /// Clean price (mid)
    pub clean_price_mid: Option<Decimal>,
    /// Clean price (ask)
    pub clean_price_ask: Option<Decimal>,
    /// Accrued interest
    pub accrued_interest: Option<Decimal>,

    // Yields (bid/mid/ask)
    /// Yield to maturity (bid)
    pub ytm_bid: Option<Decimal>,
    /// Yield to maturity (mid)
    pub ytm_mid: Option<Decimal>,
    /// Yield to maturity (ask)
    pub ytm_ask: Option<Decimal>,
    /// Yield to worst (for callable, mid)
    pub ytw: Option<Decimal>,
    /// Yield to call (mid)
    pub ytc: Option<Decimal>,

    // Spreads (bid/mid/ask)
    /// Z-spread (bid)
    pub z_spread_bid: Option<Decimal>,
    /// Z-spread (mid)
    pub z_spread_mid: Option<Decimal>,
    /// Z-spread (ask)
    pub z_spread_ask: Option<Decimal>,
    /// I-spread (bid)
    pub i_spread_bid: Option<Decimal>,
    /// I-spread (mid)
    pub i_spread_mid: Option<Decimal>,
    /// I-spread (ask)
    pub i_spread_ask: Option<Decimal>,
    /// G-spread (bid)
    pub g_spread_bid: Option<Decimal>,
    /// G-spread (mid)
    pub g_spread_mid: Option<Decimal>,
    /// G-spread (ask)
    pub g_spread_ask: Option<Decimal>,
    /// Asset swap spread (bid)
    pub asw_bid: Option<Decimal>,
    /// Asset swap spread (mid)
    pub asw_mid: Option<Decimal>,
    /// Asset swap spread (ask)
    pub asw_ask: Option<Decimal>,
    /// OAS for callable (bid)
    pub oas_bid: Option<Decimal>,
    /// OAS for callable (mid)
    pub oas_mid: Option<Decimal>,
    /// OAS for callable (ask)
    pub oas_ask: Option<Decimal>,
    /// Discount margin for FRN (bid)
    pub discount_margin_bid: Option<Decimal>,
    /// Discount margin for FRN (mid)
    pub discount_margin_mid: Option<Decimal>,
    /// Discount margin for FRN (ask)
    pub discount_margin_ask: Option<Decimal>,
    /// Simple margin for FRN (bid)
    pub simple_margin_bid: Option<Decimal>,
    /// Simple margin for FRN (mid)
    pub simple_margin_mid: Option<Decimal>,
    /// Simple margin for FRN (ask)
    pub simple_margin_ask: Option<Decimal>,

    // Duration (calculated from mid price)
    /// Modified duration
    pub modified_duration: Option<Decimal>,
    /// Macaulay duration
    pub macaulay_duration: Option<Decimal>,
    /// Effective duration (for callable)
    pub effective_duration: Option<Decimal>,
    /// Spread duration
    pub spread_duration: Option<Decimal>,

    // Convexity
    /// Convexity
    pub convexity: Option<Decimal>,
    /// Effective convexity
    pub effective_convexity: Option<Decimal>,

    // Risk
    /// DV01 (per $1M notional)
    pub dv01: Option<Decimal>,
    /// PV01
    pub pv01: Option<Decimal>,
    /// Key rate durations (tenor label, duration value)
    pub key_rate_durations: Option<Vec<(String, Decimal)>>,
    /// CS01 - Credit spread sensitivity (price change for 1bp spread increase)
    pub cs01: Option<Decimal>,

    // Metadata
    /// Calculation timestamp
    pub timestamp: i64,
    /// Pricing spec that was used
    pub pricing_spec: String,
    /// Quote source
    pub source: String,
    /// Is stale
    pub is_stale: bool,
    /// Quality indicator (0-100)
    pub quality: u8,
}

impl BondQuoteOutput {
    /// Calculate dirty price from clean price and accrued.
    pub fn dirty_price_mid(&self) -> Option<Decimal> {
        match (self.clean_price_mid, self.accrued_interest) {
            (Some(clean), Some(accrued)) => Some(clean + accrued),
            (Some(clean), None) => Some(clean),
            _ => None,
        }
    }

    /// Calculate dirty price bid.
    pub fn dirty_price_bid(&self) -> Option<Decimal> {
        match (self.clean_price_bid, self.accrued_interest) {
            (Some(clean), Some(accrued)) => Some(clean + accrued),
            (Some(clean), None) => Some(clean),
            _ => None,
        }
    }

    /// Calculate dirty price ask.
    pub fn dirty_price_ask(&self) -> Option<Decimal> {
        match (self.clean_price_ask, self.accrued_interest) {
            (Some(clean), Some(accrued)) => Some(clean + accrued),
            (Some(clean), None) => Some(clean),
            _ => None,
        }
    }

    /// Get price for specified side.
    pub fn clean_price_for_side(&self, side: crate::storage::QuoteSide) -> Option<Decimal> {
        use crate::storage::QuoteSide;
        match side {
            QuoteSide::Bid => self.clean_price_bid,
            QuoteSide::Mid => self.clean_price_mid,
            QuoteSide::Ask => self.clean_price_ask,
        }
    }

    /// Get yield for specified side.
    pub fn ytm_for_side(&self, side: crate::storage::QuoteSide) -> Option<Decimal> {
        use crate::storage::QuoteSide;
        match side {
            QuoteSide::Bid => self.ytm_bid,
            QuoteSide::Mid => self.ytm_mid,
            QuoteSide::Ask => self.ytm_ask,
        }
    }
}

/// Trait for bond quote publishing.
#[async_trait]
pub trait QuotePublisher: Send + Sync {
    /// Publish a single quote.
    async fn publish(&self, quote: &BondQuoteOutput) -> Result<(), TraitError>;

    /// Publish multiple quotes.
    async fn publish_batch(&self, quotes: &[BondQuoteOutput]) -> Result<(), TraitError>;
}

// =============================================================================
// CURVE OUTPUT
// =============================================================================

/// Curve output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveOutput {
    /// Curve identifier
    pub curve_id: CurveId,
    /// Currency
    pub currency: Currency,
    /// As-of date
    pub as_of_date: Date,
    /// Curve points (tenor in days, zero rate)
    pub points: Vec<(u32, Decimal)>,
    /// Build timestamp
    pub timestamp: i64,
    /// Build duration (ms)
    pub build_duration_ms: u64,
    /// Source
    pub source: String,
}

/// Trait for curve publishing.
#[async_trait]
pub trait CurvePublisher: Send + Sync {
    /// Publish a curve update.
    async fn publish(&self, curve: &CurveOutput) -> Result<(), TraitError>;
}

// =============================================================================
// ETF OUTPUT
// =============================================================================

/// ETF quote output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EtfQuoteOutput {
    /// ETF identifier
    pub etf_id: EtfId,
    /// ETF name
    pub name: String,
    /// Currency
    pub currency: Currency,

    /// Net Asset Value
    pub nav: Option<Decimal>,
    /// Indicative NAV
    pub inav: Option<Decimal>,
    /// Last traded price
    pub price: Option<Decimal>,
    /// Premium/discount to NAV (as decimal, e.g., 0.01 = 1%)
    pub premium_discount: Option<Decimal>,

    /// Number of holdings
    pub num_holdings: u32,
    /// Holdings coverage (% of holdings priced)
    pub coverage: Decimal,

    /// Duration (portfolio weighted)
    pub duration: Option<Decimal>,
    /// Yield (portfolio weighted)
    pub yield_value: Option<Decimal>,
    /// Spread (portfolio weighted)
    pub spread: Option<Decimal>,

    /// Calculation timestamp
    pub timestamp: i64,
    /// Is stale
    pub is_stale: bool,
}

/// Trait for ETF publishing.
#[async_trait]
pub trait EtfPublisher: Send + Sync {
    /// Publish ETF quote.
    async fn publish(&self, etf: &EtfQuoteOutput) -> Result<(), TraitError>;

    /// Publish multiple ETF quotes.
    async fn publish_batch(&self, etfs: &[EtfQuoteOutput]) -> Result<(), TraitError>;
}

// =============================================================================
// PORTFOLIO ANALYTICS OUTPUT
// =============================================================================

/// Portfolio analytics output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioAnalyticsOutput {
    /// Portfolio identifier
    pub portfolio_id: PortfolioId,
    /// Portfolio name
    pub name: String,
    /// Reporting currency
    pub currency: Currency,

    /// Total market value
    pub market_value: Decimal,
    /// Number of positions
    pub num_positions: u32,

    /// Portfolio duration
    pub duration: Decimal,
    /// Portfolio convexity
    pub convexity: Decimal,
    /// Portfolio yield
    pub yield_value: Decimal,
    /// Portfolio spread
    pub spread: Decimal,

    /// Total DV01
    pub dv01: Decimal,
    /// Key rate durations
    pub key_rate_durations: Vec<(String, Decimal)>,

    /// Sector breakdown
    pub sector_breakdown: Vec<(String, Decimal)>,
    /// Rating breakdown
    pub rating_breakdown: Vec<(String, Decimal)>,

    /// Calculation timestamp
    pub timestamp: i64,
}

/// Trait for portfolio analytics publishing.
#[async_trait]
pub trait AnalyticsPublisher: Send + Sync {
    /// Publish portfolio analytics.
    async fn publish(&self, analytics: &PortfolioAnalyticsOutput) -> Result<(), TraitError>;
}

// =============================================================================
// ALERTS
// =============================================================================

/// Alert severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    /// Informational
    Info,
    /// Warning
    Warning,
    /// Error
    Error,
    /// Critical
    Critical,
}

/// Pricing alert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingAlert {
    /// Alert ID
    pub alert_id: String,
    /// Severity
    pub severity: AlertSeverity,
    /// Alert type
    pub alert_type: String,
    /// Message
    pub message: String,
    /// Related instrument (if any)
    pub instrument_id: Option<InstrumentId>,
    /// Related curve (if any)
    pub curve_id: Option<CurveId>,
    /// Timestamp
    pub timestamp: i64,
    /// Additional details (JSON)
    pub details: Option<String>,
}

/// Trait for alert publishing.
#[async_trait]
pub trait AlertPublisher: Send + Sync {
    /// Publish an alert.
    async fn publish(&self, alert: &PricingAlert) -> Result<(), TraitError>;
}

// =============================================================================
// COMBINED OUTPUT PUBLISHER
// =============================================================================

use std::sync::Arc;

/// Combined output publisher.
pub struct OutputPublisher {
    /// Quote publisher
    pub quotes: Arc<dyn QuotePublisher>,
    /// Curve publisher
    pub curves: Arc<dyn CurvePublisher>,
    /// ETF publisher
    pub etfs: Arc<dyn EtfPublisher>,
    /// Analytics publisher
    pub analytics: Arc<dyn AnalyticsPublisher>,
    /// Alert publisher
    pub alerts: Arc<dyn AlertPublisher>,
}
