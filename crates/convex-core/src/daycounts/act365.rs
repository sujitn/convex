//! Actual/365 Fixed day count convention.

use rust_decimal::Decimal;

use super::DayCount;
use crate::types::Date;

/// Actual/365 Fixed day count convention.
///
/// The day count is the actual number of days between dates.
/// The year basis is always 365 days (ignoring leap years).
///
/// This convention is commonly used for:
/// - UK Gilts
/// - AUD and NZD markets
///
/// # Formula
///
/// $$\text{Year Fraction} = \frac{\text{Actual Days}}{365}$$
#[derive(Debug, Clone, Copy, Default)]
pub struct Act365;

impl DayCount for Act365 {
    fn name(&self) -> &'static str {
        "ACT/365"
    }

    fn year_fraction(&self, start: Date, end: Date) -> Decimal {
        let days = start.days_between(&end);
        Decimal::from(days) / Decimal::from(365)
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
    fn test_act365_year_fraction() {
        let dc = Act365;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        // Exactly 1 year for 365-day year
        assert_eq!(dc.year_fraction(start, end), dec!(1));
    }

    #[test]
    fn test_act365_leap_year() {
        let dc = Act365;
        let start = Date::from_ymd(2024, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 1, 1).unwrap();

        // 366 days / 365 > 1 in leap year
        let yf = dc.year_fraction(start, end);
        assert!(yf > Decimal::ONE);
        assert_eq!(dc.day_count(start, end), 366);
    }
}
