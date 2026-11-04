//! Secant root-finding algorithm.

use crate::bounds::{finite, interval};
use crate::error::{MathError, MathResult};
use crate::solvers::{SolverConfig, SolverResult};

/// Secant root-finding algorithm.
///
/// Similar to Newton-Raphson but approximates the derivative using
/// finite differences from the previous iteration. Does not require
/// an analytical derivative or a bracketing interval.
///
/// Convergence rate is superlinear (order ~1.618, the golden ratio).
///
/// # Arguments
///
/// * `f` - The function for which to find a root
/// * `x0` - First initial guess
/// * `x1` - Second initial guess (should be different from x0)
/// * `config` - Solver configuration
///
/// # Returns
///
/// The root and iteration statistics, or an error if convergence fails.
///
/// # Example
///
/// ```rust
/// use convex_math::solvers::{secant, SolverConfig};
///
/// // Find root of x^2 - 2 (i.e., sqrt(2))
/// let f = |x: f64| x * x - 2.0;
///
/// let result = secant(f, 1.0, 2.0, &SolverConfig::default()).unwrap();
/// assert!((result.root - std::f64::consts::SQRT_2).abs() < 1e-10);
/// ```
pub fn secant<F>(f: F, x0: f64, x1: f64, config: &SolverConfig) -> MathResult<SolverResult>
where
    F: Fn(f64) -> f64,
{
    config.validate()?;
    interval(x0, x1)?;
    let mut x_prev = x0;
    let mut x_curr = x1;
    let mut f_prev = finite(f(x_prev), "function evaluation")?;
    let mut f_curr = finite(f(x_curr), "function evaluation")?;

    for iteration in 0..config.max_iterations {
        // Check for convergence
        if f_curr.abs() < config.tolerance {
            return Ok(SolverResult {
                root: x_curr,
                iterations: iteration,
                residual: f_curr,
            });
        }

        // Check for very small denominator (parallel secant line)
        let denom = finite(f_curr - f_prev, "secant denominator")?;
        if denom.abs() < 1e-15 {
            return Err(MathError::DivisionByZero { value: denom });
        }

        // Secant step: x_next = x_curr - f(x_curr) * (x_curr - x_prev) / (f(x_curr) - f(x_prev))
        let x_next = finite(x_curr - f_curr * (x_curr - x_prev) / denom, "secant step")?;

        // Check for step convergence
        if (x_next - x_curr).abs() < config.tolerance {
            let f_next = finite(f(x_next), "function evaluation")?;
            if f_next.abs() >= config.tolerance {
                return Err(MathError::convergence_failed(iteration + 1, f_next.abs()));
            }
            return Ok(SolverResult {
                root: x_next,
                iterations: iteration + 1,
                residual: f_next,
            });
        }

        // Update for next iteration
        x_prev = x_curr;
        f_prev = f_curr;
        x_curr = x_next;
        f_curr = finite(f(x_curr), "function evaluation")?;
    }

    Err(MathError::convergence_failed(
        config.max_iterations,
        f_curr.abs(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_sqrt_2() {
        let f = |x: f64| x * x - 2.0;

        let result = secant(f, 1.0, 2.0, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-10);
    }

    #[test]
    fn test_cube_root() {
        // Find cube root of 27 (should be 3)
        let f = |x: f64| x * x * x - 27.0;

        let result = secant(f, 2.0, 4.0, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, 3.0, epsilon = 1e-10);
    }

    #[test]
    fn test_sin() {
        // Find root of sin(x) near pi
        let f = |x: f64| x.sin();

        let result = secant(f, 3.0, 3.5, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, std::f64::consts::PI, epsilon = 1e-10);
    }

    #[test]
    fn test_convergence_speed() {
        let f = |x: f64| x * x - 2.0;

        let result = secant(f, 1.0, 2.0, &SolverConfig::default()).unwrap();

        // Secant should converge reasonably fast (faster than bisection, slower than Newton)
        assert!(result.iterations < 15);
    }

    #[test]
    fn test_close_initial_guesses() {
        let f = |x: f64| x * x - 2.0;

        // Very close initial guesses
        let result = secant(f, 1.4, 1.42, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-10);
    }
}
