//! Actual/365 day count conventions.
//!
//! This module provides ACT/365 Fixed and ACT/365 Leap variants.

use rust_decimal::Decimal;

use super::DayCount;
use crate::types::Date;

/// Actual/365 Fixed day count convention.
///
/// The day count is the actual number of days between dates.
/// The year basis is always 365 days (ignoring leap years).
///
/// # Usage
///
/// - UK Gilts
/// - AUD and NZD markets
/// - Sterling interest rate swaps (fixed leg)
///
/// # Formula
///
/// $$\text{Year Fraction} = \frac{\text{Actual Days}}{365}$$
///
/// # Bloomberg
///
/// Matches Bloomberg `ACT/365F` or `ACT/365 FIXED` convention.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Act365Fixed;

impl DayCount for Act365Fixed {
    fn name(&self) -> &'static str {
        "ACT/365F"
    }

    fn year_fraction(&self, start: Date, end: Date) -> Decimal {
        let days = start.days_between(&end);
        Decimal::from(days) / Decimal::from(365)
    }

    fn day_count(&self, start: Date, end: Date) -> i64 {
        start.days_between(&end)
    }
}

/// Actual/365 Leap day count convention (ACT/365L).
///
/// The denominator is 366 if the period includes February 29 of a leap year,
/// otherwise 365.
///
/// # Usage
///
/// - ISDA 2006 definitions for certain products
/// - Some floating rate notes
///
/// # Formula
///
/// $$\text{Year Fraction} = \frac{\text{Actual Days}}{365 \text{ or } 366}$$
///
/// The denominator is 366 if:
/// - The period contains Feb 29, OR
/// - The end date is in a leap year (ISDA variant)
///
/// # Bloomberg
///
/// Matches Bloomberg `ACT/365L` convention.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Act365Leap;

impl Act365Leap {
    /// Checks if the period from start to end contains February 29.
    fn contains_feb_29(start: Date, end: Date) -> bool {
        if start >= end {
            return false;
        }

        let start_year = start.year();
        let end_year = end.year();

        for year in start_year..=end_year {
            // Check if this year is a leap year
            if !is_leap_year(year) {
                continue;
            }

            // Feb 29 of this leap year
            let feb_29 = Date::from_ymd(year, 2, 29).unwrap();

            // Check if Feb 29 falls within [start, end]
            if feb_29 > start && feb_29 <= end {
                return true;
            }
        }

        false
    }
}

impl DayCount for Act365Leap {
    fn name(&self) -> &'static str {
        "ACT/365L"
    }

    fn year_fraction(&self, start: Date, end: Date) -> Decimal {
        let days = start.days_between(&end);
        let basis = if Self::contains_feb_29(start, end) {
            366
        } else {
            365
        };
        Decimal::from(days) / Decimal::from(basis)
    }

    fn day_count(&self, start: Date, end: Date) -> i64 {
        start.days_between(&end)
    }
}

/// Helper function to check if a year is a leap year.
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Type alias for backwards compatibility.
///
/// `Act365` is equivalent to [`Act365Fixed`].
pub type Act365 = Act365Fixed;

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // ACT/365 Fixed tests
    #[test]
    fn test_act365f_full_year_non_leap() {
        let dc = Act365Fixed;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        // 365 days / 365 = 1
        assert_eq!(dc.day_count(start, end), 365);
        assert_eq!(dc.year_fraction(start, end), dec!(1));
    }

    #[test]
    fn test_act365f_full_year_leap() {
        let dc = Act365Fixed;
        let start = Date::from_ymd(2024, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 1, 1).unwrap();

        // 366 days / 365 > 1 (leap year has extra day)
        assert_eq!(dc.day_count(start, end), 366);
        let yf = dc.year_fraction(start, end);
        assert!(yf > Decimal::ONE);
        assert_eq!(yf, dec!(366) / dec!(365));
    }

    #[test]
    fn test_act365f_half_year() {
        let dc = Act365Fixed;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 7, 2).unwrap();

        // Jan=31, Feb=28, Mar=31, Apr=30, May=31, Jun=30, Jul 1 = 182 days
        assert_eq!(dc.day_count(start, end), 182);
    }

    #[test]
    fn test_act365f_same_day() {
        let dc = Act365Fixed;
        let date = Date::from_ymd(2025, 6, 15).unwrap();

        assert_eq!(dc.day_count(date, date), 0);
        assert_eq!(dc.year_fraction(date, date), dec!(0));
    }

    // ACT/365L tests
    #[test]
    fn test_act365l_contains_feb29() {
        let dc = Act365Leap;

        // Period containing Feb 29, 2024
        let start = Date::from_ymd(2024, 1, 1).unwrap();
        let end = Date::from_ymd(2024, 3, 1).unwrap();
        assert_eq!(dc.day_count(start, end), 60); // 31 + 29
        // Should use 366 as basis
        assert_eq!(dc.year_fraction(start, end), dec!(60) / dec!(366));
    }

    #[test]
    fn test_act365l_no_feb29() {
        let dc = Act365Leap;

        // Period not containing Feb 29
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 3, 1).unwrap();
        assert_eq!(dc.day_count(start, end), 59); // 31 + 28
        // Should use 365 as basis
        assert_eq!(dc.year_fraction(start, end), dec!(59) / dec!(365));
    }

    #[test]
    fn test_act365l_period_after_feb29() {
        let dc = Act365Leap;

        // Period in leap year but after Feb 29
        let start = Date::from_ymd(2024, 3, 1).unwrap();
        let end = Date::from_ymd(2024, 6, 1).unwrap();
        assert_eq!(dc.day_count(start, end), 92); // 31 + 30 + 31
        // Does not contain Feb 29, so use 365
        assert_eq!(dc.year_fraction(start, end), dec!(92) / dec!(365));
    }

    #[test]
    fn test_act365l_cross_year() {
        let dc = Act365Leap;

        // Crossing from 2023 into 2024 (leap year)
        let start = Date::from_ymd(2023, 12, 1).unwrap();
        let end = Date::from_ymd(2024, 3, 1).unwrap();
        assert_eq!(dc.day_count(start, end), 91); // 31 + 31 + 29
        // Contains Feb 29, 2024
        assert_eq!(dc.year_fraction(start, end), dec!(91) / dec!(366));
    }

    #[test]
    fn test_act365l_exactly_feb29() {
        let dc = Act365Leap;

        // Period ending exactly on Feb 29
        let start = Date::from_ymd(2024, 2, 1).unwrap();
        let end = Date::from_ymd(2024, 2, 29).unwrap();
        assert_eq!(dc.day_count(start, end), 28);
        // Feb 29 is included, so use 366
        assert_eq!(dc.year_fraction(start, end), dec!(28) / dec!(366));
    }

    // Bloomberg validation
    #[test]
    fn test_act365_bloomberg_gilt() {
        let dc = Act365Fixed;
        // UK Gilt coupon calculation
        let start = Date::from_ymd(2025, 3, 7).unwrap();
        let end = Date::from_ymd(2025, 9, 7).unwrap();
        assert_eq!(dc.day_count(start, end), 184); // Exactly 6 months
    }
}
