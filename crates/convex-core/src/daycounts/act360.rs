//! Actual/360 day count convention.
//!
//! Used primarily for money market instruments.

use rust_decimal::Decimal;

use super::DayCount;
use crate::types::Date;

/// Actual/360 day count convention.
///
/// The day count is the actual number of days between dates.
/// The year basis is always 360 days.
///
/// # Usage
///
/// - Money market instruments (T-Bills, Commercial Paper)
/// - LIBOR/SOFR-based floating rate instruments
/// - EUR interest rate swaps (floating leg)
///
/// # Formula
///
/// $$\text{Year Fraction} = \frac{\text{Actual Days}}{360}$$
///
/// # Bloomberg
///
/// Matches Bloomberg `ACT/360` convention exactly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Act360;

impl DayCount for Act360 {
    fn name(&self) -> &'static str {
        "ACT/360"
    }

    fn year_fraction(&self, start: Date, end: Date) -> Decimal {
        let days = start.days_between(&end);
        Decimal::from(days) / Decimal::from(360)
    }

    fn day_count(&self, start: Date, end: Date) -> i64 {
        start.days_between(&end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_act360_basic() {
        let dc = Act360;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 4, 1).unwrap();

        // Jan has 31, Feb has 28, Mar has 31 = 90 days
        assert_eq!(dc.day_count(start, end), 90);
        assert_eq!(dc.year_fraction(start, end), dec!(90) / dec!(360));
        assert_eq!(dc.year_fraction(start, end), dec!(0.25));
    }

    #[test]
    fn test_act360_full_year() {
        let dc = Act360;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        // Non-leap year: 365 days / 360 > 1
        assert_eq!(dc.day_count(start, end), 365);
        let yf = dc.year_fraction(start, end);
        assert!(yf > Decimal::ONE);
        assert_eq!(yf, dec!(365) / dec!(360));
    }

    #[test]
    fn test_act360_leap_year() {
        let dc = Act360;
        let start = Date::from_ymd(2024, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 1, 1).unwrap();

        // Leap year: 366 days
        assert_eq!(dc.day_count(start, end), 366);
        assert_eq!(dc.year_fraction(start, end), dec!(366) / dec!(360));
    }

    #[test]
    fn test_act360_same_day() {
        let dc = Act360;
        let date = Date::from_ymd(2025, 6, 15).unwrap();

        assert_eq!(dc.day_count(date, date), 0);
        assert_eq!(dc.year_fraction(date, date), dec!(0));
    }

    #[test]
    fn test_act360_negative() {
        let dc = Act360;
        let start = Date::from_ymd(2025, 6, 15).unwrap();
        let end = Date::from_ymd(2025, 6, 1).unwrap();

        // Negative days when end < start
        assert_eq!(dc.day_count(start, end), -14);
        assert_eq!(dc.year_fraction(start, end), dec!(-14) / dec!(360));
    }

    // Bloomberg validation case
    #[test]
    fn test_act360_bloomberg() {
        let dc = Act360;
        // Boeing bond settlement: 04/29/2020 to next coupon 06/15/2020
        let start = Date::from_ymd(2020, 4, 29).unwrap();
        let end = Date::from_ymd(2020, 6, 15).unwrap();

        // Actual days: Apr 29-30 = 1, May = 31, Jun 1-15 = 15 = 47 days
        assert_eq!(dc.day_count(start, end), 47);
    }
}
