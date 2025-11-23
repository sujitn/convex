//! Actual/360 day count convention.

use rust_decimal::Decimal;

use super::DayCount;
use crate::types::Date;

/// Actual/360 day count convention.
///
/// The day count is the actual number of days between dates.
/// The year basis is always 360 days.
///
/// This convention is commonly used for:
/// - Money market instruments
/// - LIBOR/SOFR-based instruments
/// - Commercial paper
///
/// # Formula
///
/// $$\text{Year Fraction} = \frac{\text{Actual Days}}{360}$$
#[derive(Debug, Clone, Copy, Default)]
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
    fn test_act360_year_fraction() {
        let dc = Act360;

        // 90 days = 0.25 years in ACT/360
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 4, 1).unwrap();

        let yf = dc.year_fraction(start, end);
        assert_eq!(yf, dec!(90) / dec!(360));
    }

    #[test]
    fn test_act360_full_year() {
        let dc = Act360;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        // 365 days / 360 > 1
        let yf = dc.year_fraction(start, end);
        assert!(yf > Decimal::ONE);
    }
}
