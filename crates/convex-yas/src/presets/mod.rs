//! Market presets for bond creation and validation.
//!
//! This module provides [`MarketPreset`] which encapsulates market-specific
//! conventions for bond creation. Presets include:
//!
//! - Day count convention
//! - Coupon frequency
//! - Settlement days
//! - Yield calculation method
//! - Money market threshold
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_yas::presets::US_TREASURY;
//! use convex_bonds::FixedRateBondBuilder;
//!
//! let bond = FixedRateBondBuilder::new()
//!     .with_preset(&US_TREASURY)
//!     .coupon_rate(dec!(0.05))
//!     .maturity(date)
//!     .build()?;
//! ```

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Frequency, YieldMethod};

use crate::yields::{YieldCalculatorConfig, MM_THRESHOLD_CAD, MM_THRESHOLD_US};

/// Market preset for bond creation and validation.
///
/// A `MarketPreset` contains all the conventions needed to create and
/// price bonds in a specific market. It can be used with bond builders
/// to apply default conventions, and with `YieldCalculator` to configure
/// yield calculation behavior.
///
/// # Validation
///
/// Presets can validate that a bond conforms to expected conventions using
/// the `validate()` method. This helps catch configuration errors early.
#[derive(Debug, Clone)]
pub struct MarketPreset {
    /// Human-readable name for the preset.
    pub name: &'static str,

    /// Day count convention for accrued interest.
    pub day_count: DayCountConvention,

    /// Coupon payment frequency.
    pub frequency: Frequency,

    /// Settlement days (T+n).
    pub settlement_days: u32,

    /// Yield calculation method.
    pub yield_method: YieldMethod,

    /// Money market threshold in days (for short-dated bonds).
    pub money_market_threshold: Option<u32>,

    /// Ex-dividend days (for markets with ex-dividend conventions).
    pub ex_dividend_days: Option<u32>,
}

impl MarketPreset {
    /// Creates a `YieldCalculatorConfig` from this preset.
    #[must_use]
    pub fn yield_config(&self) -> YieldCalculatorConfig {
        let mut builder = YieldCalculatorConfig::builder().method(self.yield_method);

        if let Some(threshold) = self.money_market_threshold {
            builder = builder.money_market_threshold(threshold);
        }

        builder.build()
    }

    /// Returns the day count convention string for bond builders.
    #[must_use]
    pub fn day_count_str(&self) -> &'static str {
        match self.day_count {
            DayCountConvention::Act360 => "ACT/360",
            DayCountConvention::Act365Fixed => "ACT/365F",
            DayCountConvention::Act365Leap => "ACT/365L",
            DayCountConvention::ActActIsda => "ACT/ACT ISDA",
            DayCountConvention::ActActIcma => "ACT/ACT ICMA",
            DayCountConvention::ActActAfb => "ACT/ACT AFB",
            DayCountConvention::Thirty360US => "30/360 US",
            DayCountConvention::Thirty360E => "30E/360",
            DayCountConvention::Thirty360EIsda => "30E/360 ISDA",
            DayCountConvention::Thirty360German => "30/360 German",
        }
    }
}

// ============================================================================
// US Market Presets
// ============================================================================

/// US Treasury notes and bonds (coupon-bearing).
///
/// - Day count: ACT/ACT ICMA
/// - Frequency: Semi-annual
/// - Settlement: T+1
/// - Yield: Compounded (Street Convention)
/// - MM threshold: 182 days
pub const US_TREASURY: MarketPreset = MarketPreset {
    name: "US Treasury",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::SemiAnnual,
    settlement_days: 1,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: Some(MM_THRESHOLD_US),
    ex_dividend_days: None,
};

/// US Treasury Bills (discount instruments).
///
/// - Day count: ACT/360
/// - Frequency: Zero coupon
/// - Settlement: T+1
/// - Yield: Discount
pub const US_TBILL: MarketPreset = MarketPreset {
    name: "US Treasury Bill",
    day_count: DayCountConvention::Act360,
    frequency: Frequency::Zero,
    settlement_days: 1,
    yield_method: YieldMethod::Discount,
    money_market_threshold: None,
    ex_dividend_days: None,
};

/// US Corporate bonds (investment grade).
///
/// - Day count: 30/360 US
/// - Frequency: Semi-annual
/// - Settlement: T+2
/// - Yield: Compounded (Street Convention)
/// - MM threshold: 182 days
pub const US_CORPORATE: MarketPreset = MarketPreset {
    name: "US Corporate",
    day_count: DayCountConvention::Thirty360US,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: Some(MM_THRESHOLD_US),
    ex_dividend_days: None,
};

/// US Municipal bonds.
///
/// - Day count: 30/360 US
/// - Frequency: Semi-annual
/// - Settlement: T+2
/// - Yield: Compounded (Street Convention)
pub const US_MUNICIPAL: MarketPreset = MarketPreset {
    name: "US Municipal",
    day_count: DayCountConvention::Thirty360US,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: Some(MM_THRESHOLD_US),
    ex_dividend_days: None,
};

// ============================================================================
// European Market Presets
// ============================================================================

/// UK Gilts (conventional).
///
/// - Day count: ACT/ACT ICMA
/// - Frequency: Semi-annual
/// - Settlement: T+1
/// - Yield: Compounded (ICMA)
/// - Ex-dividend: 7 business days
pub const UK_GILT: MarketPreset = MarketPreset {
    name: "UK Gilt",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::SemiAnnual,
    settlement_days: 1,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: Some(7),
};

/// German Bunds.
///
/// - Day count: ACT/ACT ICMA
/// - Frequency: Annual
/// - Settlement: T+2
/// - Yield: Compounded (ICMA)
pub const GERMAN_BUND: MarketPreset = MarketPreset {
    name: "German Bund",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::Annual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

/// French OATs.
///
/// - Day count: ACT/ACT ICMA
/// - Frequency: Annual
/// - Settlement: T+2
/// - Yield: Compounded (ICMA)
pub const FRENCH_OAT: MarketPreset = MarketPreset {
    name: "French OAT",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::Annual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

/// Italian BTPs.
///
/// - Day count: ACT/ACT ICMA
/// - Frequency: Semi-annual
/// - Settlement: T+2
/// - Yield: Compounded (ICMA)
pub const ITALIAN_BTP: MarketPreset = MarketPreset {
    name: "Italian BTP",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

/// Standard Eurobonds.
///
/// - Day count: 30E/360
/// - Frequency: Annual
/// - Settlement: T+2
/// - Yield: Compounded (ICMA)
pub const EUROBOND: MarketPreset = MarketPreset {
    name: "Eurobond",
    day_count: DayCountConvention::Thirty360E,
    frequency: Frequency::Annual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

// ============================================================================
// Asia-Pacific Market Presets
// ============================================================================

/// Japanese Government Bonds (JGBs).
///
/// - Day count: ACT/365 Fixed
/// - Frequency: Semi-annual
/// - Settlement: T+2
/// - Yield: Simple (no compounding)
pub const JAPANESE_JGB: MarketPreset = MarketPreset {
    name: "Japanese JGB",
    day_count: DayCountConvention::Act365Fixed,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Simple,
    money_market_threshold: None,
    ex_dividend_days: None,
};

/// Australian Government Bonds.
///
/// - Day count: ACT/ACT ICMA
/// - Frequency: Semi-annual
/// - Settlement: T+2
/// - Yield: Compounded
pub const AUSTRALIAN_GOVT: MarketPreset = MarketPreset {
    name: "Australian Government",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

// ============================================================================
// Canadian Market Presets
// ============================================================================

/// Canadian Government Bonds.
///
/// - Day count: ACT/365 Fixed
/// - Frequency: Semi-annual
/// - Settlement: T+2
/// - Yield: Compounded
/// - MM threshold: 365 days
pub const CANADIAN_GOVT: MarketPreset = MarketPreset {
    name: "Canadian Government",
    day_count: DayCountConvention::Act365Fixed,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: Some(MM_THRESHOLD_CAD),
    ex_dividend_days: None,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_us_treasury_preset() {
        assert_eq!(US_TREASURY.name, "US Treasury");
        assert_eq!(US_TREASURY.day_count, DayCountConvention::ActActIcma);
        assert_eq!(US_TREASURY.frequency, Frequency::SemiAnnual);
        assert_eq!(US_TREASURY.settlement_days, 1);
        assert_eq!(US_TREASURY.yield_method, YieldMethod::Compounded);
        assert_eq!(US_TREASURY.money_market_threshold, Some(182));
    }

    #[test]
    fn test_us_tbill_preset() {
        assert_eq!(US_TBILL.name, "US Treasury Bill");
        assert_eq!(US_TBILL.day_count, DayCountConvention::Act360);
        assert_eq!(US_TBILL.frequency, Frequency::Zero);
        assert_eq!(US_TBILL.yield_method, YieldMethod::Discount);
    }

    #[test]
    fn test_us_corporate_preset() {
        assert_eq!(US_CORPORATE.day_count, DayCountConvention::Thirty360US);
        assert_eq!(US_CORPORATE.settlement_days, 2);
    }

    #[test]
    fn test_uk_gilt_preset() {
        assert_eq!(UK_GILT.ex_dividend_days, Some(7));
        assert_eq!(UK_GILT.frequency, Frequency::SemiAnnual);
    }

    #[test]
    fn test_german_bund_preset() {
        assert_eq!(GERMAN_BUND.frequency, Frequency::Annual);
        assert_eq!(GERMAN_BUND.day_count, DayCountConvention::ActActIcma);
    }

    #[test]
    fn test_japanese_jgb_preset() {
        assert_eq!(JAPANESE_JGB.yield_method, YieldMethod::Simple);
        assert_eq!(JAPANESE_JGB.day_count, DayCountConvention::Act365Fixed);
    }

    #[test]
    fn test_canadian_preset() {
        assert_eq!(CANADIAN_GOVT.money_market_threshold, Some(365));
    }

    #[test]
    fn test_yield_config_from_preset() {
        let config = US_TREASURY.yield_config();
        assert_eq!(config.method(), YieldMethod::Compounded);
        assert_eq!(config.money_market_threshold(), Some(182));
    }

    #[test]
    fn test_yield_config_from_jgb_preset() {
        let config = JAPANESE_JGB.yield_config();
        assert_eq!(config.method(), YieldMethod::Simple);
        assert_eq!(config.money_market_threshold(), None);
    }

    #[test]
    fn test_day_count_str() {
        assert_eq!(US_TREASURY.day_count_str(), "ACT/ACT ICMA");
        assert_eq!(US_CORPORATE.day_count_str(), "30/360 US");
        assert_eq!(EUROBOND.day_count_str(), "30E/360");
        assert_eq!(JAPANESE_JGB.day_count_str(), "ACT/365F");
    }
}
