//! Flat (constant) extrapolation.

use super::Extrapolator;

/// Flat extrapolation - constant value from the last point.
///
/// This is the simplest extrapolation method, maintaining a constant value
/// equal to the last observed point. It implies a flat forward rate curve
/// beyond the last liquid point.
///
/// # Properties
///
/// - **Simple**: No parameters to configure
/// - **Conservative**: No trend assumptions
/// - **Discontinuous derivative**: Slope becomes zero at boundary
///
/// # Use Cases
///
/// - Short-term extrapolation where trend continuation is not desired
/// - Conservative scenarios
/// - Default fallback when no better method is available
///
/// # Example
///
/// ```rust
/// use convex_math::extrapolation::{FlatExtrapolator, Extrapolator};
///
/// let extrap = FlatExtrapolator;
///
/// // Last observed: 5% at 10 years with 0.1% slope
/// let rate = extrap.extrapolate(15.0, 10.0, 0.05, 0.001);
/// assert_eq!(rate, 0.05);  // Same as last value
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct FlatExtrapolator;

impl FlatExtrapolator {
    /// Creates a new flat extrapolator.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Extrapolator for FlatExtrapolator {
    fn extrapolate(&self, _t: f64, _last_t: f64, last_value: f64, _last_derivative: f64) -> f64 {
        // Simply return the last known value
        last_value
    }

    fn name(&self) -> &'static str {
        "Flat"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_flat_returns_last_value() {
        let extrap = FlatExtrapolator::new();

        let last_t = 20.0;
        let last_value = 0.045;
        let last_deriv = 0.002;

        // Should always return last value regardless of t
        for t in [21.0, 30.0, 50.0, 100.0, 1000.0] {
            let value = extrap.extrapolate(t, last_t, last_value, last_deriv);
            assert_relative_eq!(value, last_value, epsilon = 1e-15);
        }
    }

    #[test]
    fn test_flat_ignores_derivative() {
        let extrap = FlatExtrapolator;

        let last_t = 10.0;
        let last_value = 0.03;

        // Different derivatives should give same result
        for deriv in [-0.01, 0.0, 0.001, 0.01, 0.1] {
            let value = extrap.extrapolate(20.0, last_t, last_value, deriv);
            assert_relative_eq!(value, last_value, epsilon = 1e-15);
        }
    }

    #[test]
    fn test_flat_name() {
        let extrap = FlatExtrapolator;
        assert_eq!(extrap.name(), "Flat");
    }
}
