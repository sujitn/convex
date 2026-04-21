//! Street convention yield calculation.
//!
//! Adapter around [`convex_bonds::pricing::YieldSolver::solve_primitive`] that
//! preserves the `(dirty_price, cash_flows, times, frequency, initial_guess)`
//! primitive-array signature used by the YAS pipeline.

use rust_decimal::Decimal;

use convex_bonds::pricing::YieldSolver;

use crate::error::{AnalyticsError, AnalyticsResult};

/// Street convention yield-to-maturity.
///
/// `dirty_price`, `cash_flows` and `times` are in the usual YAS primitive form:
/// dirty price per 100, flat arrays of amounts and year fractions. Returns the
/// yield as a decimal (0.05 for 5%).
pub fn street_convention_yield(
    dirty_price: f64,
    cash_flows: &[f64],
    times: &[f64],
    frequency: u32,
    initial_guess: f64,
) -> AnalyticsResult<Decimal> {
    if cash_flows.len() != times.len() {
        return Err(AnalyticsError::InvalidInput(
            "cash_flows and times must have same length".to_string(),
        ));
    }
    if cash_flows.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "no cash flows provided".to_string(),
        ));
    }

    let cf_pairs: Vec<(f64, f64)> = times
        .iter()
        .copied()
        .zip(cash_flows.iter().copied())
        .collect();
    let periods_per_year = f64::from(frequency);

    let solver = YieldSolver::new();
    // Street convention tolerates a few starting points — give up only after all fail.
    let guesses = [
        initial_guess,
        estimate_current_yield(dirty_price, cash_flows, times, frequency),
        0.01,
        0.03,
        0.05,
        0.08,
        0.10,
        0.15,
    ];

    for guess in guesses {
        if let Ok(result) =
            solver.solve_primitive(&cf_pairs, dirty_price, periods_per_year, guess)
        {
            let y = result.yield_value;
            if y > -0.5 && y < 1.0 {
                return Ok(Decimal::from_f64_retain(y).unwrap_or(Decimal::ZERO));
            }
        }
    }

    Err(AnalyticsError::SolverConvergenceFailed {
        solver: "street convention yield".to_string(),
        iterations: 100,
        residual: f64::NAN,
    })
}

/// Rough current-yield initial guess (annual coupon / price) used as a seed.
fn estimate_current_yield(
    dirty_price: f64,
    cash_flows: &[f64],
    times: &[f64],
    frequency: u32,
) -> f64 {
    let annual_coupon = if cash_flows.len() >= 2 {
        cash_flows[0] * frequency as f64
    } else {
        let total_cf: f64 = cash_flows.iter().sum();
        let years = times.last().copied().unwrap_or(1.0);
        (total_cf - 100.0) / years
    };

    if dirty_price > 0.0 {
        (annual_coupon / dirty_price).clamp(0.001, 0.5)
    } else {
        0.05
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use rust_decimal::prelude::ToPrimitive;

    #[test]
    fn test_street_convention_par_bond() {
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];
        let ytm = street_convention_yield(100.0, &cash_flows, &times, 2, 0.05).unwrap();
        assert_relative_eq!(ytm.to_f64().unwrap(), 0.05, epsilon = 1e-4);
    }

    #[test]
    fn test_street_convention_premium_bond() {
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];
        let ytm = street_convention_yield(102.0, &cash_flows, &times, 2, 0.05).unwrap();
        assert!(ytm.to_f64().unwrap() < 0.05);
    }

    #[test]
    fn test_street_convention_discount_bond() {
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];
        let ytm = street_convention_yield(98.0, &cash_flows, &times, 2, 0.05).unwrap();
        assert!(ytm.to_f64().unwrap() > 0.05);
    }

    #[test]
    fn test_street_convention_mismatched_lengths() {
        let times = vec![0.5, 1.0];
        let cash_flows = vec![2.5, 2.5, 102.5];
        assert!(street_convention_yield(100.0, &cash_flows, &times, 2, 0.05).is_err());
    }

    #[test]
    fn test_street_convention_empty_cash_flows() {
        assert!(street_convention_yield(100.0, &[], &[], 2, 0.05).is_err());
    }
}
