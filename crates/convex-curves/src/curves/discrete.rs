//! Discrete curve implementation.
//!
//! A `DiscreteCurve` is constructed from a set of discrete data points
//! (tenor, value) with interpolation between points.

use std::sync::Arc;

use convex_core::types::Date;
use convex_math::interpolation::{
    CubicSpline, FlatForward, Interpolator, LinearInterpolator, LogLinearInterpolator,
    MonotoneConvex,
};

use crate::error::{CurveError, CurveResult};
use crate::term_structure::TermStructure;
use crate::value_type::ValueType;
use crate::{ExtrapolationMethod, InterpolationMethod};

/// A curve constructed from discrete point data with interpolation.
///
/// This is the fundamental curve type, holding a set of (tenor, value) pairs
/// and using an interpolation method to compute values at arbitrary tenors.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::{DiscreteCurve, ValueType, InterpolationMethod};
/// use convex_core::types::Date;
///
/// let today = Date::from_ymd(2024, 1, 1)?;
/// let tenors = vec![0.5, 1.0, 2.0, 5.0, 10.0];
/// let rates = vec![0.04, 0.045, 0.05, 0.055, 0.06];
///
/// let curve = DiscreteCurve::new(
///     today,
///     tenors,
///     rates,
///     ValueType::continuous_zero(DayCountConvention::Act365Fixed),
///     InterpolationMethod::MonotoneConvex,
/// )?;
///
/// let rate_3y = curve.value_at(3.0);
/// ```
#[derive(Clone)]
pub struct DiscreteCurve {
    /// Reference date for the curve.
    reference_date: Date,
    /// Tenors in years.
    tenors: Vec<f64>,
    /// Values at each tenor.
    values: Vec<f64>,
    /// What the values represent.
    value_type: ValueType,
    /// Interpolator instance.
    interpolator: Arc<dyn Interpolator>,
    /// Extrapolation method.
    extrapolation: ExtrapolationMethod,
    /// Maximum tenor.
    max_tenor: f64,
}

impl std::fmt::Debug for DiscreteCurve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscreteCurve")
            .field("reference_date", &self.reference_date)
            .field("tenors", &self.tenors)
            .field("values", &self.values)
            .field("value_type", &self.value_type)
            .field("extrapolation", &self.extrapolation)
            .field("max_tenor", &self.max_tenor)
            .finish()
    }
}

impl DiscreteCurve {
    /// Creates a new discrete curve from point data.
    ///
    /// # Arguments
    ///
    /// * `reference_date` - Valuation date for the curve
    /// * `tenors` - Times in years (must be strictly increasing)
    /// * `values` - Values at each tenor
    /// * `value_type` - What the values represent
    /// * `interpolation` - Interpolation method to use
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Tenors and values have different lengths
    /// - Fewer than 2 points are provided
    /// - Tenors are not strictly increasing
    pub fn new(
        reference_date: Date,
        tenors: Vec<f64>,
        values: Vec<f64>,
        value_type: ValueType,
        interpolation: InterpolationMethod,
    ) -> CurveResult<Self> {
        Self::with_extrapolation(
            reference_date,
            tenors,
            values,
            value_type,
            interpolation,
            ExtrapolationMethod::Flat,
        )
    }

    /// Creates a new discrete curve with specified extrapolation.
    pub fn with_extrapolation(
        reference_date: Date,
        tenors: Vec<f64>,
        values: Vec<f64>,
        value_type: ValueType,
        interpolation: InterpolationMethod,
        extrapolation: ExtrapolationMethod,
    ) -> CurveResult<Self> {
        // Validate inputs
        if tenors.len() != values.len() {
            return Err(CurveError::builder_error(format!(
                "Tenors ({}) and values ({}) must have same length",
                tenors.len(),
                values.len()
            )));
        }

        if tenors.len() < 2 {
            return Err(CurveError::insufficient_points(2, tenors.len()));
        }

        // Check monotonicity
        for i in 1..tenors.len() {
            if tenors[i] <= tenors[i - 1] {
                return Err(CurveError::non_monotonic_tenors(
                    i,
                    tenors[i - 1],
                    tenors[i],
                ));
            }
        }

        let max_tenor = *tenors.last().unwrap();

        // Create interpolator
        let interpolator: Arc<dyn Interpolator> =
            Self::create_interpolator(&tenors, &values, interpolation)?;

        Ok(Self {
            reference_date,
            tenors,
            values,
            value_type,
            interpolator,
            extrapolation,
            max_tenor,
        })
    }

    /// Creates the appropriate interpolator.
    fn create_interpolator(
        tenors: &[f64],
        values: &[f64],
        method: InterpolationMethod,
    ) -> CurveResult<Arc<dyn Interpolator>> {
        let tenors_vec = tenors.to_vec();
        let values_vec = values.to_vec();

        match method {
            InterpolationMethod::Linear => LinearInterpolator::new(tenors_vec, values_vec)
                .map(|i| Arc::new(i) as Arc<dyn Interpolator>)
                .map_err(|e| CurveError::interpolation_error(e.to_string())),
            InterpolationMethod::LogLinear => LogLinearInterpolator::new(tenors_vec, values_vec)
                .map(|i| Arc::new(i) as Arc<dyn Interpolator>)
                .map_err(|e| CurveError::interpolation_error(e.to_string())),
            InterpolationMethod::CubicSpline => CubicSpline::new(tenors_vec, values_vec)
                .map(|i| Arc::new(i) as Arc<dyn Interpolator>)
                .map_err(|e| CurveError::interpolation_error(e.to_string())),
            InterpolationMethod::MonotoneConvex => MonotoneConvex::new(tenors_vec, values_vec)
                .map(|i| Arc::new(i) as Arc<dyn Interpolator>)
                .map_err(|e| CurveError::interpolation_error(e.to_string())),
            InterpolationMethod::FlatForward => {
                // Flat forward requires positive tenors; use with_origin for curves starting at 0
                if tenors_vec.first().copied().unwrap_or(0.0) <= 0.0 {
                    FlatForward::with_origin(tenors_vec, values_vec)
                        .map(|i| Arc::new(i) as Arc<dyn Interpolator>)
                        .map_err(|e| CurveError::interpolation_error(e.to_string()))
                } else {
                    FlatForward::new(tenors_vec, values_vec)
                        .map(|i| Arc::new(i) as Arc<dyn Interpolator>)
                        .map_err(|e| CurveError::interpolation_error(e.to_string()))
                }
            }
            InterpolationMethod::PiecewiseConstant => {
                // Use linear for now, will implement piecewise constant later
                LinearInterpolator::new(tenors_vec, values_vec)
                    .map(|i| Arc::new(i) as Arc<dyn Interpolator>)
                    .map_err(|e| CurveError::interpolation_error(e.to_string()))
            }
            InterpolationMethod::NelsonSiegel | InterpolationMethod::Svensson => {
                // Parametric models need fitting, not supported in discrete curve
                Err(CurveError::interpolation_error(
                    "Parametric models require calibration, not direct construction",
                ))
            }
        }
    }

    /// Returns the reference date.
    #[must_use]
    pub fn get_reference_date(&self) -> Date {
        self.reference_date
    }

    /// Returns the tenors.
    #[must_use]
    pub fn tenors(&self) -> &[f64] {
        &self.tenors
    }

    /// Returns the values.
    #[must_use]
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// Returns the number of data points.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tenors.len()
    }

    /// Returns true if the curve has no data points.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tenors.is_empty()
    }

    /// Returns the value type.
    #[must_use]
    pub fn get_value_type(&self) -> &ValueType {
        &self.value_type
    }

    /// Returns the extrapolation method.
    #[must_use]
    pub fn extrapolation(&self) -> ExtrapolationMethod {
        self.extrapolation
    }

    /// Handles extrapolation for out-of-range tenors.
    fn extrapolate(&self, t: f64) -> f64 {
        let min_t = self.tenors[0];

        match self.extrapolation {
            ExtrapolationMethod::None => {
                // Return NaN to indicate error (caller should use try_value_at)
                f64::NAN
            }
            ExtrapolationMethod::Flat => {
                if t < min_t {
                    self.values[0]
                } else {
                    *self.values.last().unwrap()
                }
            }
            ExtrapolationMethod::Linear => {
                if t < min_t {
                    // Extrapolate linearly using first two points
                    let slope =
                        (self.values[1] - self.values[0]) / (self.tenors[1] - self.tenors[0]);
                    self.values[0] + slope * (t - self.tenors[0])
                } else {
                    // Extrapolate linearly using last two points
                    let n = self.values.len();
                    let slope = (self.values[n - 1] - self.values[n - 2])
                        / (self.tenors[n - 1] - self.tenors[n - 2]);
                    self.values[n - 1] + slope * (t - self.tenors[n - 1])
                }
            }
            ExtrapolationMethod::FlatForward => {
                // For flat forward extrapolation, we use the last instantaneous forward rate
                // This is complex and depends on value type; fall back to flat for now
                if t < min_t {
                    self.values[0]
                } else {
                    *self.values.last().unwrap()
                }
            }
        }
    }
}

impl TermStructure for DiscreteCurve {
    fn reference_date(&self) -> Date {
        self.reference_date
    }

    fn value_at(&self, t: f64) -> f64 {
        let min_t = self.tenors[0];
        let max_t = self.max_tenor;

        // Handle out-of-range tenors
        if t < min_t || t > max_t {
            return self.extrapolate(t);
        }

        // Interpolate
        self.interpolator
            .interpolate(t)
            .unwrap_or_else(|_| self.extrapolate(t))
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        (self.tenors[0], self.max_tenor)
    }

    fn value_type(&self) -> ValueType {
        self.value_type.clone()
    }

    fn derivative_at(&self, t: f64) -> Option<f64> {
        let (min_t, max_t) = self.tenor_bounds();
        if t < min_t || t > max_t {
            return None;
        }

        self.interpolator.derivative(t).ok()
    }

    fn max_date(&self) -> Date {
        self.tenor_to_date(self.max_tenor)
    }
}

// Thread safety: DiscreteCurve uses Arc<dyn Interpolator> which is Send + Sync
unsafe impl Send for DiscreteCurve {}
unsafe impl Sync for DiscreteCurve {}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Compounding;

    fn sample_curve() -> DiscreteCurve {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let tenors = vec![0.5, 1.0, 2.0, 3.0, 5.0, 10.0];
        let rates = vec![0.04, 0.045, 0.05, 0.052, 0.055, 0.06];

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
        .unwrap()
    }

    #[test]
    fn test_curve_creation() {
        let curve = sample_curve();
        assert_eq!(curve.len(), 6);
        assert!(!curve.is_empty());
    }

    #[test]
    fn test_value_at_data_points() {
        let curve = sample_curve();

        // Values at data points should match exactly (or very close)
        assert_relative_eq!(curve.value_at(0.5), 0.04, epsilon = 1e-10);
        assert_relative_eq!(curve.value_at(1.0), 0.045, epsilon = 1e-10);
        assert_relative_eq!(curve.value_at(10.0), 0.06, epsilon = 1e-10);
    }

    #[test]
    fn test_interpolation() {
        let curve = sample_curve();

        // Value between 1Y and 2Y should be interpolated
        let rate_1_5y = curve.value_at(1.5);
        assert!(rate_1_5y > 0.045 && rate_1_5y < 0.05);

        // Linear interpolation: should be midpoint
        assert_relative_eq!(rate_1_5y, 0.0475, epsilon = 1e-10);
    }

    #[test]
    fn test_flat_extrapolation() {
        let curve = sample_curve();

        // Before first point - should use first value
        assert_relative_eq!(curve.value_at(0.0), 0.04, epsilon = 1e-10);

        // After last point - should use last value
        assert_relative_eq!(curve.value_at(15.0), 0.06, epsilon = 1e-10);
    }

    #[test]
    fn test_linear_extrapolation() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let tenors = vec![1.0, 2.0, 3.0];
        let rates = vec![0.04, 0.05, 0.06];

        let curve = DiscreteCurve::with_extrapolation(
            today,
            tenors,
            rates,
            ValueType::DiscountFactor,
            InterpolationMethod::Linear,
            ExtrapolationMethod::Linear,
        )
        .unwrap();

        // Before first point - linear extrapolation
        // Slope = (0.05 - 0.04) / (2 - 1) = 0.01
        // At t=0: 0.04 + 0.01 * (0 - 1) = 0.03
        assert_relative_eq!(curve.value_at(0.0), 0.03, epsilon = 1e-10);

        // After last point - linear extrapolation
        // Slope = (0.06 - 0.05) / (3 - 2) = 0.01
        // At t=4: 0.06 + 0.01 * (4 - 3) = 0.07
        assert_relative_eq!(curve.value_at(4.0), 0.07, epsilon = 1e-10);
    }

    #[test]
    fn test_derivative() {
        let curve = sample_curve();

        // Derivative should exist within range
        let deriv = curve.derivative_at(2.0);
        assert!(deriv.is_some());

        // For linear interpolation, derivative should be constant between points
        // Between 1Y and 2Y: slope = (0.05 - 0.045) / (2 - 1) = 0.005
        let deriv_1_5 = curve.derivative_at(1.5).unwrap();
        assert_relative_eq!(deriv_1_5, 0.005, epsilon = 1e-6);
    }

    #[test]
    fn test_tenor_bounds() {
        let curve = sample_curve();
        let (min, max) = curve.tenor_bounds();
        assert_relative_eq!(min, 0.5, epsilon = 1e-10);
        assert_relative_eq!(max, 10.0, epsilon = 1e-10);
    }

    #[test]
    fn test_in_range() {
        let curve = sample_curve();
        assert!(!curve.in_range(0.0)); // Before first point
        assert!(curve.in_range(0.5)); // At first point
        assert!(curve.in_range(5.0)); // In middle
        assert!(curve.in_range(10.0)); // At last point
        assert!(!curve.in_range(15.0)); // After last point
    }

    #[test]
    fn test_try_value_at() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let curve = DiscreteCurve::with_extrapolation(
            today,
            vec![1.0, 2.0],
            vec![0.05, 0.06],
            ValueType::DiscountFactor,
            InterpolationMethod::Linear,
            ExtrapolationMethod::None,
        )
        .unwrap();

        // In range - should succeed
        assert!(curve.try_value_at(1.5).is_ok());

        // Out of range with no extrapolation - should fail
        assert!(curve.try_value_at(0.5).is_err());
        assert!(curve.try_value_at(3.0).is_err());
    }

    #[test]
    fn test_max_date() {
        let curve = sample_curve();
        let max_date = curve.max_date();
        let ref_date = curve.reference_date();

        // Should be approximately 10 years from reference date (within a few days)
        let days = ref_date.days_between(&max_date);
        // 10 years is approximately 3650 days, allow +/- 5 days for leap years
        assert!(
            (days - 3650).abs() <= 5,
            "Expected ~3650 days, got {}",
            days
        );
    }

    #[test]
    fn test_validation_errors() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();

        // Mismatched lengths
        let result = DiscreteCurve::new(
            today,
            vec![1.0, 2.0],
            vec![0.05],
            ValueType::DiscountFactor,
            InterpolationMethod::Linear,
        );
        assert!(result.is_err());

        // Too few points
        let result = DiscreteCurve::new(
            today,
            vec![1.0],
            vec![0.05],
            ValueType::DiscountFactor,
            InterpolationMethod::Linear,
        );
        assert!(result.is_err());

        // Non-monotonic tenors
        let result = DiscreteCurve::new(
            today,
            vec![1.0, 0.5, 2.0],
            vec![0.04, 0.05, 0.06],
            ValueType::DiscountFactor,
            InterpolationMethod::Linear,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_different_interpolation_methods() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let tenors = vec![1.0, 2.0, 3.0, 5.0, 10.0];
        let rates = vec![0.04, 0.045, 0.05, 0.055, 0.06];

        // All these should work
        for method in [
            InterpolationMethod::Linear,
            InterpolationMethod::CubicSpline,
            InterpolationMethod::MonotoneConvex,
            InterpolationMethod::FlatForward,
        ] {
            let curve = DiscreteCurve::new(
                today,
                tenors.clone(),
                rates.clone(),
                ValueType::DiscountFactor,
                method,
            );
            assert!(curve.is_ok(), "Failed for method {:?}", method);
        }
    }

    #[test]
    fn test_flat_forward_interpolation() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        let rates = vec![0.02, 0.025, 0.03, 0.035];

        let curve = DiscreteCurve::new(
            today,
            tenors.clone(),
            rates.clone(),
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            InterpolationMethod::FlatForward,
        )
        .unwrap();

        // Should pass through pillar points
        assert_relative_eq!(curve.value_at(1.0), 0.02, epsilon = 1e-10);
        assert_relative_eq!(curve.value_at(2.0), 0.025, epsilon = 1e-10);
        assert_relative_eq!(curve.value_at(5.0), 0.03, epsilon = 1e-10);
        assert_relative_eq!(curve.value_at(10.0), 0.035, epsilon = 1e-10);

        // Verify interpolation produces valid values
        let rate_1_5y = curve.value_at(1.5);
        assert!(
            rate_1_5y > 0.02 && rate_1_5y < 0.025,
            "Rate at 1.5Y should be between 2% and 2.5%: {}",
            rate_1_5y
        );

        // Forward rate calculation: f = (r2*t2 - r1*t1)/(t2-t1)
        // Forward from 1Y to 2Y: (0.025*2 - 0.02*1)/(2-1) = 0.03
        // r(1.5) = (0.02*1 + 0.03*0.5)/1.5 = 0.035/1.5 = 0.02333...
        let expected_rate_1_5y = (0.02 * 1.0 + 0.03 * 0.5) / 1.5;
        assert_relative_eq!(rate_1_5y, expected_rate_1_5y, epsilon = 1e-10);
    }

    #[test]
    fn test_flat_forward_with_origin() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        // Curve starting at t=0
        let tenors = vec![0.0, 1.0, 2.0, 5.0];
        let rates = vec![0.02, 0.02, 0.025, 0.03];

        let curve = DiscreteCurve::new(
            today,
            tenors,
            rates,
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            InterpolationMethod::FlatForward,
        )
        .unwrap();

        // Should work with tenors starting at 0
        assert_relative_eq!(curve.value_at(0.0), 0.02, epsilon = 1e-10);
        assert_relative_eq!(curve.value_at(1.0), 0.02, epsilon = 1e-10);
        assert_relative_eq!(curve.value_at(0.5), 0.02, epsilon = 1e-10);
    }
}
