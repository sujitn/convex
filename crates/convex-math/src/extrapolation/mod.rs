//! Extrapolation methods for yield curves.
//!
//! This module provides extrapolation algorithms for extending curves beyond
//! their last observed data point:
//!
//! - [`FlatExtrapolator`]: Constant extension from last point
//! - [`LinearExtrapolator`]: Linear slope continuation
//! - [`SmithWilson`]: Regulatory standard (EIOPA) with Ultimate Forward Rate
//!
//! # Choosing an Extrapolation Method
//!
//! | Method | Use Case | Properties |
//! |--------|----------|------------|
//! | Flat | Simple, conservative | Constant forward rate |
//! | Linear | Trend continuation | May go negative |
//! | Smith-Wilson | **Regulatory (Solvency II)** | Converges to UFR |
//!
//! # Regulatory Context
//!
//! For European insurance regulation (Solvency II), [`SmithWilson`] is required.
//! It ensures smooth convergence to the Ultimate Forward Rate (UFR) beyond the
//! Last Liquid Point (LLP), typically 20 years for EUR.
//!
//! # Example
//!
//! ```rust
//! use convex_math::extrapolation::{SmithWilson, Extrapolator};
//!
//! // Create Smith-Wilson extrapolator for EUR (EIOPA parameters)
//! let sw = SmithWilson::eiopa_eur();
//!
//! // Extrapolate from a 20Y rate to 60Y
//! let rate_20y = 0.035;
//! let rate_60y = sw.extrapolate(60.0, 20.0, rate_20y, 0.001);
//! ```

mod flat;
mod linear;
mod smith_wilson;

pub use flat::FlatExtrapolator;
pub use linear::LinearExtrapolator;
pub use smith_wilson::SmithWilson;

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
    /// Smith-Wilson with UFR convergence
    SmithWilson {
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

    #[test]
    fn test_smith_wilson_convergence() {
        let sw = SmithWilson::new(0.042, 0.1, 20.0);

        let last_t = 20.0;
        let last_value = 0.035;
        let last_deriv = 0.0005;

        // Should converge towards UFR at long maturities
        let value_30 = sw.extrapolate(30.0, last_t, last_value, last_deriv);
        let value_60 = sw.extrapolate(60.0, last_t, last_value, last_deriv);
        let value_100 = sw.extrapolate(100.0, last_t, last_value, last_deriv);

        // Values should approach UFR (0.042)
        assert!(value_30 > last_value); // Moving towards UFR
        assert!(value_60 > value_30);
        assert!((value_100 - 0.042).abs() < (value_60 - 0.042).abs());
    }
}
