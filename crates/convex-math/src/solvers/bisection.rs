//! Bisection root-finding algorithm.

use crate::error::{MathError, MathResult};
use crate::solvers::{SolverConfig, SolverResult};

/// Bisection root-finding algorithm.
///
/// A simple and reliable bracketing method that works by repeatedly
/// halving the interval and selecting the subinterval containing the root.
///
/// Requires: `f(a) * f(b) < 0` (opposite signs at endpoints)
///
/// # Arguments
///
/// * `f` - The function for which to find a root
/// * `a` - Lower bound of the bracket
/// * `b` - Upper bound of the bracket
/// * `config` - Solver configuration
///
/// # Returns
///
/// The root and iteration statistics, or an error if the bracket is invalid.
///
/// # Example
///
/// ```rust
/// use convex_math::solvers::{bisection, SolverConfig};
///
/// // Find root of x^2 - 2 (i.e., sqrt(2))
/// let f = |x: f64| x * x - 2.0;
///
/// let result = bisection(f, 1.0, 2.0, &SolverConfig::default()).unwrap();
/// assert!((result.root - std::f64::consts::SQRT_2).abs() < 1e-10);
/// ```
pub fn bisection<F>(f: F, a: f64, b: f64, config: &SolverConfig) -> MathResult<SolverResult>
where
    F: Fn(f64) -> f64,
{
    let mut lo = a.min(b);
    let mut hi = a.max(b);

    let f_lo = f(lo);
    let f_hi = f(hi);

    // Check that root is bracketed
    if f_lo * f_hi > 0.0 {
        return Err(MathError::InvalidBracket {
            a: lo,
            b: hi,
            fa: f_lo,
            fb: f_hi,
        });
    }

    // Handle case where endpoint is the root
    if f_lo.abs() < config.tolerance {
        return Ok(SolverResult {
            root: lo,
            iterations: 0,
            residual: f_lo,
        });
    }
    if f_hi.abs() < config.tolerance {
        return Ok(SolverResult {
            root: hi,
            iterations: 0,
            residual: f_hi,
        });
    }

    for iteration in 0..config.max_iterations {
        let mid = (lo + hi) / 2.0;
        let f_mid = f(mid);

        // Check for convergence
        if f_mid.abs() < config.tolerance || (hi - lo) / 2.0 < config.tolerance {
            return Ok(SolverResult {
                root: mid,
                iterations: iteration + 1,
                residual: f_mid,
            });
        }

        // Update bracket
        if f_mid * f(lo) < 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }

    let mid = (lo + hi) / 2.0;
    Err(MathError::convergence_failed(
        config.max_iterations,
        f(mid).abs(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_sqrt_2() {
        let f = |x: f64| x * x - 2.0;

        let result = bisection(f, 1.0, 2.0, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-10);
    }

    #[test]
    fn test_reversed_bracket() {
        let f = |x: f64| x * x - 2.0;

        // Reversed bracket should still work
        let result = bisection(f, 2.0, 1.0, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-10);
    }

    #[test]
    fn test_invalid_bracket() {
        let f = |x: f64| x * x - 2.0;

        // Both endpoints have same sign
        let result = bisection(f, 2.0, 3.0, &SolverConfig::default());

        assert!(result.is_err());
        if let Err(MathError::InvalidBracket { .. }) = result {
            // Expected
        } else {
            panic!("Expected InvalidBracket error");
        }
    }

    #[test]
    fn test_root_at_endpoint() {
        let f = |x: f64| x - 1.0;

        let result = bisection(f, 0.0, 1.0, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_negative_root() {
        let f = |x: f64| x + 1.0;

        let result = bisection(f, -2.0, 0.0, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, -1.0, epsilon = 1e-10);
    }
}
