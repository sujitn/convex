//! Core storage types.
//!
//! This module defines the data structures stored in the persistence layer.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use convex_core::types::{Compounding, Currency, Frequency};

// =============================================================================
// VERSIONED WRAPPER
// =============================================================================

/// A versioned wrapper for any serializable type.
///
/// Provides version tracking for optimistic locking and audit trails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Versioned<T> {
    /// The wrapped data.
    pub data: T,
    /// Version number (incremented on each update).
    pub version: u64,
    /// Timestamp when this version was created.
    pub created_at: DateTime<Utc>,
    /// User/system that created this version.
    pub created_by: Option<String>,
    /// Optional description of the change.
    pub change_description: Option<String>,
}

impl<T> Versioned<T> {
    /// Creates a new versioned wrapper with version 1.
    pub fn new(data: T, created_by: Option<String>) -> Self {
        Self {
            data,
            version: 1,
            created_at: Utc::now(),
            created_by,
            change_description: None,
        }
    }

    /// Creates a new version from an existing one.
    pub fn next_version(data: T, previous: &Versioned<T>, created_by: Option<String>) -> Self {
        Self {
            data,
            version: previous.version + 1,
            created_at: Utc::now(),
            created_by,
            change_description: None,
        }
    }

    /// Sets the change description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.change_description = Some(description.into());
        self
    }
}

// =============================================================================
// SECURITY MASTER
// =============================================================================

/// Security master record containing reference data for a bond.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityMaster {
    /// Primary identifier (ISIN, CUSIP, or internal ID).
    pub id: String,

    /// ISIN if available.
    pub isin: Option<String>,

    /// CUSIP if available.
    pub cusip: Option<String>,

    /// FIGI if available.
    pub figi: Option<String>,

    /// SEDOL if available.
    pub sedol: Option<String>,

    /// Ticker symbol.
    pub ticker: Option<String>,

    /// Issuer name.
    pub issuer: String,

    /// Issue/security name.
    pub name: Option<String>,

    /// Currency.
    pub currency: Currency,

    /// Issue date.
    pub issue_date: String,

    /// Maturity date (None for perpetuals).
    pub maturity_date: Option<String>,

    /// Coupon rate (as decimal, e.g., 0.05 for 5%).
    pub coupon_rate: Decimal,

    /// Coupon frequency.
    pub frequency: Frequency,

    /// Day count convention.
    pub day_count: String,

    /// Face value.
    pub face_value: Decimal,

    /// Security type (FixedRate, ZeroCoupon, FloatingRate, etc.).
    pub security_type: SecurityType,

    /// Sector classification.
    pub sector: Option<String>,

    /// Credit rating (e.g., "AAA", "BBB+").
    pub rating: Option<String>,

    /// Seniority level.
    pub seniority: Option<String>,

    /// Whether the bond is callable.
    pub is_callable: bool,

    /// Whether the bond is putable.
    pub is_putable: bool,

    /// Additional metadata as key-value pairs.
    pub metadata: HashMap<String, String>,

    /// Record status (Active, Matured, Defaulted, etc.).
    pub status: SecurityStatus,

    /// Timestamp when record was last updated.
    pub last_updated: DateTime<Utc>,
}

impl SecurityMaster {
    /// Creates a new security master builder.
    pub fn builder(id: impl Into<String>, issuer: impl Into<String>) -> SecurityMasterBuilder {
        SecurityMasterBuilder::new(id, issuer)
    }

    /// Returns the primary identifier.
    pub fn primary_id(&self) -> &str {
        self.isin
            .as_deref()
            .or(self.cusip.as_deref())
            .or(self.figi.as_deref())
            .unwrap_or(&self.id)
    }
}

/// Builder for SecurityMaster.
#[derive(Debug)]
pub struct SecurityMasterBuilder {
    id: String,
    issuer: String,
    isin: Option<String>,
    cusip: Option<String>,
    figi: Option<String>,
    sedol: Option<String>,
    ticker: Option<String>,
    name: Option<String>,
    currency: Currency,
    issue_date: String,
    maturity_date: Option<String>,
    coupon_rate: Decimal,
    frequency: Frequency,
    day_count: String,
    face_value: Decimal,
    security_type: SecurityType,
    sector: Option<String>,
    rating: Option<String>,
    seniority: Option<String>,
    is_callable: bool,
    is_putable: bool,
    metadata: HashMap<String, String>,
    status: SecurityStatus,
}

impl SecurityMasterBuilder {
    /// Creates a new builder with required fields.
    pub fn new(id: impl Into<String>, issuer: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            issuer: issuer.into(),
            isin: None,
            cusip: None,
            figi: None,
            sedol: None,
            ticker: None,
            name: None,
            currency: Currency::USD,
            issue_date: String::new(),
            maturity_date: None,
            coupon_rate: Decimal::ZERO,
            frequency: Frequency::SemiAnnual,
            day_count: "30/360".to_string(),
            face_value: Decimal::from(100),
            security_type: SecurityType::FixedRate,
            sector: None,
            rating: None,
            seniority: None,
            is_callable: false,
            is_putable: false,
            metadata: HashMap::new(),
            status: SecurityStatus::Active,
        }
    }

    /// Sets the ISIN.
    pub fn isin(mut self, isin: impl Into<String>) -> Self {
        self.isin = Some(isin.into());
        self
    }

    /// Sets the CUSIP.
    pub fn cusip(mut self, cusip: impl Into<String>) -> Self {
        self.cusip = Some(cusip.into());
        self
    }

    /// Sets the currency.
    pub fn currency(mut self, currency: Currency) -> Self {
        self.currency = currency;
        self
    }

    /// Sets the issue date.
    pub fn issue_date(mut self, date: impl Into<String>) -> Self {
        self.issue_date = date.into();
        self
    }

    /// Sets the maturity date.
    pub fn maturity_date(mut self, date: impl Into<String>) -> Self {
        self.maturity_date = Some(date.into());
        self
    }

    /// Sets the coupon rate.
    pub fn coupon_rate(mut self, rate: Decimal) -> Self {
        self.coupon_rate = rate;
        self
    }

    /// Sets the frequency.
    pub fn frequency(mut self, freq: Frequency) -> Self {
        self.frequency = freq;
        self
    }

    /// Sets the day count convention.
    pub fn day_count(mut self, dc: impl Into<String>) -> Self {
        self.day_count = dc.into();
        self
    }

    /// Sets the security type.
    pub fn security_type(mut self, st: SecurityType) -> Self {
        self.security_type = st;
        self
    }

    /// Sets the sector.
    pub fn sector(mut self, sector: impl Into<String>) -> Self {
        self.sector = Some(sector.into());
        self
    }

    /// Sets the rating.
    pub fn rating(mut self, rating: impl Into<String>) -> Self {
        self.rating = Some(rating.into());
        self
    }

    /// Sets whether the bond is callable.
    pub fn callable(mut self, is_callable: bool) -> Self {
        self.is_callable = is_callable;
        self
    }

    /// Adds metadata.
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Builds the SecurityMaster.
    pub fn build(self) -> SecurityMaster {
        SecurityMaster {
            id: self.id,
            isin: self.isin,
            cusip: self.cusip,
            figi: self.figi,
            sedol: self.sedol,
            ticker: self.ticker,
            issuer: self.issuer,
            name: self.name,
            currency: self.currency,
            issue_date: self.issue_date,
            maturity_date: self.maturity_date,
            coupon_rate: self.coupon_rate,
            frequency: self.frequency,
            day_count: self.day_count,
            face_value: self.face_value,
            security_type: self.security_type,
            sector: self.sector,
            rating: self.rating,
            seniority: self.seniority,
            is_callable: self.is_callable,
            is_putable: self.is_putable,
            metadata: self.metadata,
            status: self.status,
            last_updated: Utc::now(),
        }
    }
}

/// Security type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SecurityType {
    /// Fixed rate coupon bond.
    #[default]
    FixedRate,
    /// Zero coupon bond.
    ZeroCoupon,
    /// Floating rate note.
    FloatingRate,
    /// Callable bond.
    Callable,
    /// Putable bond.
    Putable,
    /// Convertible bond.
    Convertible,
    /// Inflation-linked bond.
    InflationLinked,
    /// Step-up/step-down coupon.
    StepUp,
    /// Amortizing bond.
    Amortizing,
    /// Perpetual bond.
    Perpetual,
}

/// Security status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SecurityStatus {
    /// Active and trading.
    #[default]
    Active,
    /// Matured normally.
    Matured,
    /// Called by issuer.
    Called,
    /// Defaulted.
    Defaulted,
    /// Suspended from trading.
    Suspended,
    /// Delisted.
    Delisted,
}

// =============================================================================
// CURVE SNAPSHOT
// =============================================================================

/// A point-in-time snapshot of a yield curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveSnapshot {
    /// Curve identifier (e.g., "USD.SOFR.OIS", "EUR.GOVT").
    pub name: String,

    /// Reference date for the curve.
    pub reference_date: String,

    /// Timestamp when the curve was built.
    pub build_time: DateTime<Utc>,

    /// Time taken to build the curve in microseconds.
    pub build_duration_us: u64,

    /// Curve points.
    pub points: Vec<CurvePoint>,

    /// Input instruments used in calibration.
    pub inputs: Vec<CurveInput>,

    /// Interpolation method used.
    pub interpolation: String,

    /// Day count convention.
    pub day_count: String,

    /// Compounding convention.
    pub compounding: Compounding,

    /// Checksum for change detection.
    pub checksum: String,

    /// Additional metadata.
    pub metadata: HashMap<String, String>,
}

/// A single point on a curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvePoint {
    /// Tenor as string (e.g., "1M", "2Y", "10Y").
    pub tenor: String,

    /// Tenor in years.
    pub years: f64,

    /// Zero rate at this point.
    pub zero_rate: f64,

    /// Discount factor at this point.
    pub discount_factor: f64,

    /// Instantaneous forward rate (optional).
    pub forward_rate: Option<f64>,
}

/// Input instrument used in curve calibration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveInput {
    /// Instrument type (Deposit, Swap, OIS, FRA, Bond).
    pub instrument_type: String,

    /// Tenor or maturity.
    pub tenor: String,

    /// Market rate or price.
    pub value: f64,

    /// Data source.
    pub source: Option<String>,

    /// Timestamp of the quote.
    pub timestamp: Option<DateTime<Utc>>,
}

// =============================================================================
// QUOTE HISTORY
// =============================================================================

/// Historical quote record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteRecord {
    /// Security identifier.
    pub security_id: String,

    /// Quote timestamp.
    pub timestamp: DateTime<Utc>,

    /// Quote source (Bloomberg, Refinitiv, etc.).
    pub source: String,

    /// Bid price.
    pub bid: Option<Decimal>,

    /// Ask price.
    pub ask: Option<Decimal>,

    /// Mid price.
    pub mid: Option<Decimal>,

    /// Last trade price.
    pub last: Option<Decimal>,

    /// Bid size.
    pub bid_size: Option<Decimal>,

    /// Ask size.
    pub ask_size: Option<Decimal>,

    /// Yield to maturity (if pre-calculated).
    pub ytm: Option<f64>,

    /// Z-spread (if pre-calculated).
    pub z_spread: Option<f64>,

    /// Quote condition/quality.
    pub condition: QuoteCondition,
}

/// Quote condition/quality indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum QuoteCondition {
    /// Fresh, executable quote.
    #[default]
    Firm,
    /// Indicative only.
    Indicative,
    /// Stale quote.
    Stale,
    /// Market closed.
    Closed,
    /// Trading halted.
    Halted,
}

// =============================================================================
// TIME RANGE
// =============================================================================

/// Time range for queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    /// Start of range (inclusive).
    pub from: DateTime<Utc>,
    /// End of range (inclusive).
    pub to: DateTime<Utc>,
}

impl TimeRange {
    /// Creates a new time range.
    pub fn new(from: DateTime<Utc>, to: DateTime<Utc>) -> Self {
        Self { from, to }
    }

    /// Creates a range from now back to the specified duration.
    pub fn last(duration: chrono::Duration) -> Self {
        let now = Utc::now();
        Self {
            from: now - duration,
            to: now,
        }
    }

    /// Creates a range for today.
    pub fn today() -> Self {
        let now = Utc::now();
        let start = now.date_naive().and_hms_opt(0, 0, 0).unwrap();
        Self {
            from: DateTime::from_naive_utc_and_offset(start, Utc),
            to: now,
        }
    }

    /// Checks if a timestamp is within the range.
    pub fn contains(&self, ts: DateTime<Utc>) -> bool {
        ts >= self.from && ts <= self.to
    }
}

// =============================================================================
// CONFIG TYPES
// =============================================================================

/// Configuration record for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigRecord {
    /// Configuration key/name.
    pub key: String,

    /// Configuration type (Curve, Pricing, Override, etc.).
    pub config_type: String,

    /// Serialized configuration data as JSON.
    pub data: String,

    /// Version for optimistic locking.
    pub version: u64,

    /// Timestamp when created/updated.
    pub updated_at: DateTime<Utc>,

    /// User/system that made the change.
    pub updated_by: Option<String>,

    /// Whether this config is active.
    pub is_active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_versioned_new() {
        let data = "test data";
        let versioned = Versioned::new(data, Some("user1".to_string()));

        assert_eq!(versioned.version, 1);
        assert_eq!(versioned.data, "test data");
        assert_eq!(versioned.created_by, Some("user1".to_string()));
    }

    #[test]
    fn test_versioned_next() {
        let v1 = Versioned::new("data1", Some("user1".to_string()));
        let v2 = Versioned::next_version("data2", &v1, Some("user2".to_string()));

        assert_eq!(v2.version, 2);
        assert_eq!(v2.data, "data2");
        assert_eq!(v2.created_by, Some("user2".to_string()));
    }

    #[test]
    fn test_security_master_builder() {
        let security = SecurityMaster::builder("TEST001", "Test Issuer")
            .isin("US1234567890")
            .cusip("123456789")
            .currency(Currency::USD)
            .issue_date("2020-01-15")
            .maturity_date("2030-01-15")
            .coupon_rate(dec!(0.05))
            .frequency(Frequency::SemiAnnual)
            .sector("Technology")
            .rating("AA")
            .build();

        assert_eq!(security.id, "TEST001");
        assert_eq!(security.issuer, "Test Issuer");
        assert_eq!(security.isin, Some("US1234567890".to_string()));
        assert_eq!(security.coupon_rate, dec!(0.05));
        assert_eq!(security.currency, Currency::USD);
    }

    #[test]
    fn test_time_range_contains() {
        let now = Utc::now();
        let range = TimeRange::new(
            now - chrono::Duration::hours(1),
            now + chrono::Duration::seconds(1),
        );

        assert!(range.contains(now));
        assert!(!range.contains(now - chrono::Duration::hours(2)));
    }
}
