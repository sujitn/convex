//! Rate and inflation indices for bonds.
//!
//! Re-exports the rate index from convex-curves and adds inflation-linked
//! index types specific to bond analytics, plus SOFR compounding conventions
//! for floating rate notes.

use serde::{Deserialize, Serialize};

// Re-export the comprehensive RateIndex from curves module
pub use convex_curves::multicurve::{RateIndex, Tenor};

/// SOFR compounding convention for floating rate notes.
///
/// Defines how daily SOFR rates are compounded over a coupon period.
/// The ARRC (Alternative Reference Rates Committee) has standardized
/// several conventions for SOFR-based products.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SOFRConvention {
    /// Daily SOFR compounded in arrears (most common for FRNs).
    ///
    /// The rate is determined by compounding daily SOFR rates over
    /// the interest period, with the final rate known near period end.
    CompoundedInArrears {
        /// Lookback days - shift observation period backward
        /// Typically 2-5 business days to allow payment calculation
        lookback_days: u32,
        /// If true, shift observation period; if false, shift weight
        observation_shift: bool,
        /// Days before payment when rate is locked (optional)
        lockout_days: Option<u32>,
    },

    /// Simple average of daily SOFR rates.
    ///
    /// Less common than compounded, but simpler to calculate.
    SimpleAverage {
        /// Lookback days for observation shift
        lookback_days: u32,
    },

    /// CME Term SOFR - forward-looking term rate.
    ///
    /// Published by CME for 1M, 3M, 6M, 12M tenors.
    /// Determined at the start of the interest period.
    TermSOFR(Tenor),

    /// SOFR compounded in advance (rare).
    ///
    /// Rate is determined before the interest period starts
    /// using prior period's daily SOFR rates.
    CompoundedInAdvance,
}

impl SOFRConvention {
    /// Creates standard ARRC in-arrears convention with 5-day lookback.
    #[must_use]
    pub fn arrc_standard() -> Self {
        SOFRConvention::CompoundedInArrears {
            lookback_days: 5,
            observation_shift: true,
            lockout_days: None,
        }
    }

    /// Creates convention with lockout period (common for syndicated loans).
    #[must_use]
    pub fn with_lockout(lookback_days: u32, lockout_days: u32) -> Self {
        SOFRConvention::CompoundedInArrears {
            lookback_days,
            observation_shift: true,
            lockout_days: Some(lockout_days),
        }
    }

    /// Creates Term SOFR 3M convention.
    #[must_use]
    pub fn term_3m() -> Self {
        SOFRConvention::TermSOFR(Tenor::M3)
    }

    /// Returns true if this is a compounded in arrears convention.
    #[must_use]
    pub fn is_in_arrears(&self) -> bool {
        matches!(self, SOFRConvention::CompoundedInArrears { .. })
    }

    /// Returns the lookback days if applicable.
    #[must_use]
    pub fn lookback_days(&self) -> Option<u32> {
        match self {
            SOFRConvention::CompoundedInArrears { lookback_days, .. } => Some(*lookback_days),
            SOFRConvention::SimpleAverage { lookback_days } => Some(*lookback_days),
            _ => None,
        }
    }

    /// Returns true if rate is known at period start.
    #[must_use]
    pub fn is_forward_looking(&self) -> bool {
        matches!(
            self,
            SOFRConvention::TermSOFR(_) | SOFRConvention::CompoundedInAdvance
        )
    }
}

impl Default for SOFRConvention {
    fn default() -> Self {
        Self::arrc_standard()
    }
}

impl std::fmt::Display for SOFRConvention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SOFRConvention::CompoundedInArrears {
                lookback_days,
                observation_shift,
                lockout_days,
            } => {
                write!(f, "SOFR Compounded In Arrears ({lookback_days}D lookback")?;
                if *observation_shift {
                    write!(f, ", observation shift")?;
                }
                if let Some(lock) = lockout_days {
                    write!(f, ", {lock}D lockout")?;
                }
                write!(f, ")")
            }
            SOFRConvention::SimpleAverage { lookback_days } => {
                write!(f, "SOFR Simple Average ({lookback_days}D lookback)")
            }
            SOFRConvention::TermSOFR(tenor) => write!(f, "Term SOFR {tenor}"),
            SOFRConvention::CompoundedInAdvance => write!(f, "SOFR Compounded In Advance"),
        }
    }
}

/// Inflation index type for inflation-linked bonds (TIPS, Linkers).
///
/// Represents the specific inflation index used to adjust principal
/// and/or coupon payments.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum InflationIndexType {
    // ==================== US Indices ====================
    /// US CPI-U (All Urban Consumers) - used for TIPS
    USCPIUrban,
    /// US CPI-U NSA (Not Seasonally Adjusted) - primary TIPS index
    #[default]
    USCPIUNSA,

    // ==================== UK Indices ====================
    /// UK RPI (Retail Price Index) - used for old-style Linkers
    UKRPI,
    /// UK CPIH (Consumer Price Index including Housing)
    UKCPIH,

    // ==================== Eurozone Indices ====================
    /// Eurozone HICP (Harmonised Index of Consumer Prices)
    EurozoneHICP,
    /// Eurozone HICP ex-Tobacco
    EurozoneHICPExTobacco,
    /// French CPI ex-Tobacco
    FrenchCPIExTobacco,
    /// German HICP
    GermanHICP,
    /// Italian FOI ex-Tobacco
    ItalianFOIExTobacco,

    // ==================== Other Markets ====================
    /// Japanese CPI
    JapaneseCPI,
    /// Canadian CPI
    CanadianCPI,
    /// Australian CPI
    AustralianCPI,
    /// Swedish CPI
    SwedishCPI,

    // ==================== Generic ====================
    /// Custom inflation index
    Custom(String),
}

impl InflationIndexType {
    /// Returns the standard publication lag in months.
    ///
    /// Most inflation indices are published with a lag, meaning the
    /// index value for month M is not available until month M+lag.
    #[must_use]
    pub fn publication_lag_months(&self) -> u32 {
        match self {
            // US CPI: ~2-3 weeks, but interpolation uses 3-month lag
            InflationIndexType::USCPIUrban | InflationIndexType::USCPIUNSA => 3,

            // UK RPI: 2-3 weeks lag, 2-month interpolation for old linkers
            InflationIndexType::UKRPI => 2,
            InflationIndexType::UKCPIH => 2,

            // Eurozone HICP: typically 2-3 week lag
            InflationIndexType::EurozoneHICP
            | InflationIndexType::EurozoneHICPExTobacco
            | InflationIndexType::FrenchCPIExTobacco
            | InflationIndexType::GermanHICP
            | InflationIndexType::ItalianFOIExTobacco => 3,

            // Other markets
            InflationIndexType::JapaneseCPI => 2,
            InflationIndexType::CanadianCPI => 3,
            InflationIndexType::AustralianCPI => 3,
            InflationIndexType::SwedishCPI => 2,

            InflationIndexType::Custom(_) => 3,
        }
    }

    /// Returns the interpolation lag in months for settlement.
    ///
    /// For bonds like TIPS, the index ratio uses values from
    /// several months prior to the settlement date.
    #[must_use]
    pub fn interpolation_lag_months(&self) -> u32 {
        match self {
            // TIPS use 3-month lag with linear interpolation
            InflationIndexType::USCPIUrban | InflationIndexType::USCPIUNSA => 3,

            // UK old linkers use 8-month lag, new style uses 3-month
            InflationIndexType::UKRPI => 8,
            InflationIndexType::UKCPIH => 3,

            // Eurozone linkers typically use 3-month lag
            _ => 3,
        }
    }

    /// Returns the base currency for this inflation index.
    #[must_use]
    pub fn currency(&self) -> &'static str {
        match self {
            InflationIndexType::USCPIUrban | InflationIndexType::USCPIUNSA => "USD",
            InflationIndexType::UKRPI | InflationIndexType::UKCPIH => "GBP",
            InflationIndexType::EurozoneHICP
            | InflationIndexType::EurozoneHICPExTobacco
            | InflationIndexType::FrenchCPIExTobacco
            | InflationIndexType::GermanHICP
            | InflationIndexType::ItalianFOIExTobacco => "EUR",
            InflationIndexType::JapaneseCPI => "JPY",
            InflationIndexType::CanadianCPI => "CAD",
            InflationIndexType::AustralianCPI => "AUD",
            InflationIndexType::SwedishCPI => "SEK",
            InflationIndexType::Custom(_) => "USD",
        }
    }

    /// Returns the standard base year for index rebase.
    ///
    /// Inflation indices are periodically rebased to 100 at a reference date.
    #[must_use]
    pub fn typical_base_year(&self) -> Option<u32> {
        match self {
            InflationIndexType::USCPIUrban | InflationIndexType::USCPIUNSA => Some(1982),
            InflationIndexType::UKRPI => Some(1987),
            InflationIndexType::UKCPIH => Some(2015),
            InflationIndexType::EurozoneHICP | InflationIndexType::EurozoneHICPExTobacco => {
                Some(2015)
            }
            _ => None,
        }
    }

    /// Returns true if this is a commonly used index for sovereign bonds.
    #[must_use]
    pub fn is_sovereign_index(&self) -> bool {
        matches!(
            self,
            InflationIndexType::USCPIUNSA
                | InflationIndexType::UKRPI
                | InflationIndexType::EurozoneHICPExTobacco
                | InflationIndexType::FrenchCPIExTobacco
                | InflationIndexType::GermanHICP
                | InflationIndexType::ItalianFOIExTobacco
                | InflationIndexType::JapaneseCPI
                | InflationIndexType::CanadianCPI
                | InflationIndexType::AustralianCPI
        )
    }
}

impl std::fmt::Display for InflationIndexType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            InflationIndexType::USCPIUrban => "US CPI-U",
            InflationIndexType::USCPIUNSA => "US CPI-U NSA",
            InflationIndexType::UKRPI => "UK RPI",
            InflationIndexType::UKCPIH => "UK CPIH",
            InflationIndexType::EurozoneHICP => "Eurozone HICP",
            InflationIndexType::EurozoneHICPExTobacco => "Eurozone HICP ex-Tobacco",
            InflationIndexType::FrenchCPIExTobacco => "French CPI ex-Tobacco",
            InflationIndexType::GermanHICP => "German HICP",
            InflationIndexType::ItalianFOIExTobacco => "Italian FOI ex-Tobacco",
            InflationIndexType::JapaneseCPI => "Japanese CPI",
            InflationIndexType::CanadianCPI => "Canadian CPI",
            InflationIndexType::AustralianCPI => "Australian CPI",
            InflationIndexType::SwedishCPI => "Swedish CPI",
            InflationIndexType::Custom(name) => return write!(f, "{name}"),
        };
        write!(f, "{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inflation_index_lag() {
        assert_eq!(InflationIndexType::USCPIUNSA.publication_lag_months(), 3);
        assert_eq!(InflationIndexType::USCPIUNSA.interpolation_lag_months(), 3);
        assert_eq!(InflationIndexType::UKRPI.interpolation_lag_months(), 8);
    }

    #[test]
    fn test_inflation_index_currency() {
        assert_eq!(InflationIndexType::USCPIUNSA.currency(), "USD");
        assert_eq!(InflationIndexType::UKRPI.currency(), "GBP");
        assert_eq!(InflationIndexType::EurozoneHICP.currency(), "EUR");
    }

    #[test]
    fn test_inflation_index_display() {
        assert_eq!(format!("{}", InflationIndexType::USCPIUNSA), "US CPI-U NSA");
        assert_eq!(format!("{}", InflationIndexType::UKRPI), "UK RPI");
    }

    #[test]
    fn test_is_sovereign_index() {
        assert!(InflationIndexType::USCPIUNSA.is_sovereign_index());
        assert!(InflationIndexType::UKRPI.is_sovereign_index());
        assert!(!InflationIndexType::USCPIUrban.is_sovereign_index());
    }

    #[test]
    fn test_tenor_reexport() {
        // Verify re-export works
        assert_eq!(Tenor::M3.months(), 3);
        assert!((Tenor::Y5.years() - 5.0).abs() < 1e-10);
    }
}
