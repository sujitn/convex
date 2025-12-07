//! Index conventions for rate indices.
//!
//! Provides comprehensive convention definitions for all major rate indices,
//! including publication times, fixing calendars, and data source information.

use convex_core::daycounts::DayCountConvention;
use convex_core::types::Currency;
use serde::{Deserialize, Serialize};

use crate::types::{CalendarId, RateIndex, Tenor};

/// Common conventions for overnight rate compounding in arrears.
///
/// This is a generalized structure that can be used for SOFR, SONIA, €STR,
/// SARON, and other overnight rates that compound in arrears.
///
/// # ISDA Standard
///
/// The ISDA 2021 definitions standardize lookback and observation shift
/// conventions for overnight RFRs.
///
/// # Example
///
/// ```rust,ignore
/// use convex_bonds::indices::ArrearConvention;
///
/// // ARRC standard for SOFR
/// let conv = ArrearConvention::arrc_sofr();
/// assert_eq!(conv.lookback_days, 2);
///
/// // Loan market convention with lockout
/// let loan_conv = ArrearConvention::loan_convention();
/// assert_eq!(loan_conv.lockout_days, Some(2));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArrearConvention {
    /// Lookback/observation shift days.
    ///
    /// Number of business days to shift the observation window backward
    /// from the interest period. Typical values: 2-5 days.
    pub lookback_days: u32,

    /// Whether to shift observation period or payment weights.
    pub shift_type: ShiftType,

    /// Lockout days before payment.
    ///
    /// If set, the rate is frozen for the final N days of the period,
    /// using the rate from the lockout start date. Common in loan markets.
    pub lockout_days: Option<u32>,

    /// Floor on daily rate.
    ///
    /// If set, negative daily rates are floored at this value.
    /// Common in FRNs to protect against deep negative rates.
    pub daily_floor: Option<f64>,
}

impl ArrearConvention {
    /// Creates a new arrear convention.
    #[must_use]
    pub fn new(lookback_days: u32, shift_type: ShiftType) -> Self {
        Self {
            lookback_days,
            shift_type,
            lockout_days: None,
            daily_floor: None,
        }
    }

    /// ISDA standard: 2 business day lookback with observation shift.
    #[must_use]
    pub fn isda_standard() -> Self {
        Self {
            lookback_days: 2,
            shift_type: ShiftType::ObservationShift,
            lockout_days: None,
            daily_floor: None,
        }
    }

    /// ARRC recommended for SOFR: 2 day lookback, no lockout.
    #[must_use]
    pub fn arrc_sofr() -> Self {
        Self {
            lookback_days: 2,
            shift_type: ShiftType::ObservationShift,
            lockout_days: None,
            daily_floor: None,
        }
    }

    /// ARRC recommended for SOFR FRNs: 5 day lookback.
    #[must_use]
    pub fn arrc_sofr_frn() -> Self {
        Self {
            lookback_days: 5,
            shift_type: ShiftType::ObservationShift,
            lockout_days: None,
            daily_floor: None,
        }
    }

    /// Common loan convention: 5 day lookback with 2 day lockout.
    #[must_use]
    pub fn loan_convention() -> Self {
        Self {
            lookback_days: 5,
            shift_type: ShiftType::Lookback,
            lockout_days: Some(2),
            daily_floor: None,
        }
    }

    /// SONIA market standard.
    #[must_use]
    pub fn sonia_standard() -> Self {
        Self {
            lookback_days: 5,
            shift_type: ShiftType::ObservationShift,
            lockout_days: None,
            daily_floor: None,
        }
    }

    /// €STR market standard.
    #[must_use]
    pub fn estr_standard() -> Self {
        Self {
            lookback_days: 2,
            shift_type: ShiftType::ObservationShift,
            lockout_days: None,
            daily_floor: None,
        }
    }

    /// SARON market standard.
    #[must_use]
    pub fn saron_standard() -> Self {
        Self {
            lookback_days: 2,
            shift_type: ShiftType::ObservationShift,
            lockout_days: None,
            daily_floor: None,
        }
    }

    /// Adds a lockout period.
    #[must_use]
    pub fn with_lockout(mut self, days: u32) -> Self {
        self.lockout_days = Some(days);
        self
    }

    /// Adds a daily floor.
    #[must_use]
    pub fn with_floor(mut self, floor: f64) -> Self {
        self.daily_floor = Some(floor);
        self
    }
}

impl Default for ArrearConvention {
    fn default() -> Self {
        Self::isda_standard()
    }
}

/// How observation dates relate to accrual dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ShiftType {
    /// Shift observation dates back (ISDA standard).
    ///
    /// The observation period is shifted backward by lookback days,
    /// maintaining a 1:1 relationship between observation and accrual days.
    #[default]
    ObservationShift,

    /// Shift payment weights (alternative).
    ///
    /// Observation dates remain aligned with accrual dates,
    /// but the weighting is adjusted.
    PaymentShift,

    /// No shift - use actual dates with lookback.
    ///
    /// Each accrual day looks back N business days for its rate.
    Lookback,
}

/// Publication time for rate fixings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PublicationTime {
    /// Published morning before trading (T).
    MorningT,
    /// Published end of day (T).
    EndOfDayT,
    /// Published next morning (T+1 for T's rate).
    MorningT1,
}

/// Official source for rate index fixings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IndexSource {
    /// Federal Reserve Bank of New York (SOFR).
    FederalReserveNY,
    /// Bank of England (SONIA).
    BankOfEngland,
    /// European Central Bank (€STR).
    ECB,
    /// Bank of Japan (TONA).
    BankOfJapan,
    /// SIX Swiss Exchange (SARON).
    SIX,
    /// CME Group (Term SOFR).
    CME,
    /// European Money Markets Institute (EURIBOR).
    EMMI,
    /// Japanese Bankers Association (TIBOR).
    JBA,
    /// Bureau of Labor Statistics (US CPI).
    BLS,
    /// Office for National Statistics (UK CPI/RPI).
    ONS,
    /// Eurostat (HICP).
    Eurostat,
    /// ICE Benchmark Administration (legacy LIBOR).
    IBA,
    /// Custom source.
    Custom(String),
}

impl std::fmt::Display for IndexSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexSource::FederalReserveNY => write!(f, "Federal Reserve Bank of New York"),
            IndexSource::BankOfEngland => write!(f, "Bank of England"),
            IndexSource::ECB => write!(f, "European Central Bank"),
            IndexSource::BankOfJapan => write!(f, "Bank of Japan"),
            IndexSource::SIX => write!(f, "SIX Swiss Exchange"),
            IndexSource::CME => write!(f, "CME Group"),
            IndexSource::EMMI => write!(f, "European Money Markets Institute"),
            IndexSource::JBA => write!(f, "Japanese Bankers Association"),
            IndexSource::BLS => write!(f, "Bureau of Labor Statistics"),
            IndexSource::ONS => write!(f, "Office for National Statistics"),
            IndexSource::Eurostat => write!(f, "Eurostat"),
            IndexSource::IBA => write!(f, "ICE Benchmark Administration"),
            IndexSource::Custom(s) => write!(f, "{s}"),
        }
    }
}

/// Complete conventions for a rate index.
///
/// Provides all market convention details needed for accurate rate calculations,
/// including day counts, fixing calendars, publication times, and data source
/// identifiers.
#[derive(Debug, Clone)]
pub struct IndexConventions {
    /// Day count for accrual.
    pub day_count: DayCountConvention,

    /// Currency of the index.
    pub currency: Currency,

    /// Fixing calendar.
    pub calendar: CalendarId,

    /// Days before accrual start for fixing (negative = before).
    pub fixing_lag: i32,

    /// Publication time.
    pub publication_time: Option<PublicationTime>,

    /// Spot lag for instruments referencing this index.
    pub spot_lag: u32,

    /// Whether index can go negative.
    pub allows_negative: bool,

    /// Official source.
    pub source: IndexSource,

    /// Bloomberg ticker.
    pub bloomberg_ticker: Option<String>,

    /// Refinitiv RIC.
    pub refinitiv_ric: Option<String>,

    /// Default arrear convention (for overnight rates).
    pub arrear_convention: Option<ArrearConvention>,
}

impl IndexConventions {
    /// Gets conventions for a rate index.
    #[must_use]
    pub fn for_index(index: &RateIndex) -> Self {
        match index {
            RateIndex::SOFR => Self::sofr(),
            RateIndex::SONIA => Self::sonia(),
            RateIndex::ESTR => Self::estr(),
            RateIndex::TONA => Self::tona(),
            RateIndex::SARON => Self::saron(),
            RateIndex::CORRA => Self::corra(),
            RateIndex::AONIA => Self::aonia(),
            RateIndex::TermSOFR { tenor } => Self::term_sofr(*tenor),
            RateIndex::TermSONIA { tenor } => Self::term_sonia(*tenor),
            RateIndex::EURIBOR { tenor } => Self::euribor(*tenor),
            RateIndex::TIBOR { tenor } => Self::tibor(*tenor),
            RateIndex::LIBOR { currency, tenor } => Self::libor(*currency, *tenor),
            RateIndex::Custom { currency, .. } => Self::custom(*currency),
        }
    }

    /// SOFR conventions.
    #[must_use]
    pub fn sofr() -> Self {
        Self {
            day_count: DayCountConvention::Act360,
            currency: Currency::USD,
            calendar: CalendarId::us_government(),
            fixing_lag: 0,
            publication_time: Some(PublicationTime::MorningT1),
            spot_lag: 2,
            allows_negative: true,
            source: IndexSource::FederalReserveNY,
            bloomberg_ticker: Some("SOFRRATE Index".to_string()),
            refinitiv_ric: Some("USDSOFR=".to_string()),
            arrear_convention: Some(ArrearConvention::arrc_sofr()),
        }
    }

    /// SONIA conventions.
    #[must_use]
    pub fn sonia() -> Self {
        Self {
            day_count: DayCountConvention::Act365Fixed,
            currency: Currency::GBP,
            calendar: CalendarId::uk(),
            fixing_lag: 0,
            publication_time: Some(PublicationTime::MorningT1),
            spot_lag: 0, // Same-day settlement for GBP
            allows_negative: true,
            source: IndexSource::BankOfEngland,
            bloomberg_ticker: Some("SONIA Index".to_string()),
            refinitiv_ric: Some("SONIA=".to_string()),
            arrear_convention: Some(ArrearConvention::sonia_standard()),
        }
    }

    /// €STR conventions.
    #[must_use]
    pub fn estr() -> Self {
        Self {
            day_count: DayCountConvention::Act360,
            currency: Currency::EUR,
            calendar: CalendarId::target2(),
            fixing_lag: 0,
            publication_time: Some(PublicationTime::MorningT1),
            spot_lag: 2,
            allows_negative: true,
            source: IndexSource::ECB,
            bloomberg_ticker: Some("ESTRON Index".to_string()),
            refinitiv_ric: Some("EUROSTR=".to_string()),
            arrear_convention: Some(ArrearConvention::estr_standard()),
        }
    }

    /// TONA conventions.
    #[must_use]
    pub fn tona() -> Self {
        Self {
            day_count: DayCountConvention::Act365Fixed,
            currency: Currency::JPY,
            calendar: CalendarId::japan(),
            fixing_lag: 0,
            publication_time: Some(PublicationTime::MorningT1),
            spot_lag: 2,
            allows_negative: true,
            source: IndexSource::BankOfJapan,
            bloomberg_ticker: Some("TONARTR Index".to_string()),
            refinitiv_ric: None,
            arrear_convention: Some(ArrearConvention::isda_standard()),
        }
    }

    /// SARON conventions.
    #[must_use]
    pub fn saron() -> Self {
        Self {
            day_count: DayCountConvention::Act360,
            currency: Currency::CHF,
            calendar: CalendarId::new("CHF"),
            fixing_lag: 0,
            publication_time: Some(PublicationTime::EndOfDayT),
            spot_lag: 2,
            allows_negative: true,
            source: IndexSource::SIX,
            bloomberg_ticker: Some("SRFXON3 Index".to_string()),
            refinitiv_ric: Some("SARON=".to_string()),
            arrear_convention: Some(ArrearConvention::saron_standard()),
        }
    }

    /// CORRA conventions.
    #[must_use]
    pub fn corra() -> Self {
        Self {
            day_count: DayCountConvention::Act365Fixed,
            currency: Currency::CAD,
            calendar: CalendarId::new("CAD"),
            fixing_lag: 0,
            publication_time: Some(PublicationTime::MorningT1),
            spot_lag: 1,
            allows_negative: true,
            source: IndexSource::Custom("Bank of Canada".to_string()),
            bloomberg_ticker: Some("CORRA Index".to_string()),
            refinitiv_ric: None,
            arrear_convention: Some(ArrearConvention::isda_standard()),
        }
    }

    /// AONIA conventions.
    #[must_use]
    pub fn aonia() -> Self {
        Self {
            day_count: DayCountConvention::Act365Fixed,
            currency: Currency::AUD,
            calendar: CalendarId::new("AUD"),
            fixing_lag: 0,
            publication_time: Some(PublicationTime::MorningT1),
            spot_lag: 2,
            allows_negative: true,
            source: IndexSource::Custom("Reserve Bank of Australia".to_string()),
            bloomberg_ticker: Some("RBACOR Index".to_string()),
            refinitiv_ric: None,
            arrear_convention: Some(ArrearConvention::isda_standard()),
        }
    }

    /// Term SOFR conventions.
    #[must_use]
    pub fn term_sofr(tenor: Tenor) -> Self {
        Self {
            day_count: DayCountConvention::Act360,
            currency: Currency::USD,
            calendar: CalendarId::us_government(),
            fixing_lag: -2, // T-2 fixing
            publication_time: Some(PublicationTime::MorningT),
            spot_lag: 2,
            allows_negative: true,
            source: IndexSource::CME,
            bloomberg_ticker: Some(format!("TSFR{}M Index", tenor.months())),
            refinitiv_ric: None,
            arrear_convention: None, // Term rate - no arrear convention
        }
    }

    /// Term SONIA conventions.
    #[must_use]
    pub fn term_sonia(tenor: Tenor) -> Self {
        Self {
            day_count: DayCountConvention::Act365Fixed,
            currency: Currency::GBP,
            calendar: CalendarId::uk(),
            fixing_lag: -2,
            publication_time: Some(PublicationTime::MorningT),
            spot_lag: 0,
            allows_negative: true,
            source: IndexSource::Custom("FTSE Russell".to_string()),
            bloomberg_ticker: Some(format!("BPSONIA{} Index", tenor.months())),
            refinitiv_ric: None,
            arrear_convention: None,
        }
    }

    /// EURIBOR conventions.
    #[must_use]
    pub fn euribor(tenor: Tenor) -> Self {
        Self {
            day_count: DayCountConvention::Act360,
            currency: Currency::EUR,
            calendar: CalendarId::target2(),
            fixing_lag: -2, // T-2 fixing
            publication_time: Some(PublicationTime::MorningT),
            spot_lag: 2,
            allows_negative: true,
            source: IndexSource::EMMI,
            bloomberg_ticker: Some(format!("EUR00{}M Index", tenor.months())),
            refinitiv_ric: Some(format!("EURIBOR{}M=", tenor.months())),
            arrear_convention: None,
        }
    }

    /// TIBOR conventions.
    #[must_use]
    pub fn tibor(tenor: Tenor) -> Self {
        Self {
            day_count: DayCountConvention::Act365Fixed,
            currency: Currency::JPY,
            calendar: CalendarId::japan(),
            fixing_lag: -2,
            publication_time: Some(PublicationTime::MorningT),
            spot_lag: 2,
            allows_negative: true,
            source: IndexSource::JBA,
            bloomberg_ticker: Some(format!("TIBOR{}M Index", tenor.months())),
            refinitiv_ric: None,
            arrear_convention: None,
        }
    }

    /// Legacy LIBOR conventions.
    #[must_use]
    pub fn libor(currency: Currency, _tenor: Tenor) -> Self {
        let dc = match currency {
            Currency::GBP => DayCountConvention::Act365Fixed,
            _ => DayCountConvention::Act360,
        };
        Self {
            day_count: dc,
            currency,
            calendar: CalendarId::uk(), // London fixing
            fixing_lag: -2,
            publication_time: Some(PublicationTime::MorningT),
            spot_lag: 2,
            allows_negative: true,
            source: IndexSource::IBA,
            bloomberg_ticker: None, // Discontinued
            refinitiv_ric: None,
            arrear_convention: None,
        }
    }

    /// Custom index conventions.
    #[must_use]
    pub fn custom(currency: Currency) -> Self {
        Self {
            day_count: DayCountConvention::Act360,
            currency,
            calendar: CalendarId::weekend_only(),
            fixing_lag: -2,
            publication_time: None,
            spot_lag: 2,
            allows_negative: true,
            source: IndexSource::Custom("Custom".to_string()),
            bloomberg_ticker: None,
            refinitiv_ric: None,
            arrear_convention: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arrear_convention_presets() {
        let arrc = ArrearConvention::arrc_sofr();
        assert_eq!(arrc.lookback_days, 2);
        assert_eq!(arrc.shift_type, ShiftType::ObservationShift);
        assert!(arrc.lockout_days.is_none());

        let loan = ArrearConvention::loan_convention();
        assert_eq!(loan.lookback_days, 5);
        assert_eq!(loan.shift_type, ShiftType::Lookback);
        assert_eq!(loan.lockout_days, Some(2));
    }

    #[test]
    fn test_arrear_convention_builders() {
        let conv = ArrearConvention::isda_standard()
            .with_lockout(3)
            .with_floor(0.0);

        assert_eq!(conv.lockout_days, Some(3));
        assert_eq!(conv.daily_floor, Some(0.0));
    }

    #[test]
    fn test_index_conventions_sofr() {
        let conv = IndexConventions::for_index(&RateIndex::SOFR);

        assert_eq!(conv.currency, Currency::USD);
        assert_eq!(conv.day_count, DayCountConvention::Act360);
        assert_eq!(conv.fixing_lag, 0);
        assert!(conv.allows_negative);
        assert!(matches!(conv.source, IndexSource::FederalReserveNY));
        assert!(conv.bloomberg_ticker.is_some());
        assert!(conv.arrear_convention.is_some());
    }

    #[test]
    fn test_index_conventions_sonia() {
        let conv = IndexConventions::for_index(&RateIndex::SONIA);

        assert_eq!(conv.currency, Currency::GBP);
        assert_eq!(conv.day_count, DayCountConvention::Act365Fixed);
        assert_eq!(conv.spot_lag, 0); // Same-day settlement
        assert!(matches!(conv.source, IndexSource::BankOfEngland));
    }

    #[test]
    fn test_index_conventions_estr() {
        let conv = IndexConventions::for_index(&RateIndex::ESTR);

        assert_eq!(conv.currency, Currency::EUR);
        assert_eq!(conv.day_count, DayCountConvention::Act360);
        assert!(matches!(conv.source, IndexSource::ECB));
    }

    #[test]
    fn test_index_conventions_euribor() {
        let conv = IndexConventions::for_index(&RateIndex::EURIBOR { tenor: Tenor::M3 });

        assert_eq!(conv.currency, Currency::EUR);
        assert_eq!(conv.fixing_lag, -2);
        assert!(conv.arrear_convention.is_none()); // Term rate
        assert!(conv.bloomberg_ticker.unwrap().contains("EUR00"));
    }

    #[test]
    fn test_index_conventions_term_sofr() {
        let conv = IndexConventions::for_index(&RateIndex::TermSOFR { tenor: Tenor::M3 });

        assert_eq!(conv.currency, Currency::USD);
        assert_eq!(conv.fixing_lag, -2);
        assert!(matches!(conv.source, IndexSource::CME));
        assert!(conv.arrear_convention.is_none());
    }

    #[test]
    fn test_index_source_display() {
        assert_eq!(
            format!("{}", IndexSource::FederalReserveNY),
            "Federal Reserve Bank of New York"
        );
        assert_eq!(format!("{}", IndexSource::ECB), "European Central Bank");
        assert_eq!(
            format!("{}", IndexSource::Custom("Custom Source".to_string())),
            "Custom Source"
        );
    }

    #[test]
    fn test_overnight_convention_consistency() {
        // Verify all overnight rates have arrear conventions
        let sofr = IndexConventions::for_index(&RateIndex::SOFR);
        let sonia = IndexConventions::for_index(&RateIndex::SONIA);
        let estr = IndexConventions::for_index(&RateIndex::ESTR);
        let saron = IndexConventions::for_index(&RateIndex::SARON);

        assert!(sofr.arrear_convention.is_some());
        assert!(sonia.arrear_convention.is_some());
        assert!(estr.arrear_convention.is_some());
        assert!(saron.arrear_convention.is_some());
    }
}
