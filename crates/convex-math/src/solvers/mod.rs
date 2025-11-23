//! Root-finding algorithms.
//!
//! This module provides numerical solvers for finding roots of equations:
//!
//! - [`newton_raphson`]: Fast quadratic convergence when derivative is available
//! - [`brent`]: Robust method combining bisection, secant, and inverse quadratic
//! - [`bisection`]: Simple and reliable bracketing method

mod bisection;
mod brent;
mod newton;

pub use bisection::bisection;
pub use brent::brent;
pub use newton::newton_raphson;

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

    #[test]
    fn test_solver_config() {
        let config = SolverConfig::default()
            .with_tolerance(1e-8)
            .with_max_iterations(50);

        assert_eq!(config.tolerance, 1e-8);
        assert_eq!(config.max_iterations, 50);
    }
}
