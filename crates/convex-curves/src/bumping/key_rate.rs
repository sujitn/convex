//! Key-rate duration bumping.
//!
//! Key-rate bumps apply localized shifts at specific tenors,
//! allowing calculation of partial durations and key-rate exposures.
//!
//! The bump profile is typically triangular, centered at the key tenor,
//! with weights linearly decreasing to zero at adjacent key tenors.

use std::sync::Arc;

use convex_core::types::Date;

use crate::term_structure::TermStructure;
use crate::value_type::ValueType;

/// Standard key-rate tenors used in the industry.
pub const STANDARD_KEY_TENORS: &[f64] = &[
    0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 15.0, 20.0, 25.0, 30.0,
];

/// Key-rate duration bump at a specific tenor.
///
/// Applies a triangular bump centered at the key tenor, with weights
/// linearly decreasing to zero at adjacent key tenors.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::bumping::KeyRateBump;
///
/// // Bump at 5Y tenor
/// let kr_bump = KeyRateBump::new(5.0, 1.0);  // 5Y, 1bp
/// let bumped = kr_bump.apply(&curve);
///
/// // Calculate key-rate DV01 at 5Y
/// let kr_dv01_5y = bond.price(&curve)? - bond.price(&bumped)?;
/// ```
#[derive(Debug, Clone, Copy)]
pub struct KeyRateBump {
    /// The key tenor (in years) where bump is centered.
    key_tenor: f64,
    /// Shift amount in basis points.
    shift_bps: f64,
    /// Left neighbor tenor (for triangular weight).
    left_tenor: Option<f64>,
    /// Right neighbor tenor (for triangular weight).
    right_tenor: Option<f64>,
}

impl KeyRateBump {
    /// Creates a new key-rate bump with automatic neighbor detection.
    ///
    /// Uses standard key-rate tenors to determine the triangular weight profile.
    ///
    /// # Arguments
    ///
    /// * `key_tenor` - The tenor (in years) at which to center the bump
    /// * `shift_bps` - Shift in basis points (1bp = 0.0001)
    #[must_use]
    pub fn new(key_tenor: f64, shift_bps: f64) -> Self {
        let (left, right) = Self::find_neighbors(key_tenor, STANDARD_KEY_TENORS);
        Self {
            key_tenor,
            shift_bps,
            left_tenor: left,
            right_tenor: right,
        }
    }

    /// Creates a key-rate bump with custom neighbor tenors.
    ///
    /// Use this when you have a non-standard set of key tenors.
    #[must_use]
    pub fn with_neighbors(
        key_tenor: f64,
        shift_bps: f64,
        left_tenor: Option<f64>,
        right_tenor: Option<f64>,
    ) -> Self {
        Self {
            key_tenor,
            shift_bps,
            left_tenor,
            right_tenor,
        }
    }

    /// Creates a set of standard key-rate bumps for a full profile.
    ///
    /// Returns one bump for each standard key tenor.
    #[must_use]
    pub fn standard_profile(shift_bps: f64) -> Vec<Self> {
        STANDARD_KEY_TENORS
            .iter()
            .map(|&t| Self::new(t, shift_bps))
            .collect()
    }

    /// Creates a set of key-rate bumps for custom tenors.
    #[must_use]
    pub fn custom_profile(tenors: &[f64], shift_bps: f64) -> Vec<Self> {
        tenors
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                let left = if i > 0 { Some(tenors[i - 1]) } else { None };
                let right = tenors.get(i + 1).copied();
                Self::with_neighbors(t, shift_bps, left, right)
            })
            .collect()
    }

    /// Returns the key tenor.
    #[must_use]
    pub fn key_tenor(&self) -> f64 {
        self.key_tenor
    }

    /// Returns the shift in basis points.
    #[must_use]
    pub fn shift_bps(&self) -> f64 {
        self.shift_bps
    }

    /// Returns the shift as a decimal.
    #[must_use]
    pub fn shift_decimal(&self) -> f64 {
        self.shift_bps / 10_000.0
    }

    /// Computes the triangular weight at a given tenor.
    ///
    /// The weight is:
    /// - 1.0 at the key tenor
    /// - Linearly decreasing to 0 at adjacent key tenors
    /// - 0 outside the range [left_neighbor, right_neighbor]
    #[must_use]
    pub fn weight_at(&self, t: f64) -> f64 {
        if (t - self.key_tenor).abs() < 1e-10 {
            return 1.0;
        }

        if t < self.key_tenor {
            // Left side of triangle
            match self.left_tenor {
                Some(left) if t >= left => {
                    // Linear interpolation from 0 at left to 1 at key
                    (t - left) / (self.key_tenor - left)
                }
                Some(left) if t < left => 0.0,
                None => {
                    // No left neighbor - flat from 0 to key_tenor
                    if t >= 0.0 { 1.0 } else { 0.0 }
                }
                _ => 0.0,
            }
        } else {
            // Right side of triangle
            match self.right_tenor {
                Some(right) if t <= right => {
                    // Linear interpolation from 1 at key to 0 at right
                    (right - t) / (right - self.key_tenor)
                }
                Some(right) if t > right => 0.0,
                None => {
                    // No right neighbor - flat from key_tenor onwards
                    1.0
                }
                _ => 0.0,
            }
        }
    }

    /// Applies the key-rate bump to a curve.
    #[must_use]
    pub fn apply<'a, T: TermStructure>(&self, curve: &'a T) -> KeyRateBumpedCurve<'a, T> {
        KeyRateBumpedCurve {
            base: curve,
            bump: *self,
        }
    }

    /// Applies the key-rate bump to an Arc-wrapped curve.
    #[must_use]
    pub fn apply_arc<T: TermStructure>(self, curve: Arc<T>) -> ArcKeyRateBumpedCurve<T> {
        ArcKeyRateBumpedCurve { base: curve, bump: self }
    }

    /// Finds left and right neighbors for a tenor in a sorted list.
    fn find_neighbors(tenor: f64, key_tenors: &[f64]) -> (Option<f64>, Option<f64>) {
        let mut left = None;
        let mut right = None;

        for &kt in key_tenors {
            if kt < tenor {
                left = Some(kt);
            } else if kt > tenor && right.is_none() {
                right = Some(kt);
                break;
            }
        }

        (left, right)
    }
}

/// A curve with a key-rate bump applied.
///
/// The bump is applied with triangular weighting, centered at the
/// key tenor and linearly decreasing to zero at adjacent key tenors.
#[derive(Debug, Clone, Copy)]
pub struct KeyRateBumpedCurve<'a, T: TermStructure> {
    /// The base curve.
    base: &'a T,
    /// The key-rate bump specification.
    bump: KeyRateBump,
}

impl<'a, T: TermStructure> KeyRateBumpedCurve<'a, T> {
    /// Returns a reference to the base curve.
    #[must_use]
    pub fn base(&self) -> &T {
        self.base
    }

    /// Returns the key-rate bump.
    #[must_use]
    pub fn bump(&self) -> &KeyRateBump {
        &self.bump
    }

    /// Computes the effective shift at a given tenor.
    #[must_use]
    pub fn effective_shift_at(&self, t: f64) -> f64 {
        self.bump.weight_at(t) * self.bump.shift_decimal()
    }
}

impl<T: TermStructure> TermStructure for KeyRateBumpedCurve<'_, T> {
    fn reference_date(&self) -> Date {
        self.base.reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        let base_value = self.base.value_at(t);
        let shift = self.effective_shift_at(t);

        match self.base.value_type() {
            // For rates: add weighted shift
            ValueType::ZeroRate { .. }
            | ValueType::ForwardRate { .. }
            | ValueType::InstantaneousForward
            | ValueType::HazardRate
            | ValueType::ParSwapRate { .. }
            | ValueType::CreditSpread { .. } => base_value + shift,

            // For discount factors: df' = df * exp(-shift * t)
            ValueType::DiscountFactor => base_value * (-shift * t).exp(),

            // For survival probability: same as DF
            ValueType::SurvivalProbability => base_value * (-shift * t).exp(),

            // Default additive for others
            ValueType::InflationIndexRatio | ValueType::FxForwardPoints => {
                base_value + shift
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
        // Key-rate bumps affect derivative due to non-constant shift profile
        // For simplicity, we don't provide analytical derivative
        None
    }

    fn max_date(&self) -> Date {
        self.base.max_date()
    }
}

/// Arc-owned key-rate bumped curve.
#[derive(Debug, Clone)]
pub struct ArcKeyRateBumpedCurve<T: TermStructure> {
    /// The base curve (Arc-owned).
    base: Arc<T>,
    /// The key-rate bump specification.
    bump: KeyRateBump,
}

impl<T: TermStructure> ArcKeyRateBumpedCurve<T> {
    /// Returns a reference to the base curve.
    #[must_use]
    pub fn base(&self) -> &T {
        &self.base
    }

    /// Returns the key-rate bump.
    #[must_use]
    pub fn bump(&self) -> &KeyRateBump {
        &self.bump
    }

    /// Computes the effective shift at a given tenor.
    #[must_use]
    pub fn effective_shift_at(&self, t: f64) -> f64 {
        self.bump.weight_at(t) * self.bump.shift_decimal()
    }
}

impl<T: TermStructure> TermStructure for ArcKeyRateBumpedCurve<T> {
    fn reference_date(&self) -> Date {
        self.base.reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        let base_value = self.base.value_at(t);
        let shift = self.effective_shift_at(t);

        match self.base.value_type() {
            ValueType::ZeroRate { .. }
            | ValueType::ForwardRate { .. }
            | ValueType::InstantaneousForward
            | ValueType::HazardRate
            | ValueType::ParSwapRate { .. }
            | ValueType::CreditSpread { .. } => base_value + shift,

            ValueType::DiscountFactor => base_value * (-shift * t).exp(),
            ValueType::SurvivalProbability => base_value * (-shift * t).exp(),

            ValueType::InflationIndexRatio | ValueType::FxForwardPoints => {
                base_value + shift
            }
        }
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        self.base.tenor_bounds()
    }

    fn value_type(&self) -> ValueType {
        self.base.value_type()
    }

    fn derivative_at(&self, _t: f64) -> Option<f64> {
        None
    }

    fn max_date(&self) -> Date {
        self.base.max_date()
    }
}

/// Calculates the full key-rate duration profile for a pricing function.
///
/// # Arguments
///
/// * `curve` - The base curve
/// * `price_fn` - Function that prices given a curve, returning the price
/// * `shift_bps` - Bump size in basis points (default: 1bp)
///
/// # Returns
///
/// Vector of (tenor, key_rate_dv01) pairs.
pub fn key_rate_profile<T, F>(
    curve: &T,
    price_fn: F,
    shift_bps: f64,
) -> Vec<(f64, f64)>
where
    T: TermStructure,
    F: Fn(&dyn TermStructure) -> f64,
{
    let base_price = price_fn(curve);

    STANDARD_KEY_TENORS
        .iter()
        .map(|&tenor| {
            let bump = KeyRateBump::new(tenor, shift_bps);
            let bumped = bump.apply(curve);
            let bumped_price = price_fn(&bumped);
            let dv01 = base_price - bumped_price;
            (tenor, dv01)
        })
        .collect()
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
        let tenors = vec![1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 20.0, 30.0];
        let rates = vec![0.04, 0.042, 0.044, 0.046, 0.048, 0.05, 0.052, 0.054];

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
    fn test_key_rate_bump_creation() {
        let bump = KeyRateBump::new(5.0, 1.0);
        assert_relative_eq!(bump.key_tenor(), 5.0);
        assert_relative_eq!(bump.shift_bps(), 1.0);
        assert_relative_eq!(bump.shift_decimal(), 0.0001);
    }

    #[test]
    fn test_weight_at_key_tenor() {
        let bump = KeyRateBump::new(5.0, 1.0);
        assert_relative_eq!(bump.weight_at(5.0), 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_weight_at_neighbors() {
        // 5Y has neighbors 3Y and 7Y in standard tenors
        let bump = KeyRateBump::new(5.0, 1.0);

        // At 3Y (left neighbor) - weight should be 0
        assert_relative_eq!(bump.weight_at(3.0), 0.0, epsilon = 1e-10);

        // At 7Y (right neighbor) - weight should be 0
        assert_relative_eq!(bump.weight_at(7.0), 0.0, epsilon = 1e-10);

        // At 4Y (midpoint left) - weight should be 0.5
        assert_relative_eq!(bump.weight_at(4.0), 0.5, epsilon = 1e-10);

        // At 6Y (midpoint right) - weight should be 0.5
        assert_relative_eq!(bump.weight_at(6.0), 0.5, epsilon = 1e-10);
    }

    #[test]
    fn test_weight_outside_range() {
        let bump = KeyRateBump::new(5.0, 1.0);

        // Far left of left neighbor
        assert_relative_eq!(bump.weight_at(1.0), 0.0, epsilon = 1e-10);

        // Far right of right neighbor
        assert_relative_eq!(bump.weight_at(15.0), 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_leftmost_key_rate() {
        // 0.25Y has no left neighbor
        let bump = KeyRateBump::new(0.25, 1.0);

        // At key tenor
        assert_relative_eq!(bump.weight_at(0.25), 1.0, epsilon = 1e-10);

        // At 0 (no left neighbor, should be 1.0)
        assert_relative_eq!(bump.weight_at(0.0), 1.0, epsilon = 1e-10);

        // At 0.125 (halfway to 0)
        assert_relative_eq!(bump.weight_at(0.125), 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_rightmost_key_rate() {
        // 30Y has no right neighbor
        let bump = KeyRateBump::new(30.0, 1.0);

        // At key tenor
        assert_relative_eq!(bump.weight_at(30.0), 1.0, epsilon = 1e-10);

        // Beyond 30Y (no right neighbor, should be 1.0)
        assert_relative_eq!(bump.weight_at(40.0), 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_key_rate_bump_on_zero_curve() {
        let curve = sample_zero_curve();
        let bump = KeyRateBump::new(5.0, 100.0); // 100bp at 5Y
        let bumped = bump.apply(&curve);

        // At 5Y: full shift
        let base_5y = curve.value_at(5.0);
        let bumped_5y = bumped.value_at(5.0);
        assert_relative_eq!(bumped_5y - base_5y, 0.01, epsilon = 1e-10);

        // At 3Y: no shift (at neighbor)
        let base_3y = curve.value_at(3.0);
        let bumped_3y = bumped.value_at(3.0);
        assert_relative_eq!(bumped_3y, base_3y, epsilon = 1e-10);

        // At 7Y: no shift (at neighbor)
        let base_7y = curve.value_at(7.0);
        let bumped_7y = bumped.value_at(7.0);
        assert_relative_eq!(bumped_7y, base_7y, epsilon = 1e-10);

        // At 4Y: half shift
        let base_4y = curve.value_at(4.0);
        let bumped_4y = bumped.value_at(4.0);
        assert_relative_eq!(bumped_4y - base_4y, 0.005, epsilon = 1e-10);
    }

    #[test]
    fn test_standard_profile() {
        let profile = KeyRateBump::standard_profile(1.0);
        assert_eq!(profile.len(), STANDARD_KEY_TENORS.len());

        for (bump, &expected_tenor) in profile.iter().zip(STANDARD_KEY_TENORS.iter()) {
            assert_relative_eq!(bump.key_tenor(), expected_tenor);
        }
    }

    #[test]
    fn test_custom_profile() {
        let custom_tenors = [1.0, 5.0, 10.0];
        let profile = KeyRateBump::custom_profile(&custom_tenors, 1.0);

        assert_eq!(profile.len(), 3);
        assert_relative_eq!(profile[0].key_tenor(), 1.0);
        assert_relative_eq!(profile[1].key_tenor(), 5.0);
        assert_relative_eq!(profile[2].key_tenor(), 10.0);

        // Check neighbors
        assert!(profile[0].left_tenor.is_none());
        assert_eq!(profile[0].right_tenor, Some(5.0));

        assert_eq!(profile[1].left_tenor, Some(1.0));
        assert_eq!(profile[1].right_tenor, Some(10.0));

        assert_eq!(profile[2].left_tenor, Some(5.0));
        assert!(profile[2].right_tenor.is_none());
    }

    #[test]
    fn test_preserves_curve_properties() {
        let curve = sample_zero_curve();
        let bump = KeyRateBump::new(5.0, 1.0);
        let bumped = bump.apply(&curve);

        assert_eq!(curve.reference_date(), bumped.reference_date());
        assert_eq!(curve.tenor_bounds(), bumped.tenor_bounds());
        assert_eq!(curve.value_type(), bumped.value_type());
    }

    #[test]
    fn test_key_rate_profile() {
        let curve = sample_zero_curve();

        // Simple price function: sum of rates at key tenors
        let price_fn = |c: &dyn TermStructure| -> f64 {
            STANDARD_KEY_TENORS
                .iter()
                .filter(|&&t| t <= 30.0)
                .map(|&t| c.value_at(t))
                .sum()
        };

        let profile = key_rate_profile(&curve, price_fn, 1.0);

        // Should have entries for all standard tenors
        assert_eq!(profile.len(), STANDARD_KEY_TENORS.len());

        // Key-rate DV01s should be non-zero for tenors that affect pricing
        for (tenor, dv01) in &profile {
            if *tenor <= 30.0 {
                // For this simple sum-of-rates pricing, DV01 should be negative
                // (higher rates = higher "price" in this synthetic example)
                assert!(dv01.abs() > 0.0 || *tenor < 1.0);
            }
        }
    }

    #[test]
    fn test_arc_key_rate_bump() {
        let curve = Arc::new(sample_zero_curve());
        let bump = KeyRateBump::new(5.0, 50.0);
        let bumped = bump.apply_arc(curve.clone());

        let base_rate = curve.value_at(5.0);
        let bumped_rate = bumped.value_at(5.0);

        assert_relative_eq!(bumped_rate - base_rate, 0.005, epsilon = 1e-10);
    }
}
