//! Rate indices for multi-curve framework.
//!
//! This module defines rate indices (SOFR, €STR, SONIA, EURIBOR, etc.)
//! with their associated conventions.

use convex_core::Currency;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Tenor specification for rate indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Tenor {
    /// Overnight
    ON,
    /// Tomorrow/Next
    TN,
    /// 1 Week
    W1,
    /// 2 Weeks
    W2,
    /// 1 Month
    M1,
    /// 2 Months
    M2,
    /// 3 Months
    M3,
    /// 4 Months
    M4,
    /// 6 Months
    M6,
    /// 9 Months
    M9,
    /// 12 Months / 1 Year
    M12,
    /// 2 Years
    Y2,
    /// 3 Years
    Y3,
    /// 4 Years
    Y4,
    /// 5 Years
    Y5,
    /// 7 Years
    Y7,
    /// 10 Years
    Y10,
    /// 15 Years
    Y15,
    /// 20 Years
    Y20,
    /// 30 Years
    Y30,
    /// 50 Years
    Y50,
}

impl Tenor {
    /// Returns the tenor in years as a fraction.
    #[must_use]
    pub fn years(&self) -> f64 {
        match self {
            Tenor::ON | Tenor::TN => 1.0 / 365.0,
            Tenor::W1 => 7.0 / 365.0,
            Tenor::W2 => 14.0 / 365.0,
            Tenor::M1 => 1.0 / 12.0,
            Tenor::M2 => 2.0 / 12.0,
            Tenor::M3 => 3.0 / 12.0,
            Tenor::M4 => 4.0 / 12.0,
            Tenor::M6 => 6.0 / 12.0,
            Tenor::M9 => 9.0 / 12.0,
            Tenor::M12 => 1.0,
            Tenor::Y2 => 2.0,
            Tenor::Y3 => 3.0,
            Tenor::Y4 => 4.0,
            Tenor::Y5 => 5.0,
            Tenor::Y7 => 7.0,
            Tenor::Y10 => 10.0,
            Tenor::Y15 => 15.0,
            Tenor::Y20 => 20.0,
            Tenor::Y30 => 30.0,
            Tenor::Y50 => 50.0,
        }
    }

    /// Returns the tenor in months.
    #[must_use]
    pub fn months(&self) -> u32 {
        match self {
            Tenor::ON | Tenor::TN => 0,
            Tenor::W1 | Tenor::W2 => 0,
            Tenor::M1 => 1,
            Tenor::M2 => 2,
            Tenor::M3 => 3,
            Tenor::M4 => 4,
            Tenor::M6 => 6,
            Tenor::M9 => 9,
            Tenor::M12 => 12,
            Tenor::Y2 => 24,
            Tenor::Y3 => 36,
            Tenor::Y4 => 48,
            Tenor::Y5 => 60,
            Tenor::Y7 => 84,
            Tenor::Y10 => 120,
            Tenor::Y15 => 180,
            Tenor::Y20 => 240,
            Tenor::Y30 => 360,
            Tenor::Y50 => 600,
        }
    }

    /// Parses a tenor from a string (e.g., "1M", "3M", "1Y", "10Y").
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_uppercase();
        match s.as_str() {
            "ON" | "O/N" => Some(Tenor::ON),
            "TN" | "T/N" => Some(Tenor::TN),
            "1W" => Some(Tenor::W1),
            "2W" => Some(Tenor::W2),
            "1M" => Some(Tenor::M1),
            "2M" => Some(Tenor::M2),
            "3M" => Some(Tenor::M3),
            "4M" => Some(Tenor::M4),
            "6M" => Some(Tenor::M6),
            "9M" => Some(Tenor::M9),
            "12M" | "1Y" => Some(Tenor::M12),
            "2Y" | "24M" => Some(Tenor::Y2),
            "3Y" | "36M" => Some(Tenor::Y3),
            "4Y" | "48M" => Some(Tenor::Y4),
            "5Y" | "60M" => Some(Tenor::Y5),
            "7Y" | "84M" => Some(Tenor::Y7),
            "10Y" | "120M" => Some(Tenor::Y10),
            "15Y" | "180M" => Some(Tenor::Y15),
            "20Y" | "240M" => Some(Tenor::Y20),
            "30Y" | "360M" => Some(Tenor::Y30),
            "50Y" | "600M" => Some(Tenor::Y50),
            _ => None,
        }
    }
}

impl fmt::Display for Tenor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tenor::ON => write!(f, "O/N"),
            Tenor::TN => write!(f, "T/N"),
            Tenor::W1 => write!(f, "1W"),
            Tenor::W2 => write!(f, "2W"),
            Tenor::M1 => write!(f, "1M"),
            Tenor::M2 => write!(f, "2M"),
            Tenor::M3 => write!(f, "3M"),
            Tenor::M4 => write!(f, "4M"),
            Tenor::M6 => write!(f, "6M"),
            Tenor::M9 => write!(f, "9M"),
            Tenor::M12 => write!(f, "1Y"),
            Tenor::Y2 => write!(f, "2Y"),
            Tenor::Y3 => write!(f, "3Y"),
            Tenor::Y4 => write!(f, "4Y"),
            Tenor::Y5 => write!(f, "5Y"),
            Tenor::Y7 => write!(f, "7Y"),
            Tenor::Y10 => write!(f, "10Y"),
            Tenor::Y15 => write!(f, "15Y"),
            Tenor::Y20 => write!(f, "20Y"),
            Tenor::Y30 => write!(f, "30Y"),
            Tenor::Y50 => write!(f, "50Y"),
        }
    }
}

/// Rate index identifier.
///
/// Represents a reference rate used for floating rate calculations
/// and curve construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RateIndex {
    // ==================== Overnight Risk-Free Rates (RFRs) ====================
    /// Secured Overnight Financing Rate (USD)
    SOFR,
    /// Euro Short-Term Rate (EUR)
    ESTR,
    /// Sterling Overnight Index Average (GBP)
    SONIA,
    /// Tokyo Overnight Average Rate (JPY)
    TONA,
    /// Swiss Average Rate Overnight (CHF)
    SARON,
    /// Canadian Overnight Repo Rate Average (CAD)
    CORRA,
    /// Australian Interbank Overnight Cash Rate (AUD)
    AONIA,

    // ==================== Term Rates ====================
    /// CME Term SOFR (USD)
    TermSOFR {
        /// Tenor (1M, 3M, 6M, 12M)
        tenor: Tenor,
    },
    /// EURIBOR (EUR)
    EURIBOR {
        /// Tenor (1M, 3M, 6M, 12M)
        tenor: Tenor,
    },
    /// TIBOR (JPY)
    TIBOR {
        /// Tenor (1M, 3M, 6M)
        tenor: Tenor,
    },
    /// Term SONIA (GBP)
    TermSONIA {
        /// Tenor
        tenor: Tenor,
    },

    // ==================== Legacy Rates (for existing trades) ====================
    /// Legacy LIBOR (for fallback calculations)
    LIBOR {
        /// Currency
        currency: Currency,
        /// Tenor
        tenor: Tenor,
    },

    // ==================== Custom/Generic ====================
    /// Custom rate index
    Custom {
        /// Index name
        name: String,
        /// Currency
        currency: Currency,
        /// Tenor (if applicable)
        tenor: Option<Tenor>,
    },
}

/// Day count convention for rate indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateDayCount {
    /// ACT/360 (USD, EUR)
    Act360,
    /// ACT/365 Fixed (GBP, JPY)
    Act365F,
}

/// Compounding type for overnight rates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OvernightCompounding {
    /// Simple compounding (no compounding within period)
    Simple,
    /// Daily compounded in arrears
    CompoundedInArrears,
    /// Daily averaged (SOFR average)
    Averaged,
}

impl RateIndex {
    // ==================== Convenience constructors ====================

    /// Creates Term SOFR 1M index.
    #[must_use]
    pub fn term_sofr_1m() -> Self {
        RateIndex::TermSOFR { tenor: Tenor::M1 }
    }

    /// Creates Term SOFR 3M index.
    #[must_use]
    pub fn term_sofr_3m() -> Self {
        RateIndex::TermSOFR { tenor: Tenor::M3 }
    }

    /// Creates Term SOFR 6M index.
    #[must_use]
    pub fn term_sofr_6m() -> Self {
        RateIndex::TermSOFR { tenor: Tenor::M6 }
    }

    /// Creates Term SOFR 12M index.
    #[must_use]
    pub fn term_sofr_12m() -> Self {
        RateIndex::TermSOFR { tenor: Tenor::M12 }
    }

    /// Creates EURIBOR 3M index.
    #[must_use]
    pub fn euribor_3m() -> Self {
        RateIndex::EURIBOR { tenor: Tenor::M3 }
    }

    /// Creates EURIBOR 6M index.
    #[must_use]
    pub fn euribor_6m() -> Self {
        RateIndex::EURIBOR { tenor: Tenor::M6 }
    }

    // ==================== Index properties ====================

    /// Returns the currency for this rate index.
    #[must_use]
    pub fn currency(&self) -> Currency {
        match self {
            RateIndex::SOFR | RateIndex::TermSOFR { .. } => Currency::USD,
            RateIndex::ESTR | RateIndex::EURIBOR { .. } => Currency::EUR,
            RateIndex::SONIA | RateIndex::TermSONIA { .. } => Currency::GBP,
            RateIndex::TONA | RateIndex::TIBOR { .. } => Currency::JPY,
            RateIndex::SARON => Currency::CHF,
            RateIndex::CORRA => Currency::CAD,
            RateIndex::AONIA => Currency::AUD,
            RateIndex::LIBOR { currency, .. } => *currency,
            RateIndex::Custom { currency, .. } => *currency,
        }
    }

    /// Returns the day count convention for this rate index.
    #[must_use]
    pub fn day_count(&self) -> RateDayCount {
        match self {
            // ACT/360 currencies
            RateIndex::SOFR
            | RateIndex::TermSOFR { .. }
            | RateIndex::ESTR
            | RateIndex::EURIBOR { .. }
            | RateIndex::SARON
            | RateIndex::CORRA => RateDayCount::Act360,

            // ACT/365F currencies
            RateIndex::SONIA
            | RateIndex::TermSONIA { .. }
            | RateIndex::TONA
            | RateIndex::TIBOR { .. }
            | RateIndex::AONIA => RateDayCount::Act365F,

            // LIBOR follows currency convention
            RateIndex::LIBOR { currency, .. } => match currency {
                Currency::GBP | Currency::JPY | Currency::AUD => RateDayCount::Act365F,
                _ => RateDayCount::Act360,
            },

            // Default to ACT/360
            RateIndex::Custom { .. } => RateDayCount::Act360,
        }
    }

    /// Returns the fixing lag in business days (T+n fixing).
    #[must_use]
    pub fn fixing_lag(&self) -> u32 {
        match self {
            // Same-day fixing (T+0)
            RateIndex::SOFR
            | RateIndex::ESTR
            | RateIndex::SONIA
            | RateIndex::TONA
            | RateIndex::SARON
            | RateIndex::CORRA
            | RateIndex::AONIA => 0,

            // T+2 for term rates
            RateIndex::TermSOFR { .. }
            | RateIndex::TermSONIA { .. }
            | RateIndex::EURIBOR { .. }
            | RateIndex::TIBOR { .. } => 2,

            // LIBOR was T+2
            RateIndex::LIBOR { .. } => 2,

            RateIndex::Custom { .. } => 2,
        }
    }

    /// Returns the payment lag in business days.
    #[must_use]
    pub fn payment_lag(&self) -> u32 {
        match self {
            // Overnight rates: payment on next business day
            RateIndex::SOFR
            | RateIndex::ESTR
            | RateIndex::SONIA
            | RateIndex::TONA
            | RateIndex::SARON
            | RateIndex::CORRA
            | RateIndex::AONIA => 1,

            // Term rates: standard T+2 settlement
            _ => 2,
        }
    }

    /// Returns the overnight compounding convention.
    #[must_use]
    pub fn compounding(&self) -> OvernightCompounding {
        match self {
            RateIndex::SOFR | RateIndex::ESTR | RateIndex::SONIA | RateIndex::TONA => {
                OvernightCompounding::CompoundedInArrears
            }
            RateIndex::SARON => OvernightCompounding::CompoundedInArrears,
            RateIndex::CORRA | RateIndex::AONIA => OvernightCompounding::CompoundedInArrears,
            // Term rates use simple interest
            _ => OvernightCompounding::Simple,
        }
    }

    /// Returns the tenor if this is a term rate.
    #[must_use]
    pub fn tenor(&self) -> Option<Tenor> {
        match self {
            RateIndex::TermSOFR { tenor }
            | RateIndex::EURIBOR { tenor }
            | RateIndex::TIBOR { tenor }
            | RateIndex::TermSONIA { tenor }
            | RateIndex::LIBOR { tenor, .. }
            | RateIndex::Custom { tenor: Some(tenor), .. } => Some(*tenor),
            _ => None,
        }
    }

    /// Returns true if this is an overnight rate.
    #[must_use]
    pub fn is_overnight(&self) -> bool {
        matches!(
            self,
            RateIndex::SOFR
                | RateIndex::ESTR
                | RateIndex::SONIA
                | RateIndex::TONA
                | RateIndex::SARON
                | RateIndex::CORRA
                | RateIndex::AONIA
        )
    }

    /// Returns true if this is a term rate (not overnight).
    #[must_use]
    pub fn is_term(&self) -> bool {
        !self.is_overnight()
    }

    /// Returns true if this is a legacy rate (LIBOR).
    #[must_use]
    pub fn is_legacy(&self) -> bool {
        matches!(self, RateIndex::LIBOR { .. })
    }
}

impl fmt::Display for RateIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RateIndex::SOFR => write!(f, "SOFR"),
            RateIndex::ESTR => write!(f, "€STR"),
            RateIndex::SONIA => write!(f, "SONIA"),
            RateIndex::TONA => write!(f, "TONA"),
            RateIndex::SARON => write!(f, "SARON"),
            RateIndex::CORRA => write!(f, "CORRA"),
            RateIndex::AONIA => write!(f, "AONIA"),
            RateIndex::TermSOFR { tenor } => write!(f, "Term SOFR {tenor}"),
            RateIndex::EURIBOR { tenor } => write!(f, "EURIBOR {tenor}"),
            RateIndex::TIBOR { tenor } => write!(f, "TIBOR {tenor}"),
            RateIndex::TermSONIA { tenor } => write!(f, "Term SONIA {tenor}"),
            RateIndex::LIBOR { currency, tenor } => write!(f, "{currency} LIBOR {tenor}"),
            RateIndex::Custom { name, tenor, .. } => {
                if let Some(t) = tenor {
                    write!(f, "{name} {t}")
                } else {
                    write!(f, "{name}")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenor_years() {
        assert!((Tenor::M1.years() - 1.0 / 12.0).abs() < 1e-10);
        assert!((Tenor::M3.years() - 0.25).abs() < 1e-10);
        assert!((Tenor::M6.years() - 0.5).abs() < 1e-10);
        assert!((Tenor::M12.years() - 1.0).abs() < 1e-10);
        assert!((Tenor::Y5.years() - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_tenor_parse() {
        assert_eq!(Tenor::parse("1M"), Some(Tenor::M1));
        assert_eq!(Tenor::parse("3M"), Some(Tenor::M3));
        assert_eq!(Tenor::parse("1Y"), Some(Tenor::M12));
        assert_eq!(Tenor::parse("5Y"), Some(Tenor::Y5));
        assert_eq!(Tenor::parse("ON"), Some(Tenor::ON));
        assert_eq!(Tenor::parse("invalid"), None);
    }

    #[test]
    fn test_tenor_display() {
        assert_eq!(format!("{}", Tenor::M3), "3M");
        assert_eq!(format!("{}", Tenor::Y5), "5Y");
        assert_eq!(format!("{}", Tenor::ON), "O/N");
    }

    #[test]
    fn test_rate_index_currency() {
        assert_eq!(RateIndex::SOFR.currency(), Currency::USD);
        assert_eq!(RateIndex::ESTR.currency(), Currency::EUR);
        assert_eq!(RateIndex::SONIA.currency(), Currency::GBP);
        assert_eq!(RateIndex::term_sofr_3m().currency(), Currency::USD);
        assert_eq!(RateIndex::euribor_3m().currency(), Currency::EUR);
    }

    #[test]
    fn test_rate_index_day_count() {
        assert_eq!(RateIndex::SOFR.day_count(), RateDayCount::Act360);
        assert_eq!(RateIndex::ESTR.day_count(), RateDayCount::Act360);
        assert_eq!(RateIndex::SONIA.day_count(), RateDayCount::Act365F);
        assert_eq!(RateIndex::TONA.day_count(), RateDayCount::Act365F);
    }

    #[test]
    fn test_rate_index_is_overnight() {
        assert!(RateIndex::SOFR.is_overnight());
        assert!(RateIndex::ESTR.is_overnight());
        assert!(!RateIndex::term_sofr_3m().is_overnight());
        assert!(!RateIndex::euribor_6m().is_overnight());
    }

    #[test]
    fn test_rate_index_tenor() {
        assert_eq!(RateIndex::SOFR.tenor(), None);
        assert_eq!(RateIndex::term_sofr_3m().tenor(), Some(Tenor::M3));
        assert_eq!(RateIndex::euribor_6m().tenor(), Some(Tenor::M6));
    }

    #[test]
    fn test_rate_index_display() {
        assert_eq!(format!("{}", RateIndex::SOFR), "SOFR");
        assert_eq!(format!("{}", RateIndex::term_sofr_3m()), "Term SOFR 3M");
        assert_eq!(format!("{}", RateIndex::euribor_6m()), "EURIBOR 6M");
    }
}
