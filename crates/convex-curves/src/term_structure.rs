//! Core term structure trait and type aliases.
//!
//! The `TermStructure` trait is the fundamental abstraction for any curve in the
//! Convex library. It provides a unified interface for yield curves, credit curves,
//! inflation curves, and FX curves.
//!
//! # Design Philosophy
//!
//! The trait is intentionally minimal - it provides only the raw curve access methods.
//! Domain-specific semantics are provided by wrapper types:
//!
//! - `RateCurve<T>`: Provides discount_factor(), zero_rate(), forward_rate()
//! - `CreditCurve<T>`: Provides survival_probability(), hazard_rate()
//! - `InflationCurve<T>`: Provides index_ratio(), real_rate()
//!
//! # Thread Safety
//!
//! All term structures are required to be `Send + Sync`, enabling safe use in
//! parallel pricing scenarios.

use convex_core::types::Date;
use std::sync::Arc;

use crate::error::CurveResult;
use crate::value_type::ValueType;

/// Core abstraction for any term structure.
///
/// A term structure maps time (in years from reference date) to values.
/// The interpretation of values depends on `value_type()`:
///
/// - `DiscountFactor`: P(t) where P(0) = 1
/// - `ZeroRate`: r(t) continuously compounded rate
/// - `ForwardRate`: f(t, t+Δ) for tenor Δ
/// - `SurvivalProbability`: Q(t) = P(τ > t)
/// - etc.
///
/// # Performance
///
/// Implementations should target sub-microsecond performance for `value_at()`.
/// Pre-computed interpolation coefficients are recommended.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::{TermStructure, ValueType};
///
/// fn price_bond<T: TermStructure>(curve: &T, tenors: &[f64]) -> f64 {
///     let mut pv = 0.0;
///     for &t in tenors {
///         let df = curve.value_at(t);  // Get discount factor at tenor
///         pv += df;
///     }
///     pv
/// }
/// ```
pub trait TermStructure: Send + Sync {
    /// Returns the curve's reference (valuation) date.
    ///
    /// All tenors are measured in years from this date.
    fn reference_date(&self) -> Date;

    /// Returns the raw value at time t (years from reference date).
    ///
    /// The interpretation depends on `value_type()`. For example:
    /// - `DiscountFactor`: returns P(t)
    /// - `ZeroRate`: returns r(t)
    ///
    /// # Arguments
    ///
    /// * `t` - Time in years from reference date. Must be >= 0.
    ///
    /// # Panics
    ///
    /// May panic if t is negative. Use `try_value_at` for fallible access.
    fn value_at(&self, t: f64) -> f64;

    /// Returns the valid tenor range for this curve.
    ///
    /// Returns (min, max) where min is typically 0 and max is the longest
    /// tenor for which the curve is defined.
    fn tenor_bounds(&self) -> (f64, f64);

    /// Returns what the curve's values represent.
    ///
    /// This enables semantic conversion between different representations.
    fn value_type(&self) -> ValueType;

    /// Returns the first derivative at time t, if available.
    ///
    /// The derivative is needed for computing:
    /// - Instantaneous forward rates from zero rates
    /// - Hazard rates from survival probabilities
    /// - Key-rate duration sensitivities
    ///
    /// Returns `None` if the curve implementation doesn't support derivatives.
    fn derivative_at(&self, _t: f64) -> Option<f64> {
        None
    }

    /// Returns the maximum date for which the curve is defined.
    fn max_date(&self) -> Date;

    // ========================================================================
    // Default implementations
    // ========================================================================

    /// Fallible version of `value_at` that checks tenor bounds.
    ///
    /// Returns an error if t is outside the valid range.
    fn try_value_at(&self, t: f64) -> CurveResult<f64> {
        let (min, max) = self.tenor_bounds();
        if t < min || t > max {
            return Err(crate::error::CurveError::tenor_out_of_range(t, min, max));
        }
        Ok(self.value_at(t))
    }

    /// Returns the value at a specific date.
    ///
    /// Converts the date to a year fraction from the reference date.
    fn value_at_date(&self, date: Date) -> f64 {
        let t = self.date_to_tenor(date);
        self.value_at(t)
    }

    /// Converts a date to a year fraction (tenor) from the reference date.
    ///
    /// Uses ACT/365 Fixed for simplicity. For precise calculations,
    /// use the day count convention from the value type.
    fn date_to_tenor(&self, date: Date) -> f64 {
        let days = self.reference_date().days_between(&date);
        days as f64 / 365.0
    }

    /// Converts a tenor to a date from the reference date.
    fn tenor_to_date(&self, t: f64) -> Date {
        let days = (t * 365.0).round() as i64;
        self.reference_date().add_days(days)
    }

    /// Returns true if the curve supports derivative calculation.
    fn has_derivative(&self) -> bool {
        // Default implementation: try to get derivative at t=1.0
        // and see if it returns Some
        self.derivative_at(1.0).is_some()
    }

    /// Returns true if the given tenor is within the curve's valid range.
    fn in_range(&self, t: f64) -> bool {
        let (min, max) = self.tenor_bounds();
        t >= min && t <= max
    }
}

/// Type alias for a boxed term structure trait object.
///
/// Use when you need ownership and dynamic dispatch.
pub type Curve = Box<dyn TermStructure>;

/// Type alias for a shared, reference-counted term structure.
///
/// Use when you need shared ownership across multiple consumers.
pub type CurveRef = Arc<dyn TermStructure>;

/// Blanket implementation allowing `Arc<T>` to be used as a `TermStructure`.
impl<T: TermStructure + ?Sized> TermStructure for Arc<T> {
    fn reference_date(&self) -> Date {
        (**self).reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        (**self).value_at(t)
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        (**self).tenor_bounds()
    }

    fn value_type(&self) -> ValueType {
        (**self).value_type()
    }

    fn derivative_at(&self, t: f64) -> Option<f64> {
        (**self).derivative_at(t)
    }

    fn max_date(&self) -> Date {
        (**self).max_date()
    }
}

/// Blanket implementation allowing `Box<T>` to be used as a `TermStructure`.
impl<T: TermStructure + ?Sized> TermStructure for Box<T> {
    fn reference_date(&self) -> Date {
        (**self).reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        (**self).value_at(t)
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        (**self).tenor_bounds()
    }

    fn value_type(&self) -> ValueType {
        (**self).value_type()
    }

    fn derivative_at(&self, t: f64) -> Option<f64> {
        (**self).derivative_at(t)
    }

    fn max_date(&self) -> Date {
        (**self).max_date()
    }
}

/// Blanket implementation allowing `&T` to be used as a `TermStructure`.
impl<T: TermStructure + ?Sized> TermStructure for &T {
    fn reference_date(&self) -> Date {
        (**self).reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        (**self).value_at(t)
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        (**self).tenor_bounds()
    }

    fn value_type(&self) -> ValueType {
        (**self).value_type()
    }

    fn derivative_at(&self, t: f64) -> Option<f64> {
        (**self).derivative_at(t)
    }

    fn max_date(&self) -> Date {
        (**self).max_date()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Compounding;

    /// A simple flat curve for testing.
    struct FlatCurve {
        reference_date: Date,
        value: f64,
        max_tenor: f64,
        value_type: ValueType,
    }

    impl FlatCurve {
        fn new(reference_date: Date, value: f64, max_tenor: f64) -> Self {
            Self {
                reference_date,
                value,
                max_tenor,
                value_type: ValueType::ZeroRate {
                    compounding: Compounding::Continuous,
                    day_count: DayCountConvention::Act365Fixed,
                },
            }
        }
    }

    impl TermStructure for FlatCurve {
        fn reference_date(&self) -> Date {
            self.reference_date
        }

        fn value_at(&self, _t: f64) -> f64 {
            self.value
        }

        fn tenor_bounds(&self) -> (f64, f64) {
            (0.0, self.max_tenor)
        }

        fn value_type(&self) -> ValueType {
            self.value_type.clone()
        }

        fn derivative_at(&self, _t: f64) -> Option<f64> {
            Some(0.0) // Flat curve has zero derivative
        }

        fn max_date(&self) -> Date {
            self.tenor_to_date(self.max_tenor)
        }
    }

    #[test]
    fn test_flat_curve_value_at() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let curve = FlatCurve::new(today, 0.05, 30.0);

        assert!((curve.value_at(0.0) - 0.05).abs() < 1e-10);
        assert!((curve.value_at(5.0) - 0.05).abs() < 1e-10);
        assert!((curve.value_at(30.0) - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_try_value_at() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let curve = FlatCurve::new(today, 0.05, 10.0);

        // In range
        assert!(curve.try_value_at(5.0).is_ok());

        // Out of range
        assert!(curve.try_value_at(15.0).is_err());
        assert!(curve.try_value_at(-1.0).is_err());
    }

    #[test]
    fn test_tenor_date_conversion() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let curve = FlatCurve::new(today, 0.05, 30.0);

        // 1 year from today (365 days - may not be exact calendar year due to leap years)
        let one_year = curve.tenor_to_date(1.0);
        // Check it's approximately 1 year (within a few days due to ACT/365)
        let days = today.days_between(&one_year);
        assert!((days - 365).abs() <= 1);

        // Roundtrip
        let tenor = curve.date_to_tenor(one_year);
        assert!((tenor - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_in_range() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let curve = FlatCurve::new(today, 0.05, 10.0);

        assert!(curve.in_range(0.0));
        assert!(curve.in_range(5.0));
        assert!(curve.in_range(10.0));
        assert!(!curve.in_range(-0.1));
        assert!(!curve.in_range(10.1));
    }

    #[test]
    fn test_has_derivative() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let curve = FlatCurve::new(today, 0.05, 10.0);

        assert!(curve.has_derivative());
        assert_eq!(curve.derivative_at(5.0), Some(0.0));
    }

    #[test]
    fn test_arc_wrapper() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let curve = Arc::new(FlatCurve::new(today, 0.05, 30.0));

        // Arc<T> should implement TermStructure
        assert_eq!(curve.reference_date(), today);
        assert!((curve.value_at(5.0) - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_box_wrapper() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let curve: Box<dyn TermStructure> = Box::new(FlatCurve::new(today, 0.05, 30.0));

        // Box<dyn TermStructure> should work
        assert_eq!(curve.reference_date(), today);
        assert!((curve.value_at(5.0) - 0.05).abs() < 1e-10);
    }
}
