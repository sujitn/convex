//! Market conventions for government bonds.
//!
//! Different markets use different day count conventions, payment frequencies,
//! and settlement periods. This module provides these conventions in a
//! standardized form for use in curve construction.

use convex_core::Date;

/// Market convention for government bonds.
///
/// Each variant encapsulates the day count, frequency, and typical settlement
/// for a specific government bond market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarketConvention {
    /// US Treasury: ACT/ACT, semi-annual, T+1
    USTreasury,
    /// UK Gilt: ACT/365 Fixed, semi-annual, T+1
    UKGilt,
    /// German Bund: ACT/ACT ICMA, annual, T+2
    GermanBund,
    /// French OAT: ACT/ACT ICMA, annual, T+2
    FrenchOAT,
    /// Japanese JGB: ACT/365 Fixed, semi-annual, T+2
    JapaneseJGB,
    /// Canadian Government Bond: ACT/365 Fixed, semi-annual, T+2
    CanadianGovt,
    /// Australian Government Bond: ACT/ACT ICMA, semi-annual, T+2
    AustralianGovt,
    /// Generic using ACT/365 Fixed
    Generic365,
    /// Generic using ACT/360
    Generic360,
    /// Generic using ACT/ACT
    GenericActAct,
}

impl MarketConvention {
    /// Returns the day count convention name.
    #[must_use]
    pub fn day_count_name(&self) -> &'static str {
        match self {
            Self::USTreasury => "ACT/ACT",
            Self::UKGilt => "ACT/365F",
            Self::GermanBund => "ACT/ACT ICMA",
            Self::FrenchOAT => "ACT/ACT ICMA",
            Self::JapaneseJGB => "ACT/365F",
            Self::CanadianGovt => "ACT/365F",
            Self::AustralianGovt => "ACT/ACT ICMA",
            Self::Generic365 => "ACT/365F",
            Self::Generic360 => "ACT/360",
            Self::GenericActAct => "ACT/ACT",
        }
    }

    /// Returns the coupon frequency (payments per year).
    #[must_use]
    pub fn coupons_per_year(&self) -> u32 {
        match self {
            Self::USTreasury => 2,      // Semi-annual
            Self::UKGilt => 2,          // Semi-annual
            Self::GermanBund => 1,      // Annual
            Self::FrenchOAT => 1,       // Annual
            Self::JapaneseJGB => 2,     // Semi-annual
            Self::CanadianGovt => 2,    // Semi-annual
            Self::AustralianGovt => 2,  // Semi-annual
            Self::Generic365 => 2,
            Self::Generic360 => 2,
            Self::GenericActAct => 2,
        }
    }

    /// Returns the settlement period in business days.
    #[must_use]
    pub fn settlement_days(&self) -> u32 {
        match self {
            Self::USTreasury => 1,      // T+1
            Self::UKGilt => 1,          // T+1
            Self::GermanBund => 2,      // T+2
            Self::FrenchOAT => 2,       // T+2
            Self::JapaneseJGB => 2,     // T+2
            Self::CanadianGovt => 2,    // T+2
            Self::AustralianGovt => 2,  // T+2
            Self::Generic365 => 2,
            Self::Generic360 => 2,
            Self::GenericActAct => 2,
        }
    }

    /// Returns the days-per-year divisor for the day count.
    #[must_use]
    pub fn year_basis(&self) -> f64 {
        match self {
            Self::USTreasury => 365.0,  // ACT/ACT uses actual, approx 365
            Self::UKGilt => 365.0,      // ACT/365F
            Self::GermanBund => 365.0,  // ACT/ACT ICMA
            Self::FrenchOAT => 365.0,   // ACT/ACT ICMA
            Self::JapaneseJGB => 365.0, // ACT/365F
            Self::CanadianGovt => 365.0, // ACT/365F
            Self::AustralianGovt => 365.0, // ACT/ACT ICMA
            Self::Generic365 => 365.0,
            Self::Generic360 => 360.0,
            Self::GenericActAct => 365.0,
        }
    }
}

impl std::fmt::Display for MarketConvention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::USTreasury => write!(f, "US Treasury"),
            Self::UKGilt => write!(f, "UK Gilt"),
            Self::GermanBund => write!(f, "German Bund"),
            Self::FrenchOAT => write!(f, "French OAT"),
            Self::JapaneseJGB => write!(f, "Japanese JGB"),
            Self::CanadianGovt => write!(f, "Canadian Govt"),
            Self::AustralianGovt => write!(f, "Australian Govt"),
            Self::Generic365 => write!(f, "Generic ACT/365"),
            Self::Generic360 => write!(f, "Generic ACT/360"),
            Self::GenericActAct => write!(f, "Generic ACT/ACT"),
        }
    }
}

/// Calculates the year fraction between two dates using the specified convention.
///
/// # Arguments
///
/// * `start` - Start date
/// * `end` - End date
/// * `convention` - Market convention determining day count
///
/// # Returns
///
/// The year fraction (e.g., 0.5 for 6 months).
#[must_use]
pub fn day_count_factor(start: Date, end: Date, convention: MarketConvention) -> f64 {
    let days = start.days_between(&end) as f64;
    days / convention.year_basis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_convention_day_counts() {
        assert_eq!(MarketConvention::USTreasury.day_count_name(), "ACT/ACT");
        assert_eq!(MarketConvention::UKGilt.day_count_name(), "ACT/365F");
        assert_eq!(MarketConvention::GermanBund.day_count_name(), "ACT/ACT ICMA");
        assert_eq!(MarketConvention::Generic360.year_basis(), 360.0);
    }

    #[test]
    fn test_market_convention_frequency() {
        assert_eq!(MarketConvention::USTreasury.coupons_per_year(), 2);
        assert_eq!(MarketConvention::GermanBund.coupons_per_year(), 1);
    }

    #[test]
    fn test_day_count_factor() {
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 7, 1).unwrap();

        // 181 days using ACT/365
        let dcf = day_count_factor(start, end, MarketConvention::UKGilt);
        assert!((dcf - 181.0 / 365.0).abs() < 1e-10);

        // 181 days using ACT/360
        let dcf360 = day_count_factor(start, end, MarketConvention::Generic360);
        assert!((dcf360 - 181.0 / 360.0).abs() < 1e-10);
    }
}
