//! File-based market data sources.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use dashmap::DashMap;
use rust_decimal::Decimal;
use serde::Deserialize;

use convex_core::Date;
use convex_traits::error::TraitError;
use convex_traits::ids::*;
use convex_traits::market_data::*;

// =============================================================================
// CSV QUOTE SOURCE
// =============================================================================

/// CSV record for quotes.
#[derive(Debug, Deserialize)]
struct QuoteRecord {
    instrument_id: String,
    bid_price: Option<f64>,
    ask_price: Option<f64>,
    mid_price: Option<f64>,
    last_price: Option<f64>,
}

/// CSV-based quote source for testing/EOD.
pub struct CsvQuoteSource {
    file_path: PathBuf,
    quotes: DashMap<InstrumentId, RawQuote>,
}

impl CsvQuoteSource {
    /// Create a new CSV quote source.
    pub fn new(file_path: impl AsRef<Path>) -> Result<Self, TraitError> {
        let source = Self {
            file_path: file_path.as_ref().to_path_buf(),
            quotes: DashMap::new(),
        };
        source.reload()?;
        Ok(source)
    }

    /// Reload quotes from file.
    pub fn reload(&self) -> Result<(), TraitError> {
        if !self.file_path.exists() {
            return Ok(()); // Empty source
        }

        let mut reader = csv::Reader::from_path(&self.file_path)
            .map_err(|e| TraitError::IoError(e.to_string()))?;

        for result in reader.deserialize() {
            let record: QuoteRecord = result.map_err(|e| TraitError::ParseError(e.to_string()))?;

            let quote = RawQuote {
                instrument_id: InstrumentId::new(&record.instrument_id),
                bid_price: record
                    .bid_price
                    .map(Decimal::try_from)
                    .transpose()
                    .ok()
                    .flatten(),
                ask_price: record
                    .ask_price
                    .map(Decimal::try_from)
                    .transpose()
                    .ok()
                    .flatten(),
                mid_price: record
                    .mid_price
                    .map(Decimal::try_from)
                    .transpose()
                    .ok()
                    .flatten(),
                last_price: record
                    .last_price
                    .map(Decimal::try_from)
                    .transpose()
                    .ok()
                    .flatten(),
                bid_yield: None,
                ask_yield: None,
                bid_size: None,
                ask_size: None,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64,
                source: "file".to_string(),
                venue: None,
            };

            self.quotes.insert(quote.instrument_id.clone(), quote);
        }

        Ok(())
    }
}

#[async_trait]
impl QuoteSource for CsvQuoteSource {
    fn source_type(&self) -> SourceType {
        SourceType::File
    }

    async fn get_quote(
        &self,
        instrument_id: &InstrumentId,
    ) -> Result<Option<RawQuote>, TraitError> {
        Ok(self.quotes.get(instrument_id).map(|q| q.clone()))
    }

    async fn get_quotes(
        &self,
        instrument_ids: &[InstrumentId],
    ) -> Result<Vec<RawQuote>, TraitError> {
        Ok(instrument_ids
            .iter()
            .filter_map(|id| self.quotes.get(id).map(|q| q.clone()))
            .collect())
    }

    async fn subscribe(
        &self,
        _instrument_ids: &[InstrumentId],
    ) -> Result<QuoteReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable(
            "File source does not support streaming".into(),
        ))
    }

    async fn unsubscribe(&self, _instrument_ids: &[InstrumentId]) -> Result<(), TraitError> {
        Ok(())
    }
}

// =============================================================================
// JSON CURVE INPUT SOURCE
// =============================================================================

/// JSON-based curve input source.
pub struct JsonCurveInputSource {
    file_path: PathBuf,
    inputs: DashMap<CurveId, Vec<CurveInput>>,
}

impl JsonCurveInputSource {
    /// Create a new JSON curve input source.
    pub fn new(file_path: impl AsRef<Path>) -> Result<Self, TraitError> {
        let source = Self {
            file_path: file_path.as_ref().to_path_buf(),
            inputs: DashMap::new(),
        };
        source.reload()?;
        Ok(source)
    }

    /// Reload inputs from file.
    pub fn reload(&self) -> Result<(), TraitError> {
        if !self.file_path.exists() {
            return Ok(()); // Empty source
        }

        let content = std::fs::read_to_string(&self.file_path)
            .map_err(|e| TraitError::IoError(e.to_string()))?;

        #[derive(Deserialize)]
        struct CurveData {
            curve_id: String,
            inputs: Vec<InputData>,
        }

        #[derive(Deserialize)]
        struct InputData {
            tenor: String,
            rate: f64,
        }

        let curves: Vec<CurveData> =
            serde_json::from_str(&content).map_err(|e| TraitError::ParseError(e.to_string()))?;

        for curve in curves {
            let curve_id = CurveId::new(&curve.curve_id);
            let inputs: Vec<CurveInput> = curve
                .inputs
                .into_iter()
                .filter_map(|i| {
                    let tenor = Tenor::parse(&i.tenor).ok()?;
                    Some(CurveInput {
                        curve_id: curve_id.clone(),
                        instrument_type: CurveInstrumentType::Swap,
                        tenor,
                        rate: Decimal::try_from(i.rate).ok()?,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as i64,
                        source: "file".to_string(),
                    })
                })
                .collect();

            self.inputs.insert(curve_id, inputs);
        }

        Ok(())
    }
}

#[async_trait]
impl CurveInputSource for JsonCurveInputSource {
    fn source_type(&self) -> SourceType {
        SourceType::File
    }

    async fn get_curve_inputs(&self, curve_id: &CurveId) -> Result<Vec<CurveInput>, TraitError> {
        Ok(self
            .inputs
            .get(curve_id)
            .map(|v| v.clone())
            .unwrap_or_default())
    }

    async fn subscribe(&self, _curve_ids: &[CurveId]) -> Result<CurveInputReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable(
            "File source does not support streaming".into(),
        ))
    }
}

// =============================================================================
// CSV INDEX FIXING SOURCE
// =============================================================================

/// CSV-based index fixing source.
pub struct CsvIndexFixingSource {
    file_path: PathBuf,
    /// Index fixings cache (reserved for future implementation).
    #[allow(dead_code)]
    fixings: DashMap<(String, i64), IndexFixing>,
}

impl CsvIndexFixingSource {
    /// Create a new CSV index fixing source.
    pub fn new(file_path: impl AsRef<Path>) -> Result<Self, TraitError> {
        let source = Self {
            file_path: file_path.as_ref().to_path_buf(),
            fixings: DashMap::new(),
        };
        source.reload()?;
        Ok(source)
    }

    /// Reload fixings from file.
    pub fn reload(&self) -> Result<(), TraitError> {
        if !self.file_path.exists() {
            return Ok(()); // Empty source
        }

        // Would parse CSV file with format: index,date,rate
        // For now, just return empty
        Ok(())
    }
}

#[async_trait]
impl IndexFixingSource for CsvIndexFixingSource {
    fn source_type(&self) -> SourceType {
        SourceType::File
    }

    async fn get_fixing(
        &self,
        _index: &FloatingRateIndex,
        _date: Date,
    ) -> Result<Option<IndexFixing>, TraitError> {
        Ok(None) // Would look up in fixings map
    }

    async fn get_fixings(
        &self,
        _index: &FloatingRateIndex,
        _from: Date,
        _to: Date,
    ) -> Result<Vec<IndexFixing>, TraitError> {
        Ok(vec![])
    }

    async fn subscribe(
        &self,
        _indices: &[FloatingRateIndex],
    ) -> Result<IndexFixingReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable(
            "File source does not support streaming".into(),
        ))
    }
}

// =============================================================================
// EMPTY IMPLEMENTATIONS
// =============================================================================

/// Empty volatility source.
pub struct EmptyVolatilitySource;

#[async_trait]
impl VolatilitySource for EmptyVolatilitySource {
    async fn get_surface(
        &self,
        _surface_id: &VolSurfaceId,
    ) -> Result<Option<VolatilitySurface>, TraitError> {
        Ok(None)
    }

    async fn get_atm_vol(
        &self,
        _surface_id: &VolSurfaceId,
        _expiry: Tenor,
        _underlying_tenor: Option<Tenor>,
    ) -> Result<Option<Decimal>, TraitError> {
        Ok(None)
    }

    async fn get_vol(
        &self,
        _surface_id: &VolSurfaceId,
        _expiry: Tenor,
        _underlying_tenor: Option<Tenor>,
        _strike: Option<Decimal>,
    ) -> Result<Option<Decimal>, TraitError> {
        Ok(None)
    }

    async fn subscribe(&self, _surface_ids: &[VolSurfaceId]) -> Result<VolReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

/// Empty FX rate source.
pub struct EmptyFxRateSource;

#[async_trait]
impl FxRateSource for EmptyFxRateSource {
    async fn get_spot(&self, _pair: &CurrencyPair) -> Result<Option<FxRate>, TraitError> {
        Ok(None)
    }

    async fn get_spot_triangulated(
        &self,
        _base: convex_core::Currency,
        _quote: convex_core::Currency,
    ) -> Result<Option<FxRate>, TraitError> {
        Ok(None)
    }

    async fn get_forward(
        &self,
        _pair: &CurrencyPair,
        _tenor: Tenor,
    ) -> Result<Option<Decimal>, TraitError> {
        Ok(None)
    }

    async fn subscribe(&self, _pairs: &[CurrencyPair]) -> Result<FxRateReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

/// Empty inflation fixing source.
pub struct EmptyInflationFixingSource;

#[async_trait]
impl InflationFixingSource for EmptyInflationFixingSource {
    async fn get_fixing(
        &self,
        _index: &InflationIndex,
        _month: YearMonth,
    ) -> Result<Option<InflationFixing>, TraitError> {
        Ok(None)
    }

    async fn get_latest_fixing(
        &self,
        _index: &InflationIndex,
    ) -> Result<Option<InflationFixing>, TraitError> {
        Ok(None)
    }

    async fn get_fixings_range(
        &self,
        _index: &InflationIndex,
        _from: YearMonth,
        _to: YearMonth,
    ) -> Result<Vec<InflationFixing>, TraitError> {
        Ok(vec![])
    }

    async fn get_interpolated_index(
        &self,
        _index: &InflationIndex,
        _date: Date,
        _interpolation: InflationInterpolation,
    ) -> Result<Option<Decimal>, TraitError> {
        Ok(None)
    }

    async fn subscribe(
        &self,
        _indices: &[InflationIndex],
    ) -> Result<InflationReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

/// Empty ETF quote source.
pub struct EmptyEtfQuoteSource;

#[async_trait]
impl EtfQuoteSource for EmptyEtfQuoteSource {
    async fn get_quote(&self, _etf_id: &EtfId) -> Result<Option<EtfQuote>, TraitError> {
        Ok(None)
    }

    async fn subscribe(&self, _etf_ids: &[EtfId]) -> Result<EtfQuoteReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

/// Empty quote source for testing.
pub struct EmptyQuoteSource;

#[async_trait]
impl QuoteSource for EmptyQuoteSource {
    fn source_type(&self) -> SourceType {
        SourceType::Manual
    }

    async fn get_quote(
        &self,
        _instrument_id: &InstrumentId,
    ) -> Result<Option<RawQuote>, TraitError> {
        Ok(None)
    }

    async fn get_quotes(
        &self,
        _instrument_ids: &[InstrumentId],
    ) -> Result<Vec<RawQuote>, TraitError> {
        Ok(vec![])
    }

    async fn subscribe(
        &self,
        _instrument_ids: &[InstrumentId],
    ) -> Result<QuoteReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }

    async fn unsubscribe(&self, _instrument_ids: &[InstrumentId]) -> Result<(), TraitError> {
        Ok(())
    }
}

/// Empty curve input source for testing.
pub struct EmptyCurveInputSource;

#[async_trait]
impl CurveInputSource for EmptyCurveInputSource {
    fn source_type(&self) -> SourceType {
        SourceType::Manual
    }

    async fn get_curve_inputs(&self, _curve_id: &CurveId) -> Result<Vec<CurveInput>, TraitError> {
        Ok(vec![])
    }

    async fn subscribe(&self, _curve_ids: &[CurveId]) -> Result<CurveInputReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

/// Empty index fixing source for testing.
pub struct EmptyIndexFixingSource;

#[async_trait]
impl IndexFixingSource for EmptyIndexFixingSource {
    fn source_type(&self) -> SourceType {
        SourceType::Manual
    }

    async fn get_fixing(
        &self,
        _index: &FloatingRateIndex,
        _date: Date,
    ) -> Result<Option<IndexFixing>, TraitError> {
        Ok(None)
    }

    async fn get_fixings(
        &self,
        _index: &FloatingRateIndex,
        _from: Date,
        _to: Date,
    ) -> Result<Vec<IndexFixing>, TraitError> {
        Ok(vec![])
    }

    async fn subscribe(
        &self,
        _indices: &[FloatingRateIndex],
    ) -> Result<IndexFixingReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}
