//! Curve transformation utilities.
//!
//! This module provides wrappers around existing curves that apply transformations:
//!
//! - [`ShiftedCurve`]: Applies a constant parallel shift to rates
//! - [`ScaledCurve`]: Scales all rates by a constant factor
//! - [`BlendedCurve`]: Blends two curves with configurable weights
//!
//! # Motivation
//!
//! These utilities are essential for:
//! - **Spread calculations**: Z-spread, OAS add constant spreads to curves
//! - **Risk sensitivity**: Parallel shifts for duration/convexity calculations
//! - **Scenario analysis**: Stress testing with various rate environments
//!
//! # Example
//!
//! ```rust
//! use convex_curves::curves::shifted::ShiftedCurve;
//! use convex_curves::traits::Curve;
//! use convex_curves::curves::DiscountCurveBuilder;
//! use convex_core::types::Date;
//!
//! let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
//! let base_curve = DiscountCurveBuilder::new(ref_date)
//!     .add_pillar(1.0, 0.95)
//!     .add_pillar(5.0, 0.75)
//!     .build()
//!     .unwrap();
//!
//! // Create a curve with +50bp parallel shift
//! let shifted = ShiftedCurve::new(&base_curve, 0.0050);
//!
//! // Discount factors will be lower (higher rates)
//! let base_df = base_curve.discount_factor(1.0).unwrap();
//! let shifted_df = shifted.discount_factor(1.0).unwrap();
//! assert!(shifted_df < base_df);
//! ```

use convex_core::types::Date;

use crate::compounding::Compounding;
use crate::error::CurveResult;
use crate::traits::Curve;

/// A curve wrapper that applies a constant spread to all rates.
///
/// The spread is applied to the continuous zero rate:
/// `r_shifted = r_base + spread`
///
/// Which results in discount factors:
/// `DF_shifted(t) = DF_base(t) * exp(-spread * t)`
///
/// # Use Cases
///
/// - **Z-spread calculation**: Find spread that equates PV to market price
/// - **OAS calculation**: Spread over option-adjusted tree
/// - **Risk sensitivity**: Parallel yield curve shifts for duration/convexity
/// - **Scenario analysis**: Stress testing rate environments
///
/// # Example
///
/// ```rust
/// use convex_curves::curves::shifted::ShiftedCurve;
/// use convex_curves::traits::Curve;
/// use convex_curves::curves::DiscountCurveBuilder;
/// use convex_core::types::Date;
///
/// let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
/// let base = DiscountCurveBuilder::new(ref_date)
///     .add_pillar(1.0, 0.95)
///     .add_pillar(2.0, 0.90)
///     .build()
///     .unwrap();
///
/// let shifted = ShiftedCurve::new(&base, 0.01); // +100 bps
/// let df = shifted.discount_factor(1.0).unwrap();
/// ```
pub struct ShiftedCurve<'a, C: Curve + ?Sized> {
    base: &'a C,
    spread: f64,
}

impl<'a, C: Curve + ?Sized> ShiftedCurve<'a, C> {
    /// Creates a new shifted curve.
    ///
    /// # Arguments
    ///
    /// * `base` - The underlying curve
    /// * `spread` - The spread to add (as decimal, e.g., 0.01 for 100 bps)
    pub fn new(base: &'a C, spread: f64) -> Self {
        Self { base, spread }
    }

    /// Returns the spread applied to this curve.
    pub fn spread(&self) -> f64 {
        self.spread
    }

    /// Returns a reference to the base curve.
    pub fn base(&self) -> &C {
        self.base
    }

    /// Creates a new shifted curve with a different spread.
    pub fn with_spread(&self, spread: f64) -> ShiftedCurve<'a, C> {
        ShiftedCurve {
            base: self.base,
            spread,
        }
    }
}

impl<C: Curve + ?Sized> Curve for ShiftedCurve<'_, C> {
    fn discount_factor(&self, t: f64) -> CurveResult<f64> {
        let base_df = self.base.discount_factor(t)?;

        if t <= 0.0 {
            return Ok(base_df);
        }

        // Apply spread: DF_shifted = DF_base * exp(-spread * t)
        let adjustment = (-self.spread * t).exp();
        Ok(base_df * adjustment)
    }

    fn zero_rate(&self, t: f64, compounding: Compounding) -> CurveResult<f64> {
        // For continuous compounding, simply add the spread
        let base_rate = self.base.zero_rate(t, compounding)?;

        if compounding == Compounding::Continuous {
            Ok(base_rate + self.spread)
        } else {
            // For other compounding, compute from shifted DF
            let df = self.discount_factor(t)?;
            Ok(compounding.zero_rate(df, t))
        }
    }

    fn forward_rate(&self, t1: f64, t2: f64) -> CurveResult<f64> {
        if t2 <= t1 {
            return Ok(0.0);
        }

        let df1 = self.discount_factor(t1)?;
        let df2 = self.discount_factor(t2)?;

        if df2 <= 0.0 {
            return Ok(0.0);
        }

        let tau = t2 - t1;
        Ok((df1 / df2 - 1.0) / tau)
    }

    fn instantaneous_forward(&self, t: f64) -> CurveResult<f64> {
        // Instantaneous forward = base forward + spread
        let base_inst = self.base.instantaneous_forward(t)?;
        Ok(base_inst + self.spread)
    }

    fn reference_date(&self) -> Date {
        self.base.reference_date()
    }

    fn max_date(&self) -> Date {
        self.base.max_date()
    }
}

/// A curve that scales all rates by a constant factor.
///
/// This is useful for multiplicative stress scenarios.
///
/// # Formula
///
/// For continuous compounding:
/// - `r_scaled = r_base * factor`
/// - `DF_scaled = DF_base^factor`
///
/// # Example
///
/// ```rust
/// use convex_curves::curves::shifted::ScaledCurve;
/// use convex_curves::traits::Curve;
/// use convex_curves::curves::DiscountCurveBuilder;
/// use convex_core::types::Date;
///
/// let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
/// let base = DiscountCurveBuilder::new(ref_date)
///     .add_pillar(1.0, 0.95)
///     .add_pillar(2.0, 0.90)
///     .build()
///     .unwrap();
///
/// // Scale rates by 1.5x (50% increase)
/// let scaled = ScaledCurve::new(&base, 1.5);
/// ```
pub struct ScaledCurve<'a, C: Curve + ?Sized> {
    base: &'a C,
    factor: f64,
}

impl<'a, C: Curve + ?Sized> ScaledCurve<'a, C> {
    /// Creates a new scaled curve.
    ///
    /// # Arguments
    ///
    /// * `base` - The underlying curve
    /// * `factor` - The scaling factor (e.g., 1.1 for +10%)
    pub fn new(base: &'a C, factor: f64) -> Self {
        Self { base, factor }
    }

    /// Returns the scaling factor.
    pub fn factor(&self) -> f64 {
        self.factor
    }

    /// Returns a reference to the base curve.
    pub fn base(&self) -> &C {
        self.base
    }
}

impl<C: Curve + ?Sized> Curve for ScaledCurve<'_, C> {
    fn discount_factor(&self, t: f64) -> CurveResult<f64> {
        // Scale the continuous zero rate, then convert back to DF
        // r_scaled = r_base * factor
        // DF_scaled = exp(-r_scaled * t) = DF_base^factor
        let base_df = self.base.discount_factor(t)?;

        if base_df <= 0.0 {
            return Ok(0.0);
        }

        Ok(base_df.powf(self.factor))
    }

    fn reference_date(&self) -> Date {
        self.base.reference_date()
    }

    fn max_date(&self) -> Date {
        self.base.max_date()
    }
}

/// A curve composed of two curves with a blend function.
///
/// Useful for transition scenarios between different rate environments.
///
/// # Formula
///
/// `DF = DF1^weight1 * DF2^(1-weight1)` (geometric blend)
///
/// This is equivalent to:
/// `r = weight1 * r1 + (1-weight1) * r2` (arithmetic blend of rates)
///
/// # Example
///
/// ```rust
/// use convex_curves::curves::shifted::BlendedCurve;
/// use convex_curves::traits::Curve;
/// use convex_curves::curves::DiscountCurveBuilder;
/// use convex_core::types::Date;
///
/// let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
///
/// let curve1 = DiscountCurveBuilder::new(ref_date)
///     .add_pillar(1.0, 0.96)
///     .add_pillar(2.0, 0.92)
///     .build()
///     .unwrap();
///
/// let curve2 = DiscountCurveBuilder::new(ref_date)
///     .add_pillar(1.0, 0.94)
///     .add_pillar(2.0, 0.88)
///     .build()
///     .unwrap();
///
/// // 50% weight to each curve
/// let blended = BlendedCurve::new(&curve1, &curve2, 0.5);
/// ```
pub struct BlendedCurve<'a, C1: Curve + ?Sized, C2: Curve + ?Sized> {
    curve1: &'a C1,
    curve2: &'a C2,
    weight1: f64,
}

impl<'a, C1: Curve + ?Sized, C2: Curve + ?Sized> BlendedCurve<'a, C1, C2> {
    /// Creates a new blended curve.
    ///
    /// # Arguments
    ///
    /// * `curve1` - First curve
    /// * `curve2` - Second curve
    /// * `weight1` - Weight for first curve (0.0 to 1.0)
    ///
    /// The result is: `DF = DF1^weight1 * DF2^(1-weight1)`
    pub fn new(curve1: &'a C1, curve2: &'a C2, weight1: f64) -> Self {
        Self {
            curve1,
            curve2,
            weight1: weight1.clamp(0.0, 1.0),
        }
    }

    /// Returns the weight of the first curve.
    pub fn weight1(&self) -> f64 {
        self.weight1
    }

    /// Returns the weight of the second curve.
    pub fn weight2(&self) -> f64 {
        1.0 - self.weight1
    }
}

impl<C1: Curve + ?Sized, C2: Curve + ?Sized> Curve for BlendedCurve<'_, C1, C2> {
    fn discount_factor(&self, t: f64) -> CurveResult<f64> {
        let df1 = self.curve1.discount_factor(t)?;
        let df2 = self.curve2.discount_factor(t)?;

        if df1 <= 0.0 || df2 <= 0.0 {
            return Ok(0.0);
        }

        // Geometric blend: DF = DF1^w * DF2^(1-w)
        let weight2 = 1.0 - self.weight1;
        Ok(df1.powf(self.weight1) * df2.powf(weight2))
    }

    fn reference_date(&self) -> Date {
        self.curve1.reference_date()
    }

    fn max_date(&self) -> Date {
        // Use the minimum of the two max dates
        let max1 = self.curve1.max_date();
        let max2 = self.curve2.max_date();
        if max1 < max2 {
            max1
        } else {
            max2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscountCurveBuilder;
    use approx::assert_relative_eq;

    fn create_flat_curve(rate: f64, ref_date: Date) -> impl Curve {
        DiscountCurveBuilder::new(ref_date)
            .add_pillar(0.25, (-rate * 0.25).exp())
            .add_pillar(0.5, (-rate * 0.5).exp())
            .add_pillar(1.0, (-rate * 1.0).exp())
            .add_pillar(2.0, (-rate * 2.0).exp())
            .add_pillar(5.0, (-rate * 5.0).exp())
            .add_pillar(10.0, (-rate * 10.0).exp())
            .with_extrapolation()
            .build()
            .unwrap()
    }

    #[test]
    fn test_shifted_curve_discount_factor() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let base = create_flat_curve(0.05, ref_date);
        let shifted = ShiftedCurve::new(&base, 0.01); // +100 bps

        let t = 1.0;
        let base_df = base.discount_factor(t).unwrap();
        let shifted_df = shifted.discount_factor(t).unwrap();

        // DF_shifted = DF_base * exp(-spread * t)
        let expected = base_df * (-0.01 * t).exp();
        assert_relative_eq!(shifted_df, expected, epsilon = 1e-6);

        // Shifted DF should be lower (higher rate = more discounting)
        assert!(shifted_df < base_df);
    }

    #[test]
    fn test_shifted_curve_zero_rate() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let base = create_flat_curve(0.05, ref_date);
        let shifted = ShiftedCurve::new(&base, 0.01);

        let base_rate = base.zero_rate(1.0, Compounding::Continuous).unwrap();
        let shifted_rate = shifted.zero_rate(1.0, Compounding::Continuous).unwrap();

        // Shifted rate should be ~1% higher
        assert_relative_eq!(shifted_rate - base_rate, 0.01, epsilon = 1e-4);
    }

    #[test]
    fn test_shifted_curve_negative_spread() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let base = create_flat_curve(0.05, ref_date);
        let shifted = ShiftedCurve::new(&base, -0.02); // -200 bps

        let base_df = base.discount_factor(1.0).unwrap();
        let shifted_df = shifted.discount_factor(1.0).unwrap();

        // Negative spread = higher DF (less discounting)
        assert!(shifted_df > base_df);
    }

    #[test]
    fn test_shifted_curve_forward_rate() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let base = create_flat_curve(0.05, ref_date);
        let shifted = ShiftedCurve::new(&base, 0.01);

        let base_fwd = base.forward_rate(1.0, 2.0).unwrap();
        let shifted_fwd = shifted.forward_rate(1.0, 2.0).unwrap();

        // Forward rate should also be higher
        assert!(shifted_fwd > base_fwd);
    }

    #[test]
    fn test_shifted_curve_reference_date() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let base = create_flat_curve(0.05, ref_date);
        let shifted = ShiftedCurve::new(&base, 0.01);

        assert_eq!(shifted.reference_date(), ref_date);
    }

    #[test]
    fn test_scaled_curve() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let base = create_flat_curve(0.05, ref_date);
        let scaled = ScaledCurve::new(&base, 1.5); // 50% higher rates

        let t = 1.0;
        let base_df = base.discount_factor(t).unwrap();
        let scaled_df = scaled.discount_factor(t).unwrap();

        // DF_scaled = DF_base^factor
        let expected = base_df.powf(1.5);
        assert_relative_eq!(scaled_df, expected, epsilon = 1e-6);

        // Higher rates = lower DF
        assert!(scaled_df < base_df);
    }

    #[test]
    fn test_blended_curve() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let curve1 = create_flat_curve(0.04, ref_date);
        let curve2 = create_flat_curve(0.06, ref_date);
        let blended = BlendedCurve::new(&curve1, &curve2, 0.5); // Equal weight

        let t = 1.0;
        let df1 = curve1.discount_factor(t).unwrap();
        let df2 = curve2.discount_factor(t).unwrap();
        let blended_df = blended.discount_factor(t).unwrap();

        // Geometric mean
        let expected = (df1 * df2).sqrt();
        assert_relative_eq!(blended_df, expected, epsilon = 1e-6);

        // Blended should be between the two
        assert!(blended_df > df2); // df2 is lower (higher rate)
        assert!(blended_df < df1); // df1 is higher (lower rate)
    }

    #[test]
    fn test_with_spread() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let base = create_flat_curve(0.05, ref_date);
        let shifted1 = ShiftedCurve::new(&base, 0.01);
        let shifted2 = shifted1.with_spread(0.02);

        assert_eq!(shifted1.spread(), 0.01);
        assert_eq!(shifted2.spread(), 0.02);
    }

    #[test]
    fn test_zero_time_no_shift() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let base = create_flat_curve(0.05, ref_date);
        let shifted = ShiftedCurve::new(&base, 0.01);

        // At t=0, discount factor should be 1.0 (no shift effect)
        let base_df = base.discount_factor(0.0).unwrap();
        let shifted_df = shifted.discount_factor(0.0).unwrap();

        assert_relative_eq!(base_df, shifted_df, epsilon = 1e-10);
    }

    #[test]
    fn test_instantaneous_forward() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let base = create_flat_curve(0.05, ref_date);
        let shifted = ShiftedCurve::new(&base, 0.01);

        let base_inst = base.instantaneous_forward(1.0).unwrap();
        let shifted_inst = shifted.instantaneous_forward(1.0).unwrap();

        // Instantaneous forward should also be shifted
        assert_relative_eq!(shifted_inst - base_inst, 0.01, epsilon = 1e-4);
    }
}
