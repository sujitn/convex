//! Root-finding algorithms.
//!
//! This module provides numerical solvers for finding roots of equations:
//!
//! - [`newton_raphson`]: Fast quadratic convergence when derivative is available
//! - [`brent`]: Robust method combining bisection, secant, and inverse quadratic
//! - [`bisection`]: Simple and reliable bracketing method
//! - [`secant`]: Derivative-free method using finite differences
//! - [`hybrid`]: Newton-Raphson with Brent fallback for robust convergence
//!
//! # Choosing a Solver
//!
//! | Solver | Speed | Reliability | Requires |
//! |--------|-------|-------------|----------|
//! | Newton-Raphson | Fastest (quadratic) | May diverge | Derivative |
//! | Brent | Fast (superlinear) | Guaranteed | Bracket |
//! | Secant | Fast (superlinear) | May diverge | Two guesses |
//! | Bisection | Slow (linear) | Guaranteed | Bracket |
//! | Hybrid | Fast | Guaranteed* | Initial guess |
//!
//! *When bounds are provided or can be found automatically.
//!
//! # Performance
//!
//! For typical financial calculations:
//! - YTM calculation: < 1μs (Newton, ~5-8 iterations)
//! - Z-spread calculation: < 50μs (Brent with 50 points)
//!
//! # Example: YTM Calculation
//!
//! ```rust
//! use convex_math::solvers::{hybrid, SolverConfig};
//!
//! // Bond: 5% coupon, 5 years, price 95
//! let price_fn = |y: f64| {
//!     let mut pv = 0.0;
//!     for t in 1..=5 {
//!         pv += 5.0 / (1.0 + y).powi(t);  // Coupon
//!     }
//!     pv += 100.0 / (1.0 + y).powi(5);    // Principal
//!     pv - 95.0
//! };
//!
//! let d_price_fn = |y: f64| {
//!     let mut dpv = 0.0;
//!     for t in 1..=5 {
//!         dpv -= (t as f64) * 5.0 / (1.0 + y).powi(t + 1);
//!     }
//!     dpv -= 5.0 * 100.0 / (1.0 + y).powi(6);
//!     dpv
//! };
//!
//! let result = hybrid(price_fn, d_price_fn, 0.05, Some((0.0, 0.20)), &SolverConfig::default()).unwrap();
//! assert!(result.root > 0.05);  // YTM > coupon rate for discount bond
//! ```

mod bisection;
mod brent;
mod hybrid;
mod newton;
mod secant;

pub use bisection::bisection;
pub use brent::brent;
pub use hybrid::{hybrid, hybrid_numerical};
pub use newton::{newton_raphson, newton_raphson_numerical};
pub use secant::secant;

use crate::error::MathResult;

/// Default tolerance for root-finding algorithms.
pub const DEFAULT_TOLERANCE: f64 = 1e-10;

/// Default maximum iterations for root-finding algorithms.
pub const DEFAULT_MAX_ITERATIONS: u32 = 100;

/// Configuration for root-finding algorithms.
#[derive(Debug, Clone, Copy)]
pub struct SolverConfig {
    /// Tolerance for convergence.
    pub tolerance: f64,
    /// Maximum number of iterations.
    pub max_iterations: u32,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            tolerance: DEFAULT_TOLERANCE,
            max_iterations: DEFAULT_MAX_ITERATIONS,
        }
    }
}

impl SolverConfig {
    /// Creates a new solver configuration.
    #[must_use]
    pub fn new(tolerance: f64, max_iterations: u32) -> Self {
        Self {
            tolerance,
            max_iterations,
        }
    }

    /// Sets the tolerance.
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Sets the maximum iterations.
    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.max_iterations = max_iterations;
        self
    }
}

/// Trait for root-finding algorithms.
pub trait RootFinder {
    /// Finds a root of the given function.
    ///
    /// # Arguments
    ///
    /// * `f` - The function for which to find a root
    /// * `initial_guess` - Starting point for the search
    /// * `config` - Solver configuration
    fn find_root<F>(&self, f: F, initial_guess: f64, config: &SolverConfig) -> MathResult<f64>
    where
        F: Fn(f64) -> f64;
}

/// Trait for root-finding solvers with optional derivative.
///
/// This trait provides a unified interface for all solvers, allowing
/// the caller to optionally provide a derivative function for faster
/// convergence.
///
/// # Example
///
/// ```rust
/// use convex_math::solvers::{Solver, NewtonSolver, SolverConfig};
///
/// let solver = NewtonSolver::default();
/// let f = |x: f64| x * x - 2.0;
/// let df = |x: f64| 2.0 * x;
///
/// let result = solver.solve(f, Some(df), 1.5, None, &SolverConfig::default()).unwrap();
/// assert!((result.root - std::f64::consts::SQRT_2).abs() < 1e-10);
/// ```
pub trait Solver: Send + Sync {
    /// Solves for a root of the given function.
    ///
    /// # Arguments
    ///
    /// * `f` - The function for which to find a root
    /// * `derivative` - Optional derivative function (used if available)
    /// * `initial_guess` - Starting point for the search
    /// * `bounds` - Optional bracketing interval (a, b)
    /// * `config` - Solver configuration
    ///
    /// # Returns
    ///
    /// The solution result including root, iterations, and residual.
    fn solve<F, D>(
        &self,
        f: F,
        derivative: Option<D>,
        initial_guess: f64,
        bounds: Option<(f64, f64)>,
        config: &SolverConfig,
    ) -> MathResult<SolverResult>
    where
        F: Fn(f64) -> f64,
        D: Fn(f64) -> f64;

    /// Returns the name of the solver.
    fn name(&self) -> &'static str;
}

/// Newton-Raphson solver implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct NewtonSolver;

impl Solver for NewtonSolver {
    fn solve<F, D>(
        &self,
        f: F,
        derivative: Option<D>,
        initial_guess: f64,
        _bounds: Option<(f64, f64)>,
        config: &SolverConfig,
    ) -> MathResult<SolverResult>
    where
        F: Fn(f64) -> f64,
        D: Fn(f64) -> f64,
    {
        match derivative {
            Some(df) => newton_raphson(f, df, initial_guess, config),
            None => newton_raphson_numerical(f, initial_guess, config),
        }
    }

    fn name(&self) -> &'static str {
        "Newton-Raphson"
    }
}

/// Brent's method solver implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct BrentSolver;

impl Solver for BrentSolver {
    fn solve<F, D>(
        &self,
        f: F,
        _derivative: Option<D>,
        initial_guess: f64,
        bounds: Option<(f64, f64)>,
        config: &SolverConfig,
    ) -> MathResult<SolverResult>
    where
        F: Fn(f64) -> f64,
        D: Fn(f64) -> f64,
    {
        let (a, b) = bounds.unwrap_or((initial_guess - 1.0, initial_guess + 1.0));
        brent(f, a, b, config)
    }

    fn name(&self) -> &'static str {
        "Brent"
    }
}

/// Bisection solver implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct BisectionSolver;

impl Solver for BisectionSolver {
    fn solve<F, D>(
        &self,
        f: F,
        _derivative: Option<D>,
        initial_guess: f64,
        bounds: Option<(f64, f64)>,
        config: &SolverConfig,
    ) -> MathResult<SolverResult>
    where
        F: Fn(f64) -> f64,
        D: Fn(f64) -> f64,
    {
        let (a, b) = bounds.unwrap_or((initial_guess - 1.0, initial_guess + 1.0));
        bisection(f, a, b, config)
    }

    fn name(&self) -> &'static str {
        "Bisection"
    }
}

/// Secant method solver implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct SecantSolver;

impl Solver for SecantSolver {
    fn solve<F, D>(
        &self,
        f: F,
        _derivative: Option<D>,
        initial_guess: f64,
        bounds: Option<(f64, f64)>,
        config: &SolverConfig,
    ) -> MathResult<SolverResult>
    where
        F: Fn(f64) -> f64,
        D: Fn(f64) -> f64,
    {
        let (x0, x1) = bounds.unwrap_or((initial_guess - 0.1, initial_guess + 0.1));
        secant(f, x0, x1, config)
    }

    fn name(&self) -> &'static str {
        "Secant"
    }
}

/// Hybrid solver (Newton + Brent fallback).
#[derive(Debug, Clone, Copy, Default)]
pub struct HybridSolver;

impl Solver for HybridSolver {
    fn solve<F, D>(
        &self,
        f: F,
        derivative: Option<D>,
        initial_guess: f64,
        bounds: Option<(f64, f64)>,
        config: &SolverConfig,
    ) -> MathResult<SolverResult>
    where
        F: Fn(f64) -> f64,
        D: Fn(f64) -> f64,
    {
        match derivative {
            Some(df) => hybrid(f, df, initial_guess, bounds, config),
            None => hybrid_numerical(f, initial_guess, bounds, config),
        }
    }

    fn name(&self) -> &'static str {
        "Hybrid (Newton + Brent)"
    }
}

/// Result of a root-finding iteration.
#[derive(Debug, Clone, Copy)]
pub struct SolverResult {
    /// The root found.
    pub root: f64,
    /// Number of iterations used.
    pub iterations: u32,
    /// Final residual (function value at root).
    pub residual: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_solver_config() {
        let config = SolverConfig::default()
            .with_tolerance(1e-8)
            .with_max_iterations(50);

        assert!((config.tolerance - 1e-8).abs() < f64::EPSILON);
        assert_eq!(config.max_iterations, 50);
    }

    #[test]
    fn test_solver_trait_newton() {
        let solver = NewtonSolver;
        let f = |x: f64| x * x - 2.0;
        let df = |x: f64| 2.0 * x;

        let result = solver
            .solve(f, Some(df), 1.5, None, &SolverConfig::default())
            .unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-10);
        assert_eq!(solver.name(), "Newton-Raphson");
    }

    #[test]
    fn test_solver_trait_brent() {
        let solver = BrentSolver;
        let f = |x: f64| x * x - 2.0;
        let no_deriv: Option<fn(f64) -> f64> = None;

        let result = solver
            .solve(f, no_deriv, 1.5, Some((1.0, 2.0)), &SolverConfig::default())
            .unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-10);
        assert_eq!(solver.name(), "Brent");
    }

    #[test]
    fn test_solver_trait_hybrid() {
        let solver = HybridSolver;
        let f = |x: f64| x * x - 2.0;
        let df = |x: f64| 2.0 * x;

        let result = solver
            .solve(f, Some(df), 1.5, Some((1.0, 2.0)), &SolverConfig::default())
            .unwrap();

        assert_relative_eq!(result.root, std::f64::consts::SQRT_2, epsilon = 1e-10);
    }

    // ============ YTM-like Financial Tests ============

    /// Helper to calculate bond price from yield
    fn bond_price(yield_rate: f64, coupon: f64, face: f64, years: i32, freq: i32) -> f64 {
        let periods = years * freq;
        let coupon_per_period = coupon / freq as f64;
        let discount_rate = yield_rate / freq as f64;

        let mut pv = 0.0;
        for t in 1..=periods {
            pv += coupon_per_period / (1.0 + discount_rate).powi(t);
        }
        pv += face / (1.0 + discount_rate).powi(periods);
        pv
    }

    /// Helper to calculate derivative of bond price w.r.t. yield
    fn bond_price_derivative(
        yield_rate: f64,
        coupon: f64,
        face: f64,
        years: i32,
        freq: i32,
    ) -> f64 {
        let periods = years * freq;
        let coupon_per_period = coupon / freq as f64;
        let discount_rate = yield_rate / freq as f64;

        let mut dpv = 0.0;
        for t in 1..=periods {
            dpv -= (t as f64 / freq as f64) * coupon_per_period / (1.0 + discount_rate).powi(t + 1);
        }
        dpv -= (periods as f64 / freq as f64) * face / (1.0 + discount_rate).powi(periods + 1);
        dpv
    }

    #[test]
    fn test_ytm_par_bond() {
        // A bond trading at par should have YTM = coupon rate
        let coupon = 5.0;
        let face = 100.0;
        let target_price = 100.0; // Par
        let years = 10;
        let freq = 2; // Semi-annual

        let f = |y: f64| bond_price(y, coupon, face, years, freq) - target_price;
        let df = |y: f64| bond_price_derivative(y, coupon, face, years, freq);

        let result = newton_raphson(f, df, 0.05, &SolverConfig::default()).unwrap();

        // YTM should equal coupon rate for par bond
        assert_relative_eq!(result.root, 0.05, epsilon = 1e-10);
    }

    #[test]
    fn test_ytm_discount_bond() {
        // A bond trading below par should have YTM > coupon rate
        let coupon = 5.0;
        let face = 100.0;
        let target_price = 95.0; // Below par
        let years = 5;
        let freq = 2;

        let f = |y: f64| bond_price(y, coupon, face, years, freq) - target_price;
        let df = |y: f64| bond_price_derivative(y, coupon, face, years, freq);

        let result = hybrid(f, df, 0.05, Some((0.0, 0.20)), &SolverConfig::default()).unwrap();

        // YTM should be higher than coupon rate
        assert!(result.root > 0.05);
        // Verify the price matches
        assert!(f(result.root).abs() < 1e-10);
    }

    #[test]
    fn test_ytm_premium_bond() {
        // A bond trading above par should have YTM < coupon rate
        let coupon = 7.0;
        let face = 100.0;
        let target_price = 105.0; // Above par
        let years = 5;
        let freq = 2;

        let f = |y: f64| bond_price(y, coupon, face, years, freq) - target_price;
        let df = |y: f64| bond_price_derivative(y, coupon, face, years, freq);

        let result = hybrid(f, df, 0.07, Some((0.0, 0.20)), &SolverConfig::default()).unwrap();

        // YTM should be lower than coupon rate
        assert!(result.root < 0.07);
        assert!(f(result.root).abs() < 1e-10);
    }

    #[test]
    fn test_ytm_all_solvers_agree() {
        // All solvers should find the same YTM
        let coupon = 6.0;
        let face = 100.0;
        let target_price = 98.0;
        let years = 7;
        let freq = 2;

        let f = |y: f64| bond_price(y, coupon, face, years, freq) - target_price;
        let df = |y: f64| bond_price_derivative(y, coupon, face, years, freq);
        let config = SolverConfig::default();

        let newton_result = newton_raphson(f, df, 0.06, &config).unwrap();
        let brent_result = brent(f, 0.0, 0.20, &config).unwrap();
        let hybrid_result = hybrid(f, df, 0.06, Some((0.0, 0.20)), &config).unwrap();
        let secant_result = secant(f, 0.05, 0.07, &config).unwrap();

        // All should agree within tolerance
        assert_relative_eq!(newton_result.root, brent_result.root, epsilon = 1e-8);
        assert_relative_eq!(newton_result.root, hybrid_result.root, epsilon = 1e-8);
        assert_relative_eq!(newton_result.root, secant_result.root, epsilon = 1e-8);
    }

    #[test]
    fn test_z_spread_like_calculation() {
        // Simulate Z-spread: find constant spread over zero curve
        // Simplified: assume flat zero curve at 3%
        let zero_rate = 0.03;
        let target_price = 97.0;
        let coupon = 5.0;
        let face = 100.0;
        let years = 5;

        let price_with_spread = |spread: f64| {
            let mut pv = 0.0;
            for t in 1..=years {
                let discount = (-((zero_rate + spread) * t as f64)).exp();
                pv += coupon * discount;
            }
            pv += face * (-((zero_rate + spread) * years as f64)).exp();
            pv
        };

        let f = |spread: f64| price_with_spread(spread) - target_price;

        // Z-spread should be positive since bond is below par
        let result = brent(f, -0.05, 0.10, &SolverConfig::default()).unwrap();

        assert!(result.root > 0.0);
        assert!(f(result.root).abs() < 1e-10);
    }

    #[test]
    fn test_solver_convergence_speed() {
        // Newton should converge faster than Brent
        let f = |x: f64| x * x - 2.0;
        let df = |x: f64| 2.0 * x;
        let config = SolverConfig::default();

        let newton_result = newton_raphson(f, df, 1.5, &config).unwrap();
        let brent_result = brent(f, 1.0, 2.0, &config).unwrap();

        // Newton should use fewer iterations
        assert!(newton_result.iterations <= brent_result.iterations);
    }

    #[test]
    fn test_high_yield_bond() {
        // High yield bond with YTM around 12%
        let coupon = 8.0;
        let face = 100.0;
        let target_price = 85.0; // Deep discount
        let years = 5;
        let freq = 2;

        let f = |y: f64| bond_price(y, coupon, face, years, freq) - target_price;
        let df = |y: f64| bond_price_derivative(y, coupon, face, years, freq);

        let result = hybrid(f, df, 0.10, Some((0.0, 0.30)), &SolverConfig::default()).unwrap();

        // Should find a high yield
        assert!(result.root > 0.10);
        assert!(f(result.root).abs() < 1e-10);
    }

    #[test]
    fn test_zero_coupon_bond() {
        // Zero coupon bond - simpler case
        // Price = Face / (1 + y)^n
        // At 10% yield over 5 years: Price = 100 / (1.10)^5 = 62.0921...
        let face = 100.0;
        let target_price = 62.0921; // Exact 10% yield over 5 years
        let years = 5;

        let f = |y: f64| face / (1.0 + y).powi(years) - target_price;
        let df = |y: f64| -(years as f64) * face / (1.0 + y).powi(years + 1);

        let result = newton_raphson(f, df, 0.08, &SolverConfig::default()).unwrap();

        // Should be close to 10%
        assert_relative_eq!(result.root, 0.10, epsilon = 0.001);
    }
}
