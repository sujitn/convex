//! Explicit constant-rate cash-account projection and checked bill measures.
//! Projection rates/basis are supplied; no market/YAS convention is inferred.
use crate::error::{AnalyticsError, AnalyticsResult};
use convex_bonds::{
    pricing::reinvestment::{self, ProjectionCash, ProjectionTerms},
    traits::BondCashFlow,
};
use convex_core::{contracts::ExactDecimal, types::Date};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Annualized model return. The default CALENDAR horizon equals the explicitly
/// supplied reporting-year day count; it is not a market calendar-year roll.
pub fn constant_rate_return(
    cash: &[BondCashFlow],
    initial: Decimal,
    model_rate: Decimal,
    settlement: Date,
    maturity: Date,
    frequency: u32,
    reporting_days_per_year: u32,
) -> AnalyticsResult<Decimal> {
    constant_rate_return_with_horizon(
        cash,
        initial,
        model_rate,
        settlement,
        maturity,
        frequency,
        reporting_days_per_year,
        reporting_days_per_year,
    )
}
/// Checked original account projection with the SAME explicitly supplied model
/// rate for residual valuation and reinvestment. For distinct rates/full output,
/// use the native reinvestment::project operation. Cash and initial value must
/// have matching absolute units. Original terminal cash is required at maturity.
#[allow(clippy::too_many_arguments)]
pub fn constant_rate_return_with_horizon(
    cash: &[BondCashFlow],
    initial: Decimal,
    model_rate: Decimal,
    settlement: Date,
    maturity: Date,
    frequency: u32,
    reporting_days_per_year: u32,
    max_horizon_days: u32,
) -> AnalyticsResult<Decimal> {
    if cash.is_empty() || cash.len() > 4096 {
        return Err(AnalyticsError::InvalidInput(
            "1..4096 projection transfers required".into(),
        ));
    }
    let cash: Vec<_> = cash
        .iter()
        .map(|c| ProjectionCash {
            date: c.date,
            amount: ExactDecimal::new(c.amount),
        })
        .collect();
    let terms = ProjectionTerms {
        settlement,
        maturity,
        initial_cash_value: ExactDecimal::new(initial),
        valuation_rate: ExactDecimal::new(model_rate),
        reinvestment_rate: ExactDecimal::new(model_rate),
        frequency,
        reporting_days_per_year,
        max_horizon_days,
    };
    reinvestment::project(&cash, &terms)
        .map(|v| v.annualized_return.decimal())
        .map_err(Into::into)
}

// ============================================================================
// T-Bill Yield Functions
// ============================================================================

/// Calculate discount yield (bank discount basis).
///
/// Used for T-Bills and other discount instruments.
///
/// # Formula
///
/// ```text
/// Discount Yield = (Face - Price) / Face × (360 / Days) × 100
/// ```
///
/// # Returns
///
/// Discount yield as percentage (e.g., 6.0 for 6%)
pub fn discount_yield(
    price: Decimal,
    face_value: Decimal,
    days_to_maturity: u32,
) -> AnalyticsResult<Decimal> {
    bill_percent(convex_bonds::pricing::bill::bank_discount_yield(
        price,
        face_value,
        i64::from(days_to_maturity),
    ))
}

/// Calculate a fixed-365 simple yield for instruments of at most 182 days.
///
/// This date-free helper returns a percent rate. Treasury investment-rate
/// quotation requires original issue, settlement and maturity dates; use
/// [`super::treasury_bill::quote_treasury_bill`] for that convention, including
/// short bills whose issue-year basis is 366 and all long bills.
///
/// # Formula (for instruments ≤ 182 days)
///
/// ```text
/// BEY = (Face - Price) / Price × (365 / Days) × 100
/// ```
///
/// # Returns
///
/// Simple yield as percentage, or an error for invalid cash, unsupported term
/// or Decimal overflow. No approximate long-bill calculation is performed.
pub fn bond_equivalent_yield(
    price: Decimal,
    face_value: Decimal,
    days_to_maturity: u32,
) -> AnalyticsResult<Decimal> {
    bill_percent(convex_bonds::pricing::bill::simple_bill_yield(
        price,
        face_value,
        i64::from(days_to_maturity),
    ))
}

/// Calculate CD equivalent yield.
///
/// Compares discount instruments with CDs on add-on interest basis.
///
/// # Formula
///
/// ```text
/// CD Equivalent = (Face - Price) / Price × (360 / Days) × 100
/// ```
pub fn cd_equivalent_yield(
    price: Decimal,
    face_value: Decimal,
    days_to_maturity: u32,
) -> AnalyticsResult<Decimal> {
    bill_percent(convex_bonds::pricing::bill::money_market_yield(
        price,
        face_value,
        i64::from(days_to_maturity),
    ))
}

pub(super) fn bill_error(error: convex_bonds::BondError) -> AnalyticsError {
    match error {
        convex_bonds::BondError::InvalidSpec { reason }
        | convex_bonds::BondError::InvalidPrice { reason } => AnalyticsError::InvalidInput(reason),
        convex_bonds::BondError::PricingFailed { reason } => {
            AnalyticsError::CalculationFailed(reason)
        }
        other => AnalyticsError::BondError(other.to_string()),
    }
}

fn bill_percent(rate: convex_bonds::BondResult<Decimal>) -> AnalyticsResult<Decimal> {
    rate.map_err(bill_error)?
        .checked_mul(dec!(100))
        .ok_or_else(|| {
            AnalyticsError::CalculationFailed(
                "bill percent rate exceeds Decimal arithmetic domain".into(),
            )
        })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use super::{
        constant_rate_return as money_market_yield,
        constant_rate_return_with_horizon as money_market_yield_with_horizon,
    };
    use approx::assert_relative_eq;
    use rust_decimal::prelude::ToPrimitive;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    // ========================================================================
    // Money Market Yield Tests
    // ========================================================================

    #[test]
    fn test_mmy_single_cash_flow() {
        let settlement = date(2020, 4, 29);
        let maturity = date(2020, 10, 29); // ~6 months

        let cash_flows = vec![BondCashFlow::coupon_and_principal(
            maturity,
            dec!(3.75),
            dec!(100),
        )];

        let mmy = money_market_yield(
            &cash_flows,
            dec!(101.5), // dirty price
            dec!(0.05),  // 5% YTM
            settlement,
            maturity,
            2,   // semi-annual
            360, // USD
        )
        .unwrap();

        assert!(mmy > Decimal::ZERO);
    }

    #[test]
    fn test_mmy_multiple_coupons() {
        let settlement = date(2020, 1, 15);
        let maturity = date(2021, 1, 15);

        let cash_flows = vec![
            BondCashFlow::coupon(date(2020, 7, 15), dec!(2.5)),
            BondCashFlow::coupon_and_principal(maturity, dec!(2.5), dec!(100)),
        ];

        let mmy = money_market_yield(
            &cash_flows,
            dec!(100.0),
            dec!(0.05),
            settlement,
            maturity,
            2,
            360,
        )
        .unwrap();

        // MMY should be positive and reasonable
        assert!(mmy > Decimal::ZERO);
        assert!(mmy < dec!(0.10)); // Less than 10%
    }

    #[test]
    fn test_mmy_with_custom_horizon() {
        let settlement = date(2020, 4, 29);
        let maturity = date(2025, 6, 15); // Long bond

        let cash_flows = vec![
            BondCashFlow::coupon(date(2020, 6, 15), dec!(3.75)),
            BondCashFlow::coupon(date(2020, 12, 15), dec!(3.75)),
        ];

        let mmy = money_market_yield_with_horizon(
            &cash_flows,
            dec!(110.5),
            dec!(0.049),
            settlement,
            maturity,
            2,
            360,
            180, // 6-month horizon
        )
        .unwrap();

        assert!(mmy > Decimal::ZERO);
    }

    #[test]
    fn test_mmy_different_day_bases() {
        let settlement = date(2020, 4, 29);
        let maturity = date(2020, 10, 29);

        let cash_flows = vec![BondCashFlow::coupon_and_principal(
            maturity,
            dec!(3.75),
            dec!(100),
        )];

        let mmy_360 = money_market_yield(
            &cash_flows,
            dec!(100.0),
            dec!(0.05),
            settlement,
            maturity,
            2,
            360, // USD/EUR convention
        )
        .unwrap();

        let mmy_365 = money_market_yield(
            &cash_flows,
            dec!(100.0),
            dec!(0.05),
            settlement,
            maturity,
            2,
            365, // GBP/AUD convention
        )
        .unwrap();

        // Both should be positive and close to YTM
        assert!(mmy_360 > Decimal::ZERO);
        assert!(mmy_365 > Decimal::ZERO);

        // MMY = return / (days/days_per_year)
        // With 365 days_per_year, time fraction is smaller, so MMY is larger
        assert!(mmy_365 > mmy_360);
    }

    #[test]
    fn test_mmy_invalid_inputs() {
        let settlement = date(2020, 4, 29);
        let maturity = date(2020, 10, 29);

        // Empty cash flows
        assert!(
            money_market_yield(&[], dec!(100), dec!(0.05), settlement, maturity, 2, 360).is_err()
        );

        // Zero price
        let cfs = vec![BondCashFlow::coupon(maturity, dec!(5))];
        assert!(
            money_market_yield(&cfs, dec!(0), dec!(0.05), settlement, maturity, 2, 360).is_err()
        );

        // Negative price
        assert!(
            money_market_yield(&cfs, dec!(-100), dec!(0.05), settlement, maturity, 2, 360).is_err()
        );
    }

    // ========================================================================
    // T-Bill Tests
    // ========================================================================

    #[test]
    fn test_discount_yield() {
        let dy = discount_yield(dec!(98.5), dec!(100.0), 90).unwrap();
        // (100 - 98.5) / 100 × (360/90) × 100 = 6.0%
        assert_relative_eq!(dy.to_f64().unwrap(), 6.0, epsilon = 0.01);
    }

    #[test]
    fn test_bond_equivalent_yield_short() {
        let bey = bond_equivalent_yield(dec!(98.5), dec!(100.0), 90).unwrap();
        // (100 - 98.5) / 98.5 × (365/90) × 100 ≈ 6.17%
        assert_relative_eq!(bey.to_f64().unwrap(), 6.17, epsilon = 0.02);
    }

    #[test]
    fn test_cd_equivalent_yield() {
        let cd = cd_equivalent_yield(dec!(98.5), dec!(100.0), 90).unwrap();
        // (100 - 98.5) / 98.5 × (360/90) × 100 ≈ 6.09%
        assert_relative_eq!(cd.to_f64().unwrap(), 6.09, epsilon = 0.02);
    }

    #[test]
    fn test_discount_yield_invalid() {
        assert!(discount_yield(dec!(98.5), dec!(100), 0).is_err());
        assert!(discount_yield(dec!(98.5), dec!(0), 90).is_err());
    }

    #[test]
    fn test_bey_invalid() {
        assert!(bond_equivalent_yield(dec!(0), dec!(100), 90).is_err());
        assert!(bond_equivalent_yield(dec!(98.5), dec!(100), 0).is_err());
    }
}
