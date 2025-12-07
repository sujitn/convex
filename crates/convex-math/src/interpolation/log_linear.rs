//! Log-linear interpolation.
//!
//! Interpolates the logarithm of values, which is useful for discount factors
//! as it ensures positive values and can produce more stable forward rates.

use crate::error::{MathError, MathResult};
use crate::interpolation::Interpolator;

/// Log-linear interpolation between data points.
///
/// Interpolates the natural logarithm of y values, then exponentiates the result.
/// This is commonly used for discount factor interpolation as it:
/// - Guarantees positive interpolated values
/// - Produces piecewise constant forward rates
///
/// The interpolation formula is:
/// ```text
/// y(x) = exp(linear_interpolate(x, ln(y)))
/// ```
///
/// # Example
///
/// ```rust
/// use convex_math::interpolation::{LogLinearInterpolator, Interpolator};
///
/// // Discount factors at different maturities
/// let times = vec![0.0, 1.0, 2.0, 3.0];
/// let discount_factors = vec![1.0, 0.97, 0.94, 0.91];
///
/// let interp = LogLinearInterpolator::new(times, discount_factors).unwrap();
/// let df = interp.interpolate(1.5).unwrap();
/// assert!(df > 0.0);  // Always positive
/// ```
#[derive(Debug, Clone)]
pub struct LogLinearInterpolator {
    xs: Vec<f64>,
    ys: Vec<f64>,
    /// Precomputed log(y) values
    log_ys: Vec<f64>,
    allow_extrapolation: bool,
}

impl LogLinearInterpolator {
    /// Creates a new log-linear interpolator.
    ///
    /// # Arguments
    ///
    /// * `xs` - X coordinates (must be sorted in ascending order)
    /// * `ys` - Y coordinates (must all be positive)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - There are fewer than 2 points
    /// - Lengths differ
    /// - Any y value is non-positive
    pub fn new(xs: Vec<f64>, ys: Vec<f64>) -> MathResult<Self> {
        if xs.len() < 2 {
            return Err(MathError::insufficient_data(2, xs.len()));
        }
        if xs.len() != ys.len() {
            return Err(MathError::invalid_input(format!(
                "xs and ys must have same length: {} vs {}",
                xs.len(),
                ys.len()
            )));
        }

        // Check that xs are sorted
        for i in 1..xs.len() {
            if xs[i] <= xs[i - 1] {
                return Err(MathError::invalid_input(
                    "x values must be strictly increasing",
                ));
            }
        }

        // Check that all y values are positive and compute log
        let mut log_ys = Vec::with_capacity(ys.len());
        for (i, &y) in ys.iter().enumerate() {
            if y <= 0.0 {
                return Err(MathError::invalid_input(format!(
                    "y[{i}] = {y} is not positive; log-linear requires positive values"
                )));
            }
            log_ys.push(y.ln());
        }

        Ok(Self {
            xs,
            ys,
            log_ys,
            allow_extrapolation: false,
        })
    }

    /// Enables extrapolation beyond the data range.
    #[must_use]
    pub fn with_extrapolation(mut self) -> Self {
        self.allow_extrapolation = true;
        self
    }

    /// Finds the index i such that xs[i] <= x < xs[i+1].
    fn find_segment(&self, x: f64) -> usize {
        match self
            .xs
            .binary_search_by(|probe| probe.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal))
        {
            Ok(i) => i.min(self.xs.len() - 2),
            Err(i) => (i.saturating_sub(1)).min(self.xs.len() - 2),
        }
    }

    /// Returns the original y values.
    #[must_use]
    pub fn y_values(&self) -> &[f64] {
        &self.ys
    }
}

impl Interpolator for LogLinearInterpolator {
    fn interpolate(&self, x: f64) -> MathResult<f64> {
        // Check bounds
        if !self.allow_extrapolation && (x < self.xs[0] || x > self.xs[self.xs.len() - 1]) {
            return Err(MathError::ExtrapolationNotAllowed {
                x,
                min: self.xs[0],
                max: self.xs[self.xs.len() - 1],
            });
        }

        let i = self.find_segment(x);

        let x0 = self.xs[i];
        let x1 = self.xs[i + 1];
        let log_y0 = self.log_ys[i];
        let log_y1 = self.log_ys[i + 1];

        // Linear interpolation on log values
        let t = (x - x0) / (x1 - x0);
        let log_y = log_y0 + t * (log_y1 - log_y0);

        Ok(log_y.exp())
    }

    fn derivative(&self, x: f64) -> MathResult<f64> {
        // Check bounds
        if !self.allow_extrapolation && (x < self.xs[0] || x > self.xs[self.xs.len() - 1]) {
            return Err(MathError::ExtrapolationNotAllowed {
                x,
                min: self.xs[0],
                max: self.xs[self.xs.len() - 1],
            });
        }

        let i = self.find_segment(x);

        let x0 = self.xs[i];
        let x1 = self.xs[i + 1];
        let log_y0 = self.log_ys[i];
        let log_y1 = self.log_ys[i + 1];

        // y(x) = exp(log_y0 + t * (log_y1 - log_y0))
        // dy/dx = y(x) * d(log_y)/dx = y(x) * (log_y1 - log_y0) / (x1 - x0)
        let t = (x - x0) / (x1 - x0);
        let log_y = log_y0 + t * (log_y1 - log_y0);
        let y = log_y.exp();
        let d_log_y = (log_y1 - log_y0) / (x1 - x0);

        Ok(y * d_log_y)
    }

    fn allows_extrapolation(&self) -> bool {
        self.allow_extrapolation
    }

    fn min_x(&self) -> f64 {
        self.xs[0]
    }

    fn max_x(&self) -> f64 {
        self.xs[self.xs.len() - 1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_log_linear_through_points() {
        let xs = vec![0.0, 1.0, 2.0, 3.0];
        let ys = vec![1.0, 0.97, 0.94, 0.91];

        let interp = LogLinearInterpolator::new(xs.clone(), ys.clone()).unwrap();

        // Should pass through all data points
        for (x, y) in xs.iter().zip(ys.iter()) {
            assert_relative_eq!(interp.interpolate(*x).unwrap(), *y, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_log_linear_positive_values() {
        let xs = vec![0.0, 1.0, 2.0, 3.0];
        let ys = vec![1.0, 0.5, 0.25, 0.125];

        let interp = LogLinearInterpolator::new(xs, ys).unwrap();

        // All interpolated values should be positive
        for x in [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0] {
            let y = interp.interpolate(x).unwrap();
            assert!(y > 0.0, "y({}) = {} should be positive", x, y);
        }
    }

    #[test]
    fn test_log_linear_exponential_decay() {
        // For y = exp(-r*t), log-linear should exactly reproduce
        let r: f64 = 0.05;
        let xs = vec![0.0, 1.0, 2.0, 3.0];
        let ys: Vec<f64> = xs.iter().map(|&t: &f64| (-r * t).exp()).collect();

        let interp = LogLinearInterpolator::new(xs, ys).unwrap();

        // Check at intermediate point
        let t = 1.5;
        let expected = (-r * t).exp();
        assert_relative_eq!(interp.interpolate(t).unwrap(), expected, epsilon = 1e-10);
    }

    #[test]
    fn test_log_linear_derivative() {
        // For y = exp(-r*t), dy/dt = -r * exp(-r*t)
        let r: f64 = 0.05;
        let xs = vec![0.0, 1.0, 2.0, 3.0];
        let ys: Vec<f64> = xs.iter().map(|&t: &f64| (-r * t).exp()).collect();

        let interp = LogLinearInterpolator::new(xs, ys).unwrap();

        let t = 1.5;
        let expected_derivative = -r * (-r * t).exp();
        assert_relative_eq!(
            interp.derivative(t).unwrap(),
            expected_derivative,
            epsilon = 1e-10
        );
    }

    #[test]
    fn test_log_linear_rejects_non_positive() {
        let xs = vec![0.0, 1.0, 2.0];
        let ys = vec![1.0, 0.0, -1.0]; // Contains non-positive values

        assert!(LogLinearInterpolator::new(xs, ys).is_err());
    }

    #[test]
    fn test_log_linear_extrapolation_disabled() {
        let xs = vec![0.0, 1.0, 2.0];
        let ys = vec![1.0, 0.9, 0.8];

        let interp = LogLinearInterpolator::new(xs, ys).unwrap();

        assert!(interp.interpolate(-0.5).is_err());
        assert!(interp.interpolate(2.5).is_err());
    }

    #[test]
    fn test_log_linear_extrapolation_enabled() {
        let xs = vec![0.0, 1.0, 2.0];
        let ys = vec![1.0, 0.9, 0.81];

        let interp = LogLinearInterpolator::new(xs, ys)
            .unwrap()
            .with_extrapolation();

        // Should allow extrapolation and produce positive values
        let y_left = interp.interpolate(-0.5).unwrap();
        let y_right = interp.interpolate(2.5).unwrap();

        assert!(y_left > 0.0);
        assert!(y_right > 0.0);
    }

    #[test]
    fn test_log_linear_discount_factors() {
        // Realistic discount factor curve
        let times = vec![0.25, 0.5, 1.0, 2.0, 3.0, 5.0];
        let dfs = vec![0.9975, 0.9950, 0.9901, 0.9802, 0.9706, 0.9512];

        let interp = LogLinearInterpolator::new(times.clone(), dfs.clone()).unwrap();

        // Interpolated values should be monotonically decreasing
        let mut prev = interp.interpolate(times[0]).unwrap();
        for t in [0.3, 0.75, 1.5, 2.5, 4.0] {
            let current = interp.interpolate(t).unwrap();
            assert!(
                current < prev,
                "DF should decrease: DF({}) = {} should be < {}",
                t,
                current,
                prev
            );
            prev = current;
        }
    }
}
