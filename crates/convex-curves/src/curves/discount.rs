//! Discount factor curve with interpolation and extrapolation support.
//!
//! A [`DiscountCurve`] stores discount factors at pillar points and interpolates
//! between them using configurable interpolation methods.

use std::sync::Arc;

use convex_core::Date;
use convex_math::interpolation::{
    CubicSpline, Interpolator, LinearInterpolator, LogLinearInterpolator, MonotoneConvex,
};

use crate::compounding::Compounding;
use crate::error::{CurveError, CurveResult};
use crate::interpolation::InterpolationMethod;
use crate::traits::Curve;

/// A discount factor curve with configurable interpolation and extrapolation.
///
/// This is the primary curve type for discounting cash flows. It stores
/// discount factors at discrete pillar points and interpolates between them.
///
/// # Construction
///
/// Use [`DiscountCurveBuilder`] for ergonomic curve construction:
///
/// ```rust,ignore
/// use convex_curves::prelude::*;
///
/// let curve = DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap())
///     .add_pillar(0.25, 0.99)
///     .add_pillar(1.0, 0.96)
///     .add_pillar(5.0, 0.80)
///     .with_interpolation(InterpolationMethod::LogLinear)
///     .build()
///     .unwrap();
/// ```
///
/// # Interpolation Methods
///
/// The curve supports multiple interpolation methods:
///
/// | Method | Property | Use Case |
/// |--------|----------|----------|
/// | LogLinear | Preserves DF monotonicity | Production default |
/// | Linear | Simple, fast | Quick prototyping |
/// | CubicSpline | Smooth curves | Presentation |
/// | MonotoneConvex | Positive forwards | Zero rates |
///
/// For discount factor curves, **LogLinear** is recommended as it:
/// - Ensures discount factors are always positive
/// - Equivalent to linear interpolation on continuously compounded rates
/// - Produces sensible forward rates
#[derive(Clone)]
pub struct DiscountCurve {
    /// Reference (valuation) date.
    reference_date: Date,
    /// Pillar times (year fractions from reference date).
    pillar_times: Vec<f64>,
    /// Discount factors at each pillar.
    discount_factors: Vec<f64>,
    /// Interpolation method.
    interpolation: InterpolationMethod,
    /// Cached interpolator (built on demand).
    interpolator: Option<Arc<dyn Interpolator>>,
    /// Allow extrapolation beyond curve range.
    allow_extrapolation: bool,
}

impl std::fmt::Debug for DiscountCurve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscountCurve")
            .field("reference_date", &self.reference_date)
            .field("pillar_times", &self.pillar_times)
            .field("discount_factors", &self.discount_factors)
            .field("interpolation", &self.interpolation)
            .field("allow_extrapolation", &self.allow_extrapolation)
            .finish()
    }
}

impl DiscountCurve {
    /// Creates a new discount curve.
    ///
    /// # Arguments
    ///
    /// * `reference_date` - Curve valuation date
    /// * `pillar_times` - Times in years for each pillar
    /// * `discount_factors` - Discount factors at each pillar
    /// * `interpolation` - Interpolation method
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No pillar points provided
    /// - Pillar times and discount factors have different lengths
    /// - Pillar times are not sorted
    /// - Any discount factor is non-positive
    pub fn new(
        reference_date: Date,
        pillar_times: Vec<f64>,
        discount_factors: Vec<f64>,
        interpolation: InterpolationMethod,
    ) -> CurveResult<Self> {
        if pillar_times.is_empty() {
            return Err(CurveError::EmptyCurve);
        }

        if pillar_times.len() != discount_factors.len() {
            return Err(CurveError::invalid_data(format!(
                "pillar_times ({}) and discount_factors ({}) must have same length",
                pillar_times.len(),
                discount_factors.len()
            )));
        }

        // Validate sorted
        for i in 1..pillar_times.len() {
            if pillar_times[i] <= pillar_times[i - 1] {
                return Err(CurveError::invalid_data(
                    "pillar_times must be strictly increasing",
                ));
            }
        }

        // Validate positive DFs
        for (i, &df) in discount_factors.iter().enumerate() {
            if df <= 0.0 {
                return Err(CurveError::invalid_data(format!(
                    "discount_factor[{}] = {} is not positive",
                    i, df
                )));
            }
        }

        let mut curve = Self {
            reference_date,
            pillar_times,
            discount_factors,
            interpolation,
            interpolator: None,
            allow_extrapolation: false,
        };

        // Build interpolator
        curve.build_interpolator()?;

        Ok(curve)
    }

    /// Creates the curve with extrapolation enabled.
    #[must_use]
    pub fn with_extrapolation(mut self) -> Self {
        self.allow_extrapolation = true;
        self
    }

    /// Returns the pillar times.
    #[must_use]
    pub fn pillar_times(&self) -> &[f64] {
        &self.pillar_times
    }

    /// Returns the discount factors at pillars.
    #[must_use]
    pub fn discount_factors_raw(&self) -> &[f64] {
        &self.discount_factors
    }

    /// Returns the interpolation method.
    #[must_use]
    pub fn interpolation(&self) -> InterpolationMethod {
        self.interpolation
    }

    /// Returns the minimum time in the curve.
    #[must_use]
    pub fn min_time(&self) -> f64 {
        *self.pillar_times.first().unwrap_or(&0.0)
    }

    /// Returns the maximum time in the curve.
    #[must_use]
    pub fn max_time(&self) -> f64 {
        *self.pillar_times.last().unwrap_or(&0.0)
    }

    /// Builds the internal interpolator.
    fn build_interpolator(&mut self) -> CurveResult<()> {
        // For discount factor interpolation, we use log-linear on DFs
        // which is equivalent to linear on continuously compounded rates
        let interp: Box<dyn Interpolator> = match self.interpolation {
            InterpolationMethod::Linear => {
                // Linear on DFs (not recommended but supported)
                Box::new(
                    LinearInterpolator::new(
                        self.pillar_times.clone(),
                        self.discount_factors.clone(),
                    )
                    .map_err(|e| CurveError::InterpolationFailed {
                        reason: e.to_string(),
                    })?
                    .with_extrapolation(),
                )
            }
            InterpolationMethod::LogLinear => {
                // Log-linear on DFs (recommended for DF curves)
                Box::new(
                    LogLinearInterpolator::new(
                        self.pillar_times.clone(),
                        self.discount_factors.clone(),
                    )
                    .map_err(|e| CurveError::InterpolationFailed {
                        reason: e.to_string(),
                    })?
                    .with_extrapolation(),
                )
            }
            InterpolationMethod::CubicSpline | InterpolationMethod::CubicSplineOnDiscount => {
                // Cubic spline on log(DF)
                let log_dfs: Vec<f64> = self.discount_factors.iter().map(|df| df.ln()).collect();
                Box::new(
                    CubicSpline::new(self.pillar_times.clone(), log_dfs)
                        .map_err(|e| CurveError::InterpolationFailed {
                            reason: e.to_string(),
                        })?
                        .with_extrapolation(),
                )
            }
            InterpolationMethod::MonotoneConvex => {
                // Monotone convex on zero rates, then convert back to DF
                let zero_rates: Vec<f64> = self
                    .pillar_times
                    .iter()
                    .zip(self.discount_factors.iter())
                    .map(|(t, df)| {
                        if *t > 0.0 {
                            -df.ln() / t
                        } else {
                            0.0
                        }
                    })
                    .collect();
                Box::new(
                    MonotoneConvex::new(self.pillar_times.clone(), zero_rates)
                        .map_err(|e| CurveError::InterpolationFailed {
                            reason: e.to_string(),
                        })?
                        .with_extrapolation(),
                )
            }
            _ => {
                // Default to log-linear for other methods
                Box::new(
                    LogLinearInterpolator::new(
                        self.pillar_times.clone(),
                        self.discount_factors.clone(),
                    )
                    .map_err(|e| CurveError::InterpolationFailed {
                        reason: e.to_string(),
                    })?
                    .with_extrapolation(),
                )
            }
        };

        self.interpolator = Some(Arc::from(interp));
        Ok(())
    }

    /// Internal interpolation method.
    fn interpolate_df(&self, t: f64) -> CurveResult<f64> {
        if t <= 0.0 {
            return Ok(1.0);
        }

        // Check range
        if !self.allow_extrapolation && (t < self.min_time() || t > self.max_time()) {
            return Err(CurveError::DateOutOfRange {
                date: self.reference_date.add_days((t * 365.0) as i64),
                min_date: self.reference_date.add_days((self.min_time() * 365.0) as i64),
                max_date: self.reference_date.add_days((self.max_time() * 365.0) as i64),
            });
        }

        let interp = self.interpolator.as_ref().ok_or_else(|| {
            CurveError::InterpolationFailed {
                reason: "Interpolator not built".to_string(),
            }
        })?;

        match self.interpolation {
            InterpolationMethod::CubicSpline | InterpolationMethod::CubicSplineOnDiscount => {
                // Spline is on log(DF), so convert back
                let log_df = interp
                    .interpolate(t)
                    .map_err(|e| CurveError::InterpolationFailed {
                        reason: e.to_string(),
                    })?;
                Ok(log_df.exp())
            }
            InterpolationMethod::MonotoneConvex => {
                // Monotone convex gives zero rate, convert to DF
                let zero_rate = interp
                    .interpolate(t)
                    .map_err(|e| CurveError::InterpolationFailed {
                        reason: e.to_string(),
                    })?;
                Ok((-zero_rate * t).exp())
            }
            _ => {
                // Direct interpolation on DFs
                interp
                    .interpolate(t)
                    .map_err(|e| CurveError::InterpolationFailed {
                        reason: e.to_string(),
                    })
            }
        }
    }
}

impl Curve for DiscountCurve {
    fn discount_factor(&self, t: f64) -> CurveResult<f64> {
        self.interpolate_df(t)
    }

    fn reference_date(&self) -> Date {
        self.reference_date
    }

    fn max_date(&self) -> Date {
        let max_t = self.max_time();
        self.reference_date
            .add_days((max_t * 365.0).round() as i64)
    }
}

/// Builder for constructing discount curves.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::prelude::*;
///
/// let curve = DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap())
///     .add_pillar(0.25, 0.9975)  // 3M
///     .add_pillar(0.5, 0.995)    // 6M
///     .add_pillar(1.0, 0.98)     // 1Y
///     .add_pillar(2.0, 0.96)     // 2Y
///     .add_pillar(5.0, 0.90)     // 5Y
///     .add_pillar(10.0, 0.78)    // 10Y
///     .with_interpolation(InterpolationMethod::LogLinear)
///     .with_extrapolation()
///     .build()
///     .unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct DiscountCurveBuilder {
    reference_date: Date,
    pillars: Vec<(f64, f64)>, // (time, df)
    interpolation: InterpolationMethod,
    allow_extrapolation: bool,
}

impl DiscountCurveBuilder {
    /// Creates a new builder with the given reference date.
    #[must_use]
    pub fn new(reference_date: Date) -> Self {
        Self {
            reference_date,
            pillars: Vec::new(),
            interpolation: InterpolationMethod::LogLinear,
            allow_extrapolation: false,
        }
    }

    /// Adds a pillar point (time, discount factor).
    ///
    /// # Arguments
    ///
    /// * `time` - Time in years from reference date
    /// * `df` - Discount factor at this time
    #[must_use]
    pub fn add_pillar(mut self, time: f64, df: f64) -> Self {
        self.pillars.push((time, df));
        self
    }

    /// Adds a pillar point by date.
    ///
    /// # Arguments
    ///
    /// * `date` - Pillar date
    /// * `df` - Discount factor at this date
    #[must_use]
    pub fn add_pillar_date(mut self, date: Date, df: f64) -> Self {
        let time = self.reference_date.days_between(&date) as f64 / 365.0;
        self.pillars.push((time, df));
        self
    }

    /// Adds a pillar from a zero rate.
    ///
    /// # Arguments
    ///
    /// * `time` - Time in years from reference date
    /// * `rate` - Zero rate (continuously compounded)
    #[must_use]
    pub fn add_zero_rate(mut self, time: f64, rate: f64) -> Self {
        let df = (-rate * time).exp();
        self.pillars.push((time, df));
        self
    }

    /// Adds a pillar from a zero rate with specified compounding.
    ///
    /// # Arguments
    ///
    /// * `time` - Time in years from reference date
    /// * `rate` - Zero rate
    /// * `compounding` - Compounding convention for the rate
    #[must_use]
    pub fn add_zero_rate_compounded(mut self, time: f64, rate: f64, compounding: Compounding) -> Self {
        let df = compounding.discount_factor(rate, time);
        self.pillars.push((time, df));
        self
    }

    /// Adds multiple pillars.
    #[must_use]
    pub fn add_pillars(mut self, pillars: impl IntoIterator<Item = (f64, f64)>) -> Self {
        self.pillars.extend(pillars);
        self
    }

    /// Sets the interpolation method.
    #[must_use]
    pub fn with_interpolation(mut self, method: InterpolationMethod) -> Self {
        self.interpolation = method;
        self
    }

    /// Enables extrapolation beyond curve range.
    #[must_use]
    pub fn with_extrapolation(mut self) -> Self {
        self.allow_extrapolation = true;
        self
    }

    /// Builds the discount curve.
    ///
    /// # Errors
    ///
    /// Returns an error if no pillars were added or if data is invalid.
    pub fn build(mut self) -> CurveResult<DiscountCurve> {
        if self.pillars.is_empty() {
            return Err(CurveError::EmptyCurve);
        }

        // Sort by time
        self.pillars.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        let (times, dfs): (Vec<f64>, Vec<f64>) = self.pillars.into_iter().unzip();

        let mut curve = DiscountCurve::new(self.reference_date, times, dfs, self.interpolation)?;

        if self.allow_extrapolation {
            curve = curve.with_extrapolation();
        }

        Ok(curve)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn sample_curve() -> DiscountCurve {
        DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap())
            .add_pillar(0.25, 0.99)
            .add_pillar(0.5, 0.98)
            .add_pillar(1.0, 0.96)
            .add_pillar(2.0, 0.92)
            .add_pillar(5.0, 0.80)
            .add_pillar(10.0, 0.65)
            .with_interpolation(InterpolationMethod::LogLinear)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    #[test]
    fn test_build_discount_curve() {
        let curve = sample_curve();
        assert_eq!(curve.pillar_times().len(), 6);
    }

    #[test]
    fn test_discount_factor_at_pillars() {
        let curve = sample_curve();

        // At pillar points, should match exactly
        assert_relative_eq!(curve.discount_factor(0.25).unwrap(), 0.99, epsilon = 1e-10);
        assert_relative_eq!(curve.discount_factor(1.0).unwrap(), 0.96, epsilon = 1e-10);
        assert_relative_eq!(curve.discount_factor(5.0).unwrap(), 0.80, epsilon = 1e-10);
    }

    #[test]
    fn test_discount_factor_interpolated() {
        let curve = sample_curve();

        // Between 1Y (0.96) and 2Y (0.92)
        let df_1_5 = curve.discount_factor(1.5).unwrap();
        assert!(df_1_5 > 0.92 && df_1_5 < 0.96);
    }

    #[test]
    fn test_discount_factor_at_zero() {
        let curve = sample_curve();
        assert_relative_eq!(curve.discount_factor(0.0).unwrap(), 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_zero_rate() {
        let curve = sample_curve();
        let rate = curve.zero_rate(1.0, Compounding::Continuous).unwrap();

        // r = -ln(0.96) / 1 ≈ 0.0408
        let expected = -(0.96_f64.ln()) / 1.0;
        assert_relative_eq!(rate, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_forward_rate() {
        let curve = sample_curve();
        let fwd = curve.forward_rate(1.0, 2.0).unwrap();

        // F = (DF_1 / DF_2 - 1) / tau = (0.96/0.92 - 1) / 1 ≈ 0.0435
        let expected = (0.96 / 0.92 - 1.0) / 1.0;
        assert_relative_eq!(fwd, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_instantaneous_forward() {
        let curve = sample_curve();
        let inst_fwd = curve.instantaneous_forward(1.0).unwrap();

        // Should be positive and reasonable
        assert!(inst_fwd > 0.0 && inst_fwd < 0.15);
    }

    #[test]
    fn test_builder_from_zero_rates() {
        let curve = DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap())
            .add_zero_rate(1.0, 0.05)
            .add_zero_rate(2.0, 0.055)
            .add_zero_rate(5.0, 0.06)
            .with_interpolation(InterpolationMethod::LogLinear)
            .build()
            .unwrap();

        // Check DF at 1Y: e^(-0.05) ≈ 0.9512
        assert_relative_eq!(
            curve.discount_factor(1.0).unwrap(),
            (-0.05_f64).exp(),
            epsilon = 1e-10
        );
    }

    #[test]
    fn test_monotonicity() {
        let curve = sample_curve();

        // DFs should be monotonically decreasing
        let mut prev_df = 1.0;
        for t in [0.1, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0] {
            let df = curve.discount_factor(t).unwrap();
            assert!(
                df < prev_df,
                "DF at t={} ({}) should be < previous ({})",
                t,
                df,
                prev_df
            );
            prev_df = df;
        }
    }

    #[test]
    fn test_extrapolation_enabled() {
        let curve = sample_curve();

        // Should work beyond 10Y with extrapolation enabled
        let df_15 = curve.discount_factor(15.0).unwrap();
        assert!(df_15 > 0.0 && df_15 < 0.65);
    }

    #[test]
    fn test_extrapolation_disabled() {
        let curve = DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap())
            .add_pillar(1.0, 0.96)
            .add_pillar(5.0, 0.80)
            // No with_extrapolation()
            .build()
            .unwrap();

        // Should fail beyond curve range
        let result = curve.discount_factor(10.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_curve_error() {
        let result = DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap()).build();
        assert!(matches!(result, Err(CurveError::EmptyCurve)));
    }

    #[test]
    fn test_invalid_df_error() {
        let result = DiscountCurve::new(
            Date::from_ymd(2025, 1, 1).unwrap(),
            vec![1.0, 2.0],
            vec![0.96, -0.1], // Negative DF!
            InterpolationMethod::LogLinear,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_monotone_convex_interpolation() {
        let curve = DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap())
            .add_pillar(0.25, 0.995)
            .add_pillar(0.5, 0.99)
            .add_pillar(1.0, 0.97)
            .add_pillar(2.0, 0.94)
            .add_pillar(5.0, 0.85)
            .with_interpolation(InterpolationMethod::MonotoneConvex)
            .with_extrapolation()
            .build()
            .unwrap();

        // Check interpolation works
        let df = curve.discount_factor(1.5).unwrap();
        assert!(df > 0.0 && df < 1.0);

        // Check forward rates are positive (key property of monotone convex)
        for t in [0.5, 1.0, 1.5, 2.0, 3.0, 4.0] {
            let fwd = curve.forward_rate(t, t + 0.25).unwrap();
            assert!(fwd >= 0.0, "Forward at t={} is {}", t, fwd);
        }
    }
}
