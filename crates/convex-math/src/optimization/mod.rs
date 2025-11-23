//! Optimization algorithms.
//!
//! This module provides optimization routines for curve fitting
//! and other financial calculations.

use crate::error::MathResult;

/// Configuration for optimization algorithms.
#[derive(Debug, Clone, Copy)]
pub struct OptimizationConfig {
    /// Tolerance for convergence.
    pub tolerance: f64,
    /// Maximum number of iterations.
    pub max_iterations: u32,
    /// Step size for numerical gradients.
    pub step_size: f64,
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            tolerance: 1e-10,
            max_iterations: 100,
            step_size: 1e-8,
        }
    }
}

/// Result of an optimization run.
#[derive(Debug, Clone)]
pub struct OptimizationResult {
    /// Optimal parameters found.
    pub parameters: Vec<f64>,
    /// Final objective function value.
    pub objective_value: f64,
    /// Number of iterations used.
    pub iterations: u32,
    /// Whether the optimization converged.
    pub converged: bool,
}

/// Simple gradient descent optimizer.
///
/// Minimizes a function using steepest descent with numerical gradients.
pub fn gradient_descent<F>(
    f: F,
    initial: &[f64],
    config: &OptimizationConfig,
) -> MathResult<OptimizationResult>
where
    F: Fn(&[f64]) -> f64,
{
    let mut params = initial.to_vec();
    let mut best_value = f(&params);
    let n = params.len();

    for iteration in 0..config.max_iterations {
        // Compute numerical gradient
        let mut gradient = vec![0.0; n];
        for i in 0..n {
            let mut params_plus = params.clone();
            let mut params_minus = params.clone();
            params_plus[i] += config.step_size;
            params_minus[i] -= config.step_size;

            gradient[i] = (f(&params_plus) - f(&params_minus)) / (2.0 * config.step_size);
        }

        // Compute gradient magnitude
        let grad_mag: f64 = gradient.iter().map(|g| g * g).sum::<f64>().sqrt();

        if grad_mag < config.tolerance {
            return Ok(OptimizationResult {
                parameters: params,
                objective_value: best_value,
                iterations: iteration,
                converged: true,
            });
        }

        // Line search with backtracking
        let mut step = 1.0;
        let c = 0.5; // Armijo parameter

        loop {
            let mut new_params = params.clone();
            for i in 0..n {
                new_params[i] -= step * gradient[i];
            }

            let new_value = f(&new_params);
            if new_value < best_value - c * step * grad_mag * grad_mag {
                params = new_params;
                best_value = new_value;
                break;
            }

            step *= 0.5;
            if step < 1e-15 {
                // Can't make progress
                return Ok(OptimizationResult {
                    parameters: params,
                    objective_value: best_value,
                    iterations: iteration,
                    converged: false,
                });
            }
        }
    }

    Ok(OptimizationResult {
        parameters: params,
        objective_value: best_value,
        iterations: config.max_iterations,
        converged: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_gradient_descent_quadratic() {
        // Minimize (x-2)^2 + (y-3)^2
        let f = |params: &[f64]| {
            let x = params[0];
            let y = params[1];
            (x - 2.0).powi(2) + (y - 3.0).powi(2)
        };

        let initial = vec![0.0, 0.0];
        let result = gradient_descent(f, &initial, &OptimizationConfig::default()).unwrap();

        assert!(result.converged);
        assert_relative_eq!(result.parameters[0], 2.0, epsilon = 1e-5);
        assert_relative_eq!(result.parameters[1], 3.0, epsilon = 1e-5);
    }
}
