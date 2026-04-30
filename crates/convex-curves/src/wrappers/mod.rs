//! Domain-specific wrappers over [`TermStructure`]: [`RateCurve`] for rate
//! semantics, [`CreditCurve`] for credit. [`RateCurveDyn`] is the object-safe
//! flavour for trait-object dispatch.

mod credit_curve;
mod rate_curve;

pub use credit_curve::CreditCurve;
pub use rate_curve::RateCurve;

use convex_core::types::{Compounding, Date};

use crate::error::CurveResult;
use crate::term_structure::TermStructure;

/// Object-safe rate curve.
pub trait RateCurveDyn: Send + Sync {
    fn discount_factor(&self, t: f64) -> CurveResult<f64>;
    fn zero_rate(&self, t: f64, compounding: Compounding) -> CurveResult<f64>;
    fn forward_rate(&self, t1: f64, t2: f64) -> CurveResult<f64>;
    fn instantaneous_forward(&self, t: f64) -> CurveResult<f64>;
    fn reference_date(&self) -> Date;
    fn max_date(&self) -> Date;

    /// Par swap rate `c = (1 − DF(T)) / Σ τ · DF(i·τ)` on a regular schedule,
    /// `τ = 1/frequency`. Stubs and explicit fixed-leg day counts go through
    /// [`RateCurve::par_swap_rate`] on the concrete wrapper instead.
    fn par_swap_rate(&self, t_maturity: f64, frequency: u32) -> CurveResult<f64> {
        if t_maturity <= 0.0 || frequency == 0 {
            return Err(crate::error::CurveError::invalid_value(
                "par_swap_rate: t_maturity and frequency must be positive",
            ));
        }
        let tau = 1.0 / frequency as f64;
        let n = (t_maturity * frequency as f64).round() as usize;
        if n == 0 {
            return Err(crate::error::CurveError::invalid_value(
                "par_swap_rate: maturity too short for given frequency",
            ));
        }
        let annuity: f64 = (1..=n)
            .map(|i| self.discount_factor(i as f64 * tau).map(|df| tau * df))
            .sum::<CurveResult<f64>>()?;
        if annuity.abs() < 1e-12 {
            return Err(crate::error::CurveError::math_error(
                "par_swap_rate: annuity is zero",
            ));
        }
        Ok((1.0 - self.discount_factor(t_maturity)?) / annuity)
    }
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
