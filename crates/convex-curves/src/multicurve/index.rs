//! Rate index definitions for multi-curve framework.
//!
//! Defines standard rate indices used in the fixed income markets:
//! - Overnight rates: SOFR, ESTR, SONIA, TONAR
//! - Term rates: Euribor, legacy LIBOR
//!
//! Each index has associated conventions for day count, fixing lag, and tenor.

use convex_core::daycounts::DayCountConvention;
use serde::{Deserialize, Serialize};
use std::fmt;

// ============================================================================
// Tenor Definition
// ============================================================================

/// Standard tenor periods used in fixed income markets.
///
/// Represents time periods like 1M, 3M, 6M, 1Y, 5Y, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tenor {
    /// 1 day (overnight)
    ON,
    /// 1 week
    W1,
    /// 2 weeks
    W2,
    /// 1 month
    M1,
    /// 2 months
    M2,
    /// 3 months
    M3,
    /// 6 months
    M6,
    /// 9 months
    M9,
    /// 12 months (1 year)
    M12,
    /// 1 year
    Y1,
    /// 2 years
    Y2,
    /// 3 years
    Y3,
    /// 5 years
    Y5,
    /// 7 years
    Y7,
    /// 10 years
    Y10,
    /// 15 years
    Y15,
    /// 20 years
    Y20,
    /// 30 years
    Y30,
    /// Custom tenor in months
    Custom(u32),
}

impl Tenor {
    /// Returns the tenor in months.
    #[must_use]
    pub fn months(&self) -> u32 {
        match self {
            Tenor::ON => 0,
            Tenor::W1 => 0, // Less than 1 month
            Tenor::W2 => 0,
            Tenor::M1 => 1,
            Tenor::M2 => 2,
            Tenor::M3 => 3,
            Tenor::M6 => 6,
            Tenor::M9 => 9,
            Tenor::M12 | Tenor::Y1 => 12,
            Tenor::Y2 => 24,
            Tenor::Y3 => 36,
            Tenor::Y5 => 60,
            Tenor::Y7 => 84,
            Tenor::Y10 => 120,
            Tenor::Y15 => 180,
            Tenor::Y20 => 240,
            Tenor::Y30 => 360,
            Tenor::Custom(m) => *m,
        }
    }

    /// Returns the tenor in years as a floating-point number.
    #[must_use]
    pub fn years(&self) -> f64 {
        match self {
            Tenor::ON => 1.0 / 365.0,
            Tenor::W1 => 7.0 / 365.0,
            Tenor::W2 => 14.0 / 365.0,
            _ => f64::from(self.months()) / 12.0,
        }
    }

    /// Returns the tenor in days (approximate).
    #[must_use]
    pub fn days(&self) -> i64 {
        match self {
            Tenor::ON => 1,
            Tenor::W1 => 7,
            Tenor::W2 => 14,
            _ => (f64::from(self.months()) * 30.4375).round() as i64,
        }
    }

    /// Creates a tenor from months.
    #[must_use]
    pub fn from_months(months: u32) -> Self {
        match months {
            0 => Tenor::ON,
            1 => Tenor::M1,
            2 => Tenor::M2,
            3 => Tenor::M3,
            6 => Tenor::M6,
            9 => Tenor::M9,
            12 => Tenor::Y1,
            24 => Tenor::Y2,
            36 => Tenor::Y3,
            60 => Tenor::Y5,
            84 => Tenor::Y7,
            120 => Tenor::Y10,
            180 => Tenor::Y15,
            240 => Tenor::Y20,
            360 => Tenor::Y30,
            m => Tenor::Custom(m),
        }
    }
}

impl fmt::Display for Tenor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tenor::ON => write!(f, "O/N"),
            Tenor::W1 => write!(f, "1W"),
            Tenor::W2 => write!(f, "2W"),
            Tenor::M1 => write!(f, "1M"),
            Tenor::M2 => write!(f, "2M"),
            Tenor::M3 => write!(f, "3M"),
            Tenor::M6 => write!(f, "6M"),
            Tenor::M9 => write!(f, "9M"),
            Tenor::M12 => write!(f, "12M"),
            Tenor::Y1 => write!(f, "1Y"),
            Tenor::Y2 => write!(f, "2Y"),
            Tenor::Y3 => write!(f, "3Y"),
            Tenor::Y5 => write!(f, "5Y"),
            Tenor::Y7 => write!(f, "7Y"),
            Tenor::Y10 => write!(f, "10Y"),
            Tenor::Y15 => write!(f, "15Y"),
            Tenor::Y20 => write!(f, "20Y"),
            Tenor::Y30 => write!(f, "30Y"),
            Tenor::Custom(m) => write!(f, "{}M", m),
        }
    }
}

// ============================================================================
// Rate Index Definition
// ============================================================================

/// Standard rate indices used in fixed income markets.
///
/// These are the benchmark rates used for floating leg calculations
/// and curve construction in the post-LIBOR world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RateIndex {
    // =========================================================================
    // Overnight Indices (Risk-Free Rates)
    // =========================================================================
    /// SOFR - Secured Overnight Financing Rate (USD)
    ///
    /// The primary USD risk-free rate, based on overnight repo transactions.
    /// Published by the NY Fed.
    Sofr,

    /// ESTR - Euro Short-Term Rate (EUR)
    ///
    /// The primary EUR risk-free rate, based on overnight unsecured transactions.
    /// Published by the ECB.
    Estr,

    /// SONIA - Sterling Overnight Index Average (GBP)
    ///
    /// The primary GBP risk-free rate, based on overnight unsecured transactions.
    /// Published by the Bank of England.
    Sonia,

    /// TONAR - Tokyo Overnight Average Rate (JPY)
    ///
    /// The primary JPY risk-free rate for overnight unsecured transactions.
    /// Published by the Bank of Japan.
    Tonar,

    /// SARON - Swiss Average Rate Overnight (CHF)
    ///
    /// The primary CHF risk-free rate, based on repo transactions.
    /// Published by SIX.
    Saron,

    /// CORRA - Canadian Overnight Repo Rate Average (CAD)
    ///
    /// The primary CAD risk-free rate, based on overnight repo transactions.
    /// Published by the Bank of Canada.
    Corra,

    /// AONIA - Australia Overnight Index Average (AUD)
    ///
    /// The primary AUD risk-free rate, based on overnight interbank transactions.
    /// Published by the RBA.
    Aonia,

    /// HONIA - Hong Kong Overnight Index Average (HKD)
    ///
    /// The primary HKD risk-free rate.
    /// Published by the HKMA.
    Honia,

    // =========================================================================
    // Term Rates (IBOR successors and legacy)
    // =========================================================================
    /// Euribor 1M - Euro Interbank Offered Rate (1 month)
    Euribor1M,

    /// Euribor 3M - Euro Interbank Offered Rate (3 months)
    Euribor3M,

    /// Euribor 6M - Euro Interbank Offered Rate (6 months)
    Euribor6M,

    /// Euribor 12M - Euro Interbank Offered Rate (12 months)
    Euribor12M,

    /// TIBOR 3M - Tokyo Interbank Offered Rate (3 months)
    Tibor3M,

    // =========================================================================
    // Legacy Rates (for historical analysis)
    // =========================================================================
    /// USD LIBOR 3M (discontinued Dec 2021 for most, June 2023 for USD)
    #[deprecated(note = "LIBOR has been discontinued - use SOFR")]
    UsdLibor3M,

    /// GBP LIBOR 3M (discontinued Dec 2021)
    #[deprecated(note = "LIBOR has been discontinued - use SONIA")]
    GbpLibor3M,

    /// CHF LIBOR 3M (discontinued Dec 2021)
    #[deprecated(note = "LIBOR has been discontinued - use SARON")]
    ChfLibor3M,
}

impl RateIndex {
    /// Returns the currency for this index.
    #[must_use]
    pub fn currency(&self) -> Currency {
        match self {
            RateIndex::Sofr => Currency::Usd,
            #[allow(deprecated)]
            RateIndex::UsdLibor3M => Currency::Usd,
            RateIndex::Estr
            | RateIndex::Euribor1M
            | RateIndex::Euribor3M
            | RateIndex::Euribor6M
            | RateIndex::Euribor12M => Currency::Eur,
            RateIndex::Sonia => Currency::Gbp,
            #[allow(deprecated)]
            RateIndex::GbpLibor3M => Currency::Gbp,
            RateIndex::Tonar | RateIndex::Tibor3M => Currency::Jpy,
            RateIndex::Saron => Currency::Chf,
            #[allow(deprecated)]
            RateIndex::ChfLibor3M => Currency::Chf,
            RateIndex::Corra => Currency::Cad,
            RateIndex::Aonia => Currency::Aud,
            RateIndex::Honia => Currency::Hkd,
        }
    }

    /// Returns true if this is an overnight rate.
    #[must_use]
    pub fn is_overnight(&self) -> bool {
        matches!(
            self,
            RateIndex::Sofr
                | RateIndex::Estr
                | RateIndex::Sonia
                | RateIndex::Tonar
                | RateIndex::Saron
                | RateIndex::Corra
                | RateIndex::Aonia
                | RateIndex::Honia
        )
    }

    /// Returns the standard day count convention.
    #[must_use]
    pub fn day_count(&self) -> DayCountConvention {
        match self {
            // USD uses ACT/360 for money market
            RateIndex::Sofr => DayCountConvention::Act360,
            #[allow(deprecated)]
            RateIndex::UsdLibor3M => DayCountConvention::Act360,

            // EUR uses ACT/360
            RateIndex::Estr
            | RateIndex::Euribor1M
            | RateIndex::Euribor3M
            | RateIndex::Euribor6M
            | RateIndex::Euribor12M => DayCountConvention::Act360,

            // GBP uses ACT/365
            RateIndex::Sonia => DayCountConvention::Act365Fixed,
            #[allow(deprecated)]
            RateIndex::GbpLibor3M => DayCountConvention::Act365Fixed,

            // JPY uses ACT/365
            RateIndex::Tonar | RateIndex::Tibor3M => DayCountConvention::Act365Fixed,

            // CHF uses ACT/360
            RateIndex::Saron => DayCountConvention::Act360,
            #[allow(deprecated)]
            RateIndex::ChfLibor3M => DayCountConvention::Act360,

            // CAD uses ACT/365
            RateIndex::Corra => DayCountConvention::Act365Fixed,

            // AUD uses ACT/365
            RateIndex::Aonia => DayCountConvention::Act365Fixed,

            // HKD uses ACT/365
            RateIndex::Honia => DayCountConvention::Act365Fixed,
        }
    }

    /// Returns the tenor in years (0 for overnight).
    #[must_use]
    pub fn tenor_years(&self) -> f64 {
        match self {
            // Overnight rates
            RateIndex::Sofr
            | RateIndex::Estr
            | RateIndex::Sonia
            | RateIndex::Tonar
            | RateIndex::Saron
            | RateIndex::Corra
            | RateIndex::Aonia
            | RateIndex::Honia => 0.0,

            // 1-month rates
            RateIndex::Euribor1M => 1.0 / 12.0,

            // 3-month rates
            RateIndex::Euribor3M | RateIndex::Tibor3M => 0.25,
            #[allow(deprecated)]
            RateIndex::UsdLibor3M | RateIndex::GbpLibor3M | RateIndex::ChfLibor3M => 0.25,

            // 6-month rates
            RateIndex::Euribor6M => 0.5,

            // 12-month rates
            RateIndex::Euribor12M => 1.0,
        }
    }

    /// Returns the fixing lag in business days.
    ///
    /// This is the number of days before the accrual period starts
    /// that the rate is fixed.
    #[must_use]
    pub fn fixing_lag(&self) -> i32 {
        match self {
            // Overnight rates typically fix same day or T-1
            RateIndex::Sofr | RateIndex::Sonia | RateIndex::Saron => 0,
            RateIndex::Estr => 1,
            RateIndex::Tonar => 0,
            RateIndex::Corra => 0,
            RateIndex::Aonia => 0,
            RateIndex::Honia => 0,

            // Term rates typically fix T-2
            RateIndex::Euribor1M
            | RateIndex::Euribor3M
            | RateIndex::Euribor6M
            | RateIndex::Euribor12M => 2,
            RateIndex::Tibor3M => 2,

            #[allow(deprecated)]
            RateIndex::UsdLibor3M | RateIndex::GbpLibor3M | RateIndex::ChfLibor3M => 2,
        }
    }

    /// Returns the index name as used in market conventions.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            RateIndex::Sofr => "SOFR",
            RateIndex::Estr => "€STR",
            RateIndex::Sonia => "SONIA",
            RateIndex::Tonar => "TONAR",
            RateIndex::Saron => "SARON",
            RateIndex::Corra => "CORRA",
            RateIndex::Aonia => "AONIA",
            RateIndex::Honia => "HONIA",
            RateIndex::Euribor1M => "EURIBOR 1M",
            RateIndex::Euribor3M => "EURIBOR 3M",
            RateIndex::Euribor6M => "EURIBOR 6M",
            RateIndex::Euribor12M => "EURIBOR 12M",
            RateIndex::Tibor3M => "TIBOR 3M",
            #[allow(deprecated)]
            RateIndex::UsdLibor3M => "USD LIBOR 3M",
            #[allow(deprecated)]
            RateIndex::GbpLibor3M => "GBP LIBOR 3M",
            #[allow(deprecated)]
            RateIndex::ChfLibor3M => "CHF LIBOR 3M",
        }
    }

    /// Returns all currently active (non-deprecated) indices.
    #[must_use]
    pub fn active_indices() -> Vec<RateIndex> {
        vec![
            RateIndex::Sofr,
            RateIndex::Estr,
            RateIndex::Sonia,
            RateIndex::Tonar,
            RateIndex::Saron,
            RateIndex::Corra,
            RateIndex::Aonia,
            RateIndex::Honia,
            RateIndex::Euribor1M,
            RateIndex::Euribor3M,
            RateIndex::Euribor6M,
            RateIndex::Euribor12M,
            RateIndex::Tibor3M,
        ]
    }

    /// Returns all overnight risk-free rates.
    #[must_use]
    pub fn overnight_rates() -> Vec<RateIndex> {
        vec![
            RateIndex::Sofr,
            RateIndex::Estr,
            RateIndex::Sonia,
            RateIndex::Tonar,
            RateIndex::Saron,
            RateIndex::Corra,
            RateIndex::Aonia,
            RateIndex::Honia,
        ]
    }
}

impl fmt::Display for RateIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Currency codes for rate indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Currency {
    /// US Dollar
    Usd,
    /// Euro
    Eur,
    /// British Pound
    Gbp,
    /// Japanese Yen
    Jpy,
    /// Swiss Franc
    Chf,
    /// Canadian Dollar
    Cad,
    /// Australian Dollar
    Aud,
    /// Hong Kong Dollar
    Hkd,
}

impl Currency {
    /// Returns the ISO 4217 code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Currency::Usd => "USD",
            Currency::Eur => "EUR",
            Currency::Gbp => "GBP",
            Currency::Jpy => "JPY",
            Currency::Chf => "CHF",
            Currency::Cad => "CAD",
            Currency::Aud => "AUD",
            Currency::Hkd => "HKD",
        }
    }

    /// Returns the primary overnight rate for this currency.
    #[must_use]
    pub fn overnight_rate(&self) -> RateIndex {
        match self {
            Currency::Usd => RateIndex::Sofr,
            Currency::Eur => RateIndex::Estr,
            Currency::Gbp => RateIndex::Sonia,
            Currency::Jpy => RateIndex::Tonar,
            Currency::Chf => RateIndex::Saron,
            Currency::Cad => RateIndex::Corra,
            Currency::Aud => RateIndex::Aonia,
            Currency::Hkd => RateIndex::Honia,
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())
    }
}

/// A currency pair for FX operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrencyPair {
    /// Base currency (first in the pair).
    pub base: Currency,
    /// Quote currency (second in the pair).
    pub quote: Currency,
}

impl CurrencyPair {
    /// Creates a new currency pair.
    #[must_use]
    pub fn new(base: Currency, quote: Currency) -> Self {
        Self { base, quote }
    }

    /// EUR/USD pair.
    pub const EURUSD: CurrencyPair = CurrencyPair {
        base: Currency::Eur,
        quote: Currency::Usd,
    };

    /// GBP/USD pair.
    pub const GBPUSD: CurrencyPair = CurrencyPair {
        base: Currency::Gbp,
        quote: Currency::Usd,
    };

    /// USD/JPY pair.
    pub const USDJPY: CurrencyPair = CurrencyPair {
        base: Currency::Usd,
        quote: Currency::Jpy,
    };

    /// USD/CHF pair.
    pub const USDCHF: CurrencyPair = CurrencyPair {
        base: Currency::Usd,
        quote: Currency::Chf,
    };

    /// Returns the inverted pair.
    #[must_use]
    pub fn invert(&self) -> Self {
        Self {
            base: self.quote,
            quote: self.base,
        }
    }
}

impl fmt::Display for CurrencyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.base.code(), self.quote.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sofr_properties() {
        let idx = RateIndex::Sofr;
        assert_eq!(idx.currency(), Currency::Usd);
        assert!(idx.is_overnight());
        assert_eq!(idx.day_count(), DayCountConvention::Act360);
        assert_eq!(idx.tenor_years(), 0.0);
        assert_eq!(idx.fixing_lag(), 0);
        assert_eq!(idx.name(), "SOFR");
    }

    #[test]
    fn test_euribor_properties() {
        let idx = RateIndex::Euribor3M;
        assert_eq!(idx.currency(), Currency::Eur);
        assert!(!idx.is_overnight());
        assert_eq!(idx.day_count(), DayCountConvention::Act360);
        assert_eq!(idx.tenor_years(), 0.25);
        assert_eq!(idx.fixing_lag(), 2);
    }

    #[test]
    fn test_sonia_properties() {
        let idx = RateIndex::Sonia;
        assert_eq!(idx.currency(), Currency::Gbp);
        assert!(idx.is_overnight());
        assert_eq!(idx.day_count(), DayCountConvention::Act365Fixed);
    }

    #[test]
    fn test_currency_overnight_rate() {
        assert_eq!(Currency::Usd.overnight_rate(), RateIndex::Sofr);
        assert_eq!(Currency::Eur.overnight_rate(), RateIndex::Estr);
        assert_eq!(Currency::Gbp.overnight_rate(), RateIndex::Sonia);
    }

    #[test]
    fn test_active_indices() {
        let active = RateIndex::active_indices();
        assert!(!active.is_empty());
        assert!(active.contains(&RateIndex::Sofr));
        assert!(active.contains(&RateIndex::Euribor3M));
    }

    #[test]
    fn test_currency_pair() {
        let pair = CurrencyPair::EURUSD;
        assert_eq!(pair.base, Currency::Eur);
        assert_eq!(pair.quote, Currency::Usd);
        assert_eq!(format!("{}", pair), "EUR/USD");

        let inverted = pair.invert();
        assert_eq!(inverted.base, Currency::Usd);
        assert_eq!(inverted.quote, Currency::Eur);
    }

    #[test]
    fn test_index_display() {
        assert_eq!(format!("{}", RateIndex::Sofr), "SOFR");
        assert_eq!(format!("{}", RateIndex::Estr), "€STR");
    }
}
