//! Simple yield calculation.
//!
//! Simple yield adds the annualized capital gain/loss to the current yield.

use crate::YasError;
use rust_decimal::Decimal;

/// Calculate simple yield.
///
/// Simple yield = (Annual Coupon + (Par - Price) / Years) / Price
///
/// This provides a quick approximation that accounts for both income
/// and capital gain/loss, but ignores compounding.
///
/// # Arguments
///
/// * `annual_coupon` - Annual coupon amount per $100 face
/// * `clean_price` - Clean price per $100 face
/// * `par_value` - Par value (usually 100)
/// * `years_to_maturity` - Years until maturity
///
/// # Returns
///
/// Simple yield as percentage
pub fn simple_yield(
    annual_coupon: Decimal,
    clean_price: Decimal,
    par_value: Decimal,
    years_to_maturity: Decimal,
) -> Result<Decimal, YasError> {
    if clean_price <= Decimal::ZERO {
        return Err(YasError::InvalidInput(
            "clean price must be positive".to_string(),
        ));
    }

    if years_to_maturity <= Decimal::ZERO {
        return Err(YasError::InvalidInput(
            "years to maturity must be positive".to_string(),
        ));
    }

    // Annualized capital gain/loss
    let capital_gain = (par_value - clean_price) / years_to_maturity;

    // Simple yield = (coupon + capital gain) / price × 100
    let simple = (annual_coupon + capital_gain) / clean_price * Decimal::ONE_HUNDRED;
    Ok(simple)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use rust_decimal_macros::dec;

    #[test]
    fn test_simple_yield_par() {
        // At par, simple yield ≈ coupon rate
        let coupon = dec!(5.0);
        let price = dec!(100.0);
        let par = dec!(100.0);
        let years = dec!(5.0);

        let sy = simple_yield(coupon, price, par, years).unwrap();

        assert_relative_eq!(
            sy.to_string().parse::<f64>().unwrap(),
            5.0,
            epsilon = 0.001
        );
    }

    #[test]
    fn test_simple_yield_discount() {
        // Discount bond: price < par → positive capital gain
        let coupon = dec!(5.0);
        let price = dec!(95.0);
        let par = dec!(100.0);
        let years = dec!(5.0);

        let sy = simple_yield(coupon, price, par, years).unwrap();

        // Capital gain = (100 - 95) / 5 = 1
        // Simple yield = (5 + 1) / 95 × 100 ≈ 6.32%
        assert_relative_eq!(
            sy.to_string().parse::<f64>().unwrap(),
            6.32,
            epsilon = 0.01
        );
    }

    #[test]
    fn test_simple_yield_premium() {
        // Premium bond: price > par → negative capital gain (loss)
        let coupon = dec!(5.0);
        let price = dec!(105.0);
        let par = dec!(100.0);
        let years = dec!(5.0);

        let sy = simple_yield(coupon, price, par, years).unwrap();

        // Capital loss = (100 - 105) / 5 = -1
        // Simple yield = (5 - 1) / 105 × 100 ≈ 3.81%
        assert_relative_eq!(
            sy.to_string().parse::<f64>().unwrap(),
            3.81,
            epsilon = 0.01
        );
    }
}
