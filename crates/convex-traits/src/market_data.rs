//! Market data source traits.
//!
//! These traits define interfaces for market data providers:
//! - [`QuoteSource`]: Bond quotes (bid/ask/last)
//! - [`CurveInputSource`]: Curve inputs (deposits, swaps, futures)
//! - [`IndexFixingSource`]: Floating rate index fixings
//! - [`VolatilitySource`]: Volatility surfaces
//! - [`FxRateSource`]: FX spot and forward rates
//! - [`InflationFixingSource`]: Inflation index fixings
//!
//! All sources support both snapshot (request/response) and streaming modes.

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::TraitError;
use crate::ids::*;
use convex_core::Date;

/// Source type for market data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceType {
    /// Real-time streaming (Bloomberg B-PIPE, Refinitiv Elektron)
    Streaming,
    /// Snapshot/request-response (Bloomberg SAPI, REST APIs)
    Snapshot,
    /// File-based (CSV, JSON, Parquet)
    File,
    /// Database (for historical/EOD)
    Database,
    /// Manual entry
    Manual,
}

// =============================================================================
// QUOTE SOURCE
// =============================================================================

/// Raw quote from market data source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawQuote {
    /// Instrument identifier
    pub instrument_id: InstrumentId,
    /// Bid price
    pub bid_price: Option<Decimal>,
    /// Ask price
    pub ask_price: Option<Decimal>,
    /// Mid price
    pub mid_price: Option<Decimal>,
    /// Last traded price
    pub last_price: Option<Decimal>,
    /// Bid yield
    pub bid_yield: Option<Decimal>,
    /// Ask yield
    pub ask_yield: Option<Decimal>,
    /// Bid size
    pub bid_size: Option<Decimal>,
    /// Ask size
    pub ask_size: Option<Decimal>,
    /// Timestamp of the quote
    pub timestamp: i64,
    /// Source of the quote
    pub source: String,
    /// Trading venue
    pub venue: Option<String>,
}

/// Trait for quote providers (streaming or snapshot).
#[async_trait]
pub trait QuoteSource: Send + Sync {
    /// Source type.
    fn source_type(&self) -> SourceType;

    /// Get current quote (snapshot).
    async fn get_quote(&self, instrument_id: &InstrumentId) -> Result<Option<RawQuote>, TraitError>;

    /// Get quotes for multiple instruments.
    async fn get_quotes(&self, instrument_ids: &[InstrumentId]) -> Result<Vec<RawQuote>, TraitError>;

    /// Subscribe to quote updates (streaming sources only).
    async fn subscribe(&self, instrument_ids: &[InstrumentId]) -> Result<QuoteReceiver, TraitError>;

    /// Unsubscribe from quote updates.
    async fn unsubscribe(&self, instrument_ids: &[InstrumentId]) -> Result<(), TraitError>;
}

/// Receiver for streaming quotes.
pub struct QuoteReceiver {
    rx: tokio::sync::broadcast::Receiver<RawQuote>,
}

impl QuoteReceiver {
    /// Create a new quote receiver.
    pub fn new(rx: tokio::sync::broadcast::Receiver<RawQuote>) -> Self {
        Self { rx }
    }

    /// Receive the next quote.
    pub async fn recv(&mut self) -> Option<RawQuote> {
        self.rx.recv().await.ok()
    }
}

// =============================================================================
// CURVE INPUT SOURCE
// =============================================================================

/// Curve instrument type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CurveInstrumentType {
    /// Money market deposit
    Deposit,
    /// Interest rate future
    Future {
        /// Contract code
        contract_code: String,
    },
    /// Forward rate agreement
    Fra {
        /// Start tenor
        start_tenor: Tenor,
    },
    /// Interest rate swap
    Swap,
    /// Basis swap
    BasisSwap,
    /// OIS swap
    OisSwap,
    /// Cross-currency swap
    CrossCurrencySwap,
    /// Bond yield
    BondYield {
        /// Bond instrument ID
        instrument_id: InstrumentId,
    },
}

/// Input for curve building.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveInput {
    /// Curve identifier
    pub curve_id: CurveId,
    /// Instrument type
    pub instrument_type: CurveInstrumentType,
    /// Tenor
    pub tenor: Tenor,
    /// Rate value
    pub rate: Decimal,
    /// Timestamp
    pub timestamp: i64,
    /// Source
    pub source: String,
}

/// Trait for curve input providers.
#[async_trait]
pub trait CurveInputSource: Send + Sync {
    /// Source type.
    fn source_type(&self) -> SourceType;

    /// Get all inputs for a curve.
    async fn get_curve_inputs(&self, curve_id: &CurveId) -> Result<Vec<CurveInput>, TraitError>;

    /// Subscribe to curve input updates.
    async fn subscribe(&self, curve_ids: &[CurveId]) -> Result<CurveInputReceiver, TraitError>;
}

/// Receiver for curve input updates.
pub struct CurveInputReceiver {
    rx: tokio::sync::broadcast::Receiver<CurveInput>,
}

impl CurveInputReceiver {
    /// Create a new curve input receiver.
    pub fn new(rx: tokio::sync::broadcast::Receiver<CurveInput>) -> Self {
        Self { rx }
    }

    /// Receive the next curve input.
    pub async fn recv(&mut self) -> Option<CurveInput> {
        self.rx.recv().await.ok()
    }
}

// =============================================================================
// INDEX FIXING SOURCE
// =============================================================================

/// Index fixing (SOFR, EURIBOR, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexFixing {
    /// Index identifier
    pub index: FloatingRateIndex,
    /// Fixing date
    pub date: Date,
    /// Rate value
    pub rate: Decimal,
    /// Source
    pub source: String,
    /// Timestamp
    pub timestamp: i64,
}

/// Trait for index fixing providers.
#[async_trait]
pub trait IndexFixingSource: Send + Sync {
    /// Source type.
    fn source_type(&self) -> SourceType;

    /// Get fixing for a specific date.
    async fn get_fixing(
        &self,
        index: &FloatingRateIndex,
        date: Date,
    ) -> Result<Option<IndexFixing>, TraitError>;

    /// Get historical fixings.
    async fn get_fixings(
        &self,
        index: &FloatingRateIndex,
        from: Date,
        to: Date,
    ) -> Result<Vec<IndexFixing>, TraitError>;

    /// Subscribe to new fixings.
    async fn subscribe(
        &self,
        indices: &[FloatingRateIndex],
    ) -> Result<IndexFixingReceiver, TraitError>;
}

/// Receiver for index fixing updates.
pub struct IndexFixingReceiver {
    rx: tokio::sync::broadcast::Receiver<IndexFixing>,
}

impl IndexFixingReceiver {
    /// Create a new index fixing receiver.
    pub fn new(rx: tokio::sync::broadcast::Receiver<IndexFixing>) -> Self {
        Self { rx }
    }

    /// Receive the next fixing.
    pub async fn recv(&mut self) -> Option<IndexFixing> {
        self.rx.recv().await.ok()
    }
}

// =============================================================================
// VOLATILITY SOURCE
// =============================================================================

/// Volatility surface type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VolSurfaceType {
    /// Swaption volatility surface (expiry x underlying tenor)
    Swaption,
    /// Cap/floor volatility curve
    CapFloor,
    /// Yield volatility
    YieldVol,
}

/// Vol quote type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VolQuoteType {
    /// Normal (basis point) volatility
    Normal,
    /// Lognormal (Black) volatility
    Lognormal,
    /// SABR parameters
    Sabr {
        /// Alpha
        alpha: Decimal,
        /// Beta
        beta: Decimal,
        /// Rho
        rho: Decimal,
        /// Nu
        nu: Decimal,
    },
}

/// Volatility point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolPoint {
    /// Option expiry
    pub expiry: Tenor,
    /// Underlying tenor (for swaptions)
    pub underlying_tenor: Option<Tenor>,
    /// Strike (None = ATM)
    pub strike: Option<Decimal>,
    /// Volatility value
    pub vol: Decimal,
    /// Timestamp
    pub timestamp: i64,
}

/// Volatility surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolatilitySurface {
    /// Surface identifier
    pub surface_id: VolSurfaceId,
    /// Currency
    pub currency: convex_core::Currency,
    /// Surface type
    pub surface_type: VolSurfaceType,
    /// As-of timestamp
    pub as_of: i64,
    /// Volatility points
    pub points: Vec<VolPoint>,
    /// Quote type
    pub quote_type: VolQuoteType,
    /// Source
    pub source: String,
}

/// Trait for volatility data providers.
#[async_trait]
pub trait VolatilitySource: Send + Sync {
    /// Get full volatility surface.
    async fn get_surface(
        &self,
        surface_id: &VolSurfaceId,
    ) -> Result<Option<VolatilitySurface>, TraitError>;

    /// Get ATM vol for specific expiry/tenor.
    async fn get_atm_vol(
        &self,
        surface_id: &VolSurfaceId,
        expiry: Tenor,
        underlying_tenor: Option<Tenor>,
    ) -> Result<Option<Decimal>, TraitError>;

    /// Get interpolated vol for any point.
    async fn get_vol(
        &self,
        surface_id: &VolSurfaceId,
        expiry: Tenor,
        underlying_tenor: Option<Tenor>,
        strike: Option<Decimal>,
    ) -> Result<Option<Decimal>, TraitError>;

    /// Subscribe to vol updates.
    async fn subscribe(&self, surface_ids: &[VolSurfaceId]) -> Result<VolReceiver, TraitError>;
}

/// Receiver for volatility updates.
pub struct VolReceiver {
    rx: tokio::sync::broadcast::Receiver<VolatilitySurface>,
}

impl VolReceiver {
    /// Create a new vol receiver.
    pub fn new(rx: tokio::sync::broadcast::Receiver<VolatilitySurface>) -> Self {
        Self { rx }
    }

    /// Receive the next vol surface update.
    pub async fn recv(&mut self) -> Option<VolatilitySurface> {
        self.rx.recv().await.ok()
    }
}

// =============================================================================
// FX RATE SOURCE
// =============================================================================

/// FX rate quote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FxRate {
    /// Currency pair
    pub pair: CurrencyPair,
    /// Bid rate
    pub bid: Option<Decimal>,
    /// Ask rate
    pub ask: Option<Decimal>,
    /// Mid rate
    pub mid: Decimal,
    /// Timestamp
    pub timestamp: i64,
    /// Source
    pub source: String,
}

/// Trait for FX rate providers.
#[async_trait]
pub trait FxRateSource: Send + Sync {
    /// Get spot rate for currency pair.
    async fn get_spot(&self, pair: &CurrencyPair) -> Result<Option<FxRate>, TraitError>;

    /// Get spot rate with triangulation if direct pair unavailable.
    async fn get_spot_triangulated(
        &self,
        base: convex_core::Currency,
        quote: convex_core::Currency,
    ) -> Result<Option<FxRate>, TraitError>;

    /// Get forward rate for tenor.
    async fn get_forward(
        &self,
        pair: &CurrencyPair,
        tenor: Tenor,
    ) -> Result<Option<Decimal>, TraitError>;

    /// Subscribe to FX rate updates.
    async fn subscribe(&self, pairs: &[CurrencyPair]) -> Result<FxRateReceiver, TraitError>;
}

/// Receiver for FX rate updates.
pub struct FxRateReceiver {
    rx: tokio::sync::broadcast::Receiver<FxRate>,
}

impl FxRateReceiver {
    /// Create a new FX rate receiver.
    pub fn new(rx: tokio::sync::broadcast::Receiver<FxRate>) -> Self {
        Self { rx }
    }

    /// Receive the next FX rate update.
    pub async fn recv(&mut self) -> Option<FxRate> {
        self.rx.recv().await.ok()
    }
}

// =============================================================================
// INFLATION FIXING SOURCE
// =============================================================================

/// Inflation interpolation method.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum InflationInterpolation {
    /// 3-month lag linear (standard for TIPS)
    ThreeMonthLagLinear,
    /// 2-month lag linear
    TwoMonthLagLinear,
    /// Flat (use latest fixing)
    Flat,
}

/// Inflation index fixing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InflationFixing {
    /// Index identifier
    pub index: InflationIndex,
    /// Reference month
    pub reference_month: YearMonth,
    /// Index value
    pub value: Decimal,
    /// Publication date
    pub publication_date: Date,
    /// Is preliminary
    pub is_preliminary: bool,
    /// Source
    pub source: String,
}

/// Trait for inflation fixing providers.
#[async_trait]
pub trait InflationFixingSource: Send + Sync {
    /// Get fixing for specific month.
    async fn get_fixing(
        &self,
        index: &InflationIndex,
        month: YearMonth,
    ) -> Result<Option<InflationFixing>, TraitError>;

    /// Get latest available fixing.
    async fn get_latest_fixing(
        &self,
        index: &InflationIndex,
    ) -> Result<Option<InflationFixing>, TraitError>;

    /// Get range of fixings.
    async fn get_fixings_range(
        &self,
        index: &InflationIndex,
        from: YearMonth,
        to: YearMonth,
    ) -> Result<Vec<InflationFixing>, TraitError>;

    /// Get interpolated index value for a date.
    async fn get_interpolated_index(
        &self,
        index: &InflationIndex,
        date: Date,
        interpolation: InflationInterpolation,
    ) -> Result<Option<Decimal>, TraitError>;

    /// Subscribe to new fixings.
    async fn subscribe(
        &self,
        indices: &[InflationIndex],
    ) -> Result<InflationReceiver, TraitError>;
}

/// Receiver for inflation fixing updates.
pub struct InflationReceiver {
    rx: tokio::sync::broadcast::Receiver<InflationFixing>,
}

impl InflationReceiver {
    /// Create a new inflation receiver.
    pub fn new(rx: tokio::sync::broadcast::Receiver<InflationFixing>) -> Self {
        Self { rx }
    }

    /// Receive the next inflation fixing.
    pub async fn recv(&mut self) -> Option<InflationFixing> {
        self.rx.recv().await.ok()
    }
}

// =============================================================================
// ETF QUOTE SOURCE
// =============================================================================

/// ETF quote (NAV, iNAV, price).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EtfQuote {
    /// ETF identifier
    pub etf_id: EtfId,
    /// Net Asset Value
    pub nav: Option<Decimal>,
    /// Indicative NAV
    pub inav: Option<Decimal>,
    /// Last traded price
    pub price: Option<Decimal>,
    /// Premium/discount to NAV
    pub premium_discount: Option<Decimal>,
    /// Timestamp
    pub timestamp: i64,
    /// Source
    pub source: String,
}

/// Trait for ETF quote providers.
#[async_trait]
pub trait EtfQuoteSource: Send + Sync {
    /// Get current ETF quote.
    async fn get_quote(&self, etf_id: &EtfId) -> Result<Option<EtfQuote>, TraitError>;

    /// Subscribe to ETF quote updates.
    async fn subscribe(&self, etf_ids: &[EtfId]) -> Result<EtfQuoteReceiver, TraitError>;
}

/// Receiver for ETF quote updates.
pub struct EtfQuoteReceiver {
    rx: tokio::sync::broadcast::Receiver<EtfQuote>,
}

impl EtfQuoteReceiver {
    /// Create a new ETF quote receiver.
    pub fn new(rx: tokio::sync::broadcast::Receiver<EtfQuote>) -> Self {
        Self { rx }
    }

    /// Receive the next ETF quote.
    pub async fn recv(&mut self) -> Option<EtfQuote> {
        self.rx.recv().await.ok()
    }
}

// =============================================================================
// COMPOSITE MARKET DATA PROVIDER
// =============================================================================

use std::sync::Arc;

/// Combined market data provider.
pub struct MarketDataProvider {
    /// Quote source
    pub quotes: Arc<dyn QuoteSource>,
    /// Curve input source
    pub curve_inputs: Arc<dyn CurveInputSource>,
    /// Index fixing source
    pub index_fixings: Arc<dyn IndexFixingSource>,
    /// Volatility source
    pub volatility: Arc<dyn VolatilitySource>,
    /// FX rate source
    pub fx_rates: Arc<dyn FxRateSource>,
    /// Inflation fixing source
    pub inflation_fixings: Arc<dyn InflationFixingSource>,
    /// ETF quote source
    pub etf_quotes: Arc<dyn EtfQuoteSource>,
}
