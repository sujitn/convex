//! G-Spread calculation.

use rust_decimal::Decimal;

use convex_core::types::{Date, Spread, SpreadType};
use convex_curves::curves::ZeroCurve;
use convex_bonds::instruments::{Bond, FixedBond};

use crate::error::{SpreadError, SpreadResult};

/// Calculates G-spread for a bond.
///
/// G-spread = Bond YTM - Interpolated Government Yield
///
/// The government yield is interpolated at the bond's maturity.
pub fn calculate(
    bond: &FixedBond,
    govt_curve: &ZeroCurve,
    bond_yield: Decimal,
    settlement: Date,
) -> SpreadResult<Spread> {
    let maturity = bond.maturity();

    if settlement >= maturity {
        return Err(SpreadError::SettlementAfterMaturity {
            settlement: settlement.to_string(),
            maturity: maturity.to_string(),
        });
    }

    // Get government yield at bond's maturity
    let govt_yield = govt_curve
        .zero_rate_at(maturity)
        .map_err(|e| SpreadError::curve_error(e.to_string()))?;

    // G-spread = Bond yield - Government yield
    let spread = bond_yield - govt_yield;
    let spread_bps = (spread * Decimal::from(10_000)).trunc();
    Ok(Spread::new(spread_bps, SpreadType::GSpread))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_placeholder() {
        // Placeholder - real tests require curve and bond fixtures
        assert!(true);
    }
}
