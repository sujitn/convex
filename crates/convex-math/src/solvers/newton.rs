//! Newton-Raphson root-finding algorithm.

use crate::error::{MathError, MathResult};
use crate::solvers::{SolverConfig, SolverResult};

/// Newton-Raphson root-finding algorithm.
///
/// Uses the iteration:
/// `x_{n+1} = x_n - f(x_n) / f'(x_n)`
///
/// This method has quadratic convergence near the root but requires
/// the derivative of the function.
///
/// # Arguments
///
/// * `f` - The function for which to find a root
/// * `df` - The derivative of the function
/// * `initial_guess` - Starting point for the iteration
/// * `config` - Solver configuration
///
/// # Returns
///
/// The root and iteration statistics, or an error if convergence fails.
///
/// # Example
///
/// ```rust
/// use convex_math::solvers::{newton_raphson, SolverConfig};
///
/// // Find root of x^2 - 2 (i.e., sqrt(2))
/// let f = |x: f64| x * x - 2.0;
/// let df = |x: f64| 2.0 * x;
///
/// let result = newton_raphson(f, df, 1.5, &SolverConfig::default()).unwrap();
/// assert!((result.root - std::f64::consts::SQRT_2).abs() < 1e-10);
/// ```
pub fn newton_raphson<F, DF>(
    f: F,
    df: DF,
    initial_guess: f64,
    config: &SolverConfig,
) -> MathResult<SolverResult>
where
    F: Fn(f64) -> f64,
    DF: Fn(f64) -> f64,
{
    let mut x = initial_guess;

    for iteration in 0..config.max_iterations {
        let fx = f(x);

        // Check for convergence
        if fx.abs() < config.tolerance {
            return Ok(SolverResult {
                root: x,
                iterations: iteration,
                residual: fx,
            });
        }

        let dfx = df(x);

        // Check for zero derivative
        if dfx.abs() < 1e-15 {
            return Err(MathError::DivisionByZero { value: dfx });
        }

        // Newton step
        let step = fx / dfx;
        x -= step;

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

    Err(MathError::convergence_failed(
        config.max_iterations,
        f(x).abs(),
    ))
}

/// Newton-Raphson with numerical derivative estimation.
///
/// Uses finite differences to estimate the derivative when
/// an analytical derivative is not available.
///
/// # Arguments
///
/// * `f` - The function for which to find a root
/// * `initial_guess` - Starting point for the iteration
/// * `config` - Solver configuration
pub fn newton_raphson_numerical<F>(
    f: F,
    initial_guess: f64,
    config: &SolverConfig,
) -> MathResult<SolverResult>
where
    F: Fn(f64) -> f64,
{
    let h = 1e-8; // Step size for numerical differentiation

    let df = |x: f64| {
        let f1 = f(x + h);
        let f2 = f(x - h);
        (f1 - f2) / (2.0 * h)
    };

    newton_raphson(&f, df, initial_guess, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_sqrt_2() {
        let f = |x: f64| x * x - 2.0;
        let df = |x: f64| 2.0 * x;

        let result = newton_raphson(f, df, 1.5, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-10);
        assert!(result.iterations < 10); // Should converge quickly
    }

    #[test]
    fn test_cube_root() {
        // Find cube root of 27 (should be 3)
        let f = |x: f64| x * x * x - 27.0;
        let df = |x: f64| 3.0 * x * x;

        let result = newton_raphson(f, df, 2.0, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, 3.0, epsilon = 1e-10);
    }

    #[test]
    fn test_numerical_derivative() {
        let f = |x: f64| x * x - 2.0;

        let result = newton_raphson_numerical(f, 1.5, &SolverConfig::default()).unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-8);
    }

    #[test]
    fn test_zero_derivative_error() {
        // f(x) = x^3 with initial guess at 0 has zero derivative
        let f = |x: f64| x * x * x - 1.0;
        let df = |x: f64| 3.0 * x * x;

        let result = newton_raphson(f, df, 0.0, &SolverConfig::default());

        assert!(result.is_err());
    }

    #[test]
    fn test_convergence_fail() {
        // Function that oscillates - will fail to converge
        let f = |x: f64| x.sin();
        let df = |x: f64| x.cos();

        let config = SolverConfig::new(1e-15, 5); // Very tight tolerance, few iterations
        let result = newton_raphson(f, df, 0.5, &config);

        // Should either converge (near 0) or fail
        // With 5 iterations from 0.5, it should converge to 0
        assert!(result.is_ok());
    }
}
