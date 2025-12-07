//! Forward rate curve.
//!
//! A [`ForwardCurve`] provides forward rates for a specific tenor, optionally
//! including a spread over a base curve.

use std::sync::Arc;

use convex_core::Date;

use crate::error::CurveResult;
use crate::traits::Curve;

/// A forward rate curve.
///
/// Provides forward rates from an underlying curve for a specified tenor.
/// Optionally supports an additive spread over the base forward rates.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::prelude::*;
/// use std::sync::Arc;
///
/// // Create a base discount curve
/// let discount_curve = Arc::new(
///     DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap())
///         .add_pillar(1.0, 0.96)
///         .add_pillar(5.0, 0.80)
///         .build()
///         .unwrap()
/// );
///
/// // Create 3-month forward curve
/// let forward_curve = ForwardCurve::new(discount_curve, 0.25);
///
/// // Get 3M forward rate starting at 1Y
/// let fwd_rate = forward_curve.forward_rate_at(1.0).unwrap();
/// ```
#[derive(Clone)]
pub struct ForwardCurve {
    /// Underlying discount curve.
    base_curve: Arc<dyn Curve>,
    /// Forward rate tenor in years.
    tenor: f64,
    /// Additive spread over base forward rates (optional).
    spread: f64,
}

impl std::fmt::Debug for ForwardCurve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForwardCurve")
            .field("tenor", &self.tenor)
            .field("spread", &self.spread)
            .finish()
    }
}

impl ForwardCurve {
    /// Creates a new forward curve with specified tenor.
    ///
    /// # Arguments
    ///
    /// * `base_curve` - The underlying discount curve
    /// * `tenor` - Forward rate tenor in years (e.g., 0.25 for 3M)
    #[must_use]
    pub fn new(base_curve: Arc<dyn Curve>, tenor: f64) -> Self {
        Self {
            base_curve,
            tenor,
            spread: 0.0,
        }
    }

    /// Creates a forward curve from tenor in months.
    ///
    /// # Arguments
    ///
    /// * `base_curve` - The underlying discount curve
    /// * `tenor_months` - Forward rate tenor in months (e.g., 3 for 3M)
    #[must_use]
    pub fn from_months(base_curve: Arc<dyn Curve>, tenor_months: u32) -> Self {
        Self {
            base_curve,
            tenor: f64::from(tenor_months) / 12.0,
            spread: 0.0,
        }
    }

    /// Adds an additive spread to all forward rates.
    ///
    /// # Arguments
    ///
    /// * `spread` - Spread as a decimal (e.g., 0.0010 for 10bp)
    #[must_use]
    pub fn with_spread(mut self, spread: f64) -> Self {
        self.spread = spread;
        self
    }

    /// Returns the tenor in years.
    #[must_use]
    pub fn tenor(&self) -> f64 {
        self.tenor
    }

    /// Returns the tenor in months.
    #[must_use]
    pub fn tenor_months(&self) -> f64 {
        self.tenor * 12.0
    }

    /// Returns the spread.
    #[must_use]
    pub fn spread(&self) -> f64 {
        self.spread
    }

    /// Returns the reference date.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.base_curve.reference_date()
    }

    /// Returns the forward rate starting at time t.
    ///
    /// The forward rate is computed as the simply-compounded rate
    /// for a deposit from t to t + tenor.
    ///
    /// # Arguments
    ///
    /// * `t` - Start time in years
    ///
    /// # Formula
    ///
    /// `F(t, t+τ) = (DF(t) / DF(t+τ) - 1) / τ + spread`
    pub fn forward_rate_at(&self, t: f64) -> CurveResult<f64> {
        let t_end = t + self.tenor;

        let df_start = self.base_curve.discount_factor(t)?;
        let df_end = self.base_curve.discount_factor(t_end)?;

        if df_end <= 0.0 {
            return Ok(self.spread);
        }

        let fwd = (df_start / df_end - 1.0) / self.tenor;
        Ok(fwd + self.spread)
    }

    /// Returns the instantaneous forward rate at time t.
    ///
    /// This is the limit of the forward rate as the tenor approaches zero.
    ///
    /// # Arguments
    ///
    /// * `t` - Time in years
    pub fn instantaneous_forward(&self, t: f64) -> CurveResult<f64> {
        let inst_fwd = self.base_curve.instantaneous_forward(t)?;
        Ok(inst_fwd + self.spread)
    }

    /// Returns the discount factor implied by the forward curve.
    ///
    /// This assumes the forward curve represents the continuously compounded
    /// instantaneous forward rate, and integrates to get the discount factor.
    ///
    /// For most use cases, use the base curve's discount factor directly.
    pub fn implied_discount_factor(&self, t: f64) -> CurveResult<f64> {
        // With additive spread, DF = DF_base * exp(-spread * t)
        let df_base = self.base_curve.discount_factor(t)?;
        Ok(df_base * (-self.spread * t).exp())
    }

    /// Returns the underlying curve.
    #[must_use]
    pub fn base_curve(&self) -> &dyn Curve {
        self.base_curve.as_ref()
    }

    /// Creates a new forward curve with a different tenor.
    ///
    /// Useful for creating curves for different index tenors from
    /// the same base curve.
    #[must_use]
    pub fn with_tenor(&self, tenor: f64) -> Self {
        Self {
            base_curve: Arc::clone(&self.base_curve),
            tenor,
            spread: self.spread,
        }
    }
}

/// Builder for forward curves with more configuration options.
pub struct ForwardCurveBuilder {
    base_curve: Option<Arc<dyn Curve>>,
    tenor: f64,
    spread: f64,
}

impl Default for ForwardCurveBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ForwardCurveBuilder {
    /// Creates a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            base_curve: None,
            tenor: 0.25, // Default 3M
            spread: 0.0,
        }
    }

    /// Sets the base curve.
    #[must_use]
    pub fn base_curve(mut self, curve: Arc<dyn Curve>) -> Self {
        self.base_curve = Some(curve);
        self
    }

    /// Sets the tenor in years.
    #[must_use]
    pub fn tenor(mut self, tenor: f64) -> Self {
        self.tenor = tenor;
        self
    }

    /// Sets the tenor in months.
    #[must_use]
    pub fn tenor_months(mut self, months: u32) -> Self {
        self.tenor = f64::from(months) / 12.0;
        self
    }

    /// Sets the spread.
    #[must_use]
    pub fn spread(mut self, spread: f64) -> Self {
        self.spread = spread;
        self
    }

    /// Sets the spread in basis points.
    #[must_use]
    pub fn spread_bps(mut self, bps: f64) -> Self {
        self.spread = bps / 10000.0;
        self
    }

    /// Builds the forward curve.
    pub fn build(self) -> CurveResult<ForwardCurve> {
        let base = self
            .base_curve
            .ok_or_else(|| crate::error::CurveError::invalid_data("Base curve is required"))?;

        Ok(ForwardCurve {
            base_curve: base,
            tenor: self.tenor,
            spread: self.spread,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscountCurveBuilder;
    use crate::interpolation::InterpolationMethod;
    use approx::assert_relative_eq;

    fn sample_curve() -> Arc<dyn Curve> {
        Arc::new(
            DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap())
                .add_pillar(0.25, 0.9975)
                .add_pillar(0.5, 0.995)
                .add_pillar(1.0, 0.98)
                .add_pillar(2.0, 0.96)
                .add_pillar(5.0, 0.90)
                .add_pillar(10.0, 0.78)
                .with_interpolation(InterpolationMethod::LogLinear)
                .with_extrapolation()
                .build()
                .unwrap(),
        )
    }

    #[test]
    fn test_forward_rate_3m() {
        let base = sample_curve();
        let fwd = ForwardCurve::new(base.clone(), 0.25);

        // Forward rate at time 0 (spot 3M rate)
        let rate = fwd.forward_rate_at(0.0).unwrap();

        // Manually calculate: (DF(0)/DF(0.25) - 1) / 0.25
        let df_0 = 1.0; // t=0
        let df_3m = base.discount_factor(0.25).unwrap();
        let expected = (df_0 / df_3m - 1.0) / 0.25;

        assert_relative_eq!(rate, expected, epsilon = 1e-10);
    }

    #[test]
    fn test_forward_rate_with_spread() {
        let base = sample_curve();
        let fwd = ForwardCurve::new(base.clone(), 0.25).with_spread(0.001); // 10bp

        let rate_no_spread = ForwardCurve::new(base, 0.25).forward_rate_at(1.0).unwrap();
        let rate_with_spread = fwd.forward_rate_at(1.0).unwrap();

        assert_relative_eq!(rate_with_spread - rate_no_spread, 0.001, epsilon = 1e-10);
    }

    #[test]
    fn test_from_months() {
        let base = sample_curve();
        let fwd_3m = ForwardCurve::from_months(base.clone(), 3);
        let fwd_6m = ForwardCurve::from_months(base, 6);

        assert_relative_eq!(fwd_3m.tenor(), 0.25, epsilon = 1e-10);
        assert_relative_eq!(fwd_6m.tenor(), 0.5, epsilon = 1e-10);
    }

    #[test]
    fn test_instantaneous_forward() {
        let base = sample_curve();
        let fwd = ForwardCurve::new(base.clone(), 0.25);

        let inst = fwd.instantaneous_forward(1.0).unwrap();

        // Should be positive and reasonable
        assert!(inst > 0.0 && inst < 0.15);

        // With spread, should be higher
        let fwd_spread = fwd.with_spread(0.01);
        let inst_spread = fwd_spread.instantaneous_forward(1.0).unwrap();
        assert_relative_eq!(inst_spread - inst, 0.01, epsilon = 1e-10);
    }

    #[test]
    fn test_implied_discount_factor() {
        let base = sample_curve();
        let fwd = ForwardCurve::new(base.clone(), 0.25).with_spread(0.01);

        let df_base = base.discount_factor(1.0).unwrap();
        let df_implied = fwd.implied_discount_factor(1.0).unwrap();

        // With additive spread, DF = DF_base * exp(-spread * t)
        let expected = df_base * (-0.01_f64).exp();
        assert_relative_eq!(df_implied, expected, epsilon = 1e-10);
    }

    #[test]
    fn test_builder() {
        let base = sample_curve();

        let fwd = ForwardCurveBuilder::new()
            .base_curve(base)
            .tenor_months(6)
            .spread_bps(10.0)
            .build()
            .unwrap();

        assert_relative_eq!(fwd.tenor(), 0.5, epsilon = 1e-10);
        assert_relative_eq!(fwd.spread(), 0.001, epsilon = 1e-10);
    }

    #[test]
    fn test_with_tenor() {
        let base = sample_curve();
        let fwd_3m = ForwardCurve::new(base, 0.25);
        let fwd_6m = fwd_3m.with_tenor(0.5);

        assert_relative_eq!(fwd_6m.tenor(), 0.5, epsilon = 1e-10);
    }
}
