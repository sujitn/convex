//! True yield calculation.
//!
//! True yield adjusts for the actual settlement mechanics and reinvestment
//! assumptions that differ from the street convention.

use rust_decimal::Decimal;

/// Calculate true yield.
///
/// True yield differs from street convention by accounting for:
/// - Actual settlement date (not standard T+2)
/// - Actual reinvestment assumptions
///
/// For most purposes, the difference is small (a few basis points).
///
/// # Arguments
///
/// * `street_yield` - Street convention yield as decimal
/// * `settlement_adjustment` - Adjustment factor for settlement (usually small)
///
/// # Returns
///
/// True yield as decimal
pub fn true_yield(street_yield: Decimal, settlement_adjustment: Decimal) -> Decimal {
    street_yield + settlement_adjustment
}

/// Calculate the settlement adjustment between street and true yield.
///
/// This is a simplified calculation. A full implementation would consider
/// the actual settlement mechanics for the specific bond type.
pub fn settlement_adjustment(_days_to_settlement: i32, _yield_level: f64) -> Decimal {
    // Simplified: typically a small adjustment
    // Full implementation would depend on bond type and market
    Decimal::ZERO
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_true_yield_no_adjustment() {
        let street = dec!(0.05);
        let adj = dec!(0.0);

        let true_y = true_yield(street, adj);
        assert_eq!(true_y, dec!(0.05));
    }

    #[test]
    fn test_true_yield_with_adjustment() {
        let street = dec!(0.05);
        let adj = dec!(-0.0001); // -1bp adjustment

        let true_y = true_yield(street, adj);
        assert_eq!(true_y, dec!(0.0499));
    }
}
