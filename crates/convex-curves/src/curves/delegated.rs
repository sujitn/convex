//! Delegated curve implementation.
//!
//! A `DelegatedCurve` wraps another curve with fallback behavior
//! for out-of-range tenors.

use std::sync::Arc;

use convex_core::types::Date;

use crate::error::{CurveError, CurveResult};
use crate::term_structure::TermStructure;
use crate::value_type::ValueType;

/// Fallback behavior when accessing tenors outside the underlying curve's range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DelegationFallback {
    /// Trust that the underlying curve handles all tenors.
    /// Does not check bounds.
    #[default]
    Trust,

    /// Return an error if outside the underlying's range.
    Strict,

    /// Use flat extrapolation at the boundaries.
    FlatExtrapolation,

    /// Clamp tenor to the underlying's valid range.
    Clamp,
}

/// A curve that delegates to another curve with fallback handling.
///
/// This is useful for:
/// - Providing explicit bounds checking
/// - Adding extrapolation behavior to a curve
/// - Wrapping curves with stricter error handling
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::{DelegatedCurve, DelegationFallback};
/// use std::sync::Arc;
///
/// let base = Arc::new(build_swap_curve()?);
/// let delegated = DelegatedCurve::new(base, DelegationFallback::Clamp);
///
/// // Will clamp to [0, 30] instead of extrapolating or erroring
/// let rate = delegated.value_at(50.0);
/// ```
#[derive(Clone)]
pub struct DelegatedCurve<T: TermStructure + ?Sized> {
    /// The underlying curve.
    underlying: Arc<T>,
    /// Fallback behavior for out-of-range tenors.
    fallback: DelegationFallback,
    /// Optional override for tenor bounds.
    bounds_override: Option<(f64, f64)>,
}

impl<T: TermStructure + ?Sized> std::fmt::Debug for DelegatedCurve<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DelegatedCurve")
            .field("underlying_ref_date", &self.underlying.reference_date())
            .field("fallback", &self.fallback)
            .field("bounds_override", &self.bounds_override)
            .finish()
    }
}

impl<T: TermStructure + ?Sized> DelegatedCurve<T> {
    /// Creates a new delegated curve.
    #[must_use]
    pub fn new(underlying: Arc<T>, fallback: DelegationFallback) -> Self {
        Self {
            underlying,
            fallback,
            bounds_override: None,
        }
    }

    /// Creates a delegated curve with trust (no bounds checking).
    #[must_use]
    pub fn trust(underlying: Arc<T>) -> Self {
        Self::new(underlying, DelegationFallback::Trust)
    }

    /// Creates a delegated curve with strict bounds checking.
    #[must_use]
    pub fn strict(underlying: Arc<T>) -> Self {
        Self::new(underlying, DelegationFallback::Strict)
    }

    /// Creates a delegated curve with clamping.
    #[must_use]
    pub fn clamped(underlying: Arc<T>) -> Self {
        Self::new(underlying, DelegationFallback::Clamp)
    }

    /// Sets custom bounds, overriding the underlying curve's bounds.
    #[must_use]
    pub fn with_bounds(mut self, min: f64, max: f64) -> Self {
        self.bounds_override = Some((min, max));
        self
    }

    /// Returns a reference to the underlying curve.
    #[must_use]
    pub fn underlying(&self) -> &T {
        &self.underlying
    }

    /// Returns the fallback behavior.
    #[must_use]
    pub fn fallback(&self) -> DelegationFallback {
        self.fallback
    }

    /// Gets the effective bounds (override or underlying).
    fn effective_bounds(&self) -> (f64, f64) {
        self.bounds_override
            .unwrap_or_else(|| self.underlying.tenor_bounds())
    }

    /// Tries to get a value, returning Result for strict mode.
    pub fn try_value_at(&self, t: f64) -> CurveResult<f64> {
        let (min, max) = self.effective_bounds();

        match self.fallback {
            DelegationFallback::Trust => Ok(self.underlying.value_at(t)),
            DelegationFallback::Strict => {
                if t < min || t > max {
                    Err(CurveError::tenor_out_of_range(t, min, max))
                } else {
                    Ok(self.underlying.value_at(t))
                }
            }
            DelegationFallback::FlatExtrapolation => {
                if t < min {
                    Ok(self.underlying.value_at(min))
                } else if t > max {
                    Ok(self.underlying.value_at(max))
                } else {
                    Ok(self.underlying.value_at(t))
                }
            }
            DelegationFallback::Clamp => {
                let clamped = t.clamp(min, max);
                Ok(self.underlying.value_at(clamped))
            }
        }
    }
}

impl<T: TermStructure + ?Sized> TermStructure for DelegatedCurve<T> {
    fn reference_date(&self) -> Date {
        self.underlying.reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        let (min, max) = self.effective_bounds();

        match self.fallback {
            DelegationFallback::Trust => self.underlying.value_at(t),
            DelegationFallback::Strict => {
                // In value_at, we can't return error, so we check and panic if needed
                // Users should use try_value_at for strict mode
                if t < min || t > max {
                    f64::NAN // Return NaN to indicate error
                } else {
                    self.underlying.value_at(t)
                }
            }
            DelegationFallback::FlatExtrapolation => {
                if t < min {
                    self.underlying.value_at(min)
                } else if t > max {
                    self.underlying.value_at(max)
                } else {
                    self.underlying.value_at(t)
                }
            }
            DelegationFallback::Clamp => {
                let clamped = t.clamp(min, max);
                self.underlying.value_at(clamped)
            }
        }
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        self.effective_bounds()
    }

    fn value_type(&self) -> ValueType {
        self.underlying.value_type()
    }

    fn derivative_at(&self, t: f64) -> Option<f64> {
        let (min, max) = self.effective_bounds();

        if t < min || t > max {
            match self.fallback {
                DelegationFallback::Trust => self.underlying.derivative_at(t),
                DelegationFallback::Strict => None,
                DelegationFallback::FlatExtrapolation => Some(0.0), // Flat = zero derivative
                DelegationFallback::Clamp => {
                    let clamped = t.clamp(min, max);
                    self.underlying.derivative_at(clamped)
                }
            }
        } else {
            self.underlying.derivative_at(t)
        }
    }

    fn max_date(&self) -> Date {
        let (_, max) = self.effective_bounds();
        self.tenor_to_date(max)
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
    fn test_trust_fallback() {
        let base = sample_base_curve();
        let delegated = DelegatedCurve::trust(base);

        // Should work for any tenor
        let rate = delegated.value_at(5.0);
        assert_relative_eq!(rate, 0.05, epsilon = 1e-10);

        // Should also work outside original bounds (flat extrapolation from base)
        let _ = delegated.value_at(15.0);
    }

    #[test]
    fn test_strict_fallback() {
        let base = sample_base_curve();
        let delegated = DelegatedCurve::strict(base);

        // Within bounds - should work
        let result = delegated.try_value_at(5.0);
        assert!(result.is_ok());
        assert_relative_eq!(result.unwrap(), 0.05, epsilon = 1e-10);

        // Outside bounds - should error
        let result = delegated.try_value_at(15.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_clamp_fallback() {
        let base = sample_base_curve();
        let delegated = DelegatedCurve::clamped(base.clone());

        // At max bound (10Y)
        let rate_at_10 = base.value_at(10.0);

        // Beyond max should clamp to max
        let rate_at_15 = delegated.value_at(15.0);
        assert_relative_eq!(rate_at_15, rate_at_10, epsilon = 1e-10);

        // At min bound (1Y)
        let rate_at_1 = base.value_at(1.0);

        // Below min should clamp to min
        let rate_below_min = delegated.value_at(0.5);
        assert_relative_eq!(rate_below_min, rate_at_1, epsilon = 1e-10);
    }

    #[test]
    fn test_flat_extrapolation() {
        let base = sample_base_curve();
        let delegated = DelegatedCurve::new(base.clone(), DelegationFallback::FlatExtrapolation);

        // Should use boundary value for out-of-range
        let rate_at_max = base.value_at(10.0);
        let rate_beyond = delegated.value_at(20.0);
        assert_relative_eq!(rate_beyond, rate_at_max, epsilon = 1e-10);
    }

    #[test]
    fn test_bounds_override() {
        let base = sample_base_curve();
        let delegated = DelegatedCurve::strict(base).with_bounds(2.0, 8.0);

        // New bounds should be [2, 8]
        let (min, max) = delegated.tenor_bounds();
        assert_relative_eq!(min, 2.0, epsilon = 1e-10);
        assert_relative_eq!(max, 8.0, epsilon = 1e-10);

        // Within new bounds - OK
        assert!(delegated.try_value_at(5.0).is_ok());

        // Outside new bounds - Error
        assert!(delegated.try_value_at(1.5).is_err());
        assert!(delegated.try_value_at(9.0).is_err());
    }

    #[test]
    fn test_derivative() {
        let base = sample_base_curve();
        let delegated = DelegatedCurve::trust(base.clone());

        // Derivative should match underlying
        let base_deriv = base.derivative_at(5.0);
        let delegated_deriv = delegated.derivative_at(5.0);

        assert_eq!(base_deriv.is_some(), delegated_deriv.is_some());
    }

    #[test]
    fn test_flat_extrapolation_derivative() {
        let base = sample_base_curve();
        let delegated = DelegatedCurve::new(base, DelegationFallback::FlatExtrapolation);

        // Derivative for flat extrapolation should be 0
        let deriv = delegated.derivative_at(20.0);
        assert_eq!(deriv, Some(0.0));
    }
}
