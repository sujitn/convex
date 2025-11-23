//! Day count conventions for fixed income calculations.
//!
//! Day count conventions determine how accrued interest is calculated
//! by specifying how to count days between two dates.
//!
//! # Supported Conventions
//!
//! - [`Act360`]: Actual/360 - Money market convention
//! - [`Act365`]: Actual/365 Fixed - UK Gilts
//! - [`Thirty360`]: 30/360 - US corporate bonds
//! - [`Thirty360E`]: 30E/360 - European convention
//! - [`ActActIsda`]: Actual/Actual ISDA - Government bonds
//! - [`ActActIcma`]: Actual/Actual ICMA - ISMA convention

mod act360;
mod act365;
mod actact;
mod thirty360;

pub use act360::Act360;
pub use act365::Act365;
pub use actact::{ActActIcma, ActActIsda};
pub use thirty360::{Thirty360, Thirty360E};

use crate::types::Date;
use rust_decimal::Decimal;

/// Trait for day count conventions.
///
/// Implementations provide the year fraction calculation between two dates
/// according to specific market conventions.
pub trait DayCount: Send + Sync {
    /// Returns the name of the day count convention.
    fn name(&self) -> &'static str;

    /// Calculates the year fraction between two dates.
    ///
    /// # Arguments
    ///
    /// * `start` - Start date (exclusive)
    /// * `end` - End date (inclusive)
    ///
    /// # Returns
    ///
    /// The fraction of a year between the two dates.
    fn year_fraction(&self, start: Date, end: Date) -> Decimal;

    /// Calculates the day count between two dates.
    ///
    /// Returns the number of days according to the convention.
    fn day_count(&self, start: Date, end: Date) -> i64;
}

/// Enumeration of all supported day count conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayCountConvention {
    /// Actual/360
    Act360,
    /// Actual/365 Fixed
    Act365,
    /// 30/360 US
    Thirty360,
    /// 30E/360 European
    Thirty360E,
    /// Actual/Actual ISDA
    ActActIsda,
    /// Actual/Actual ICMA
    ActActIcma,
}

impl DayCountConvention {
    /// Creates a boxed day count implementation.
    #[must_use]
    pub fn to_day_count(&self) -> Box<dyn DayCount> {
        match self {
            DayCountConvention::Act360 => Box::new(Act360),
            DayCountConvention::Act365 => Box::new(Act365),
            DayCountConvention::Thirty360 => Box::new(Thirty360),
            DayCountConvention::Thirty360E => Box::new(Thirty360E),
            DayCountConvention::ActActIsda => Box::new(ActActIsda),
            DayCountConvention::ActActIcma => Box::new(ActActIcma::default()),
        }
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
    fn test_act365() {
        let dc = Act365;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        assert_eq!(dc.day_count(start, end), 365);
        assert_eq!(dc.year_fraction(start, end), dec!(1));
    }
}
