//! Extrapolation methods for yield curves.
//!
//! This module provides extrapolation algorithms for extending curves beyond
//! their last observed data point:
//!
//! - [`FlatExtrapolator`]: Constant extension from last point
//! - [`LinearExtrapolator`]: Linear slope continuation
//! - [`UfrConvergence`]: Forward rate converging to an Ultimate Forward Rate
//!
//! # Choosing an Extrapolation Method
//!
//! | Method | Use Case | Properties |
//! |--------|----------|------------|
//! | Flat | Simple, conservative | Constant forward rate |
//! | Linear | Trend continuation | May go negative |
//! | UFR-convergence | Long-end / liability curves | Forward converges to UFR |
//!
//! [`UfrConvergence`] is a heuristic tail extrapolator, not the EIOPA / Solvency
//! II Smith-Wilson method; see its module docs for the distinction.
//!
//! # Example
//!
//! ```rust
//! use convex_math::extrapolation::{UfrConvergence, Extrapolator};
//!
//! // Forward converges to a 4.2% UFR beyond the 20Y last liquid point.
//! let ext = UfrConvergence::new(0.042, 0.1, 20.0);
//!
//! // Extrapolate from a 20Y rate to 60Y.
//! let rate_60y = ext.extrapolate(60.0, 20.0, 0.035, 0.001);
//! ```

mod flat;
mod linear;
mod ufr_convergence;

pub use flat::FlatExtrapolator;
pub use linear::LinearExtrapolator;
pub use ufr_convergence::UfrConvergence;

/// Trait for extrapolation methods.
///
/// Extrapolators extend curves beyond their last observed point.
pub trait Extrapolator: Send + Sync {
    /// Extrapolates to time `t` given the last known point.
    ///
    /// # Arguments
    ///
    /// * `t` - Target time for extrapolation
    /// * `last_t` - Time of last known point
    /// * `last_value` - Value at last known point
    /// * `last_derivative` - Derivative at last known point (slope)
    fn extrapolate(&self, t: f64, last_t: f64, last_value: f64, last_derivative: f64) -> f64;

    /// Returns the name of the extrapolation method.
    fn name(&self) -> &'static str;
}

/// Configuration for extrapolation beyond curve boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ExtrapolationMethod {
    /// No extrapolation - return error outside range
    None,
    /// Constant value from boundary
    #[default]
    Flat,
    /// Linear continuation with boundary slope
    Linear,
    /// Forward rate converging to an Ultimate Forward Rate (see [`UfrConvergence`])
    UfrConvergence {
        /// Ultimate forward rate
        ufr: f64,
        /// Convergence speed (alpha)
        alpha: f64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_flat_extrapolator() {
        let extrap = FlatExtrapolator;

        let last_t = 10.0;
        let last_value = 0.05;
        let last_deriv = 0.001;

        // Flat should ignore derivative and return last value
        let value = extrap.extrapolate(15.0, last_t, last_value, last_deriv);
        assert_relative_eq!(value, last_value, epsilon = 1e-10);

        let value = extrap.extrapolate(100.0, last_t, last_value, last_deriv);
        assert_relative_eq!(value, last_value, epsilon = 1e-10);
    }

    #[test]
    fn test_linear_extrapolator() {
        let extrap = LinearExtrapolator;

        let last_t = 10.0;
        let last_value = 0.05;
        let last_deriv = 0.001; // 0.1% per year

        // Linear should continue with the slope
        let value = extrap.extrapolate(15.0, last_t, last_value, last_deriv);
        let expected = last_value + last_deriv * (15.0 - last_t);
        assert_relative_eq!(value, expected, epsilon = 1e-10);
    }
}
