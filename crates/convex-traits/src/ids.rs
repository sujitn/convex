//! Identifier types used across the pricing engine.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Instrument identifier (ISIN, CUSIP, or internal ID).
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct InstrumentId(pub String);

impl InstrumentId {
    /// Create a new instrument ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstrumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for InstrumentId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for InstrumentId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Curve identifier.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct CurveId(pub String);

impl CurveId {
    /// Create a new curve ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CurveId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for CurveId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for CurveId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Volatility surface identifier.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct VolSurfaceId(pub String);

impl VolSurfaceId {
    /// Create a new vol surface ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Create swaption vol surface ID for a currency.
    pub fn swaption(currency: &str) -> Self {
        Self(format!("{}.SWAPTION", currency.to_uppercase()))
    }

    /// Create cap/floor vol surface ID for a currency.
    pub fn cap_floor(currency: &str) -> Self {
        Self(format!("{}.CAPFLOOR", currency.to_uppercase()))
    }

    /// Get the ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VolSurfaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// ETF identifier.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct EtfId(pub String);

impl EtfId {
    /// Create a new ETF ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EtfId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Portfolio identifier.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct PortfolioId(pub String);

impl PortfolioId {
    /// Create a new portfolio ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PortfolioId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Tenor representation (e.g., 3M, 1Y, 10Y).
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum Tenor {
    /// Days
    Days(u32),
    /// Weeks
    Weeks(u32),
    /// Months
    Months(u32),
    /// Years
    Years(u32),
}

impl Tenor {
    /// Parse tenor from string (e.g., "3M", "1Y", "10Y").
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim().to_uppercase();
        if s.is_empty() {
            return Err("empty tenor string".to_string());
        }

        let (num_str, unit) = s.split_at(s.len() - 1);
        let num: u32 = num_str
            .parse()
            .map_err(|_| format!("invalid tenor number: {}", num_str))?;

        match unit {
            "D" => Ok(Tenor::Days(num)),
            "W" => Ok(Tenor::Weeks(num)),
            "M" => Ok(Tenor::Months(num)),
            "Y" => Ok(Tenor::Years(num)),
            _ => Err(format!("invalid tenor unit: {}", unit)),
        }
    }

    /// Convert tenor to approximate days.
    pub fn to_days(&self) -> u32 {
        match self {
            Tenor::Days(d) => *d,
            Tenor::Weeks(w) => w * 7,
            Tenor::Months(m) => m * 30,
            Tenor::Years(y) => y * 365,
        }
    }
}

impl fmt::Display for Tenor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tenor::Days(d) => write!(f, "{}D", d),
            Tenor::Weeks(w) => write!(f, "{}W", w),
            Tenor::Months(m) => write!(f, "{}M", m),
            Tenor::Years(y) => write!(f, "{}Y", y),
        }
    }
}

/// Year and month for inflation fixings.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct YearMonth {
    /// Year
    pub year: i32,
    /// Month (1-12)
    pub month: u32,
}

impl YearMonth {
    /// Create a new year-month.
    pub fn new(year: i32, month: u32) -> Self {
        Self { year, month }
    }
}

impl fmt::Display for YearMonth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}

/// Currency pair for FX rates.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct CurrencyPair {
    /// Base currency (e.g., EUR in EUR/USD)
    pub base: convex_core::Currency,
    /// Quote currency (e.g., USD in EUR/USD)
    pub quote: convex_core::Currency,
}

impl CurrencyPair {
    /// Create a new currency pair.
    pub fn new(base: convex_core::Currency, quote: convex_core::Currency) -> Self {
        Self { base, quote }
    }

    /// EUR/USD pair.
    pub fn eurusd() -> Self {
        Self::new(convex_core::Currency::EUR, convex_core::Currency::USD)
    }

    /// GBP/USD pair.
    pub fn gbpusd() -> Self {
        Self::new(convex_core::Currency::GBP, convex_core::Currency::USD)
    }

    /// USD/JPY pair.
    pub fn usdjpy() -> Self {
        Self::new(convex_core::Currency::USD, convex_core::Currency::JPY)
    }
}

impl fmt::Display for CurrencyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.base, self.quote)
    }
}

/// Floating rate index (e.g., SOFR, EURIBOR).
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum FloatingRateIndex {
    /// Secured Overnight Financing Rate
    Sofr,
    /// Euro Short-Term Rate
    Estr,
    /// Sterling Overnight Index Average
    Sonia,
    /// Euro Interbank Offered Rate
    Euribor(Tenor),
    /// Term SOFR
    TermSofr(Tenor),
    /// Other index
    Other(String),
}

impl fmt::Display for FloatingRateIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FloatingRateIndex::Sofr => write!(f, "SOFR"),
            FloatingRateIndex::Estr => write!(f, "ESTR"),
            FloatingRateIndex::Sonia => write!(f, "SONIA"),
            FloatingRateIndex::Euribor(t) => write!(f, "EURIBOR-{}", t),
            FloatingRateIndex::TermSofr(t) => write!(f, "TERM-SOFR-{}", t),
            FloatingRateIndex::Other(s) => write!(f, "{}", s),
        }
    }
}

/// Inflation index identifier.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum InflationIndex {
    /// US CPI-U (for TIPS)
    UsCpiU,
    /// UK RPI (for UK Linkers)
    UkRpi,
    /// Eurozone HICP ex-Tobacco
    EuHicp,
    /// French CPI ex-Tobacco
    FrCpi,
    /// Other index
    Other(String),
}

impl fmt::Display for InflationIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InflationIndex::UsCpiU => write!(f, "US-CPI-U"),
            InflationIndex::UkRpi => write!(f, "UK-RPI"),
            InflationIndex::EuHicp => write!(f, "EU-HICP"),
            InflationIndex::FrCpi => write!(f, "FR-CPI"),
            InflationIndex::Other(s) => write!(f, "{}", s),
        }
    }
}
