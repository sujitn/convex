//! Brent's root-finding algorithm.

use crate::error::{MathError, MathResult};
use crate::solvers::{SolverConfig, SolverResult};

/// Brent's root-finding algorithm.
///
/// Combines the reliability of bisection with the speed of the secant method
/// and inverse quadratic interpolation. This is generally the best choice
/// when a derivative is not available.
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
/// use convex_math::solvers::{brent, SolverConfig};
///
/// // Find root of x^3 - x - 2
/// let f = |x: f64| x * x * x - x - 2.0;
///
/// let result = brent(f, 1.0, 2.0, &SolverConfig::default()).unwrap();
/// assert!((f(result.root)).abs() < 1e-10);
/// ```
#[allow(clippy::many_single_char_names)]
pub fn brent<F>(f: F, a: f64, b: f64, config: &SolverConfig) -> MathResult<SolverResult>
where
    F: Fn(f64) -> f64,
{
    let mut a = a;
    let mut b = b;
    let mut fa = f(a);
    let mut fb = f(b);

    // Check that root is bracketed
    if fa * fb > 0.0 {
        return Err(MathError::InvalidBracket { a, b, fa, fb });
    }

    // Ensure |f(a)| >= |f(b)|
    if fa.abs() < fb.abs() {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut fa, &mut fb);
    }

    let mut c = a;
    let mut fc = fa;
    let mut d = b - a;
    let mut e = d;

    for iteration in 0..config.max_iterations {
        // Check for convergence
        if fb.abs() < config.tolerance {
            return Ok(SolverResult {
                root: b,
                iterations: iteration,
                residual: fb,
            });
        }

        if (b - a).abs() < config.tolerance {
            return Ok(SolverResult {
                root: b,
                iterations: iteration,
                residual: fb,
            });
        }

        // Try inverse quadratic interpolation
        let mut use_bisection = true;
        let mut s = 0.0;

        if (fa - fc).abs() > 1e-15 && (fb - fc).abs() > 1e-15 {
            // Inverse quadratic interpolation
            let r = fb / fc;
            let p_val = fa / fc;
            let q = fa / fb;

            s = b
                - (q * (q - r) * (b - a) + (1.0 - r) * (b - c) * p_val)
                    / ((q - 1.0) * (r - 1.0) * (p_val - 1.0));

            // Check if interpolation is acceptable
            let m = (a + b) / 2.0;
            if s > m.min(b) && s < m.max(b) && (s - b).abs() < e.abs() / 2.0 {
                use_bisection = false;
            }
        } else if (fb - fa).abs() > 1e-15 {
            // Secant method
            s = b - fb * (b - a) / (fb - fa);

            let m = (a + b) / 2.0;
            if s > m.min(b) && s < m.max(b) && (s - b).abs() < e.abs() / 2.0 {
                use_bisection = false;
            }
        }

        if use_bisection {
            s = (a + b) / 2.0;
            e = b - a;
            d = e;
        } else {
            e = d;
            d = s - b;
        }

        // Move last best guess to a
        c = b;
        fc = fb;

        // Evaluate new point
        let fs = f(s);

        if fa * fs < 0.0 {
            b = s;
            fb = fs;
        } else {
            a = s;
            fa = fs;
        }

        // Ensure |f(a)| >= |f(b)|
        if fa.abs() < fb.abs() {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut fa, &mut fb);
        }
    }

    Err(MathError::convergence_failed(
        config.max_iterations,
        fb.abs(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_sqrt_2() {
        let f = |x: f64| x * x - 2.0;

        let result = brent(f, 1.0, 2.0, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-10);
    }

    #[test]
    fn test_cubic() {
        // x^3 - x - 2 has a root near 1.52
        let f = |x: f64| x * x * x - x - 2.0;

        let result = brent(f, 1.0, 2.0, &SolverConfig::default()).unwrap();

        assert!(f(result.root).abs() < 1e-10);
        assert_relative_eq!(result.root, 1.521_379_706_804_568, epsilon = 1e-10);
    }

    #[test]
    fn test_sin() {
        // Find root of sin(x) near pi
        let f = |x: f64| x.sin();

        let result = brent(f, 3.0, 4.0, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, std::f64::consts::PI, epsilon = 1e-10);
    }

    #[test]
    fn test_invalid_bracket() {
        let f = |x: f64| x * x - 2.0;

        let result = brent(f, 2.0, 3.0, &SolverConfig::default());

        assert!(result.is_err());
    }

    #[test]
    fn test_faster_than_bisection() {
        let f = |x: f64| x * x - 2.0;
        let config = SolverConfig::default();

        let brent_result = brent(f, 1.0, 2.0, &config).unwrap();

        // Brent should converge faster than bisection
        // Bisection needs ~34 iterations for 1e-10 tolerance
        assert!(brent_result.iterations < 20);
    }
}
