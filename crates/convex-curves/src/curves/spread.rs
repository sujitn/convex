//! Spread curves that apply an additive or multiplicative spread over a base curve.
//!
//! A [`SpreadCurve`] allows you to create credit curves, basis curves, or other
//! curve adjustments relative to a base discounting curve.

use std::sync::Arc;

use convex_core::Date;
use convex_math::interpolation::{Interpolator, LinearInterpolator};

use crate::error::{CurveError, CurveResult};
use crate::traits::Curve;

/// Type of spread adjustment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpreadType {
    /// Additive spread: z_spread(t) = z_base(t) + spread(t)
    ///
    /// This is the most common type, used for Z-spreads and credit spreads.
    /// The discount factor is: DF(t) = exp(-(r_base + spread) * t)
    Additive,

    /// Multiplicative spread: DF_spread(t) = DF_base(t) * spread_factor(t)
    ///
    /// Used for certain FX and basis adjustments.
    Multiplicative,
}

impl Default for SpreadType {
    fn default() -> Self {
        Self::Additive
    }
}

impl std::fmt::Display for SpreadType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Additive => write!(f, "Additive"),
            Self::Multiplicative => write!(f, "Multiplicative"),
        }
    }
}

/// A spread curve that adds or multiplies a spread over a base curve.
///
/// Spread curves are used for:
/// - Credit spread curves (Z-spread over government curve)
/// - Basis curves (tenor basis, cross-currency basis)
/// - FX forward curves
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::prelude::*;
/// use std::sync::Arc;
///
/// // Create a base OIS discount curve
/// let base_curve = Arc::new(
///     DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap())
///         .add_pillar(1.0, 0.96)
///         .add_pillar(5.0, 0.80)
///         .build()
///         .unwrap()
/// );
///
/// // Add a 100bp credit spread
/// let credit_curve = SpreadCurve::constant_spread(base_curve, 0.01, SpreadType::Additive);
///
/// // The credit curve will have lower discount factors (higher rates)
/// let df_base = base_curve.discount_factor(1.0).unwrap();
/// let df_credit = credit_curve.discount_factor(1.0).unwrap();
/// assert!(df_credit < df_base);
/// ```
#[derive(Clone)]
pub struct SpreadCurve {
    /// Base curve for discounting.
    base_curve: Arc<dyn Curve>,
    /// Spread type (additive or multiplicative).
    spread_type: SpreadType,
    /// Pillar times for term structure of spread.
    pillar_times: Vec<f64>,
    /// Spread values at each pillar.
    spreads: Vec<f64>,
    /// Interpolator for spreads.
    interpolator: Option<Arc<dyn Interpolator>>,
    /// Allow extrapolation.
    allow_extrapolation: bool,
}

impl std::fmt::Debug for SpreadCurve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpreadCurve")
            .field("spread_type", &self.spread_type)
            .field("pillar_times", &self.pillar_times)
            .field("spreads", &self.spreads)
            .field("allow_extrapolation", &self.allow_extrapolation)
            .finish()
    }
}

impl SpreadCurve {
    /// Creates a spread curve with a term structure of spreads.
    ///
    /// # Arguments
    ///
    /// * `base_curve` - The underlying curve
    /// * `pillar_times` - Times in years for each spread pillar
    /// * `spreads` - Spread values at each pillar
    /// * `spread_type` - Type of spread (additive or multiplicative)
    pub fn new(
        base_curve: Arc<dyn Curve>,
        pillar_times: Vec<f64>,
        spreads: Vec<f64>,
        spread_type: SpreadType,
    ) -> CurveResult<Self> {
        if pillar_times.is_empty() {
            return Err(CurveError::EmptyCurve);
        }

        if pillar_times.len() != spreads.len() {
            return Err(CurveError::invalid_data(format!(
                "pillar_times ({}) and spreads ({}) must have same length",
                pillar_times.len(),
                spreads.len()
            )));
        }

        let mut curve = Self {
            base_curve,
            spread_type,
            pillar_times,
            spreads,
            interpolator: None,
            allow_extrapolation: false,
        };

        curve.build_interpolator()?;

        Ok(curve)
    }

    /// Creates a spread curve with a constant (flat) spread.
    ///
    /// # Arguments
    ///
    /// * `base_curve` - The underlying curve
    /// * `spread` - Constant spread value
    /// * `spread_type` - Type of spread
    pub fn constant_spread(
        base_curve: Arc<dyn Curve>,
        spread: f64,
        spread_type: SpreadType,
    ) -> Self {
        Self {
            base_curve,
            spread_type,
            pillar_times: vec![0.0, 100.0], // Flat from 0 to 100Y
            spreads: vec![spread, spread],
            interpolator: None,
            allow_extrapolation: true,
        }
    }

    /// Enables extrapolation beyond the spread curve range.
    #[must_use]
    pub fn with_extrapolation(mut self) -> Self {
        self.allow_extrapolation = true;
        self
    }

    /// Returns the base curve.
    #[must_use]
    pub fn base_curve(&self) -> &dyn Curve {
        self.base_curve.as_ref()
    }

    /// Returns the spread type.
    #[must_use]
    pub fn spread_type(&self) -> SpreadType {
        self.spread_type
    }

    /// Returns the pillar times.
    #[must_use]
    pub fn pillar_times(&self) -> &[f64] {
        &self.pillar_times
    }

    /// Returns the spread values.
    #[must_use]
    pub fn spreads(&self) -> &[f64] {
        &self.spreads
    }

    /// Returns the spread at time t.
    pub fn spread_at(&self, t: f64) -> CurveResult<f64> {
        if self.pillar_times.len() == 2
            && self.spreads[0] == self.spreads[1]
            && self.allow_extrapolation
        {
            // Constant spread - fast path
            return Ok(self.spreads[0]);
        }

        if let Some(ref interp) = self.interpolator {
            interp
                .interpolate(t)
                .map_err(|e| CurveError::InterpolationFailed {
                    reason: e.to_string(),
                })
        } else {
            // Fall back to linear interpolation
            if t <= self.pillar_times[0] {
                Ok(self.spreads[0])
            } else if t >= *self.pillar_times.last().unwrap() {
                Ok(*self.spreads.last().unwrap())
            } else {
                // Find bracketing pillars
                let idx = self
                    .pillar_times
                    .iter()
                    .position(|&pt| pt > t)
                    .unwrap_or(self.pillar_times.len() - 1);
                let t0 = self.pillar_times[idx - 1];
                let t1 = self.pillar_times[idx];
                let s0 = self.spreads[idx - 1];
                let s1 = self.spreads[idx];

                let weight = (t - t0) / (t1 - t0);
                Ok(s0 + weight * (s1 - s0))
            }
        }
    }

    /// Builds the interpolator for spread values.
    fn build_interpolator(&mut self) -> CurveResult<()> {
        if self.pillar_times.len() >= 2 {
            let interp = LinearInterpolator::new(
                self.pillar_times.clone(),
                self.spreads.clone(),
            )
            .map_err(|e| CurveError::InterpolationFailed {
                reason: e.to_string(),
            })?
            .with_extrapolation();

            self.interpolator = Some(Arc::new(interp));
        }
        Ok(())
    }
}

impl Curve for SpreadCurve {
    fn discount_factor(&self, t: f64) -> CurveResult<f64> {
        if t <= 0.0 {
            return Ok(1.0);
        }

        let base_df = self.base_curve.discount_factor(t)?;
        let spread = self.spread_at(t)?;

        match self.spread_type {
            SpreadType::Additive => {
                // DF_spread = DF_base * exp(-spread * t)
                // Equivalent to adding spread to continuously compounded rate
                Ok(base_df * (-spread * t).exp())
            }
            SpreadType::Multiplicative => {
                // DF_spread = DF_base * spread_factor
                // spread is the multiplicative factor directly
                Ok(base_df * spread)
            }
        }
    }

    fn reference_date(&self) -> Date {
        self.base_curve.reference_date()
    }

    fn max_date(&self) -> Date {
        self.base_curve.max_date()
    }
}

/// Builder for spread curves.
#[derive(Clone)]
pub struct SpreadCurveBuilder {
    base_curve: Option<Arc<dyn Curve>>,
    pillars: Vec<(f64, f64)>, // (time, spread)
    spread_type: SpreadType,
    allow_extrapolation: bool,
}

impl Default for SpreadCurveBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SpreadCurveBuilder {
    /// Creates a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            base_curve: None,
            pillars: Vec::new(),
            spread_type: SpreadType::Additive,
            allow_extrapolation: false,
        }
    }

    /// Sets the base curve.
    #[must_use]
    pub fn base_curve(mut self, curve: Arc<dyn Curve>) -> Self {
        self.base_curve = Some(curve);
        self
    }

    /// Adds a spread pillar.
    #[must_use]
    pub fn add_spread(mut self, time: f64, spread: f64) -> Self {
        self.pillars.push((time, spread));
        self
    }

    /// Adds a spread in basis points.
    #[must_use]
    pub fn add_spread_bps(mut self, time: f64, spread_bps: f64) -> Self {
        self.pillars.push((time, spread_bps / 10000.0));
        self
    }

    /// Sets the spread type.
    #[must_use]
    pub fn spread_type(mut self, st: SpreadType) -> Self {
        self.spread_type = st;
        self
    }

    /// Enables extrapolation.
    #[must_use]
    pub fn with_extrapolation(mut self) -> Self {
        self.allow_extrapolation = true;
        self
    }

    /// Builds the spread curve.
    pub fn build(mut self) -> CurveResult<SpreadCurve> {
        let base = self.base_curve.ok_or_else(|| {
            CurveError::invalid_data("Base curve is required")
        })?;

        if self.pillars.is_empty() {
            return Err(CurveError::EmptyCurve);
        }

        // Sort by time
        self.pillars.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        let (times, spreads): (Vec<f64>, Vec<f64>) = self.pillars.into_iter().unzip();

        let mut curve = SpreadCurve::new(base, times, spreads, self.spread_type)?;

        if self.allow_extrapolation {
            curve = curve.with_extrapolation();
        }

        Ok(curve)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscountCurveBuilder;
    use crate::interpolation::InterpolationMethod;
    use approx::assert_relative_eq;

    fn base_curve() -> Arc<dyn Curve> {
        Arc::new(
            DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap())
                .add_pillar(1.0, 0.96)
                .add_pillar(2.0, 0.92)
                .add_pillar(5.0, 0.80)
                .add_pillar(10.0, 0.65)
                .with_interpolation(InterpolationMethod::LogLinear)
                .with_extrapolation()
                .build()
                .unwrap()
        )
    }

    #[test]
    fn test_constant_additive_spread() {
        let base = base_curve();
        let spread_curve = SpreadCurve::constant_spread(base.clone(), 0.01, SpreadType::Additive);

        // At 1Y, base DF = 0.96
        // With 1% additive spread: DF = 0.96 * exp(-0.01 * 1) ≈ 0.9505
        let df_spread = spread_curve.discount_factor(1.0).unwrap();
        let df_base = base.discount_factor(1.0).unwrap();

        let expected = df_base * (-0.01_f64).exp();
        assert_relative_eq!(df_spread, expected, epsilon = 1e-10);

        // Spread curve DF should be lower (higher rate)
        assert!(df_spread < df_base);
    }

    #[test]
    fn test_term_structure_spread() {
        let base = base_curve();

        let spread_curve = SpreadCurve::new(
            base.clone(),
            vec![1.0, 2.0, 5.0, 10.0],
            vec![0.005, 0.008, 0.012, 0.015], // 50bp, 80bp, 120bp, 150bp
            SpreadType::Additive,
        )
        .unwrap()
        .with_extrapolation();

        // Check spread values at pillars
        assert_relative_eq!(spread_curve.spread_at(1.0).unwrap(), 0.005, epsilon = 1e-10);
        assert_relative_eq!(spread_curve.spread_at(5.0).unwrap(), 0.012, epsilon = 1e-10);

        // Check interpolated spread
        let spread_3y = spread_curve.spread_at(3.0).unwrap();
        assert!(spread_3y > 0.008 && spread_3y < 0.012);
    }

    #[test]
    fn test_multiplicative_spread() {
        let base = base_curve();

        // Multiplicative spread of 0.99 (1% discount)
        let spread_curve = SpreadCurve::constant_spread(base.clone(), 0.99, SpreadType::Multiplicative);

        let df_base = base.discount_factor(1.0).unwrap();
        let df_spread = spread_curve.discount_factor(1.0).unwrap();

        assert_relative_eq!(df_spread, df_base * 0.99, epsilon = 1e-10);
    }

    #[test]
    fn test_spread_curve_builder() {
        let base = base_curve();

        let spread_curve = SpreadCurveBuilder::new()
            .base_curve(base)
            .add_spread_bps(1.0, 50.0)   // 50bp
            .add_spread_bps(5.0, 100.0)  // 100bp
            .add_spread_bps(10.0, 150.0) // 150bp
            .spread_type(SpreadType::Additive)
            .with_extrapolation()
            .build()
            .unwrap();

        assert_relative_eq!(spread_curve.spread_at(1.0).unwrap(), 0.005, epsilon = 1e-10);
        assert_relative_eq!(spread_curve.spread_at(10.0).unwrap(), 0.015, epsilon = 1e-10);
    }

    #[test]
    fn test_spread_curve_inherits_reference_date() {
        let base = base_curve();
        let spread_curve = SpreadCurve::constant_spread(base.clone(), 0.01, SpreadType::Additive);

        assert_eq!(spread_curve.reference_date(), base.reference_date());
    }

    #[test]
    fn test_zero_time_returns_one() {
        let base = base_curve();
        let spread_curve = SpreadCurve::constant_spread(base, 0.01, SpreadType::Additive);

        assert_relative_eq!(spread_curve.discount_factor(0.0).unwrap(), 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_forward_rate_with_spread() {
        let base = base_curve();
        let spread_curve = SpreadCurve::constant_spread(base.clone(), 0.01, SpreadType::Additive);

        let fwd_base = base.forward_rate(1.0, 2.0).unwrap();
        let fwd_spread = spread_curve.forward_rate(1.0, 2.0).unwrap();

        // Forward rate with spread should be higher
        // The difference should be approximately the spread
        assert!(fwd_spread > fwd_base);
    }
}
