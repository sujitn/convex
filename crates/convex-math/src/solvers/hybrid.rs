//! Hybrid root-finding algorithm.
//!
//! Combines Newton-Raphson with Brent's method for robust convergence.

use crate::error::{MathError, MathResult};
use crate::solvers::{brent, SolverConfig, SolverResult};

/// Hybrid root-finding algorithm.
///
/// Starts with Newton-Raphson for fast quadratic convergence, but falls back
/// to Brent's method if Newton diverges or encounters problems. This provides
/// the speed of Newton when it works, with the reliability of Brent as a safety net.
///
/// # Strategy
///
/// 1. Try Newton-Raphson with a limited number of iterations
/// 2. If Newton diverges (step increases), switch to Brent
/// 3. If Newton encounters zero derivative, switch to Brent
/// 4. If Newton succeeds, return the result
///
/// # Arguments
///
/// * `f` - The function for which to find a root
/// * `df` - The derivative of the function
/// * `initial_guess` - Starting point for Newton iteration
/// * `bounds` - Optional bracketing interval for Brent fallback (a, b)
/// * `config` - Solver configuration
///
/// # Returns
///
/// The root and iteration statistics, or an error if all methods fail.
///
/// # Example
///
/// ```rust
/// use convex_math::solvers::{hybrid, SolverConfig};
///
/// // Find root of x^3 - x - 2
/// let f = |x: f64| x * x * x - x - 2.0;
/// let df = |x: f64| 3.0 * x * x - 1.0;
///
/// let result = hybrid(f, df, 1.5, Some((1.0, 2.0)), &SolverConfig::default()).unwrap();
/// assert!((f(result.root)).abs() < 1e-10);
/// ```
pub fn hybrid<F, DF>(
    f: F,
    df: DF,
    initial_guess: f64,
    bounds: Option<(f64, f64)>,
    config: &SolverConfig,
) -> MathResult<SolverResult>
where
    F: Fn(f64) -> f64,
    DF: Fn(f64) -> f64,
{
    // Try Newton-Raphson first
    let newton_result = newton_with_monitoring(&f, &df, initial_guess, config);

    match newton_result {
        Ok(result) => Ok(result),
        Err(_) => {
            // Newton failed, try Brent if we have bounds
            if let Some((a, b)) = bounds {
                brent(&f, a, b, config)
            } else {
                // No bounds provided, try to find them
                match find_bracket(&f, initial_guess) {
                    Some((a, b)) => brent(&f, a, b, config),
                    None => Err(MathError::invalid_input(
                        "Newton-Raphson failed and could not find bracketing interval for Brent",
                    )),
                }
            }
        }
    }
}

/// Newton-Raphson with divergence detection.
///
/// Monitors the iteration and fails fast if divergence is detected.
fn newton_with_monitoring<F, DF>(
    f: &F,
    df: &DF,
    initial_guess: f64,
    config: &SolverConfig,
) -> MathResult<SolverResult>
where
    F: Fn(f64) -> f64,
    DF: Fn(f64) -> f64,
{
    let mut x = initial_guess;
    let mut prev_residual = f64::MAX;
    let mut divergence_count = 0;
    const MAX_DIVERGENCE: u32 = 3; // Allow a few divergent steps before giving up

    // Use fewer iterations for Newton in hybrid mode - fail fast
    let newton_max_iter = config.max_iterations.min(20);

    for iteration in 0..newton_max_iter {
        let fx = f(x);
        let residual = fx.abs();

        // Check for convergence
        if residual < config.tolerance {
            return Ok(SolverResult {
                root: x,
                iterations: iteration,
                residual: fx,
            });
        }

        // Check for divergence
        if residual > prev_residual * 2.0 {
            divergence_count += 1;
            if divergence_count >= MAX_DIVERGENCE {
                return Err(MathError::invalid_input("Newton-Raphson diverging"));
            }
        } else {
            divergence_count = 0;
        }
        prev_residual = residual;

        let dfx = df(x);

        // Check for zero derivative
        if dfx.abs() < 1e-15 {
            return Err(MathError::DivisionByZero { value: dfx });
        }

        // Newton step
        let step = fx / dfx;

        // Check for very large step (sign of trouble)
        if step.abs() > 1e10 {
            return Err(MathError::invalid_input("Newton step too large"));
        }

        x -= step;

        // Check for NaN or infinity
        if !x.is_finite() {
            return Err(MathError::invalid_input("Newton produced non-finite value"));
        }

        // Check for step convergence
        if step.abs() < config.tolerance {
            let final_fx = f(x);
            return Ok(SolverResult {
                root: x,
                iterations: iteration + 1,
                residual: final_fx,
            });
        }
    }

    Err(MathError::convergence_failed(newton_max_iter, f(x).abs()))
}

/// Attempts to find a bracketing interval for the root.
///
/// Uses exponential expansion from the initial guess.
fn find_bracket<F>(f: &F, initial_guess: f64) -> Option<(f64, f64)>
where
    F: Fn(f64) -> f64,
{
    let mut left = initial_guess;
    let mut right = initial_guess;
    let mut delta = 0.1;

    // Handle case where initial guess is at or near zero
    if initial_guess.abs() < 1e-10 {
        left = -1.0;
        right = 1.0;
    }

    let f_init = f(initial_guess);

    for _ in 0..50 {
        left -= delta;
        right += delta;

        let f_left = f(left);
        let f_right = f(right);

        // Check if we've bracketed a root
        if f_left * f_init < 0.0 {
            return Some((left, initial_guess));
        }
        if f_right * f_init < 0.0 {
            return Some((initial_guess, right));
        }
        if f_left * f_right < 0.0 {
            return Some((left, right));
        }

        // Exponentially expand the search
        delta *= 2.0;

        // Don't search too far
        if delta > 1e6 {
            break;
        }
    }

    None
}

/// Hybrid solver without derivative (uses numerical differentiation).
///
/// # Arguments
///
/// * `f` - The function for which to find a root
/// * `initial_guess` - Starting point for iteration
/// * `bounds` - Optional bracketing interval for Brent fallback
/// * `config` - Solver configuration
pub fn hybrid_numerical<F>(
    f: F,
    initial_guess: f64,
    bounds: Option<(f64, f64)>,
    config: &SolverConfig,
) -> MathResult<SolverResult>
where
    F: Fn(f64) -> f64,
{
    let h = 1e-8;
    let df = |x: f64| (f(x + h) - f(x - h)) / (2.0 * h);

    hybrid(&f, df, initial_guess, bounds, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_sqrt_2() {
        let f = |x: f64| x * x - 2.0;
        let df = |x: f64| 2.0 * x;

        let result = hybrid(f, df, 1.5, Some((1.0, 2.0)), &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-10);
    }

    #[test]
    fn test_cubic() {
        let f = |x: f64| x * x * x - x - 2.0;
        let df = |x: f64| 3.0 * x * x - 1.0;

        let result = hybrid(f, df, 1.5, Some((1.0, 2.0)), &SolverConfig::default()).unwrap();

        assert!(f(result.root).abs() < 1e-10);
    }

    #[test]
    fn test_fallback_to_brent() {
        // Function where Newton might struggle from a bad initial guess
        let f = |x: f64| x * x * x - 2.0 * x - 5.0;
        let df = |x: f64| 3.0 * x * x - 2.0;

        // Start from a point where Newton might overshoot
        let result = hybrid(f, df, 0.0, Some((1.0, 3.0)), &SolverConfig::default()).unwrap();

        assert!(f(result.root).abs() < 1e-10);
    }

    #[test]
    fn test_numerical_derivative() {
        let f = |x: f64| x * x - 2.0;

        let result =
            hybrid_numerical(f, 1.5, Some((1.0, 2.0)), &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-8);
    }

    #[test]
    fn test_auto_bracket_finding() {
        let f = |x: f64| x * x - 2.0;
        let df = |x: f64| 2.0 * x;

        // Don't provide bounds - should find them automatically
        let result = hybrid(f, df, 1.5, None, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-10);
    }

    #[test]
    fn test_ytm_like_calculation() {
        // Simulate a bond YTM calculation
        // Price = sum of discounted cash flows
        // For a 5% coupon, 5-year bond at price 95
        let target_price = 95.0;
        let coupon = 5.0;
        let face = 100.0;
        let years = 5;

        let price_from_yield = |y: f64| {
            let mut pv = 0.0;
            for t in 1..=years {
                pv += coupon / (1.0 + y).powi(t);
            }
            pv += face / (1.0 + y).powi(years);
            pv - target_price
        };

        let d_price_from_yield = |y: f64| {
            let mut dpv = 0.0;
            for t in 1..=years {
                dpv -= (t as f64) * coupon / (1.0 + y).powi(t + 1);
            }
            dpv -= (years as f64) * face / (1.0 + y).powi(years + 1);
            dpv
        };

        let result = hybrid(
            price_from_yield,
            d_price_from_yield,
            0.05, // Initial guess of 5%
            Some((0.0, 0.20)),
            &SolverConfig::default(),
        )
        .unwrap();

        // Verify the yield produces the target price
        assert!(price_from_yield(result.root).abs() < 1e-10);
        // YTM should be higher than coupon rate since bond is below par
        assert!(result.root > 0.05);
    }
}
