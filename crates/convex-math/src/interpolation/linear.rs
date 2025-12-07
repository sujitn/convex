//! Linear interpolation.

use crate::error::{MathError, MathResult};
use crate::interpolation::Interpolator;

/// Linear interpolation between data points.
///
/// The simplest form of interpolation, connecting consecutive points
/// with straight lines.
///
/// # Example
///
/// ```rust
/// use convex_math::interpolation::{LinearInterpolator, Interpolator};
///
/// let xs = vec![0.0, 1.0, 2.0, 3.0];
/// let ys = vec![0.0, 1.0, 4.0, 9.0];
///
/// let interp = LinearInterpolator::new(xs, ys).unwrap();
/// let y = interp.interpolate(1.5).unwrap();
/// // y = 2.5 (linear interpolation between (1, 1) and (2, 4))
/// ```
#[derive(Debug, Clone)]
pub struct LinearInterpolator {
    xs: Vec<f64>,
    ys: Vec<f64>,
    allow_extrapolation: bool,
}

impl LinearInterpolator {
    /// Creates a new linear interpolator.
    ///
    /// # Arguments
    ///
    /// * `xs` - X coordinates (must be sorted in ascending order)
    /// * `ys` - Y coordinates
    ///
    /// # Errors
    ///
    /// Returns an error if there are fewer than 2 points or if lengths differ.
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

        Ok(Self {
            xs,
            ys,
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
        // Binary search
        match self
            .xs
            .binary_search_by(|probe| probe.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal))
        {
            Ok(i) => i.min(self.xs.len() - 2),
            Err(i) => (i.saturating_sub(1)).min(self.xs.len() - 2),
        }
    }
}

impl Interpolator for LinearInterpolator {
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
        let y0 = self.ys[i];
        let y1 = self.ys[i + 1];

        // Linear interpolation formula
        let t = (x - x0) / (x1 - x0);
        Ok(y0 + t * (y1 - y0))
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
        let y0 = self.ys[i];
        let y1 = self.ys[i + 1];

        // Derivative of linear interpolation is constant slope
        Ok((y1 - y0) / (x1 - x0))
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
    fn test_linear_interpolation() {
        let xs = vec![0.0, 1.0, 2.0];
        let ys = vec![0.0, 2.0, 4.0];

        let interp = LinearInterpolator::new(xs, ys).unwrap();

        // Test at exact points
        assert_relative_eq!(interp.interpolate(0.0).unwrap(), 0.0, epsilon = 1e-10);
        assert_relative_eq!(interp.interpolate(1.0).unwrap(), 2.0, epsilon = 1e-10);
        assert_relative_eq!(interp.interpolate(2.0).unwrap(), 4.0, epsilon = 1e-10);

        // Test interpolation
        assert_relative_eq!(interp.interpolate(0.5).unwrap(), 1.0, epsilon = 1e-10);
        assert_relative_eq!(interp.interpolate(1.5).unwrap(), 3.0, epsilon = 1e-10);
    }

    #[test]
    fn test_extrapolation_disabled() {
        let xs = vec![0.0, 1.0, 2.0];
        let ys = vec![0.0, 1.0, 2.0];

        let interp = LinearInterpolator::new(xs, ys).unwrap();

        assert!(interp.interpolate(-0.5).is_err());
        assert!(interp.interpolate(2.5).is_err());
    }

    #[test]
    fn test_extrapolation_enabled() {
        let xs = vec![0.0, 1.0, 2.0];
        let ys = vec![0.0, 1.0, 2.0];

        let interp = LinearInterpolator::new(xs, ys)
            .unwrap()
            .with_extrapolation();

        // Should extrapolate linearly
        assert_relative_eq!(interp.interpolate(-1.0).unwrap(), -1.0, epsilon = 1e-10);
        assert_relative_eq!(interp.interpolate(3.0).unwrap(), 3.0, epsilon = 1e-10);
    }

    #[test]
    fn test_insufficient_points() {
        let xs = vec![0.0];
        let ys = vec![1.0];

        assert!(LinearInterpolator::new(xs, ys).is_err());
    }

    #[test]
    fn test_unsorted_error() {
        let xs = vec![1.0, 0.0, 2.0]; // Not sorted
        let ys = vec![1.0, 0.0, 2.0];

        assert!(LinearInterpolator::new(xs, ys).is_err());
    }
}
