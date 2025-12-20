//! Parallel (uniform) curve bumping.
//!
//! A parallel bump shifts the entire curve by a constant amount,
//! useful for calculating DV01, PV01, and parallel sensitivity.

use std::sync::Arc;

use convex_core::types::Date;

use crate::term_structure::TermStructure;
use crate::value_type::ValueType;

/// A parallel (uniform) shift applied to a curve.
///
/// The shift is applied in basis points and affects the entire curve
/// uniformly. This is the standard bump for DV01/PV01 calculations.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::bumping::ParallelBump;
///
/// // Create a 1bp upward shift
/// let bump = ParallelBump::new(1.0);
///
/// // Apply to curve (zero-copy)
/// let bumped = bump.apply(&curve);
///
/// // Calculate DV01
/// let dv01 = bond.price(&curve)? - bond.price(&bumped)?;
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ParallelBump {
    /// Shift amount in basis points.
    shift_bps: f64,
}

impl ParallelBump {
    /// Creates a new parallel bump.
    ///
    /// # Arguments
    ///
    /// * `shift_bps` - Shift in basis points (1bp = 0.0001 = 0.01%)
    ///
    /// # Example
    ///
    /// ```rust
    /// use convex_curves::bumping::ParallelBump;
    ///
    /// let bump_up = ParallelBump::new(1.0);    // +1bp
    /// let bump_down = ParallelBump::new(-1.0); // -1bp
    /// let bump_25 = ParallelBump::new(25.0);   // +25bp
    /// ```
    #[must_use]
    pub fn new(shift_bps: f64) -> Self {
        Self { shift_bps }
    }

    /// Creates a 1bp upward shift (standard for DV01).
    #[must_use]
    pub fn one_bp_up() -> Self {
        Self::new(1.0)
    }

    /// Creates a 1bp downward shift.
    #[must_use]
    pub fn one_bp_down() -> Self {
        Self::new(-1.0)
    }

    /// Creates symmetric up/down bumps for central difference DV01.
    ///
    /// Returns (up_bump, down_bump) pair.
    #[must_use]
    pub fn symmetric(half_shift_bps: f64) -> (Self, Self) {
        (Self::new(half_shift_bps), Self::new(-half_shift_bps))
    }

    /// Returns the shift in basis points.
    #[must_use]
    pub fn shift_bps(&self) -> f64 {
        self.shift_bps
    }

    /// Returns the shift as a decimal (0.0001 = 1bp).
    #[must_use]
    pub fn shift_decimal(&self) -> f64 {
        self.shift_bps / 10_000.0
    }

    /// Applies the bump to a curve, returning a zero-copy bumped curve.
    ///
    /// The bumped curve computes shifted values on-the-fly without
    /// copying the underlying curve data.
    #[must_use]
    pub fn apply<'a, T: TermStructure>(&self, curve: &'a T) -> BumpedCurve<'a, T> {
        BumpedCurve {
            base: curve,
            shift_decimal: self.shift_decimal(),
        }
    }

    /// Applies the bump to an Arc-wrapped curve.
    #[must_use]
    pub fn apply_arc<T: TermStructure>(self, curve: Arc<T>) -> ArcBumpedCurve<T> {
        ArcBumpedCurve {
            base: curve,
            shift_decimal: self.shift_decimal(),
        }
    }
}

impl Default for ParallelBump {
    fn default() -> Self {
        Self::one_bp_up()
    }
}

/// A curve with a parallel shift applied.
///
/// This is a zero-copy wrapper that applies the shift on-the-fly
/// during value access. The shift is applied differently depending
/// on the underlying curve's value type:
///
/// - **Zero rates**: rate + shift
/// - **Discount factors**: df * exp(-shift * t)
/// - **Forward rates**: forward + shift
/// - **Hazard rates**: hazard + shift
/// - **Survival probabilities**: surv * exp(-shift * t)
#[derive(Debug, Clone, Copy)]
pub struct BumpedCurve<'a, T: TermStructure> {
    /// The base curve.
    base: &'a T,
    /// Shift in decimal form (0.0001 = 1bp).
    shift_decimal: f64,
}

impl<'a, T: TermStructure> BumpedCurve<'a, T> {
    /// Returns a reference to the base curve.
    #[must_use]
    pub fn base(&self) -> &T {
        self.base
    }

    /// Returns the applied shift in decimal form.
    #[must_use]
    pub fn shift_decimal(&self) -> f64 {
        self.shift_decimal
    }

    /// Returns the applied shift in basis points.
    #[must_use]
    pub fn shift_bps(&self) -> f64 {
        self.shift_decimal * 10_000.0
    }
}

impl<T: TermStructure> TermStructure for BumpedCurve<'_, T> {
    fn reference_date(&self) -> Date {
        self.base.reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        let base_value = self.base.value_at(t);

        match self.base.value_type() {
            // For rates: add shift directly
            ValueType::ZeroRate { .. }
            | ValueType::ForwardRate { .. }
            | ValueType::InstantaneousForward
            | ValueType::HazardRate
            | ValueType::ParSwapRate { .. } => base_value + self.shift_decimal,

            // For discount factors: df_bumped = df * exp(-shift * t)
            // Derivation: if zero rate increases by shift, then
            // df = exp(-r*t) becomes df' = exp(-(r+shift)*t) = df * exp(-shift*t)
            ValueType::DiscountFactor => {
                base_value * (-self.shift_decimal * t).exp()
            }

            // For survival probability: similar to discount factor
            // S(t) = exp(-h*t) becomes S'(t) = exp(-(h+shift)*t) = S * exp(-shift*t)
            ValueType::SurvivalProbability => {
                base_value * (-self.shift_decimal * t).exp()
            }

            // For credit spreads: add shift to spread
            ValueType::CreditSpread { .. } => base_value + self.shift_decimal,

            // For inflation and FX, shifts don't apply in the same way
            // but we provide additive shift as a reasonable default
            ValueType::InflationIndexRatio | ValueType::FxForwardPoints => {
                base_value + self.shift_decimal
            }
        }
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        self.base.tenor_bounds()
    }

    fn value_type(&self) -> ValueType {
        self.base.value_type()
    }

    fn derivative_at(&self, t: f64) -> Option<f64> {
        // For additive shifts, derivative is unchanged
        // For multiplicative (DF, survival), derivative changes
        match self.base.value_type() {
            ValueType::DiscountFactor | ValueType::SurvivalProbability => {
                // d/dt[f(t) * exp(-s*t)] = f'(t)*exp(-s*t) - s*f(t)*exp(-s*t)
                let base_value = self.base.value_at(t);
                let base_deriv = self.base.derivative_at(t)?;
                let exp_factor = (-self.shift_decimal * t).exp();
                Some(
                    base_deriv * exp_factor
                        - self.shift_decimal * base_value * exp_factor,
                )
            }
            _ => self.base.derivative_at(t),
        }
    }

    fn max_date(&self) -> Date {
        self.base.max_date()
    }
}

/// Arc-owned bumped curve for longer-lived scenarios.
///
/// Unlike `BumpedCurve`, this owns the base curve via Arc,
/// allowing the bumped curve to outlive any particular scope.
#[derive(Debug, Clone)]
pub struct ArcBumpedCurve<T: TermStructure> {
    /// The base curve (Arc-owned).
    base: Arc<T>,
    /// Shift in decimal form.
    shift_decimal: f64,
}

impl<T: TermStructure> ArcBumpedCurve<T> {
    /// Creates a new Arc-owned bumped curve.
    #[must_use]
    pub fn new(base: Arc<T>, shift_bps: f64) -> Self {
        Self {
            base,
            shift_decimal: shift_bps / 10_000.0,
        }
    }

    /// Returns a reference to the base curve.
    #[must_use]
    pub fn base(&self) -> &T {
        &self.base
    }

    /// Returns the shift in basis points.
    #[must_use]
    pub fn shift_bps(&self) -> f64 {
        self.shift_decimal * 10_000.0
    }
}

impl<T: TermStructure> TermStructure for ArcBumpedCurve<T> {
    fn reference_date(&self) -> Date {
        self.base.reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        let base_value = self.base.value_at(t);

        match self.base.value_type() {
            ValueType::ZeroRate { .. }
            | ValueType::ForwardRate { .. }
            | ValueType::InstantaneousForward
            | ValueType::HazardRate
            | ValueType::ParSwapRate { .. } => base_value + self.shift_decimal,

            ValueType::DiscountFactor => {
                base_value * (-self.shift_decimal * t).exp()
            }

            ValueType::SurvivalProbability => {
                base_value * (-self.shift_decimal * t).exp()
            }

            ValueType::CreditSpread { .. } => base_value + self.shift_decimal,

            ValueType::InflationIndexRatio | ValueType::FxForwardPoints => {
                base_value + self.shift_decimal
            }
        }
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        self.base.tenor_bounds()
    }

    fn value_type(&self) -> ValueType {
        self.base.value_type()
    }

    fn derivative_at(&self, t: f64) -> Option<f64> {
        match self.base.value_type() {
            ValueType::DiscountFactor | ValueType::SurvivalProbability => {
                let base_value = self.base.value_at(t);
                let base_deriv = self.base.derivative_at(t)?;
                let exp_factor = (-self.shift_decimal * t).exp();
                Some(
                    base_deriv * exp_factor
                        - self.shift_decimal * base_value * exp_factor,
                )
            }
            _ => self.base.derivative_at(t),
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

    fn sample_zero_curve() -> DiscreteCurve {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        let rates = vec![0.04, 0.045, 0.05, 0.055];

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

    fn sample_df_curve() -> DiscreteCurve {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        // DFs consistent with ~4-5.5% rates
        let dfs = vec![0.9608, 0.9139, 0.7788, 0.5769];

        DiscreteCurve::new(
            today,
            tenors,
            dfs,
            ValueType::DiscountFactor,
            InterpolationMethod::LogLinear,
        )
        .unwrap()
    }

    #[test]
    fn test_parallel_bump_creation() {
        let bump = ParallelBump::new(25.0);
        assert_relative_eq!(bump.shift_bps(), 25.0);
        assert_relative_eq!(bump.shift_decimal(), 0.0025);
    }

    #[test]
    fn test_one_bp_bump() {
        let bump = ParallelBump::one_bp_up();
        assert_relative_eq!(bump.shift_bps(), 1.0);
        assert_relative_eq!(bump.shift_decimal(), 0.0001);
    }

    #[test]
    fn test_symmetric_bumps() {
        let (up, down) = ParallelBump::symmetric(0.5);
        assert_relative_eq!(up.shift_bps(), 0.5);
        assert_relative_eq!(down.shift_bps(), -0.5);
    }

    #[test]
    fn test_zero_rate_bump() {
        let curve = sample_zero_curve();
        let bump = ParallelBump::new(50.0); // +50bp
        let bumped = bump.apply(&curve);

        // At 5Y: base = 5%, bumped = 5.5%
        let base_rate = curve.value_at(5.0);
        let bumped_rate = bumped.value_at(5.0);

        assert_relative_eq!(base_rate, 0.05, epsilon = 1e-10);
        assert_relative_eq!(bumped_rate, 0.055, epsilon = 1e-10);
        assert_relative_eq!(bumped_rate - base_rate, 0.005, epsilon = 1e-10);
    }

    #[test]
    fn test_df_bump() {
        let curve = sample_df_curve();
        let bump = ParallelBump::new(100.0); // +100bp
        let bumped = bump.apply(&curve);

        // For DF: df' = df * exp(-shift * t)
        let t = 5.0;
        let base_df = curve.value_at(t);
        let bumped_df = bumped.value_at(t);

        let expected_bumped_df = base_df * (-0.01 * t).exp();
        assert_relative_eq!(bumped_df, expected_bumped_df, epsilon = 1e-10);

        // Bumped DF should be smaller (higher rates = lower DF)
        assert!(bumped_df < base_df);
    }

    #[test]
    fn test_bump_preserves_tenor_bounds() {
        let curve = sample_zero_curve();
        let bump = ParallelBump::new(1.0);
        let bumped = bump.apply(&curve);

        assert_eq!(curve.tenor_bounds(), bumped.tenor_bounds());
    }

    #[test]
    fn test_bump_preserves_value_type() {
        let curve = sample_zero_curve();
        let bump = ParallelBump::new(1.0);
        let bumped = bump.apply(&curve);

        assert_eq!(curve.value_type(), bumped.value_type());
    }

    #[test]
    fn test_bump_preserves_reference_date() {
        let curve = sample_zero_curve();
        let bump = ParallelBump::new(1.0);
        let bumped = bump.apply(&curve);

        assert_eq!(curve.reference_date(), bumped.reference_date());
    }

    #[test]
    fn test_negative_bump() {
        let curve = sample_zero_curve();
        let bump = ParallelBump::new(-25.0); // -25bp
        let bumped = bump.apply(&curve);

        let base_rate = curve.value_at(5.0);
        let bumped_rate = bumped.value_at(5.0);

        assert!(bumped_rate < base_rate);
        assert_relative_eq!(base_rate - bumped_rate, 0.0025, epsilon = 1e-10);
    }

    #[test]
    fn test_arc_bumped_curve() {
        let curve = Arc::new(sample_zero_curve());
        let bump = ParallelBump::new(10.0);
        let bumped = bump.apply_arc(curve.clone());

        let base_rate = curve.value_at(2.0);
        let bumped_rate = bumped.value_at(2.0);

        assert_relative_eq!(bumped_rate - base_rate, 0.001, epsilon = 1e-10);
    }

    #[test]
    fn test_zero_bump() {
        let curve = sample_zero_curve();
        let bump = ParallelBump::new(0.0);
        let bumped = bump.apply(&curve);

        // Zero bump should give same values
        for t in [1.0, 2.0, 5.0, 10.0] {
            assert_relative_eq!(curve.value_at(t), bumped.value_at(t), epsilon = 1e-10);
        }
    }

    #[test]
    fn test_derivative_zero_rate() {
        let curve = sample_zero_curve();
        let bump = ParallelBump::new(50.0);
        let bumped = bump.apply(&curve);

        // For additive shifts, derivative should be unchanged
        if let (Some(base_deriv), Some(bumped_deriv)) =
            (curve.derivative_at(3.0), bumped.derivative_at(3.0))
        {
            assert_relative_eq!(base_deriv, bumped_deriv, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_dv01_calculation() {
        let curve = sample_zero_curve();

        // Calculate DV01 using symmetric bumps for better accuracy
        let (up, down) = ParallelBump::symmetric(0.5);
        let curve_up = up.apply(&curve);
        let curve_down = down.apply(&curve);

        // Verify the bumps are correct
        let base = curve.value_at(5.0);
        let up_val = curve_up.value_at(5.0);
        let down_val = curve_down.value_at(5.0);

        assert_relative_eq!(up_val - base, 0.00005, epsilon = 1e-10);
        assert_relative_eq!(base - down_val, 0.00005, epsilon = 1e-10);
    }
}
