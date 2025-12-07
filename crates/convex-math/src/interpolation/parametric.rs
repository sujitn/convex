//! Parametric yield curve models.
//!
//! This module provides parametric models for yield curve fitting:
//! - Nelson-Siegel: 4-parameter model
//! - Svensson: 6-parameter extension
//!
//! These models are used for curve fitting rather than point-by-point interpolation.

use crate::error::{MathError, MathResult};
use crate::interpolation::Interpolator;

/// Nelson-Siegel yield curve model.
///
/// The model parameterizes the zero rate curve as:
/// ```text
/// z(t) = β₀ + β₁ * ((1 - e^(-t/τ)) / (t/τ))
///           + β₂ * ((1 - e^(-t/τ)) / (t/τ) - e^(-t/τ))
/// ```
///
/// Where:
/// - β₀: Long-term level (asymptotic zero rate)
/// - β₁: Short-term component (slope)
/// - β₂: Medium-term component (curvature/hump)
/// - τ: Decay factor (controls where the hump occurs)
///
/// # Financial Interpretation
///
/// - β₀: Long-run equilibrium rate
/// - β₀ + β₁: Instantaneous short rate (as t → 0)
/// - β₂ > 0: Hump in curve; β₂ < 0: U-shape
/// - τ: Time to maximum hump effect (~2-3 years typical)
///
/// # Example
///
/// ```rust
/// use convex_math::interpolation::{NelsonSiegel, Interpolator};
///
/// // Create a typical upward-sloping curve
/// let ns = NelsonSiegel::new(
///     0.045,  // β₀: 4.5% long rate
///     -0.02,  // β₁: negative for upward slope
///     0.01,   // β₂: slight hump
///     2.0,    // τ: 2 years
/// ).unwrap();
///
/// let short_rate = ns.interpolate(0.25).unwrap();
/// let long_rate = ns.interpolate(30.0).unwrap();
/// assert!(short_rate < long_rate);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct NelsonSiegel {
    /// Long-term level
    beta0: f64,
    /// Short-term component
    beta1: f64,
    /// Medium-term component
    beta2: f64,
    /// Decay factor
    tau: f64,
}

impl NelsonSiegel {
    /// Creates a new Nelson-Siegel curve.
    ///
    /// # Arguments
    ///
    /// * `beta0` - Long-term level (asymptotic rate)
    /// * `beta1` - Short-term component
    /// * `beta2` - Medium-term component (hump)
    /// * `tau` - Decay factor (must be positive)
    ///
    /// # Errors
    ///
    /// Returns an error if tau is not positive.
    pub fn new(beta0: f64, beta1: f64, beta2: f64, tau: f64) -> MathResult<Self> {
        if tau <= 0.0 {
            return Err(MathError::invalid_input(format!(
                "tau must be positive, got {tau}"
            )));
        }

        Ok(Self {
            beta0,
            beta1,
            beta2,
            tau,
        })
    }

    /// Returns the instantaneous forward rate at time t.
    ///
    /// ```text
    /// f(t) = β₀ + β₁ * e^(-t/τ) + β₂ * (t/τ) * e^(-t/τ)
    /// ```
    pub fn forward_rate(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return self.beta0 + self.beta1;
        }

        let x = t / self.tau;
        let exp_x = (-x).exp();

        self.beta0 + self.beta1 * exp_x + self.beta2 * x * exp_x
    }

    /// Returns the model parameters as (β₀, β₁, β₂, τ).
    pub fn parameters(&self) -> (f64, f64, f64, f64) {
        (self.beta0, self.beta1, self.beta2, self.tau)
    }

    /// Helper function: (1 - e^(-x)) / x
    fn loading_factor_1(x: f64) -> f64 {
        if x.abs() < 1e-10 {
            1.0 - x / 2.0 + x * x / 6.0 // Taylor expansion for numerical stability
        } else {
            (1.0 - (-x).exp()) / x
        }
    }

    /// Helper function: (1 - e^(-x)) / x - e^(-x)
    fn loading_factor_2(x: f64) -> f64 {
        if x.abs() < 1e-10 {
            x / 2.0 - x * x / 3.0 // Taylor expansion for numerical stability
        } else {
            Self::loading_factor_1(x) - (-x).exp()
        }
    }
}

impl Interpolator for NelsonSiegel {
    fn interpolate(&self, t: f64) -> MathResult<f64> {
        if t <= 0.0 {
            // As t → 0, z(t) → β₀ + β₁
            return Ok(self.beta0 + self.beta1);
        }

        let x = t / self.tau;

        let z = self.beta0
            + self.beta1 * Self::loading_factor_1(x)
            + self.beta2 * Self::loading_factor_2(x);

        Ok(z)
    }

    fn derivative(&self, t: f64) -> MathResult<f64> {
        if t <= 0.0 {
            return Ok(0.0);
        }

        let x = t / self.tau;
        let exp_x = (-x).exp();

        // dz/dt = (1/τ) * [β₁ * d(L1)/dx + β₂ * d(L2)/dx]
        // where L1 = (1 - e^(-x))/x and L2 = L1 - e^(-x)

        // d(L1)/dx = (e^(-x) - L1) / x
        let l1 = Self::loading_factor_1(x);
        let dl1 = (exp_x - l1) / x;

        // d(L2)/dx = d(L1)/dx + e^(-x)
        let dl2 = dl1 + exp_x;

        let dz_dx = self.beta1 * dl1 + self.beta2 * dl2;

        Ok(dz_dx / self.tau)
    }

    fn allows_extrapolation(&self) -> bool {
        true // Parametric model works for any t > 0
    }

    fn min_x(&self) -> f64 {
        0.0
    }

    fn max_x(&self) -> f64 {
        f64::INFINITY
    }
}

/// Svensson yield curve model.
///
/// An extension of Nelson-Siegel with an additional hump term:
/// ```text
/// z(t) = β₀ + β₁ * ((1 - e^(-t/τ₁)) / (t/τ₁))
///           + β₂ * ((1 - e^(-t/τ₁)) / (t/τ₁) - e^(-t/τ₁))
///           + β₃ * ((1 - e^(-t/τ₂)) / (t/τ₂) - e^(-t/τ₂))
/// ```
///
/// The extra term (β₃, τ₂) allows for a second hump in the curve,
/// providing more flexibility for fitting complex curve shapes.
///
/// # Example
///
/// ```rust
/// use convex_math::interpolation::{Svensson, Interpolator};
///
/// // Create a curve with two humps
/// let sv = Svensson::new(
///     0.045,  // β₀: 4.5% long rate
///     -0.02,  // β₁: upward slope
///     0.01,   // β₂: first hump
///     -0.005, // β₃: second (negative) hump
///     2.0,    // τ₁: 2 years for first hump
///     8.0,    // τ₂: 8 years for second hump
/// ).unwrap();
///
/// let rate = sv.interpolate(5.0).unwrap();
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Svensson {
    /// Long-term level
    beta0: f64,
    /// Short-term component
    beta1: f64,
    /// First hump component
    beta2: f64,
    /// Second hump component
    beta3: f64,
    /// First decay factor
    tau1: f64,
    /// Second decay factor
    tau2: f64,
}

impl Svensson {
    /// Creates a new Svensson curve.
    ///
    /// # Arguments
    ///
    /// * `beta0` - Long-term level
    /// * `beta1` - Short-term component
    /// * `beta2` - First hump component
    /// * `beta3` - Second hump component
    /// * `tau1` - First decay factor (must be positive)
    /// * `tau2` - Second decay factor (must be positive)
    ///
    /// # Errors
    ///
    /// Returns an error if either tau is not positive.
    pub fn new(
        beta0: f64,
        beta1: f64,
        beta2: f64,
        beta3: f64,
        tau1: f64,
        tau2: f64,
    ) -> MathResult<Self> {
        if tau1 <= 0.0 {
            return Err(MathError::invalid_input(format!(
                "tau1 must be positive, got {tau1}"
            )));
        }
        if tau2 <= 0.0 {
            return Err(MathError::invalid_input(format!(
                "tau2 must be positive, got {tau2}"
            )));
        }

        Ok(Self {
            beta0,
            beta1,
            beta2,
            beta3,
            tau1,
            tau2,
        })
    }

    /// Returns the instantaneous forward rate at time t.
    pub fn forward_rate(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return self.beta0 + self.beta1;
        }

        let x1 = t / self.tau1;
        let x2 = t / self.tau2;
        let exp_x1 = (-x1).exp();
        let exp_x2 = (-x2).exp();

        self.beta0 + self.beta1 * exp_x1 + self.beta2 * x1 * exp_x1 + self.beta3 * x2 * exp_x2
    }

    /// Returns the model parameters as (β₀, β₁, β₂, β₃, τ₁, τ₂).
    pub fn parameters(&self) -> (f64, f64, f64, f64, f64, f64) {
        (
            self.beta0, self.beta1, self.beta2, self.beta3, self.tau1, self.tau2,
        )
    }

    /// Helper function: (1 - e^(-x)) / x
    fn loading_factor_1(x: f64) -> f64 {
        if x.abs() < 1e-10 {
            1.0 - x / 2.0 + x * x / 6.0
        } else {
            (1.0 - (-x).exp()) / x
        }
    }

    /// Helper function: (1 - e^(-x)) / x - e^(-x)
    fn loading_factor_2(x: f64) -> f64 {
        if x.abs() < 1e-10 {
            x / 2.0 - x * x / 3.0
        } else {
            Self::loading_factor_1(x) - (-x).exp()
        }
    }
}

impl Interpolator for Svensson {
    fn interpolate(&self, t: f64) -> MathResult<f64> {
        if t <= 0.0 {
            return Ok(self.beta0 + self.beta1);
        }

        let x1 = t / self.tau1;
        let x2 = t / self.tau2;

        let z = self.beta0
            + self.beta1 * Self::loading_factor_1(x1)
            + self.beta2 * Self::loading_factor_2(x1)
            + self.beta3 * Self::loading_factor_2(x2);

        Ok(z)
    }

    fn derivative(&self, t: f64) -> MathResult<f64> {
        if t <= 0.0 {
            return Ok(0.0);
        }

        let x1 = t / self.tau1;
        let x2 = t / self.tau2;
        let exp_x1 = (-x1).exp();
        let exp_x2 = (-x2).exp();

        // Derivatives of loading factors
        let l1_1 = Self::loading_factor_1(x1);
        let dl1_1 = (exp_x1 - l1_1) / x1;
        let dl2_1 = dl1_1 + exp_x1;

        let l1_2 = Self::loading_factor_1(x2);
        let dl1_2 = (exp_x2 - l1_2) / x2;
        let dl2_2 = dl1_2 + exp_x2;

        let dz_dt = self.beta1 * dl1_1 / self.tau1
            + self.beta2 * dl2_1 / self.tau1
            + self.beta3 * dl2_2 / self.tau2;

        Ok(dz_dt)
    }

    fn allows_extrapolation(&self) -> bool {
        true
    }

    fn min_x(&self) -> f64 {
        0.0
    }

    fn max_x(&self) -> f64 {
        f64::INFINITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    // ============ Nelson-Siegel Tests ============

    #[test]
    fn test_nelson_siegel_asymptotic() {
        let ns = NelsonSiegel::new(0.045, -0.02, 0.01, 2.0).unwrap();

        // As t → ∞, z(t) → β₀
        let long_rate = ns.interpolate(100.0).unwrap();
        assert_relative_eq!(long_rate, 0.045, epsilon = 0.001);
    }

    #[test]
    fn test_nelson_siegel_short_rate() {
        let ns = NelsonSiegel::new(0.045, -0.02, 0.01, 2.0).unwrap();

        // At t → 0, z(t) → β₀ + β₁
        let short_rate = ns.interpolate(0.001).unwrap();
        assert_relative_eq!(short_rate, 0.045 - 0.02, epsilon = 0.01);
    }

    #[test]
    fn test_nelson_siegel_upward_slope() {
        // β₁ < 0 creates upward sloping curve
        let ns = NelsonSiegel::new(0.045, -0.02, 0.0, 2.0).unwrap();

        let r_short = ns.interpolate(0.5).unwrap();
        let r_long = ns.interpolate(10.0).unwrap();

        assert!(r_short < r_long);
    }

    #[test]
    fn test_nelson_siegel_hump() {
        // β₂ > 0 creates a hump
        let ns = NelsonSiegel::new(0.03, 0.0, 0.02, 2.0).unwrap();

        let r_short = ns.interpolate(0.5).unwrap();
        let r_mid = ns.interpolate(2.0).unwrap();
        let r_long = ns.interpolate(20.0).unwrap();

        // Hump: mid-term rate should be highest
        assert!(r_mid > r_short);
        assert!(r_mid > r_long);
    }

    #[test]
    fn test_nelson_siegel_forward_rate() {
        let ns = NelsonSiegel::new(0.045, -0.02, 0.01, 2.0).unwrap();

        // Forward rate should converge to β₀
        let f_long = ns.forward_rate(100.0);
        assert_relative_eq!(f_long, 0.045, epsilon = 0.001);

        // Short forward is β₀ + β₁
        let f_short = ns.forward_rate(0.0);
        assert_relative_eq!(f_short, 0.025, epsilon = 0.001);
    }

    #[test]
    fn test_nelson_siegel_derivative() {
        let ns = NelsonSiegel::new(0.045, -0.02, 0.01, 2.0).unwrap();

        // Numerical derivative check
        let t = 3.0;
        let h = 1e-6;

        let z_plus = ns.interpolate(t + h).unwrap();
        let z_minus = ns.interpolate(t - h).unwrap();
        let numerical = (z_plus - z_minus) / (2.0 * h);

        let analytical = ns.derivative(t).unwrap();

        assert_relative_eq!(analytical, numerical, epsilon = 1e-6);
    }

    #[test]
    fn test_nelson_siegel_invalid_tau() {
        assert!(NelsonSiegel::new(0.045, -0.02, 0.01, 0.0).is_err());
        assert!(NelsonSiegel::new(0.045, -0.02, 0.01, -1.0).is_err());
    }

    // ============ Svensson Tests ============

    #[test]
    fn test_svensson_asymptotic() {
        let sv = Svensson::new(0.045, -0.02, 0.01, -0.005, 2.0, 8.0).unwrap();

        // As t → ∞, z(t) → β₀
        let long_rate = sv.interpolate(100.0).unwrap();
        assert_relative_eq!(long_rate, 0.045, epsilon = 0.001);
    }

    #[test]
    fn test_svensson_short_rate() {
        let sv = Svensson::new(0.045, -0.02, 0.01, -0.005, 2.0, 8.0).unwrap();

        // At t → 0, z(t) → β₀ + β₁
        let short_rate = sv.interpolate(0.001).unwrap();
        assert_relative_eq!(short_rate, 0.045 - 0.02, epsilon = 0.01);
    }

    #[test]
    fn test_svensson_reduces_to_nelson_siegel() {
        // With β₃ = 0, Svensson should equal Nelson-Siegel
        let ns = NelsonSiegel::new(0.045, -0.02, 0.01, 2.0).unwrap();
        let sv = Svensson::new(0.045, -0.02, 0.01, 0.0, 2.0, 5.0).unwrap();

        for t in [0.5, 1.0, 2.0, 5.0, 10.0] {
            let ns_rate = ns.interpolate(t).unwrap();
            let sv_rate = sv.interpolate(t).unwrap();
            assert_relative_eq!(ns_rate, sv_rate, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_svensson_two_humps() {
        // β₂ and β₃ with opposite signs can create two humps
        let sv = Svensson::new(0.03, 0.0, 0.02, -0.015, 2.0, 8.0).unwrap();

        let r_1y = sv.interpolate(1.0).unwrap();
        let r_2y = sv.interpolate(2.0).unwrap();
        let r_5y = sv.interpolate(5.0).unwrap();
        let r_10y = sv.interpolate(10.0).unwrap();

        // First hump around 2 years
        assert!(r_2y > r_1y);

        // Dip in the middle due to negative β₃
        assert!(r_5y < r_2y);

        // Eventually converges to long-term rate (just verify it exists)
        assert!(r_10y > 0.0);
    }

    #[test]
    fn test_svensson_derivative() {
        let sv = Svensson::new(0.045, -0.02, 0.01, -0.005, 2.0, 8.0).unwrap();

        let t = 5.0;
        let h = 1e-6;

        let z_plus = sv.interpolate(t + h).unwrap();
        let z_minus = sv.interpolate(t - h).unwrap();
        let numerical = (z_plus - z_minus) / (2.0 * h);

        let analytical = sv.derivative(t).unwrap();

        assert_relative_eq!(analytical, numerical, epsilon = 1e-5);
    }

    #[test]
    fn test_svensson_invalid_tau() {
        assert!(Svensson::new(0.045, -0.02, 0.01, -0.005, 0.0, 8.0).is_err());
        assert!(Svensson::new(0.045, -0.02, 0.01, -0.005, 2.0, -1.0).is_err());
    }

    #[test]
    fn test_svensson_forward_rate() {
        let sv = Svensson::new(0.045, -0.02, 0.01, -0.005, 2.0, 8.0).unwrap();

        // Forward rate should converge to β₀
        let f_long = sv.forward_rate(100.0);
        assert_relative_eq!(f_long, 0.045, epsilon = 0.001);
    }

    // ============ Common Trait Tests ============

    #[test]
    fn test_parametric_allows_extrapolation() {
        let ns = NelsonSiegel::new(0.045, -0.02, 0.01, 2.0).unwrap();
        let sv = Svensson::new(0.045, -0.02, 0.01, -0.005, 2.0, 8.0).unwrap();

        assert!(ns.allows_extrapolation());
        assert!(sv.allows_extrapolation());

        // Should work for any t > 0
        assert!(ns.interpolate(50.0).is_ok());
        assert!(sv.interpolate(50.0).is_ok());
    }
}
