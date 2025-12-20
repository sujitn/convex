//! Domain-specific curve wrappers.
//!
//! These wrappers provide semantic operations on top of any `TermStructure`:
//!
//! - [`RateCurve`]: Interest rate operations (discount, zero, forward)
//! - [`CreditCurve`]: Credit operations (survival, hazard, spread)
//!
//! # Dynamic Dispatch
//!
//! For scenarios requiring trait objects (e.g., heterogeneous curve collections),
//! use [`RateCurveDyn`] which provides an object-safe interface.

mod credit_curve;
mod rate_curve;

pub use credit_curve::CreditCurve;
pub use rate_curve::RateCurve;

use convex_core::types::{Compounding, Date};

use crate::error::CurveResult;
use crate::term_structure::TermStructure;

// ============================================================================
// Object-Safe Rate Curve Trait
// ============================================================================

/// Object-safe trait for rate curves.
///
/// This trait enables dynamic dispatch for rate curves, useful when you need
/// to store heterogeneous curves in collections or pass curves across API
/// boundaries without generics.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::{RateCurve, RateCurveDyn, DiscreteCurve};
/// use std::sync::Arc;
///
/// fn price_bond(curve: &dyn RateCurveDyn, ...) -> f64 {
///     let df = curve.discount_factor(1.0)?;
///     // ...
/// }
///
/// let curve: Arc<dyn RateCurveDyn> = Arc::new(RateCurve::new(discrete_curve));
/// ```
pub trait RateCurveDyn: Send + Sync {
    /// Returns the discount factor for a given tenor in years.
    fn discount_factor(&self, t: f64) -> CurveResult<f64>;

    /// Returns the zero rate for a given tenor with specified compounding.
    fn zero_rate(&self, t: f64, compounding: Compounding) -> CurveResult<f64>;

    /// Returns the forward rate between two tenors (continuously compounded).
    fn forward_rate(&self, t1: f64, t2: f64) -> CurveResult<f64>;

    /// Returns the instantaneous forward rate at a tenor.
    fn instantaneous_forward(&self, t: f64) -> CurveResult<f64>;

    /// Returns the reference date of the curve.
    fn reference_date(&self) -> Date;

    /// Returns the maximum date for which the curve is defined.
    fn max_date(&self) -> Date;
}

// Implement RateCurveDyn for RateCurve<T>
impl<T: TermStructure> RateCurveDyn for RateCurve<T> {
    fn discount_factor(&self, t: f64) -> CurveResult<f64> {
        self.discount_factor_at_tenor(t)
    }

    fn zero_rate(&self, t: f64, compounding: Compounding) -> CurveResult<f64> {
        self.zero_rate_at_tenor(t, compounding)
    }

    fn forward_rate(&self, t1: f64, t2: f64) -> CurveResult<f64> {
        self.forward_rate_at_tenors(t1, t2, Compounding::Continuous)
    }

    fn instantaneous_forward(&self, t: f64) -> CurveResult<f64> {
        self.instantaneous_forward_at_tenor(t)
    }

    fn reference_date(&self) -> Date {
        RateCurve::reference_date(self)
    }

    fn max_date(&self) -> Date {
        self.inner().max_date()
    }
}
