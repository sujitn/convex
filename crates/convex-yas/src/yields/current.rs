//! Current yield calculation.
//!
//! Current yield is a simple measure: annual coupon divided by clean price.
//! It ignores the time value of money and any capital gain/loss at maturity.

use crate::YasError;
use rust_decimal::Decimal;

/// Calculate current yield.
///
/// Current yield = Annual Coupon / Clean Price × 100%
///
/// # Arguments
///
/// * `annual_coupon` - Annual coupon rate as decimal (e.g., 0.05 for 5%)
/// * `clean_price` - Clean price as percentage of par (e.g., 98.5)
///
/// # Returns
///
/// Current yield as percentage (e.g., 5.08 for 5.08%)
pub fn current_yield(annual_coupon: Decimal, clean_price: Decimal) -> Result<Decimal, YasError> {
    if clean_price <= Decimal::ZERO {
        return Err(YasError::InvalidInput(
            "clean price must be positive".to_string(),
        ));
    }

    // Current yield = (coupon rate × 100) / price × 100
    // e.g., 7.5% coupon at 110.503 price = 7.5 / 110.503 × 100 = 6.79%
    let current = (annual_coupon * Decimal::ONE_HUNDRED) / clean_price * Decimal::ONE_HUNDRED;
    Ok(current)
}

/// Calculate current yield from coupon amount and price.
///
/// # Arguments
///
/// * `coupon_amount` - Annual coupon payment per $100 face
/// * `clean_price` - Clean price per $100 face
pub fn current_yield_from_amount(
    coupon_amount: Decimal,
    clean_price: Decimal,
) -> Result<Decimal, YasError> {
    if clean_price <= Decimal::ZERO {
        return Err(YasError::InvalidInput(
            "clean price must be positive".to_string(),
        ));
    }

    // Current yield = coupon / price × 100
    let current = coupon_amount / clean_price * Decimal::ONE_HUNDRED;
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use rust_decimal_macros::dec;

    #[test]
    fn test_current_yield_par() {
        // 5% coupon at par
        let coupon = dec!(0.05);
        let price = dec!(100.0);

        let cy = current_yield(coupon, price).unwrap();

        // At par, current yield = coupon rate
        assert_relative_eq!(cy.to_string().parse::<f64>().unwrap(), 5.0, epsilon = 0.001);
    }

    #[test]
    fn test_current_yield_boeing() {
        // Boeing 7.5% at 110.503
        let coupon = dec!(0.075);
        let price = dec!(110.503);

        let cy = current_yield(coupon, price).unwrap();

        // Expected: 7.5 / 110.503 × 100 ≈ 6.79%
        assert_relative_eq!(cy.to_string().parse::<f64>().unwrap(), 6.79, epsilon = 0.01);
    }

    #[test]
    fn test_current_yield_from_amount() {
        // $7.50 coupon per $100 face at 110.503
        let coupon = dec!(7.5);
        let price = dec!(110.503);

        let cy = current_yield_from_amount(coupon, price).unwrap();

        assert_relative_eq!(cy.to_string().parse::<f64>().unwrap(), 6.79, epsilon = 0.01);
    }

    #[test]
    fn test_current_yield_zero_price_error() {
        let result = current_yield(dec!(0.05), dec!(0.0));
        assert!(result.is_err());
    }
}
