//! Settlement date and amount calculations.

use chrono::NaiveDate;
use rust_decimal::Decimal;

/// Calculate settlement date from trade date.
///
/// # Arguments
///
/// * `trade_date` - The trade date
/// * `settlement_days` - Number of business days to settlement (e.g., 1 for T+1, 2 for T+2)
/// * `is_business_day` - Function to check if a date is a business day
///
/// # Returns
///
/// The settlement date
pub fn calculate_settlement_date<F>(
    trade_date: NaiveDate,
    settlement_days: u32,
    is_business_day: F,
) -> NaiveDate
where
    F: Fn(NaiveDate) -> bool,
{
    let mut date = trade_date;
    let mut business_days = 0;

    while business_days < settlement_days {
        date = date.succ_opt().unwrap_or(date);
        if is_business_day(date) {
            business_days += 1;
        }
    }

    date
}

/// Calculate proceeds from a bond sale.
///
/// # Arguments
///
/// * `face_value` - Face value of the bond
/// * `clean_price` - Clean price as percentage of par
/// * `accrued_interest` - Accrued interest as percentage of par
///
/// # Returns
///
/// Total proceeds
pub fn calculate_proceeds(
    face_value: Decimal,
    clean_price: Decimal,
    accrued_interest: Decimal,
) -> Decimal {
    let principal = face_value * clean_price / Decimal::ONE_HUNDRED;
    let accrued = face_value * accrued_interest / Decimal::ONE_HUNDRED;
    principal + accrued
}

/// Calculate the dollar amount of accrued interest.
///
/// # Arguments
///
/// * `face_value` - Face value of the bond
/// * `coupon_rate` - Annual coupon rate as decimal
/// * `accrued_fraction` - Day count fraction for accrued period
///
/// # Returns
///
/// Dollar amount of accrued interest
pub fn calculate_accrued_amount(
    face_value: Decimal,
    coupon_rate: Decimal,
    accrued_fraction: Decimal,
) -> Decimal {
    face_value * coupon_rate * accrued_fraction
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use rust_decimal_macros::dec;

    #[test]
    fn test_calculate_proceeds() {
        let face = dec!(1000000.0);
        let clean = dec!(110.503);
        let accrued = dec!(2.698611);

        let proceeds = calculate_proceeds(face, clean, accrued);

        // Principal = 1M × 110.503% = 1,105,030
        // Accrued = 1M × 2.698611% = 26,986.11
        // Total = 1,132,016.11
        let expected = dec!(1132016.11);
        let diff = (proceeds - expected).abs();
        assert!(diff < dec!(0.01));
    }

    #[test]
    fn test_calculate_accrued_amount() {
        let face = dec!(1000000.0);
        let coupon = dec!(0.075); // 7.5%
        let fraction = dec!(0.3722222); // 134/360

        let accrued = calculate_accrued_amount(face, coupon, fraction);

        // 1M × 7.5% × (134/360) = 27,916.67
        let expected = dec!(27916.67);
        let diff = (accrued - expected).abs();
        assert!(diff < dec!(1.0));
    }

    #[test]
    fn test_settlement_date() {
        let trade = NaiveDate::from_ymd_opt(2020, 4, 27).unwrap(); // Monday

        // Simple weekend check (not a full calendar)
        let is_business_day = |d: NaiveDate| {
            let weekday = d.weekday();
            weekday != chrono::Weekday::Sat && weekday != chrono::Weekday::Sun
        };

        let settle = calculate_settlement_date(trade, 2, is_business_day);

        // T+2 from Monday = Wednesday
        assert_eq!(settle, NaiveDate::from_ymd_opt(2020, 4, 29).unwrap());
    }
}
