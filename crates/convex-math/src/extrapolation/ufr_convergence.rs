//! UFR (Ultimate Forward Rate) convergence extrapolation.
//!
//! Beyond the last liquid point (LLP), the instantaneous forward rate is modelled
//! as decaying exponentially from its observed value at the LLP towards an
//! Ultimate Forward Rate (UFR):
//!
//! ```text
//! f(t) = UFR + (f_llp - UFR) * exp(-alpha * (t - LLP))
//! ```
//!
//! and the zero rate is the exact integral of that forward. The result is
//! continuous in both level and instantaneous forward at the LLP (it honours the
//! curve slope there) and converges to the UFR at long maturities, with `alpha`
//! controlling the convergence speed.
//!
//! Note: this is a heuristic tail extrapolator, **not** the EIOPA / Solvency II
//! Smith-Wilson method. True Smith-Wilson fits a Wilson-kernel curve to all input
//! instruments so it reprices them exactly, which cannot be expressed as a
//! pointwise tail extrapolation. If you need regulatory Smith-Wilson, fit it at
//! curve-construction time instead.

use super::Extrapolator;
use crate::{MathError, MathResult};

/// Heuristic UFR-convergence extrapolator (see module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UfrConvergence {
    /// Ultimate forward rate (continuously compounded).
    pub ultimate_forward_rate: f64,
    /// Convergence speed (alpha); larger values converge to the UFR faster.
    pub convergence_speed: f64,
    /// Last liquid point (years).
    pub last_liquid_point: f64,
}

impl UfrConvergence {
    /// Creates a finite validated extrapolator without panicking.
    pub fn try_new(ufr: f64, alpha: f64, llp: f64) -> MathResult<Self> {
        if !ufr.is_finite() || !alpha.is_finite() || alpha <= 0.0 || !llp.is_finite() || llp <= 0.0
        {
            return Err(MathError::invalid_input(
                "UFR must be finite; alpha and LLP finite and positive",
            ));
        }
        Ok(Self {
            ultimate_forward_rate: ufr,
            convergence_speed: alpha,
            last_liquid_point: llp,
        })
    }
    /// Creates a new UFR-convergence extrapolator.
    ///
    /// # Arguments
    ///
    /// * `ufr` - Ultimate forward rate (e.g. `0.042` for 4.2%)
    /// * `alpha` - Convergence speed (higher = faster convergence)
    /// * `llp` - Last liquid point in years
    ///
    /// # Panics
    ///
    /// Panics on invalid parameters. Compatibility only; external callers use
    /// [`Self::try_new`]. Exception ASR-001 tracks this infallible signature.
    #[must_use]
    pub fn new(ufr: f64, alpha: f64, llp: f64) -> Self {
        assert!(alpha > 0.0, "alpha must be positive");
        assert!(llp > 0.0, "llp must be positive");
        Self {
            ultimate_forward_rate: ufr,
            convergence_speed: alpha,
            last_liquid_point: llp,
        }
    }

    /// Returns the ultimate forward rate.
    #[must_use]
    pub fn ufr(&self) -> f64 {
        self.ultimate_forward_rate
    }

    /// Returns the convergence speed (alpha).
    #[must_use]
    pub fn alpha(&self) -> f64 {
        self.convergence_speed
    }

    /// Returns the last liquid point.
    #[must_use]
    pub fn llp(&self) -> f64 {
        self.last_liquid_point
    }

    /// Exact integrated tail zero from the declared LLP and its actual left
    /// zero/instantaneous forward. This checked route cannot ignore an LLP or
    /// replace an unavailable derivative with zero. Rates are continuous per
    /// year of the supplied time axis; negative rates remain valid.
    pub fn try_zero_rate(&self, t: f64, zero_at_llp: f64, forward_at_llp: f64) -> MathResult<f64> {
        self.validate_tail(t)?;
        crate::bounds::finite(zero_at_llp, "LLP zero")?;
        crate::bounds::finite(forward_at_llp, "LLP forward")?;
        crate::bounds::finite(
            self.integrated_zero(t, self.last_liquid_point, zero_at_llp, forward_at_llp),
            "UFR zero",
        )
    }

    /// Original analytic instantaneous tail forward, including the exact LLP
    /// limit. Queries before the LLP fail; no sampled derivative is used.
    pub fn try_forward_rate(&self, t: f64, forward_at_llp: f64) -> MathResult<f64> {
        self.validate_tail(t)?;
        crate::bounds::finite(forward_at_llp, "LLP forward")?;
        if t == self.last_liquid_point {
            return Ok(forward_at_llp);
        }
        let decay = (-self.convergence_speed * (t - self.last_liquid_point)).exp();
        crate::bounds::finite(
            self.ultimate_forward_rate + (forward_at_llp - self.ultimate_forward_rate) * decay,
            "UFR forward",
        )
    }

    fn validate_tail(&self, t: f64) -> MathResult<()> {
        self.validate()?;
        crate::bounds::finite(t, "UFR query time")?;
        if t < self.last_liquid_point {
            return Err(MathError::invalid_input(
                "UFR query precedes the declared LLP",
            ));
        }
        Ok(())
    }

    fn integrated_zero(&self, t: f64, last_t: f64, zero: f64, forward: f64) -> f64 {
        if t <= last_t {
            return zero;
        }
        let tau = t - last_t;
        let alpha = self.convergence_speed;
        let x = alpha * tau;
        // Integral of exp(-alpha*s). expm1 retains small alpha*time changes;
        // the zero limit also handles a product rounded to subnormal zero.
        let integrated_decay = if x == 0.0 {
            tau
        } else if x.is_infinite() {
            1.0 / alpha
        } else {
            tau * (-(-x).exp_m1() / x)
        };
        let ufr = self.ultimate_forward_rate;
        (zero * last_t + ufr * tau + (forward - ufr) * integrated_decay) / t
    }
}

impl Extrapolator for UfrConvergence {
    fn validate(&self) -> MathResult<()> {
        Self::try_new(
            self.ultimate_forward_rate,
            self.convergence_speed,
            self.last_liquid_point,
        )
        .map(|_| ())
    }
    fn extrapolate(&self, t: f64, last_t: f64, last_value: f64, last_derivative: f64) -> f64 {
        if t <= last_t {
            return last_value;
        }

        // Instantaneous forward at the LLP: f = d/dt[t * z(t)] = z(LLP) + LLP * z'(LLP).
        let f_llp = last_value + last_t * last_derivative;
        self.integrated_zero(t, last_t, last_value, f_llp)
    }

    fn name(&self) -> &'static str {
        "UFR-convergence"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn returns_last_value_at_llp() {
        let ext = UfrConvergence::new(0.042, 0.1, 20.0);
        assert_relative_eq!(
            ext.extrapolate(20.0, 20.0, 0.035, 0.001),
            0.035,
            epsilon = 1e-12
        );
    }

    #[test]
    fn converges_towards_ufr() {
        let ext = UfrConvergence::new(0.042, 0.1, 20.0);
        // Flat slope so the forward starts at the last zero rate (below the UFR).
        let z30 = ext.extrapolate(30.0, 20.0, 0.035, 0.0);
        let z60 = ext.extrapolate(60.0, 20.0, 0.035, 0.0);
        let z100 = ext.extrapolate(100.0, 20.0, 0.035, 0.0);
        assert!(z30 > 0.035 && z60 > z30);
        assert!((z100 - 0.042).abs() < (z60 - 0.042).abs());
    }

    #[test]
    fn faster_alpha_converges_faster() {
        let slow = UfrConvergence::new(0.042, 0.05, 20.0);
        let fast = UfrConvergence::new(0.042, 0.30, 20.0);
        let slow_40 = slow.extrapolate(40.0, 20.0, 0.03, 0.0);
        let fast_40 = fast.extrapolate(40.0, 20.0, 0.03, 0.0);
        assert!((fast_40 - 0.042).abs() < (slow_40 - 0.042).abs());
    }

    #[test]
    fn honours_the_curve_slope() {
        // A steeper input slope at the LLP must raise the extrapolated zero rate
        // just beyond it; this guards against the slope being ignored.
        let ext = UfrConvergence::new(0.042, 0.1, 20.0);
        let flat = ext.extrapolate(21.0, 20.0, 0.035, 0.0);
        let steep = ext.extrapolate(21.0, 20.0, 0.035, 0.002);
        assert!(steep > flat);
    }
}
