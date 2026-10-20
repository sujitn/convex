//! Deterministic file snapshots with exact decimals and atomic reload.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::Deserialize;

use convex_core::ids::*;
use convex_core::Date;
use convex_ports::error::TraitError;
use convex_ports::market_data::*;

const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RECORDS: usize = 100_000;
const MAX_CURVE_INPUTS: usize = 4096;

fn read_file(path: &Path) -> Result<Vec<u8>, TraitError> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| TraitError::IoError(e.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|e| TraitError::IoError(e.to_string()))?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(TraitError::CapacityExceeded);
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| TraitError::IoError(e.to_string()))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(TraitError::CapacityExceeded);
    }
    Ok(bytes)
}

fn csv_headers(
    reader: &mut csv::Reader<&[u8]>,
    required: &[&str],
    allowed: &[&str],
) -> Result<(), TraitError> {
    let headers = reader
        .headers()
        .map_err(|e| TraitError::ParseError(e.to_string()))?;
    let mut unique = HashSet::new();
    if headers
        .iter()
        .any(|header| !allowed.contains(&header) || !unique.insert(header))
        || required.iter().any(|header| !unique.contains(header))
    {
        return Err(TraitError::InvalidInput(
            "missing, duplicate or unknown market CSV header",
        ));
    }
    Ok(())
}

fn decimal(value: &str) -> Result<Decimal, TraitError> {
    Decimal::from_str_exact(value).map_err(|e| TraitError::ParseError(e.to_string()))
}

fn metadata(id: &str, source: &str, timestamp: i64) -> Result<(), TraitError> {
    validate_text(id, 128)?;
    validate_text(source, 128)?;
    if timestamp < 0 {
        return Err(TraitError::InvalidInput("negative market timestamp"));
    }
    Ok(())
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> TraitError {
    TraitError::SourceNotAvailable("file snapshot lock poisoned".into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuoteRecord {
    instrument_id: String,
    bid_price: Option<String>,
    ask_price: Option<String>,
    mid_price: Option<String>,
    last_price: Option<String>,
    timestamp: i64,
    source: String,
}

/// CSV bond quote snapshot. Explicit timestamp (UTC milliseconds) and source are required.
pub struct CsvQuoteSource {
    file_path: PathBuf,
    quotes: RwLock<HashMap<InstrumentId, RawQuote>>,
}

impl CsvQuoteSource {
    /// Open and validate a complete snapshot. Missing files are errors.
    pub fn new(file_path: impl AsRef<Path>) -> Result<Self, TraitError> {
        let source = Self {
            file_path: file_path.as_ref().to_path_buf(),
            quotes: RwLock::new(HashMap::new()),
        };
        source.reload()?;
        Ok(source)
    }

    /// Parse the entire bounded file before replacing the snapshot. Failure leaves prior data intact.
    pub fn reload(&self) -> Result<(), TraitError> {
        let bytes = read_file(&self.file_path)?;
        let mut reader = csv::Reader::from_reader(bytes.as_slice());
        csv_headers(
            &mut reader,
            &["instrument_id", "timestamp", "source"],
            &[
                "instrument_id",
                "bid_price",
                "ask_price",
                "mid_price",
                "last_price",
                "timestamp",
                "source",
            ],
        )?;
        let mut quotes = HashMap::new();
        for record in reader.deserialize() {
            let record: QuoteRecord = record.map_err(|e| TraitError::ParseError(e.to_string()))?;
            if quotes.len() >= MAX_RECORDS {
                return Err(TraitError::CapacityExceeded);
            }
            metadata(&record.instrument_id, &record.source, record.timestamp)?;
            let quote = RawQuote {
                instrument_id: InstrumentId::new(record.instrument_id),
                bid_price: record.bid_price.as_deref().map(decimal).transpose()?,
                ask_price: record.ask_price.as_deref().map(decimal).transpose()?,
                mid_price: record.mid_price.as_deref().map(decimal).transpose()?,
                last_price: record.last_price.as_deref().map(decimal).transpose()?,
                bid_yield: None,
                ask_yield: None,
                bid_size: None,
                ask_size: None,
                timestamp: record.timestamp,
                source: record.source,
                venue: None,
            };
            if quote
                .bid_price
                .zip(quote.ask_price)
                .is_some_and(|(bid, ask)| bid > ask)
            {
                return Err(TraitError::InvalidInput("crossed CSV quote"));
            }
            if quotes.insert(quote.instrument_id.clone(), quote).is_some() {
                return Err(TraitError::Conflict("duplicate quote key"));
            }
        }
        *self.quotes.write().map_err(lock_error)? = quotes;
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
        Ok(self
            .quotes
            .read()
            .map_err(lock_error)?
            .get(instrument_id)
            .cloned())
    }
    async fn get_quotes(
        &self,
        instrument_ids: &[InstrumentId],
    ) -> Result<Vec<RawQuote>, TraitError> {
        if instrument_ids.len() > MAX_RECORDS {
            return Err(TraitError::CapacityExceeded);
        }
        let quotes = self.quotes.read().map_err(lock_error)?;
        Ok(instrument_ids
            .iter()
            .filter_map(|id| quotes.get(id).cloned())
            .collect())
    }
    async fn subscribe(
        &self,
        _ids: &[InstrumentId],
    ) -> Result<StreamReceiver<QuoteEvent>, TraitError> {
        Err(TraitError::Unsupported(
            "snapshot source; use FileQuoteReplay for sequenced events",
        ))
    }
    async fn unsubscribe(&self, _ids: &[InstrumentId]) -> Result<(), TraitError> {
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CurveRecord {
    curve_id: String,
    inputs: Vec<InputRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputRecord {
    tenor: String,
    rate: String,
    instrument_type: CurveInstrumentType,
    timestamp: i64,
    source: String,
}

/// JSON curve inputs with required instrument type, exact rate string and recorded time.
pub struct JsonCurveInputSource {
    file_path: PathBuf,
    inputs: RwLock<HashMap<CurveId, Vec<CurveInput>>>,
}

impl JsonCurveInputSource {
    /// Open and validate a complete curve-input snapshot.
    pub fn new(file_path: impl AsRef<Path>) -> Result<Self, TraitError> {
        let source = Self {
            file_path: file_path.as_ref().to_path_buf(),
            inputs: RwLock::new(HashMap::new()),
        };
        source.reload()?;
        Ok(source)
    }
    /// Replace all inputs atomically; no silent drops or invented Swap instrument types.
    pub fn reload(&self) -> Result<(), TraitError> {
        let curves: Vec<CurveRecord> = serde_json::from_slice(&read_file(&self.file_path)?)
            .map_err(|e| TraitError::ParseError(e.to_string()))?;
        if curves.len() > MAX_RECORDS {
            return Err(TraitError::CapacityExceeded);
        }
        let mut all = HashMap::new();
        let mut total = 0;
        for curve in curves {
            validate_text(&curve.curve_id, 128)?;
            if curve.inputs.len() > MAX_CURVE_INPUTS {
                return Err(TraitError::CapacityExceeded);
            }
            total += curve.inputs.len();
            if total > MAX_RECORDS {
                return Err(TraitError::CapacityExceeded);
            }
            let id = CurveId::new(curve.curve_id);
            let mut inputs = Vec::new();
            for input in curve.inputs {
                metadata(id.as_str(), &input.source, input.timestamp)?;
                let tenor = tenor(&input.tenor)?;
                match &input.instrument_type {
                    CurveInstrumentType::Future { contract_code } => {
                        validate_text(contract_code, 128)?
                    }
                    CurveInstrumentType::Fra { start_tenor } => validate_tenor(start_tenor)?,
                    CurveInstrumentType::BondYield { instrument_id } => {
                        validate_text(instrument_id.as_str(), 128)?
                    }
                    _ => {}
                }
                inputs.push(CurveInput {
                    curve_id: id.clone(),
                    tenor,
                    rate: decimal(&input.rate)?,
                    instrument_type: input.instrument_type,
                    timestamp: input.timestamp,
                    source: input.source,
                });
            }
            if all.insert(id, inputs).is_some() {
                return Err(TraitError::Conflict("duplicate curve key"));
            }
        }
        *self.inputs.write().map_err(lock_error)? = all;
        Ok(())
    }
}

#[async_trait]
impl CurveInputSource for JsonCurveInputSource {
    fn source_type(&self) -> SourceType {
        SourceType::File
    }
    async fn get_curve_inputs(&self, id: &CurveId) -> Result<Vec<CurveInput>, TraitError> {
        Ok(self
            .inputs
            .read()
            .map_err(lock_error)?
            .get(id)
            .cloned()
            .unwrap_or_default())
    }
    async fn subscribe(&self, _ids: &[CurveId]) -> Result<StreamReceiver<CurveInput>, TraitError> {
        Err(TraitError::Unsupported(
            "file curve input source is snapshot-only",
        ))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixingRecord {
    index: String,
    date: String,
    rate: String,
    timestamp: i64,
    source: String,
}

/// CSV fixing snapshots with actual date/index/rate records. Revisions require a replacement snapshot.
pub struct CsvIndexFixingSource {
    file_path: PathBuf,
    fixings: RwLock<HashMap<(String, Date), IndexFixing>>,
}

fn tenor(value: &str) -> Result<Tenor, TraitError> {
    // Guard the existing tenor parser's byte split and arithmetic at this external boundary.
    if value.len() < 2 || value.len() > 12 || !value.is_ascii() {
        return Err(TraitError::InvalidInput("invalid market tenor"));
    }
    let tenor = Tenor::parse(value).map_err(TraitError::ParseError)?;
    validate_tenor(&tenor)?;
    Ok(tenor)
}

fn validate_tenor(tenor: &Tenor) -> Result<(), TraitError> {
    let valid = match *tenor {
        Tenor::Days(n) => n > 0 && n <= 365_000,
        Tenor::Weeks(n) => n > 0 && n <= 52_000,
        Tenor::Months(n) => n > 0 && n <= 12_000,
        Tenor::Years(n) => n > 0 && n <= 1_000,
    };
    if !valid {
        return Err(TraitError::InvalidInput("market tenor out of bounds"));
    }
    Ok(())
}

fn index(value: &str) -> Result<FloatingRateIndex, TraitError> {
    validate_text(value, 128)?;
    match value {
        "SOFR" => Ok(FloatingRateIndex::Sofr),
        "ESTR" => Ok(FloatingRateIndex::Estr),
        "SONIA" => Ok(FloatingRateIndex::Sonia),
        _ if value.starts_with("EURIBOR-") => Ok(FloatingRateIndex::Euribor(tenor(&value[8..])?)),
        _ if value.starts_with("TERM-SOFR-") => {
            Ok(FloatingRateIndex::TermSofr(tenor(&value[10..])?))
        }
        _ => Ok(FloatingRateIndex::Other(value.to_owned())),
    }
}

impl CsvIndexFixingSource {
    /// Open and validate fixing records. Missing files are errors.
    pub fn new(file_path: impl AsRef<Path>) -> Result<Self, TraitError> {
        let source = Self {
            file_path: file_path.as_ref().to_path_buf(),
            fixings: RwLock::new(HashMap::new()),
        };
        source.reload()?;
        Ok(source)
    }
    /// Reload atomically; duplicate index/date records and invalid records fail.
    pub fn reload(&self) -> Result<(), TraitError> {
        let bytes = read_file(&self.file_path)?;
        let mut reader = csv::Reader::from_reader(bytes.as_slice());
        csv_headers(
            &mut reader,
            &["index", "date", "rate", "timestamp", "source"],
            &["index", "date", "rate", "timestamp", "source"],
        )?;
        let mut fixings = HashMap::new();
        for record in reader.deserialize() {
            let record: FixingRecord = record.map_err(|e| TraitError::ParseError(e.to_string()))?;
            if fixings.len() >= MAX_RECORDS {
                return Err(TraitError::CapacityExceeded);
            }
            metadata(&record.index, &record.source, record.timestamp)?;
            let index = index(&record.index)?;
            let date =
                Date::parse(&record.date).map_err(|e| TraitError::ParseError(e.to_string()))?;
            let fixing = IndexFixing {
                index: index.clone(),
                date,
                rate: decimal(&record.rate)?,
                source: record.source,
                timestamp: record.timestamp,
            };
            if fixings.insert((index.to_string(), date), fixing).is_some() {
                return Err(TraitError::Conflict("duplicate fixing index/date"));
            }
        }
        *self.fixings.write().map_err(lock_error)? = fixings;
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
        index: &FloatingRateIndex,
        date: Date,
    ) -> Result<Option<IndexFixing>, TraitError> {
        Ok(self
            .fixings
            .read()
            .map_err(lock_error)?
            .get(&(index.to_string(), date))
            .cloned())
    }
    async fn get_fixings(
        &self,
        index: &FloatingRateIndex,
        from: Date,
        to: Date,
    ) -> Result<Vec<IndexFixing>, TraitError> {
        if from > to {
            return Err(TraitError::InvalidInput("inverted fixing date range"));
        }
        let fixings = self.fixings.read().map_err(lock_error)?;
        let mut result: Vec<_> = fixings
            .values()
            .filter(|f| &f.index == index && f.date >= from && f.date <= to)
            .cloned()
            .collect();
        result.sort_by_key(|f| f.date);
        Ok(result)
    }
    async fn subscribe(
        &self,
        _indices: &[FloatingRateIndex],
    ) -> Result<StreamReceiver<IndexFixing>, TraitError> {
        Err(TraitError::Unsupported(
            "file fixing source is snapshot-only",
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

    async fn subscribe(
        &self,
        _surface_ids: &[VolSurfaceId],
    ) -> Result<StreamReceiver<VolatilitySurface>, TraitError> {
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

    async fn subscribe(
        &self,
        _pairs: &[CurrencyPair],
    ) -> Result<StreamReceiver<FxRate>, TraitError> {
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
    ) -> Result<StreamReceiver<InflationFixing>, TraitError> {
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

    async fn subscribe(&self, _etf_ids: &[EtfId]) -> Result<StreamReceiver<EtfQuote>, TraitError> {
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
    ) -> Result<StreamReceiver<QuoteEvent>, TraitError> {
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

    async fn subscribe(
        &self,
        _curve_ids: &[CurveId],
    ) -> Result<StreamReceiver<CurveInput>, TraitError> {
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
    ) -> Result<StreamReceiver<IndexFixing>, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}
