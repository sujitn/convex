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

    // Prices
    /// Clean price (mid)
    pub clean_price: Option<Decimal>,
    /// Dirty price (mid)
    pub dirty_price: Option<Decimal>,
    /// Accrued interest
    pub accrued_interest: Option<Decimal>,

    // Yields
    /// Yield to maturity
    pub ytm: Option<Decimal>,
    /// Yield to worst (for callable)
    pub ytw: Option<Decimal>,
    /// Yield to call
    pub ytc: Option<Decimal>,

    // Spreads
    /// Z-spread
    pub z_spread: Option<Decimal>,
    /// I-spread
    pub i_spread: Option<Decimal>,
    /// G-spread
    pub g_spread: Option<Decimal>,
    /// Asset swap spread
    pub asw: Option<Decimal>,
    /// OAS (for callable)
    pub oas: Option<Decimal>,
    /// Discount margin (for FRN)
    pub discount_margin: Option<Decimal>,
    /// Simple margin (for FRN)
    pub simple_margin: Option<Decimal>,

    // Duration
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
    /// Pricing model used
    pub pricing_model: String,
    /// Quote source
    pub source: String,
    /// Is stale
    pub is_stale: bool,
    /// Quality indicator (0-100)
    pub quality: u8,
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
