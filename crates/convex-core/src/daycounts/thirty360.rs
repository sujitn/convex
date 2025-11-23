//! 30/360 day count conventions.

use rust_decimal::Decimal;

use super::DayCount;
use crate::types::Date;

/// 30/360 US day count convention.
///
/// Also known as "Bond Basis" or "30/360 ISDA".
///
/// This convention assumes 30-day months and 360-day years.
///
/// # Rules
///
/// - If D1 is 31, change to 30
/// - If D2 is 31 and D1 is 30 or 31, change D2 to 30
///
/// # Formula
///
/// $$\text{Days} = 360 \times (Y_2 - Y_1) + 30 \times (M_2 - M_1) + (D_2 - D_1)$$
#[derive(Debug, Clone, Copy, Default)]
pub struct Thirty360;

impl DayCount for Thirty360 {
    fn name(&self) -> &'static str {
        "30/360"
    }

    fn year_fraction(&self, start: Date, end: Date) -> Decimal {
        let days = self.day_count(start, end);
        Decimal::from(days) / Decimal::from(360)
    }

    fn day_count(&self, start: Date, end: Date) -> i64 {
        let mut d1 = start.day() as i64;
        let mut d2 = end.day() as i64;
        let m1 = start.month() as i64;
        let m2 = end.month() as i64;
        let y1 = start.year() as i64;
        let y2 = end.year() as i64;

        // 30/360 US rules
        if d1 == 31 {
            d1 = 30;
        }
        if d2 == 31 && d1 >= 30 {
            d2 = 30;
        }

        360 * (y2 - y1) + 30 * (m2 - m1) + (d2 - d1)
    }
}

/// 30E/360 European day count convention.
///
/// Also known as "Eurobond Basis" or "30/360 ICMA".
///
/// # Rules
///
/// - If D1 is 31, change to 30
/// - If D2 is 31, change to 30
///
/// # Formula
///
/// $$\text{Days} = 360 \times (Y_2 - Y_1) + 30 \times (M_2 - M_1) + (D_2 - D_1)$$
#[derive(Debug, Clone, Copy, Default)]
pub struct Thirty360E;

impl DayCount for Thirty360E {
    fn name(&self) -> &'static str {
        "30E/360"
    }

    fn year_fraction(&self, start: Date, end: Date) -> Decimal {
        let days = self.day_count(start, end);
        Decimal::from(days) / Decimal::from(360)
    }

    fn day_count(&self, start: Date, end: Date) -> i64 {
        let mut d1 = start.day() as i64;
        let mut d2 = end.day() as i64;
        let m1 = start.month() as i64;
        let m2 = end.month() as i64;
        let y1 = start.year() as i64;
        let y2 = end.year() as i64;

        // 30E/360 rules - simpler than US
        if d1 == 31 {
            d1 = 30;
        }
        if d2 == 31 {
            d2 = 30;
        }

        360 * (y2 - y1) + 30 * (m2 - m1) + (d2 - d1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_thirty360_full_year() {
        let dc = Thirty360;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        assert_eq!(dc.day_count(start, end), 360);
        assert_eq!(dc.year_fraction(start, end), dec!(1));
    }

    #[test]
    fn test_thirty360_half_year() {
        let dc = Thirty360;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 7, 1).unwrap();

        assert_eq!(dc.day_count(start, end), 180);
        assert_eq!(dc.year_fraction(start, end), dec!(0.5));
    }

    #[test]
    fn test_thirty360_day_31_adjustment() {
        let dc = Thirty360;
        let start = Date::from_ymd(2025, 1, 31).unwrap();
        let end = Date::from_ymd(2025, 3, 31).unwrap();

        // D1=31->30, D2=31->30 (since D1>=30)
        // Days = 30*(3-1) + (30-30) = 60
        assert_eq!(dc.day_count(start, end), 60);
    }

    #[test]
    fn test_thirty360e_day_31_adjustment() {
        let dc = Thirty360E;
        let start = Date::from_ymd(2025, 1, 31).unwrap();
        let end = Date::from_ymd(2025, 3, 31).unwrap();

        // Both D1 and D2 become 30
        assert_eq!(dc.day_count(start, end), 60);
    }

    #[test]
    fn test_thirty360_vs_thirty360e() {
        let us = Thirty360;
        let eu = Thirty360E;

        let start = Date::from_ymd(2025, 1, 30).unwrap();
        let end = Date::from_ymd(2025, 3, 31).unwrap();

        // US: D1=30, D2=31 stays 31 since D1<30 is false (D1=30)
        // Actually D1 >= 30, so D2 becomes 30
        let us_days = us.day_count(start, end);

        // EU: D2=31 always becomes 30
        let eu_days = eu.day_count(start, end);

        assert_eq!(us_days, eu_days); // In this case they're equal
    }
}
