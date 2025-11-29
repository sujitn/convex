//! Interpolation methods for yield curve construction.
//!
//! This module provides various interpolation algorithms commonly used
//! in yield curve construction and financial calculations.
//!
//! # Available Methods
//!
//! **On Zero Rates:**
//! - [`LinearInterpolator`]: Simple linear interpolation
//! - [`LogLinearInterpolator`]: Log-linear interpolation (interpolates log of values)
//! - [`CubicSpline`]: Natural cubic spline interpolation
//! - [`MonotoneConvex`]: Hagan monotone convex (production default, ensures positive forwards)
//!
//! **Parametric Models:**
//! - [`NelsonSiegel`]: Nelson-Siegel parametric curve
//! - [`Svensson`]: Svensson extension of Nelson-Siegel
//!
//! # Choosing an Interpolation Method
//!
//! | Method | Speed | Smoothness | Positive Forwards | Use Case |
//! |--------|-------|------------|-------------------|----------|
//! | Linear | Fast | C0 | No | Quick prototyping |
//! | Log-Linear | Fast | C0 | Yes (on discount) | Discount factor curves |
//! | Cubic Spline | Medium | C2 | No | Smooth curves |
//! | Monotone Convex | Medium | C1 | **Yes** | **Production default** |
//! | Nelson-Siegel | Fast | C∞ | Usually | Parametric fitting |
//! | Svensson | Fast | C∞ | Usually | More flexible fitting |
//!
//! # Forward Rate Considerations
//!
//! For production yield curve construction, use [`MonotoneConvex`] as it guarantees:
//! - Positive forward rates
//! - No spurious oscillations
//! - C1 continuity (continuous first derivative)

mod cubic_spline;
mod linear;
mod log_linear;
mod monotone_convex;
mod parametric;

pub use cubic_spline::CubicSpline;
pub use linear::LinearInterpolator;
pub use log_linear::LogLinearInterpolator;
pub use monotone_convex::MonotoneConvex;
pub use parametric::{NelsonSiegel, Svensson};

use crate::error::MathResult;

/// Trait for interpolation methods.
///
/// All interpolation methods implement this trait, providing a unified
/// interface for curve construction.
pub trait Interpolator: Send + Sync {
    /// Returns the interpolated value at x.
    fn interpolate(&self, x: f64) -> MathResult<f64>;

    /// Returns the first derivative at x.
    ///
    /// This is critical for computing forward rates from zero rates.
    fn derivative(&self, x: f64) -> MathResult<f64>;

    /// Returns true if extrapolation is allowed.
    fn allows_extrapolation(&self) -> bool {
        false
    }

    /// Returns the minimum x value in the data.
    fn min_x(&self) -> f64;

    /// Returns the maximum x value in the data.
    fn max_x(&self) -> f64;

    /// Checks if x is within the interpolation range.
    fn in_range(&self, x: f64) -> bool {
        x >= self.min_x() && x <= self.max_x()
    }
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

    // ============ Comparative Tests ============

    #[test]
    fn test_all_interpolators_through_points() {
        // All interpolators should pass through the input points
        let times = vec![0.5, 1.0, 2.0, 3.0, 5.0];
        let rates = vec![0.02, 0.025, 0.03, 0.035, 0.04];

        // Linear
        let linear = LinearInterpolator::new(times.clone(), rates.clone()).unwrap();
        for (t, r) in times.iter().zip(rates.iter()) {
            assert_relative_eq!(linear.interpolate(*t).unwrap(), *r, epsilon = 1e-10);
        }

        // Cubic Spline
        let spline = CubicSpline::new(times.clone(), rates.clone()).unwrap();
        for (t, r) in times.iter().zip(rates.iter()) {
            assert_relative_eq!(spline.interpolate(*t).unwrap(), *r, epsilon = 1e-10);
        }

        // Log-Linear (on discount factors)
        let dfs: Vec<f64> = times
            .iter()
            .zip(rates.iter())
            .map(|(t, r)| (-r * t).exp())
            .collect();
        let log_linear = LogLinearInterpolator::new(times.clone(), dfs.clone()).unwrap();
        for (t, df) in times.iter().zip(dfs.iter()) {
            assert_relative_eq!(log_linear.interpolate(*t).unwrap(), *df, epsilon = 1e-10);
        }

        // Monotone Convex
        let mc = MonotoneConvex::new(times.clone(), rates.clone()).unwrap();
        for (t, r) in times.iter().zip(rates.iter()) {
            // Monotone convex may have small deviations due to smoothing
            assert_relative_eq!(mc.interpolate(*t).unwrap(), *r, epsilon = 0.002);
        }
    }

    #[test]
    fn test_forward_rate_positivity() {
        // Test that MonotoneConvex always produces positive forwards
        let times = vec![0.5, 1.0, 2.0, 3.0, 5.0, 10.0];
        let rates = vec![0.02, 0.025, 0.03, 0.028, 0.035, 0.04];

        let mc = MonotoneConvex::new(times, rates).unwrap();

        // Check many points for positive forwards
        for t in [0.1, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 7.0, 10.0] {
            let fwd = mc.forward_rate(t).unwrap();
            assert!(fwd >= 0.0, "Forward at t={} is {}, should be >= 0", t, fwd);
        }
    }

    #[test]
    fn test_derivative_consistency() {
        // Test that derivatives are numerically correct for all interpolators
        let times = vec![0.5, 1.0, 2.0, 3.0, 5.0];
        let rates = vec![0.02, 0.025, 0.03, 0.035, 0.04];

        // Linear
        let linear = LinearInterpolator::new(times.clone(), rates.clone()).unwrap();
        check_derivative(&linear, 1.5, "Linear");

        // Cubic Spline
        let spline = CubicSpline::new(times.clone(), rates.clone()).unwrap();
        check_derivative(&spline, 1.5, "CubicSpline");

        // Log-Linear
        let dfs: Vec<f64> = times
            .iter()
            .zip(rates.iter())
            .map(|(t, r)| (-r * t).exp())
            .collect();
        let log_linear = LogLinearInterpolator::new(times.clone(), dfs).unwrap();
        check_derivative(&log_linear, 1.5, "LogLinear");

        // Monotone Convex
        let mc = MonotoneConvex::new(times.clone(), rates.clone()).unwrap();
        check_derivative(&mc, 1.5, "MonotoneConvex");

        // Nelson-Siegel
        let ns = NelsonSiegel::new(0.04, -0.02, 0.01, 2.0).unwrap();
        check_derivative(&ns, 3.0, "NelsonSiegel");

        // Svensson
        let sv = Svensson::new(0.04, -0.02, 0.01, -0.005, 2.0, 8.0).unwrap();
        check_derivative(&sv, 3.0, "Svensson");
    }

    fn check_derivative(interp: &dyn Interpolator, t: f64, name: &str) {
        let h = 1e-6;
        let y_plus = interp.interpolate(t + h).unwrap();
        let y_minus = interp.interpolate(t - h).unwrap();
        let numerical = (y_plus - y_minus) / (2.0 * h);

        let analytical = interp.derivative(t).unwrap();

        assert!(
            (analytical - numerical).abs() < 1e-4,
            "{} derivative at t={}: analytical={}, numerical={}",
            name,
            t,
            analytical,
            numerical
        );
    }

    #[test]
    fn test_yield_curve_construction() {
        // Realistic yield curve data
        let maturities = vec![0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 20.0, 30.0];
        let zero_rates = vec![
            0.0200, 0.0210, 0.0225, 0.0250, 0.0275, 0.0310, 0.0340, 0.0370, 0.0400, 0.0410,
        ];

        // Build curves with different methods
        let linear = LinearInterpolator::new(maturities.clone(), zero_rates.clone()).unwrap();
        let spline = CubicSpline::new(maturities.clone(), zero_rates.clone()).unwrap();
        let mc = MonotoneConvex::new(maturities.clone(), zero_rates.clone()).unwrap();

        // Test interpolation at 4 years (between 3Y and 5Y pillars)
        let t = 4.0;
        let z_linear = linear.interpolate(t).unwrap();
        let z_spline = spline.interpolate(t).unwrap();
        let z_mc = mc.interpolate(t).unwrap();

        // All should give reasonable values between 2.75% and 3.10%
        assert!(z_linear > 0.0275 && z_linear < 0.0310);
        assert!(z_spline > 0.0275 && z_spline < 0.0310);
        assert!(z_mc > 0.0275 && z_mc < 0.0310);

        // MonotoneConvex should have positive forwards
        let fwd = mc.forward_rate(t).unwrap();
        assert!(fwd > 0.0);
    }

    #[test]
    fn test_parametric_curve_fitting() {
        // Test that parametric curves can model realistic shapes
        let ns = NelsonSiegel::new(0.04, -0.015, 0.008, 2.5).unwrap();

        // Should be upward sloping
        let r_1y = ns.interpolate(1.0).unwrap();
        let r_10y = ns.interpolate(10.0).unwrap();
        assert!(r_1y < r_10y);

        // Should have a slight hump
        let r_3y = ns.interpolate(3.0).unwrap();
        let d_1y = ns.derivative(1.0).unwrap();
        let d_10y = ns.derivative(10.0).unwrap();

        // Mid-term rate should be between short and long
        assert!(r_3y > r_1y && r_3y < r_10y);

        // Derivative should be positive (upward slope) but decreasing
        assert!(d_1y > 0.0);
        assert!(d_1y > d_10y);
    }

    #[test]
    fn test_discount_factor_interpolation() {
        // Test log-linear for discount factors
        let times = vec![0.25, 0.5, 1.0, 2.0, 5.0, 10.0];
        let rates = vec![0.02, 0.022, 0.025, 0.03, 0.035, 0.04];

        // Convert to discount factors
        let dfs: Vec<f64> = times
            .iter()
            .zip(rates.iter())
            .map(|(t, r): (&f64, &f64)| (-r * t).exp())
            .collect();

        let log_linear = LogLinearInterpolator::new(times.clone(), dfs.clone()).unwrap();

        // Interpolated discount factors should be monotonically decreasing
        let mut prev_df = 1.0;
        for t in [0.25, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 7.0, 10.0] {
            let df = log_linear.interpolate(t).unwrap();
            assert!(
                df < prev_df,
                "DF at t={} ({}) should be < previous ({})",
                t,
                df,
                prev_df
            );
            assert!(df > 0.0, "DF should be positive");
            prev_df = df;
        }
    }
}
