//! Derived curve implementation.
//!
//! A `DerivedCurve` applies a transformation to a base curve, such as
//! parallel shift, spread, or scale.

use std::sync::Arc;

use convex_core::types::Date;

use crate::term_structure::TermStructure;
use crate::value_type::ValueType;

/// Transformations that can be applied to a base curve.
#[derive(Debug, Clone)]
pub enum CurveTransform {
    /// Parallel shift in basis points.
    ///
    /// Adds a constant value to the curve at all tenors.
    /// For rates: new_rate = base_rate + shift_bps / 10000
    ParallelShift {
        /// Shift amount in basis points.
        bps: f64,
    },

    /// Spread over a base curve.
    ///
    /// Similar to parallel shift but with spread type information.
    SpreadOver {
        /// Spread in basis points.
        spread_bps: f64,
    },

    /// Scale the curve by a factor.
    ///
    /// new_value = base_value * factor
    Scale {
        /// Scale factor.
        factor: f64,
    },

    /// Time shift for roll-down analysis.
    ///
    /// Shifts the time axis: new_value(t) = base_value(t + days/365)
    TimeShift {
        /// Number of days to shift.
        days: i32,
    },

    /// Twist transformation (steepener/flattener).
    ///
    /// Applies different shifts at short and long ends.
    Twist {
        /// Shift at short end (in basis points).
        short_shift_bps: f64,
        /// Shift at long end (in basis points).
        long_shift_bps: f64,
        /// Pivot tenor (in years) where shift is zero.
        pivot_tenor: f64,
    },
}

impl CurveTransform {
    /// Creates a parallel shift transformation.
    #[must_use]
    pub fn parallel_shift(bps: f64) -> Self {
        CurveTransform::ParallelShift { bps }
    }

    /// Creates a spread transformation.
    #[must_use]
    pub fn spread(bps: f64) -> Self {
        CurveTransform::SpreadOver { spread_bps: bps }
    }

    /// Creates a scale transformation.
    #[must_use]
    pub fn scale(factor: f64) -> Self {
        CurveTransform::Scale { factor }
    }

    /// Creates a time shift transformation.
    #[must_use]
    pub fn time_shift(days: i32) -> Self {
        CurveTransform::TimeShift { days }
    }

    /// Creates a twist transformation.
    #[must_use]
    pub fn twist(short_bps: f64, long_bps: f64, pivot: f64) -> Self {
        CurveTransform::Twist {
            short_shift_bps: short_bps,
            long_shift_bps: long_bps,
            pivot_tenor: pivot,
        }
    }

    /// Applies the transformation to a value at the given tenor.
    #[must_use]
    pub fn apply(&self, base_value: f64, t: f64) -> f64 {
        match self {
            CurveTransform::ParallelShift { bps } => base_value + bps / 10000.0,
            CurveTransform::SpreadOver { spread_bps } => base_value + spread_bps / 10000.0,
            CurveTransform::Scale { factor } => base_value * factor,
            CurveTransform::TimeShift { .. } => base_value, // Time shift affects tenor, not value
            CurveTransform::Twist {
                short_shift_bps,
                long_shift_bps,
                pivot_tenor,
            } => {
                // Linear interpolation between short and long shift
                let slope = (long_shift_bps - short_shift_bps) / (30.0 - 0.0); // Assume 30Y is long
                let shift_at_t = if t <= *pivot_tenor {
                    short_shift_bps + slope * t
                } else {
                    short_shift_bps + slope * t
                };
                base_value + shift_at_t / 10000.0
            }
        }
    }

    /// Transforms the tenor for time-shift.
    #[must_use]
    pub fn transform_tenor(&self, t: f64) -> f64 {
        match self {
            CurveTransform::TimeShift { days } => t + *days as f64 / 365.0,
            _ => t,
        }
    }
}

/// A curve derived from a base curve with a transformation.
///
/// This is a zero-copy wrapper that applies the transformation on-the-fly.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::{DerivedCurve, CurveTransform};
/// use std::sync::Arc;
///
/// let base_curve = Arc::new(build_sofr_curve()?);
///
/// // Create a curve shifted up by 50bps
/// let shifted = DerivedCurve::new(base_curve, CurveTransform::parallel_shift(50.0));
/// ```
#[derive(Clone)]
pub struct DerivedCurve<T: TermStructure + ?Sized> {
    /// The base curve.
    base: Arc<T>,
    /// The transformation to apply.
    transform: CurveTransform,
}

impl<T: TermStructure + ?Sized> std::fmt::Debug for DerivedCurve<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DerivedCurve")
            .field("base_ref_date", &self.base.reference_date())
            .field("transform", &self.transform)
            .finish()
    }
}

impl<T: TermStructure + ?Sized> DerivedCurve<T> {
    /// Creates a new derived curve.
    #[must_use]
    pub fn new(base: Arc<T>, transform: CurveTransform) -> Self {
        Self { base, transform }
    }

    /// Returns a reference to the base curve.
    #[must_use]
    pub fn base(&self) -> &T {
        &self.base
    }

    /// Returns the transformation.
    #[must_use]
    pub fn transform(&self) -> &CurveTransform {
        &self.transform
    }

    /// Creates a parallel-shifted curve.
    #[must_use]
    pub fn with_shift(base: Arc<T>, bps: f64) -> Self {
        Self::new(base, CurveTransform::parallel_shift(bps))
    }

    /// Creates a spread curve.
    #[must_use]
    pub fn with_spread(base: Arc<T>, spread_bps: f64) -> Self {
        Self::new(base, CurveTransform::spread(spread_bps))
    }
}

impl<T: TermStructure + ?Sized> TermStructure for DerivedCurve<T> {
    fn reference_date(&self) -> Date {
        self.base.reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        // First transform the tenor (for time shifts)
        let transformed_t = self.transform.transform_tenor(t);

        // Get base value at (possibly transformed) tenor
        let base_value = self.base.value_at(transformed_t);

        // Apply value transformation
        self.transform.apply(base_value, t)
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        let (min, max) = self.base.tenor_bounds();
        match &self.transform {
            CurveTransform::TimeShift { days } => {
                let shift = *days as f64 / 365.0;
                (min - shift, max - shift)
            }
            _ => (min, max),
        }
    }

    fn value_type(&self) -> ValueType {
        self.base.value_type()
    }

    fn derivative_at(&self, t: f64) -> Option<f64> {
        let transformed_t = self.transform.transform_tenor(t);

        match &self.transform {
            CurveTransform::ParallelShift { .. } | CurveTransform::SpreadOver { .. } => {
                // Derivative unchanged for constant shifts
                self.base.derivative_at(transformed_t)
            }
            CurveTransform::Scale { factor } => {
                // Derivative scales with factor
                self.base.derivative_at(transformed_t).map(|d| d * factor)
            }
            CurveTransform::TimeShift { .. } => {
                // Derivative unchanged, just at different tenor
                self.base.derivative_at(transformed_t)
            }
            CurveTransform::Twist { .. } => {
                // Complex - twist affects derivative
                None
            }
        }
    }

    fn max_date(&self) -> Date {
        self.base.max_date()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscreteCurve;
    use crate::InterpolationMethod;
    use approx::assert_relative_eq;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Compounding;

    fn sample_base_curve() -> Arc<DiscreteCurve> {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        let rates = vec![0.04, 0.045, 0.05, 0.055];

        Arc::new(
            DiscreteCurve::new(
                today,
                tenors,
                rates,
                ValueType::ZeroRate {
                    compounding: Compounding::Continuous,
                    day_count: DayCountConvention::Act365Fixed,
                },
                InterpolationMethod::Linear,
            )
            .unwrap(),
        )
    }

    #[test]
    fn test_parallel_shift() {
        let base = sample_base_curve();
        let shifted = DerivedCurve::with_shift(base.clone(), 50.0); // +50bps

        // At 2Y: base = 4.5%, shifted = 5.0%
        let base_rate = base.value_at(2.0);
        let shifted_rate = shifted.value_at(2.0);

        assert_relative_eq!(base_rate, 0.045, epsilon = 1e-10);
        assert_relative_eq!(shifted_rate, 0.050, epsilon = 1e-10);
        assert_relative_eq!(shifted_rate - base_rate, 0.005, epsilon = 1e-10);
    }

    #[test]
    fn test_spread() {
        let base = sample_base_curve();
        let spread_curve = DerivedCurve::with_spread(base.clone(), 100.0); // +100bps

        let base_rate = base.value_at(5.0);
        let spread_rate = spread_curve.value_at(5.0);

        assert_relative_eq!(spread_rate - base_rate, 0.01, epsilon = 1e-10);
    }

    #[test]
    fn test_scale() {
        let base = sample_base_curve();
        let scaled = DerivedCurve::new(base.clone(), CurveTransform::scale(1.1));

        let base_rate = base.value_at(5.0);
        let scaled_rate = scaled.value_at(5.0);

        assert_relative_eq!(scaled_rate, base_rate * 1.1, epsilon = 1e-10);
    }

    #[test]
    fn test_time_shift() {
        let base = sample_base_curve();
        let shifted = DerivedCurve::new(base.clone(), CurveTransform::time_shift(90));

        // At t=2.0, time-shifted curve looks at base at t=2.0 + 90/365
        let expected_t = 2.0 + 90.0 / 365.0;
        let base_at_shifted_t = base.value_at(expected_t);
        let shifted_at_2 = shifted.value_at(2.0);

        assert_relative_eq!(shifted_at_2, base_at_shifted_t, epsilon = 1e-10);
    }

    #[test]
    fn test_derivative_preserved() {
        let base = sample_base_curve();
        let shifted = DerivedCurve::with_shift(base.clone(), 50.0);

        // Derivative should be same as base
        let base_deriv = base.derivative_at(2.0);
        let shifted_deriv = shifted.derivative_at(2.0);

        assert_eq!(base_deriv.is_some(), shifted_deriv.is_some());
        if let (Some(bd), Some(sd)) = (base_deriv, shifted_deriv) {
            assert_relative_eq!(bd, sd, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_reference_date() {
        let base = sample_base_curve();
        let shifted = DerivedCurve::with_shift(base.clone(), 50.0);

        assert_eq!(base.reference_date(), shifted.reference_date());
    }

    #[test]
    fn test_value_type() {
        let base = sample_base_curve();
        let shifted = DerivedCurve::with_shift(base.clone(), 50.0);

        assert_eq!(base.value_type(), shifted.value_type());
    }
}
