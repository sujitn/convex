//! Natural cubic spline interpolation.

use crate::error::{MathError, MathResult};
use crate::interpolation::Interpolator;

/// Natural cubic spline interpolation.
///
/// Constructs a smooth curve through data points using piecewise cubic
/// polynomials with continuous first and second derivatives.
///
/// "Natural" means the second derivative is zero at the endpoints.
///
/// # Example
///
/// ```rust
/// use convex_math::interpolation::{CubicSpline, Interpolator};
///
/// let xs = vec![0.0, 1.0, 2.0, 3.0];
/// let ys = vec![0.0, 1.0, 4.0, 9.0];
///
/// let spline = CubicSpline::new(xs, ys).unwrap();
/// let y = spline.interpolate(1.5).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct CubicSpline {
    xs: Vec<f64>,
    ys: Vec<f64>,
    /// Second derivatives at each knot
    y2s: Vec<f64>,
    allow_extrapolation: bool,
}

impl CubicSpline {
    /// Creates a natural cubic spline interpolator.
    ///
    /// # Arguments
    ///
    /// * `xs` - X coordinates (must be sorted in ascending order)
    /// * `ys` - Y coordinates
    ///
    /// # Errors
    ///
    /// Returns an error if there are fewer than 3 points or if lengths differ.
    pub fn new(xs: Vec<f64>, ys: Vec<f64>) -> MathResult<Self> {
        if xs.len() < 3 {
            return Err(MathError::insufficient_data(3, xs.len()));
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

        let y2s = compute_second_derivatives(&xs, &ys)?;

        Ok(Self {
            xs,
            ys,
            y2s,
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
        match self.xs.binary_search_by(|probe| {
            probe.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal)
        }) {
            Ok(i) => i.min(self.xs.len() - 2),
            Err(i) => (i.saturating_sub(1)).min(self.xs.len() - 2),
        }
    }
}

impl Interpolator for CubicSpline {
    fn interpolate(&self, x: f64) -> MathResult<f64> {
        // Check bounds
        if !self.allow_extrapolation {
            if x < self.xs[0] || x > self.xs[self.xs.len() - 1] {
                return Err(MathError::ExtrapolationNotAllowed {
                    x,
                    min: self.xs[0],
                    max: self.xs[self.xs.len() - 1],
                });
            }
        }

        let i = self.find_segment(x);

        let x_lo = self.xs[i];
        let x_hi = self.xs[i + 1];
        let y_lo = self.ys[i];
        let y_hi = self.ys[i + 1];
        let y2_lo = self.y2s[i];
        let y2_hi = self.y2s[i + 1];

        let h = x_hi - x_lo;
        let a = (x_hi - x) / h;
        let b = (x - x_lo) / h;

        // Cubic spline formula
        let y = a * y_lo
            + b * y_hi
            + ((a * a * a - a) * y2_lo + (b * b * b - b) * y2_hi) * (h * h) / 6.0;

        Ok(y)
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

/// Computes the second derivatives for natural cubic spline.
fn compute_second_derivatives(xs: &[f64], ys: &[f64]) -> MathResult<Vec<f64>> {
    let n = xs.len();
    let mut y2s = vec![0.0; n];
    let mut u = vec![0.0; n - 1];

    // Natural spline: y2[0] = 0
    y2s[0] = 0.0;
    u[0] = 0.0;

    // Decomposition loop
    for i in 1..n - 1 {
        let sig = (xs[i] - xs[i - 1]) / (xs[i + 1] - xs[i - 1]);
        let p = sig * y2s[i - 1] + 2.0;
        y2s[i] = (sig - 1.0) / p;
        u[i] = (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i])
            - (ys[i] - ys[i - 1]) / (xs[i] - xs[i - 1]);
        u[i] = (6.0 * u[i] / (xs[i + 1] - xs[i - 1]) - sig * u[i - 1]) / p;
    }

    // Natural spline: y2[n-1] = 0
    y2s[n - 1] = 0.0;

    // Back-substitution loop
    for i in (0..n - 1).rev() {
        y2s[i] = y2s[i] * y2s[i + 1] + u[i];
    }

    Ok(y2s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_cubic_spline_through_points() {
        let xs = vec![0.0, 1.0, 2.0, 3.0];
        let ys = vec![0.0, 1.0, 4.0, 9.0];

        let spline = CubicSpline::new(xs.clone(), ys.clone()).unwrap();

        // Should pass through all data points
        for (x, y) in xs.iter().zip(ys.iter()) {
            assert_relative_eq!(spline.interpolate(*x).unwrap(), *y, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_cubic_spline_smoothness() {
        let xs = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let ys = vec![0.0, 1.0, 0.0, 1.0, 0.0];

        let spline = CubicSpline::new(xs, ys).unwrap();

        // Check that interpolation produces reasonable values
        let y = spline.interpolate(0.5).unwrap();
        assert!(y > 0.0 && y < 1.5); // Should be near the data
    }

    #[test]
    fn test_cubic_spline_extrapolation_error() {
        let xs = vec![0.0, 1.0, 2.0, 3.0];
        let ys = vec![0.0, 1.0, 4.0, 9.0];

        let spline = CubicSpline::new(xs, ys).unwrap();

        assert!(spline.interpolate(-0.5).is_err());
        assert!(spline.interpolate(3.5).is_err());
    }

    #[test]
    fn test_cubic_spline_extrapolation_enabled() {
        let xs = vec![0.0, 1.0, 2.0, 3.0];
        let ys = vec![0.0, 1.0, 4.0, 9.0];

        let spline = CubicSpline::new(xs, ys).unwrap().with_extrapolation();

        // Should allow extrapolation
        assert!(spline.interpolate(-0.5).is_ok());
        assert!(spline.interpolate(3.5).is_ok());
    }

    #[test]
    fn test_insufficient_points() {
        let xs = vec![0.0, 1.0];
        let ys = vec![0.0, 1.0];

        // Cubic spline needs at least 3 points
        assert!(CubicSpline::new(xs, ys).is_err());
    }
}
