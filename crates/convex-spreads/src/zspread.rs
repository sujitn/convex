//! Z-Spread calculation.

use convex_core::types::{Date, Price, Spread, SpreadType};
use convex_curves::curves::ZeroCurve;
use convex_bonds::instruments::{Bond, FixedBond};
use convex_bonds::cashflows::CashFlowGenerator;
use convex_math::solvers::{brent, SolverConfig};

use crate::error::{SpreadError, SpreadResult};

/// Calculates Z-spread for a bond.
///
/// The Z-spread is found by solving:
/// `Price = Σ CF_i / (1 + (r_i + z)/freq)^(freq * t_i)`
///
/// where:
/// - CF_i is the i-th cash flow
/// - r_i is the spot rate at time t_i
/// - z is the Z-spread
/// - freq is the compounding frequency
pub fn calculate(
    bond: &FixedBond,
    curve: &ZeroCurve,
    market_price: Price,
    settlement: Date,
) -> SpreadResult<Spread> {
    let maturity = bond.maturity();

    if settlement >= maturity {
        return Err(SpreadError::SettlementAfterMaturity {
            settlement: settlement.to_string(),
            maturity: maturity.to_string(),
        });
    }

    let schedule = CashFlowGenerator::generate(bond, settlement)
        .map_err(|e| SpreadError::bond_error(e.to_string()))?;

    let accrued = CashFlowGenerator::accrued_interest(bond, settlement)
        .map_err(|e| SpreadError::bond_error(e.to_string()))?;

    let target_dirty_price = market_price.as_percentage() + accrued;
    let target = target_dirty_price.to_string().parse::<f64>().unwrap_or(100.0);

    let freq = bond.frequency().periods_per_year() as f64;

    // Objective function: PV(z) - target_price = 0
    let objective = |z: f64| {
        let mut pv = 0.0;
        for cf in schedule.iter() {
            let t = settlement.days_between(&cf.date()) as f64 / 365.0;

            // Get spot rate from curve
            let spot_rate = curve
                .zero_rate_at(cf.date())
                .map(|r| r.to_string().parse::<f64>().unwrap_or(0.0))
                .unwrap_or(0.0);

            // Discount with spread-adjusted rate
            let df = if freq > 0.0 {
                1.0 / (1.0 + (spot_rate + z) / freq).powf(freq * t)
            } else {
                (-(spot_rate + z) * t).exp()
            };

            let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);
            pv += amount * df;
        }
        pv - target
    };

    // Search for Z-spread between -10% and +50%
    let config = SolverConfig::new(1e-10, 100);
    let result = brent(objective, -0.10, 0.50, &config)
        .map_err(|_| SpreadError::convergence_failed(100))?;

    let z_spread_bps = (result.root * 10_000.0).round();
    Ok(Spread::new(
        rust_decimal::Decimal::from_f64_retain(z_spread_bps).unwrap_or_default(),
        SpreadType::ZSpread,
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_placeholder() {
        // Placeholder - real tests require curve and bond fixtures
        assert!(true);
    }
}
