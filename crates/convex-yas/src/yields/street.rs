//! Street convention yield calculation.
//!
//! The street convention yield is the standard market quote for bond yields.
//! It assumes reinvestment at the yield rate and standard day count conventions.

use crate::YasError;
use rust_decimal::Decimal;

/// Calculate street convention yield using Newton-Raphson.
///
/// This is the standard yield-to-maturity calculation using market conventions.
///
/// # Arguments
///
/// * `dirty_price` - Dirty price (clean + accrued) as percentage of par
/// * `cash_flows` - Vector of cash flow amounts
/// * `times` - Vector of times to each cash flow (in years)
/// * `frequency` - Compounding frequency per year
/// * `initial_guess` - Initial yield guess (as decimal)
///
/// # Returns
///
/// Street convention yield as decimal (e.g., 0.05 for 5%)
pub fn street_convention_yield(
    dirty_price: f64,
    cash_flows: &[f64],
    times: &[f64],
    frequency: u32,
    initial_guess: f64,
) -> Result<Decimal, YasError> {
    if cash_flows.len() != times.len() {
        return Err(YasError::InvalidInput(
            "cash_flows and times must have same length".to_string(),
        ));
    }

    if cash_flows.is_empty() {
        return Err(YasError::InvalidInput("no cash flows provided".to_string()));
    }

    // Try multiple initial guesses for robustness
    let initial_guesses = [
        initial_guess,
        estimate_current_yield(dirty_price, cash_flows, times, frequency),
        0.01,
        0.03,
        0.05,
        0.08,
        0.10,
        0.15,
    ];

    for guess in initial_guesses {
        if let Ok(result) = newton_raphson_yield(dirty_price, cash_flows, times, frequency, guess) {
            return Ok(result);
        }
    }

    Err(YasError::SolverNoConvergence {
        context: "street convention yield".to_string(),
        iterations: 100,
    })
}

/// Estimate current yield as starting point
fn estimate_current_yield(dirty_price: f64, cash_flows: &[f64], times: &[f64], frequency: u32) -> f64 {
    // Find first coupon (non-principal) cash flow
    let annual_coupon = if cash_flows.len() >= 2 {
        // Assume first cash flow is coupon, multiply by frequency
        cash_flows[0] * frequency as f64
    } else {
        // Single cash flow - estimate coupon from total
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

/// Newton-Raphson solver for yield
fn newton_raphson_yield(
    dirty_price: f64,
    cash_flows: &[f64],
    times: &[f64],
    frequency: u32,
    initial_guess: f64,
) -> Result<Decimal, YasError> {
    const TOLERANCE: f64 = 1e-8;
    const MAX_ITERATIONS: u32 = 100;

    let freq = frequency as f64;
    let mut yield_val = initial_guess;

    for iteration in 0..MAX_ITERATIONS {
        let (pv, dpv) = price_and_derivative(yield_val, cash_flows, times, freq);
        let error = pv - dirty_price;

        if error.abs() < TOLERANCE {
            // Validate result is reasonable
            if yield_val > -0.5 && yield_val < 1.0 {
                return Ok(Decimal::from_f64_retain(yield_val).unwrap_or(Decimal::ZERO));
            }
        }

        if dpv.abs() < 1e-15 {
            return Err(YasError::SolverNoConvergence {
                context: "derivative too small".to_string(),
                iterations: iteration,
            });
        }

        let step = error / dpv;
        // Dampen large steps to prevent oscillation
        let damped_step = if step.abs() > 0.1 {
            step.signum() * 0.1
        } else {
            step
        };

        yield_val -= damped_step;

        // Bound yield to reasonable range
        yield_val = yield_val.clamp(-0.3, 1.0);
    }

    Err(YasError::SolverNoConvergence {
        context: "newton raphson".to_string(),
        iterations: MAX_ITERATIONS,
    })
}

/// Calculate price and its derivative with respect to yield.
fn price_and_derivative(
    yield_val: f64,
    cash_flows: &[f64],
    times: &[f64],
    freq: f64,
) -> (f64, f64) {
    let periodic_rate = yield_val / freq;
    let mut pv = 0.0;
    let mut dpv = 0.0;

    for (cf, t) in cash_flows.iter().zip(times.iter()) {
        let periods = t * freq;
        let df = (1.0 + periodic_rate).powf(-periods);
        pv += cf * df;
        // Derivative: d/dy [CF × (1+y/f)^(-tf)] = -t × CF × (1+y/f)^(-tf-1)
        dpv -= t * cf * (1.0 + periodic_rate).powf(-periods - 1.0);
    }

    (pv, dpv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_street_convention_par_bond() {
        // 5% coupon, 2-year, semi-annual, priced at par
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];
        let dirty_price = 100.0;

        let ytm = street_convention_yield(dirty_price, &cash_flows, &times, 2, 0.05).unwrap();

        // At par, YTM should equal coupon rate
        assert_relative_eq!(
            ytm.to_string().parse::<f64>().unwrap(),
            0.05,
            epsilon = 0.0001
        );
    }

    #[test]
    fn test_street_convention_premium_bond() {
        // 5% coupon, 2-year, semi-annual, priced at 102
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];
        let dirty_price = 102.0;

        let ytm = street_convention_yield(dirty_price, &cash_flows, &times, 2, 0.05).unwrap();

        // Premium bond should have YTM < coupon
        assert!(ytm.to_string().parse::<f64>().unwrap() < 0.05);
    }
}
