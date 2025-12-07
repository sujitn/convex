//! I-Spread calculation.

use rust_decimal::Decimal;

use convex_bonds::instruments::{Bond, FixedBond};
use convex_core::types::{Date, Spread, SpreadType};
use convex_curves::curves::ZeroCurve;

use crate::error::{SpreadError, SpreadResult};

/// Calculates I-spread for a bond.
///
/// I-spread = Bond YTM - Interpolated Swap Rate
///
/// The swap rate is interpolated at the bond's maturity.
pub fn calculate(
    bond: &FixedBond,
    swap_curve: &ZeroCurve,
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

    // Get swap rate at bond's maturity
    let swap_rate = swap_curve
        .zero_rate_at(maturity)
        .map_err(|e| SpreadError::curve_error(e.to_string()))?;

    // I-spread = Bond yield - Swap rate
    let spread = bond_yield - swap_rate;
    let spread_bps = (spread * Decimal::from(10_000)).trunc();
    Ok(Spread::new(spread_bps, SpreadType::ISpread))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_placeholder() {
        // Placeholder - real tests require curve and bond fixtures
        assert!(true);
    }
}
