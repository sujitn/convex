//! Flat forward interpolation.
//!
//! Flat forward interpolation assumes constant forward rates between pillar points.
//! This is a common choice for yield curve construction as it:
//! - Guarantees positive forward rates (if zero rates are positive)
//! - Produces step-function forward rate curves
//! - Is computationally efficient
//!
//! # Mathematical Background
//!
//! Given zero rates r(t) at pillar points t_i, the forward rate f_i between
//! t_i and t_{i+1} is:
//!
//! ```text
//! f_i = (r_{i+1} * t_{i+1} - r_i * t_i) / (t_{i+1} - t_i)
//! ```
//!
//! For t between t_i and t_{i+1}, the interpolated zero rate is:
//!
//! ```text
//! r(t) = (r_i * t_i + f_i * (t - t_i)) / t
//! ```
//!
//! This ensures the forward rate is constant (flat) within each segment.

use crate::error::{MathError, MathResult};
use crate::interpolation::Interpolator;

/// Flat forward interpolation for zero rate curves.
///
/// Interpolates zero rates such that forward rates are constant (flat)
/// between pillar points. This produces a step-function forward curve.
///
/// # Example
///
/// ```rust
/// use convex_math::interpolation::{FlatForward, Interpolator};
///
/// // Zero rates at 1Y, 2Y, 5Y, 10Y
/// let tenors = vec![1.0, 2.0, 5.0, 10.0];
/// let zero_rates = vec![0.02, 0.025, 0.03, 0.035];
///
/// let interp = FlatForward::new(tenors, zero_rates).unwrap();
///
/// // Interpolate at 3Y - forward rate is flat between 2Y and 5Y
/// let rate_3y = interp.interpolate(3.0).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct FlatForward {
    /// Tenors (time points) in years
    tenors: Vec<f64>,
    /// Zero rates at each tenor
    zero_rates: Vec<f64>,
    /// Pre-computed forward rates for each segment
    forward_rates: Vec<f64>,
    /// Allow extrapolation beyond data range
    allow_extrapolation: bool,
}

impl FlatForward {
    /// Creates a new flat forward interpolator from zero rates.
    ///
    /// # Arguments
    ///
    /// * `tenors` - Time points in years (must be strictly increasing, > 0)
    /// * `zero_rates` - Zero rates at each tenor (as decimals, e.g., 0.05 for 5%)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Fewer than 2 points are provided
    /// - Tenors and zero_rates have different lengths
    /// - Tenors are not strictly increasing
    /// - Any tenor is <= 0
    pub fn new(tenors: Vec<f64>, zero_rates: Vec<f64>) -> MathResult<Self> {
        if tenors.len() < 2 {
            return Err(MathError::insufficient_data(2, tenors.len()));
        }
        if tenors.len() != zero_rates.len() {
            return Err(MathError::invalid_input(format!(
                "tenors and zero_rates must have same length: {} vs {}",
                tenors.len(),
                zero_rates.len()
            )));
        }

        // Check that tenors are positive and strictly increasing
        if tenors[0] <= 0.0 {
            return Err(MathError::invalid_input(
                "First tenor must be positive for flat forward interpolation",
            ));
        }
        for i in 1..tenors.len() {
            if tenors[i] <= tenors[i - 1] {
                return Err(MathError::invalid_input(
                    "Tenors must be strictly increasing",
                ));
            }
        }

        // Pre-compute forward rates for each segment
        let forward_rates = Self::compute_forward_rates(&tenors, &zero_rates);

        Ok(Self {
            tenors,
            zero_rates,
            forward_rates,
            allow_extrapolation: false,
        })
    }

    /// Creates a flat forward interpolator with an initial point at t=0.
    ///
    /// This variant allows interpolation from t=0 by assuming the first
    /// zero rate extends back to the origin.
    ///
    /// # Arguments
    ///
    /// * `tenors` - Time points in years (must be strictly increasing, >= 0)
    /// * `zero_rates` - Zero rates at each tenor
    pub fn with_origin(mut tenors: Vec<f64>, mut zero_rates: Vec<f64>) -> MathResult<Self> {
        if tenors.is_empty() {
            return Err(MathError::insufficient_data(1, 0));
        }

        // If first tenor is not 0, prepend origin point
        if tenors[0] > 0.0 {
            // Use first zero rate for the origin (flat from 0 to first pillar)
            tenors.insert(0, 0.0);
            zero_rates.insert(0, zero_rates[0]);
        }

        // Now call the standard constructor with tenors starting at 0
        // We need to handle t=0 specially
        if tenors.len() < 2 {
            return Err(MathError::insufficient_data(2, tenors.len()));
        }
        if tenors.len() != zero_rates.len() {
            return Err(MathError::invalid_input(format!(
                "tenors and zero_rates must have same length: {} vs {}",
                tenors.len(),
                zero_rates.len()
            )));
        }

        for i in 1..tenors.len() {
            if tenors[i] <= tenors[i - 1] {
                return Err(MathError::invalid_input(
                    "Tenors must be strictly increasing",
                ));
            }
        }

        let forward_rates = Self::compute_forward_rates(&tenors, &zero_rates);

        Ok(Self {
            tenors,
            zero_rates,
            forward_rates,
            allow_extrapolation: false,
        })
    }

    /// Enables extrapolation beyond the data range.
    ///
    /// When extrapolating:
    /// - Below first tenor: uses first forward rate
    /// - Above last tenor: uses last forward rate (flat forward extension)
    #[must_use]
    pub fn with_extrapolation(mut self) -> Self {
        self.allow_extrapolation = true;
        self
    }

    /// Computes forward rates for each segment.
    ///
    /// Forward rate from t_i to t_{i+1}:
    /// f_i = (r_{i+1} * t_{i+1} - r_i * t_i) / (t_{i+1} - t_i)
    fn compute_forward_rates(tenors: &[f64], zero_rates: &[f64]) -> Vec<f64> {
        let n = tenors.len();
        let mut forwards = Vec::with_capacity(n);

        for i in 0..n - 1 {
            let t0 = tenors[i];
            let t1 = tenors[i + 1];
            let r0 = zero_rates[i];
            let r1 = zero_rates[i + 1];

            // Handle t0 = 0 case
            let fwd = if t0 == 0.0 {
                // Forward from 0 to t1 is just r1 (since r0 * 0 = 0)
                r1
            } else {
                (r1 * t1 - r0 * t0) / (t1 - t0)
            };

            forwards.push(fwd);
        }

        // Last segment uses flat forward extension
        if !forwards.is_empty() {
            forwards.push(*forwards.last().unwrap());
        } else {
            forwards.push(zero_rates[0]);
        }

        forwards
    }

    /// Finds the segment index for a given tenor.
    ///
    /// Returns i such that tenors[i] <= t < tenors[i+1]
    fn find_segment(&self, t: f64) -> usize {
        match self
            .tenors
            .binary_search_by(|probe| probe.partial_cmp(&t).unwrap_or(std::cmp::Ordering::Equal))
        {
            Ok(i) => i.min(self.tenors.len() - 2),
            Err(i) => (i.saturating_sub(1)).min(self.tenors.len() - 2),
        }
    }

    /// Returns the forward rate at tenor t.
    ///
    /// Since forward rates are flat between pillars, this returns the
    /// constant forward rate for the segment containing t.
    pub fn forward_rate(&self, t: f64) -> MathResult<f64> {
        if !self.allow_extrapolation && (t < self.tenors[0] || t > *self.tenors.last().unwrap()) {
            return Err(MathError::ExtrapolationNotAllowed {
                x: t,
                min: self.tenors[0],
                max: *self.tenors.last().unwrap(),
            });
        }

        let i = self.find_segment(t);
        Ok(self.forward_rates[i])
    }

    /// Returns the tenors.
    pub fn tenors(&self) -> &[f64] {
        &self.tenors
    }

    /// Returns the zero rates.
    pub fn zero_rates(&self) -> &[f64] {
        &self.zero_rates
    }

    /// Returns the pre-computed forward rates.
    pub fn forward_rates_vec(&self) -> &[f64] {
        &self.forward_rates
    }
}

impl Interpolator for FlatForward {
    fn interpolate(&self, t: f64) -> MathResult<f64> {
        let min_t = self.tenors[0];
        let max_t = *self.tenors.last().unwrap();

        // Check bounds
        if !self.allow_extrapolation && (t < min_t || t > max_t) {
            return Err(MathError::ExtrapolationNotAllowed {
                x: t,
                min: min_t,
                max: max_t,
            });
        }

        // Handle t <= 0 edge case
        if t <= 0.0 {
            // Return first zero rate for t=0 or negative (if extrapolating)
            return Ok(self.zero_rates[0]);
        }

        // Handle exact pillar hits
        if let Some(idx) = self.tenors.iter().position(|&x| (x - t).abs() < 1e-12) {
            return Ok(self.zero_rates[idx]);
        }

        // Handle extrapolation below first pillar
        if t < min_t {
            // Use first forward rate to extrapolate backward
            // r(t) = r_0 * t_0 / t + f_0 * (t - 0) / t = (r_0 * t_0 + f_0 * t) / t
            // But simpler: just use first zero rate (flat backward)
            return Ok(self.zero_rates[0]);
        }

        // Handle extrapolation above last pillar
        if t > max_t {
            // Flat forward extension: use last forward rate
            let n = self.tenors.len();
            let t_n = self.tenors[n - 1];
            let r_n = self.zero_rates[n - 1];
            let f_n = self.forward_rates[n - 1];

            // r(t) = (r_n * t_n + f_n * (t - t_n)) / t
            return Ok((r_n * t_n + f_n * (t - t_n)) / t);
        }

        // Normal interpolation between pillars
        let i = self.find_segment(t);
        let t_i = self.tenors[i];
        let r_i = self.zero_rates[i];
        let f_i = self.forward_rates[i];

        // r(t) = (r_i * t_i + f_i * (t - t_i)) / t
        Ok((r_i * t_i + f_i * (t - t_i)) / t)
    }

    fn derivative(&self, t: f64) -> MathResult<f64> {
        let min_t = self.tenors[0];
        let max_t = *self.tenors.last().unwrap();

        // Check bounds
        if !self.allow_extrapolation && (t < min_t || t > max_t) {
            return Err(MathError::ExtrapolationNotAllowed {
                x: t,
                min: min_t,
                max: max_t,
            });
        }

        if t <= 0.0 {
            return Ok(0.0); // Derivative at origin
        }

        // Find segment
        let i = if t > max_t {
            self.tenors.len() - 2
        } else if t < min_t {
            0
        } else {
            self.find_segment(t)
        };

        let t_i = self.tenors[i];
        let r_i = self.zero_rates[i];
        let f_i = self.forward_rates[i];

        // r(t) = (r_i * t_i + f_i * (t - t_i)) / t
        //      = r_i * t_i / t + f_i * (t - t_i) / t
        //      = r_i * t_i / t + f_i - f_i * t_i / t
        //      = (r_i * t_i - f_i * t_i) / t + f_i
        //      = (r_i - f_i) * t_i / t + f_i
        //
        // dr/dt = -(r_i - f_i) * t_i / t^2
        //       = (f_i - r_i) * t_i / t^2

        Ok((f_i - r_i) * t_i / (t * t))
    }

    fn allows_extrapolation(&self) -> bool {
        self.allow_extrapolation
    }

    fn min_x(&self) -> f64 {
        self.tenors[0]
    }

    fn max_x(&self) -> f64 {
        *self.tenors.last().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_flat_forward_through_pillars() {
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        let zero_rates = vec![0.02, 0.025, 0.03, 0.035];

        let interp = FlatForward::new(tenors.clone(), zero_rates.clone()).unwrap();

        // Should pass through all pillar points
        for (t, r) in tenors.iter().zip(zero_rates.iter()) {
            assert_relative_eq!(interp.interpolate(*t).unwrap(), *r, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_flat_forward_rates() {
        let tenors = vec![1.0, 2.0, 3.0];
        let zero_rates = vec![0.02, 0.03, 0.04];

        let interp = FlatForward::new(tenors, zero_rates).unwrap();

        // Forward rate from 1Y to 2Y:
        // f = (0.03 * 2 - 0.02 * 1) / (2 - 1) = (0.06 - 0.02) / 1 = 0.04
        assert_relative_eq!(interp.forward_rate(1.5).unwrap(), 0.04, epsilon = 1e-10);

        // Forward rate from 2Y to 3Y:
        // f = (0.04 * 3 - 0.03 * 2) / (3 - 2) = (0.12 - 0.06) / 1 = 0.06
        assert_relative_eq!(interp.forward_rate(2.5).unwrap(), 0.06, epsilon = 1e-10);
    }

    #[test]
    fn test_interpolation_between_pillars() {
        let tenors = vec![1.0, 2.0];
        let zero_rates = vec![0.02, 0.04];

        let interp = FlatForward::new(tenors, zero_rates).unwrap();

        // Forward rate = (0.04 * 2 - 0.02 * 1) / 1 = 0.06
        // At t = 1.5:
        // r(1.5) = (0.02 * 1 + 0.06 * 0.5) / 1.5 = (0.02 + 0.03) / 1.5 = 0.05 / 1.5 = 0.0333...
        let r_mid = interp.interpolate(1.5).unwrap();
        assert_relative_eq!(r_mid, 0.05 / 1.5, epsilon = 1e-10);
    }

    #[test]
    fn test_forward_rate_consistency() {
        // Verify that interpolated zero rates produce the expected forwards
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        let zero_rates = vec![0.02, 0.025, 0.03, 0.035];

        let interp = FlatForward::new(tenors.clone(), zero_rates.clone()).unwrap();

        // Check that forward rates are indeed constant within segments
        let f_segment_1 = interp.forward_rate(1.5).unwrap();
        assert_relative_eq!(
            interp.forward_rate(1.1).unwrap(),
            f_segment_1,
            epsilon = 1e-10
        );
        assert_relative_eq!(
            interp.forward_rate(1.9).unwrap(),
            f_segment_1,
            epsilon = 1e-10
        );

        // Verify forward rate calculation from zero rates
        // f(1Y, 2Y) = (r_2Y * 2 - r_1Y * 1) / (2 - 1)
        let expected_f = (0.025 * 2.0 - 0.02 * 1.0) / (2.0 - 1.0);
        assert_relative_eq!(f_segment_1, expected_f, epsilon = 1e-10);
    }

    #[test]
    fn test_positive_forward_rates() {
        // Upward sloping curve should have positive forwards
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        let zero_rates = vec![0.02, 0.025, 0.03, 0.035];

        let interp = FlatForward::new(tenors, zero_rates).unwrap();

        // All forwards should be positive
        for &f in interp.forward_rates_vec() {
            assert!(f > 0.0, "Forward rate {} should be positive", f);
        }
    }

    #[test]
    fn test_derivative_numerical() {
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        let zero_rates = vec![0.02, 0.025, 0.03, 0.035];

        let interp = FlatForward::new(tenors, zero_rates)
            .unwrap()
            .with_extrapolation();

        // Test derivative at several points
        for t in [1.5, 2.5, 4.0, 7.0] {
            let h = 1e-6;
            let r_plus = interp.interpolate(t + h).unwrap();
            let r_minus = interp.interpolate(t - h).unwrap();
            let numerical = (r_plus - r_minus) / (2.0 * h);
            let analytical = interp.derivative(t).unwrap();

            assert_relative_eq!(analytical, numerical, epsilon = 1e-5);
        }
    }

    #[test]
    fn test_extrapolation() {
        let tenors = vec![1.0, 2.0, 5.0];
        let zero_rates = vec![0.02, 0.025, 0.03];

        let interp = FlatForward::new(tenors, zero_rates)
            .unwrap()
            .with_extrapolation();

        // Should extrapolate beyond range
        assert!(interp.interpolate(0.5).is_ok());
        assert!(interp.interpolate(7.0).is_ok());

        // Extrapolation below first pillar uses first zero rate
        assert_relative_eq!(interp.interpolate(0.5).unwrap(), 0.02, epsilon = 1e-10);
    }

    #[test]
    fn test_no_extrapolation() {
        let tenors = vec![1.0, 2.0, 5.0];
        let zero_rates = vec![0.02, 0.025, 0.03];

        let interp = FlatForward::new(tenors, zero_rates).unwrap();

        // Should fail outside range
        assert!(interp.interpolate(0.5).is_err());
        assert!(interp.interpolate(7.0).is_err());
    }

    #[test]
    fn test_with_origin() {
        let tenors = vec![1.0, 2.0, 5.0];
        let zero_rates = vec![0.02, 0.025, 0.03];

        let interp = FlatForward::with_origin(tenors, zero_rates).unwrap();

        // Should allow interpolation from t=0
        assert!(interp.interpolate(0.0).is_ok());
        assert!(interp.interpolate(0.5).is_ok());
    }

    #[test]
    fn test_insufficient_points() {
        let tenors = vec![1.0];
        let zero_rates = vec![0.02];

        assert!(FlatForward::new(tenors, zero_rates).is_err());
    }

    #[test]
    fn test_mismatched_lengths() {
        let tenors = vec![1.0, 2.0, 3.0];
        let zero_rates = vec![0.02, 0.025];

        assert!(FlatForward::new(tenors, zero_rates).is_err());
    }

    #[test]
    fn test_non_positive_tenor() {
        let tenors = vec![0.0, 1.0, 2.0];
        let zero_rates = vec![0.02, 0.025, 0.03];

        // Should fail because first tenor is 0 (use with_origin instead)
        assert!(FlatForward::new(tenors, zero_rates).is_err());
    }

    #[test]
    fn test_flat_curve() {
        // Flat zero curve should have zero forward curve
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        let zero_rates = vec![0.03, 0.03, 0.03, 0.03];

        let interp = FlatForward::new(tenors.clone(), zero_rates).unwrap();

        // Forward rates should all be 0.03 (same as zero rate for flat curve)
        for &f in interp.forward_rates_vec() {
            assert_relative_eq!(f, 0.03, epsilon = 1e-10);
        }

        // Interpolated values should all be 0.03
        for t in [1.0, 1.5, 2.5, 4.0, 7.0, 10.0] {
            assert_relative_eq!(interp.interpolate(t).unwrap(), 0.03, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_inverted_curve() {
        // Inverted curve (downward sloping)
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        let zero_rates = vec![0.05, 0.04, 0.03, 0.025];

        let interp = FlatForward::new(tenors, zero_rates).unwrap();

        // Forward rates can be negative for inverted curve
        // Just verify it doesn't crash and produces reasonable values
        assert!(interp.interpolate(1.5).is_ok());
        assert!(interp.interpolate(3.0).is_ok());
    }
}
