//! Linear extrapolation.

use super::Extrapolator;

/// Linear extrapolation - continues with the last known slope.
///
/// This method extends the curve by continuing the linear trend from
/// the last observed point using its derivative (slope).
///
/// # Properties
///
/// - **Simple**: Uses only last point and slope
/// - **Trend-following**: Assumes the current trend continues
/// - **Risk**: May produce negative rates if slope is negative
///
/// # Use Cases
///
/// - Short-term extrapolation where trend continuation is expected
/// - Preliminary analysis before applying more sophisticated methods
///
/// # Warning
///
/// Linear extrapolation can produce unrealistic results (negative rates)
/// for long maturities. For regulatory or production use, consider
/// [`SmithWilson`](super::SmithWilson) instead.
///
/// # Example
///
/// ```rust
/// use convex_math::extrapolation::{LinearExtrapolator, Extrapolator};
///
/// let extrap = LinearExtrapolator;
///
/// // Last observed: 5% at 10 years with 0.1% slope per year
/// let rate = extrap.extrapolate(15.0, 10.0, 0.05, 0.001);
/// assert!((rate - 0.055).abs() < 1e-10);  // 5% + 0.1% * 5 = 5.5%
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct LinearExtrapolator;

impl LinearExtrapolator {
    /// Creates a new linear extrapolator.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Extrapolator for LinearExtrapolator {
    fn extrapolate(&self, t: f64, last_t: f64, last_value: f64, last_derivative: f64) -> f64 {
        // Linear continuation: y = y0 + slope * (t - t0)
        last_value + last_derivative * (t - last_t)
    }

    fn name(&self) -> &'static str {
        "Linear"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_linear_extrapolation() {
        let extrap = LinearExtrapolator::new();

        let last_t = 10.0;
        let last_value = 0.05;
        let last_deriv = 0.001; // 0.1% per year

        // At t=15: 0.05 + 0.001 * (15-10) = 0.055
        let value = extrap.extrapolate(15.0, last_t, last_value, last_deriv);
        assert_relative_eq!(value, 0.055, epsilon = 1e-10);

        // At t=20: 0.05 + 0.001 * (20-10) = 0.06
        let value = extrap.extrapolate(20.0, last_t, last_value, last_deriv);
        assert_relative_eq!(value, 0.06, epsilon = 1e-10);
    }

    #[test]
    fn test_linear_with_negative_slope() {
        let extrap = LinearExtrapolator;

        let last_t = 20.0;
        let last_value = 0.04;
        let last_deriv = -0.0005; // Decreasing rates

        // At t=30: 0.04 + (-0.0005) * 10 = 0.035
        let value = extrap.extrapolate(30.0, last_t, last_value, last_deriv);
        assert_relative_eq!(value, 0.035, epsilon = 1e-10);
    }

    #[test]
    fn test_linear_at_boundary() {
        let extrap = LinearExtrapolator;

        let last_t = 10.0;
        let last_value = 0.05;
        let last_deriv = 0.001;

        // At the boundary point, should return exact last value
        let value = extrap.extrapolate(last_t, last_t, last_value, last_deriv);
        assert_relative_eq!(value, last_value, epsilon = 1e-15);
    }

    #[test]
    fn test_linear_with_zero_slope() {
        let extrap = LinearExtrapolator;

        let last_t = 10.0;
        let last_value = 0.03;
        let last_deriv = 0.0;

        // Zero slope should give constant value (same as flat)
        for t in [15.0, 20.0, 50.0, 100.0] {
            let value = extrap.extrapolate(t, last_t, last_value, last_deriv);
            assert_relative_eq!(value, last_value, epsilon = 1e-15);
        }
    }

    #[test]
    fn test_linear_name() {
        let extrap = LinearExtrapolator;
        assert_eq!(extrap.name(), "Linear");
    }
}
