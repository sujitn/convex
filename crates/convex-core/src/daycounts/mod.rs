//! Day count conventions for fixed income calculations.
//!
//! Day count conventions determine how accrued interest is calculated
//! by specifying how to count days between two dates and the year basis.
//!
//! # Supported Conventions
//!
//! ## ACT Family (Actual numerator)
//!
//! - [`Act360`]: Actual/360 - Money market convention
//! - [`Act365Fixed`]: Actual/365 Fixed - UK Gilts, AUD/NZD
//! - [`Act365Leap`]: Actual/365 Leap - Adjusts basis for leap years
//! - [`ActActIsda`]: Actual/Actual ISDA - Year-based split
//! - [`ActActIcma`]: Actual/Actual ICMA - Period-based (government bonds)
//! - [`ActActAfb`]: Actual/Actual AFB - French convention
//!
//! ## 30/360 Family (Assumes 30-day months, 360-day years)
//!
//! - [`Thirty360US`]: 30/360 US - US corporate bonds (with Feb EOM rules)
//! - [`Thirty360E`]: 30E/360 - Eurobond convention
//! - [`Thirty360EIsda`]: 30E/360 ISDA - ISDA swap convention
//! - [`Thirty360German`]: 30/360 German - German market convention
//!
//! # Usage
//!
//! ```rust
//! use convex_core::daycounts::{DayCount, Act360, Thirty360US};
//! use convex_core::types::Date;
//!
//! let dc = Thirty360US;
//! let start = Date::from_ymd(2025, 1, 15).unwrap();
//! let end = Date::from_ymd(2025, 7, 15).unwrap();
//!
//! let days = dc.day_count(start, end);
//! let year_fraction = dc.year_fraction(start, end);
//! ```
//!
//! # Bloomberg Compatibility
//!
//! All implementations are designed to match Bloomberg YAS exactly:
//! - 30/360 US includes February end-of-month rules
//! - ACT/ACT ICMA uses period-based calculation
//! - All conventions handle leap years correctly

mod act360;
mod act365;
mod actact;
mod thirty360;

pub use act360::Act360;
pub use act365::{Act365, Act365Fixed, Act365Leap};
pub use actact::{ActActAfb, ActActIcma, ActActIsda};
pub use thirty360::{Thirty360, Thirty360E, Thirty360EIsda, Thirty360German, Thirty360US};

use crate::types::Date;
use rust_decimal::Decimal;

/// Trait for day count conventions.
///
/// Implementations provide the year fraction calculation between two dates
/// according to specific market conventions.
///
/// # Implementation Notes
///
/// - `year_fraction` returns the fraction of a year between dates
/// - `day_count` returns the number of days according to the convention
/// - Implementations must be thread-safe (`Send + Sync`)
pub trait DayCount: Send + Sync {
    /// Returns the name of the day count convention.
    ///
    /// This should match Bloomberg's convention naming (e.g., "ACT/360", "30/360 US").
    fn name(&self) -> &'static str;

    /// Calculates the year fraction between two dates.
    ///
    /// # Arguments
    ///
    /// * `start` - Start date (exclusive for accrual)
    /// * `end` - End date (inclusive for accrual)
    ///
    /// # Returns
    ///
    /// The fraction of a year between the two dates. Can be negative if end < start.
    fn year_fraction(&self, start: Date, end: Date) -> Decimal;

    /// Calculates the day count between two dates.
    ///
    /// Returns the number of days according to the convention.
    /// For ACT conventions, this is actual calendar days.
    /// For 30/360 conventions, this uses the 30-day month assumption.
    fn day_count(&self, start: Date, end: Date) -> i64;
}

/// Enumeration of all supported day count conventions.
///
/// This enum provides a convenient way to select conventions at runtime
/// and convert to boxed trait objects.
///
/// # Example
///
/// ```rust
/// use convex_core::daycounts::{DayCountConvention, DayCount};
/// use convex_core::types::Date;
///
/// let convention = DayCountConvention::Thirty360US;
/// let dc = convention.to_day_count();
///
/// let start = Date::from_ymd(2025, 1, 1).unwrap();
/// let end = Date::from_ymd(2025, 7, 1).unwrap();
/// let yf = dc.year_fraction(start, end);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DayCountConvention {
    // =========================================================================
    // ACT Family
    // =========================================================================
    /// Actual/360 - Money market instruments, FRNs
    Act360,

    /// Actual/365 Fixed - UK Gilts, AUD/NZD markets
    Act365Fixed,

    /// Actual/365 Leap - Adjusts denominator for leap years
    Act365Leap,

    /// Actual/Actual ISDA - Year-based calculation for swaps
    ActActIsda,

    /// Actual/Actual ICMA - Period-based calculation for bonds
    /// Uses semi-annual frequency by default
    ActActIcma,

    /// Actual/Actual AFB - French convention
    ActActAfb,

    // =========================================================================
    // 30/360 Family
    // =========================================================================
    /// 30/360 US (Bond Basis) - US corporate, agency, municipal bonds
    /// Includes Bloomberg-exact February end-of-month rules
    Thirty360US,

    /// 30E/360 (Eurobond Basis) - Eurobonds, European corporates
    Thirty360E,

    /// 30E/360 ISDA - ISDA swap convention with EOM handling
    Thirty360EIsda,

    /// 30/360 German - German market convention
    Thirty360German,
}

impl DayCountConvention {
    /// Creates a boxed day count implementation.
    ///
    /// # Returns
    ///
    /// A boxed trait object that can be used for day count calculations.
    #[must_use]
    pub fn to_day_count(&self) -> Box<dyn DayCount> {
        match self {
            // ACT Family
            DayCountConvention::Act360 => Box::new(Act360),
            DayCountConvention::Act365Fixed => Box::new(Act365Fixed),
            DayCountConvention::Act365Leap => Box::new(Act365Leap),
            DayCountConvention::ActActIsda => Box::new(ActActIsda),
            DayCountConvention::ActActIcma => Box::new(ActActIcma::default()),
            DayCountConvention::ActActAfb => Box::new(ActActAfb),

            // 30/360 Family
            DayCountConvention::Thirty360US => Box::new(Thirty360US),
            DayCountConvention::Thirty360E => Box::new(Thirty360E),
            DayCountConvention::Thirty360EIsda => Box::new(Thirty360EIsda::default()),
            DayCountConvention::Thirty360German => Box::new(Thirty360German),
        }
    }

    /// Returns the name of the convention as used by Bloomberg.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
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

    /// Returns all available day count conventions.
    #[must_use]
    pub fn all() -> &'static [DayCountConvention] {
        &[
            DayCountConvention::Act360,
            DayCountConvention::Act365Fixed,
            DayCountConvention::Act365Leap,
            DayCountConvention::ActActIsda,
            DayCountConvention::ActActIcma,
            DayCountConvention::ActActAfb,
            DayCountConvention::Thirty360US,
            DayCountConvention::Thirty360E,
            DayCountConvention::Thirty360EIsda,
            DayCountConvention::Thirty360German,
        ]
    }
}

impl std::fmt::Display for DayCountConvention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_act360() {
        let dc = Act360;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 7, 1).unwrap();

        assert_eq!(dc.day_count(start, end), 181);
        let yf = dc.year_fraction(start, end);
        assert!(yf > dec!(0.5));
    }

    #[test]
    fn test_act365_fixed() {
        let dc = Act365Fixed;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        assert_eq!(dc.day_count(start, end), 365);
        assert_eq!(dc.year_fraction(start, end), dec!(1));
    }

    #[test]
    fn test_thirty360_us() {
        let dc = Thirty360US;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        assert_eq!(dc.day_count(start, end), 360);
        assert_eq!(dc.year_fraction(start, end), dec!(1));
    }

    #[test]
    fn test_thirty360_us_feb_eom() {
        // Critical test: February end-of-month handling
        let dc = Thirty360US;

        // Feb 28 (non-leap) to Mar 31
        let start = Date::from_ymd(2025, 2, 28).unwrap();
        let end = Date::from_ymd(2025, 3, 31).unwrap();

        // D1=30 (Feb EOM), D1>=30 so D2=30
        // Days = 30*(3-2) + (30-30) = 30
        assert_eq!(dc.day_count(start, end), 30);
    }

    #[test]
    fn test_convention_enum() {
        // Test that all conventions can be created via enum
        for convention in DayCountConvention::all() {
            let dc = convention.to_day_count();
            let name = dc.name();
            assert!(!name.is_empty());

            // Test basic calculation
            let start = Date::from_ymd(2025, 1, 1).unwrap();
            let end = Date::from_ymd(2025, 7, 1).unwrap();
            let yf = dc.year_fraction(start, end);

            // All conventions should give roughly half a year
            assert!(yf > dec!(0.4) && yf < dec!(0.6));
        }
    }

    #[test]
    fn test_convention_names() {
        assert_eq!(DayCountConvention::Act360.name(), "ACT/360");
        assert_eq!(DayCountConvention::Act365Fixed.name(), "ACT/365F");
        assert_eq!(DayCountConvention::ActActIcma.name(), "ACT/ACT ICMA");
        assert_eq!(DayCountConvention::Thirty360US.name(), "30/360 US");
        assert_eq!(DayCountConvention::Thirty360E.name(), "30E/360");
    }

    #[test]
    fn test_convention_display() {
        let conv = DayCountConvention::Thirty360US;
        assert_eq!(format!("{}", conv), "30/360 US");
    }

    // =========================================================================
    // Bloomberg Validation: Boeing 7.5% 06/15/2025
    // =========================================================================

    #[test]
    fn test_bloomberg_boeing_validation() {
        // CUSIP: 097023AH7
        // Settlement: 04/29/2020
        // Price: 110.503
        // Day count: 30/360 US

        let dc = Thirty360US;
        let last_coupon = Date::from_ymd(2019, 12, 15).unwrap();
        let settlement = Date::from_ymd(2020, 4, 29).unwrap();

        // Bloomberg shows 134 accrued days
        assert_eq!(dc.day_count(last_coupon, settlement), 134);
    }

    #[test]
    fn test_actact_icma_period() {
        // Test with explicit period information
        let dc = ActActIcma::semi_annual();

        let period_start = Date::from_ymd(2024, 11, 15).unwrap();
        let period_end = Date::from_ymd(2025, 5, 15).unwrap();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let yf = dc.year_fraction_with_period(period_start, settlement, period_start, period_end);

        // 61 days accrued / (2 * 181 days in period) = 61/362
        assert_eq!(yf, dec!(61) / dec!(362));
    }
}
