//! Interpolation methods.
//!
//! This module provides various interpolation algorithms commonly used
//! in yield curve construction and financial calculations.
//!
//! - [`LinearInterpolator`]: Simple linear interpolation
//! - [`CubicSpline`]: Natural cubic spline interpolation

mod cubic_spline;
mod linear;

pub use cubic_spline::CubicSpline;
pub use linear::LinearInterpolator;

use crate::error::MathResult;

/// Trait for interpolation methods.
pub trait Interpolator: Send + Sync {
    /// Returns the interpolated value at x.
    fn interpolate(&self, x: f64) -> MathResult<f64>;

    /// Returns true if extrapolation is allowed.
    fn allows_extrapolation(&self) -> bool {
        false
    }

    /// Returns the minimum x value in the data.
    fn min_x(&self) -> f64;

    /// Returns the maximum x value in the data.
    fn max_x(&self) -> f64;
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_linear_basic() {
        let xs = vec![0.0, 1.0, 2.0];
        let ys = vec![0.0, 1.0, 2.0];

        let interp = LinearInterpolator::new(xs, ys).unwrap();

        assert_relative_eq!(interp.interpolate(0.5).unwrap(), 0.5, epsilon = 1e-10);
        assert_relative_eq!(interp.interpolate(1.5).unwrap(), 1.5, epsilon = 1e-10);
    }
}
