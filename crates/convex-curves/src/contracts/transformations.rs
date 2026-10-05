//! Exact discount views and declared roll policies, without grid resampling.

use convex_core::types::Compounding;
use serde::{Deserialize, Serialize};

use super::{build::positive, CurveContractError as Error, CurveContractResult as Result};

/// Rolling is a scenario over supplied curves, not a prediction or lifecycle
/// event. Currency/role/time basis and source interpolation remain unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RollPolicy {
    /// Preserve remaining forward discounts: P_new(t)=P_old(offset+t)/P_old(offset).
    /// Offset uses the actual old-reference/new-reference year fraction.
    StickyForward,
    /// Preserve the zero term structure by remaining model tenor:
    /// P_new(t)=P_old(t). This does not preserve discounts at fixed calendar dates.
    StickyZero,
}

fn fraction(tau: f64) -> Result<()> {
    let ceiling = convex_core::cashflows::MAX_CONTRACT_DAYS as f64 / 360.0;
    if !tau.is_finite() || !(0.0..=ceiling).contains(&tau) {
        return Err(Error::invalid(
            "year_fraction",
            "must be finite, nonnegative and within the 150-year contract span",
        ));
    }
    Ok(())
}

/// Convert an explicitly compounded decimal rate over the actual supplied
/// fraction. Negative rates are valid with positive simple/periodic bases.
/// ln_1p avoids cancellation for very small rates/periods. P(0)=1 is mathematical.
pub fn discount_from_rate(rate: f64, tau: f64, compounding: Compounding) -> Result<f64> {
    let discount = (-log_accumulation(rate, tau, compounding)?).exp();
    positive(discount, "rate-to-discount conversion")?;
    Ok(discount)
}

fn log_accumulation(rate: f64, tau: f64, compounding: Compounding) -> Result<f64> {
    fraction(tau)?;
    if !rate.is_finite() {
        return Err(Error::invalid("rate", "must be finite"));
    }
    let result = match compounding {
        Compounding::Continuous => rate * tau,
        Compounding::Simple => {
            let accrual = rate * tau;
            positive(1.0 + accrual, "simple compounding base")?;
            accrual.ln_1p()
        }
        _ => {
            let periods = f64::from(compounding.periods_per_year());
            positive(1.0 + rate / periods, "periodic compounding base")?;
            periods * tau * (rate / periods).ln_1p()
        }
    };
    if !result.is_finite() {
        return Err(Error::Numerical("compounding log accumulation".into()));
    }
    Ok(result)
}

/// Recover an annualized decimal rate from a positive finite discount over
/// a strictly positive supplied fraction. exp_m1 preserves short-period rates.
pub fn rate_from_discount(discount: f64, tau: f64, compounding: Compounding) -> Result<f64> {
    fraction(tau)?;
    positive(tau, "rate year fraction")?;
    positive(discount, "period discount factor")?;
    rate_from_log_accumulation(-discount.ln(), tau, compounding)
}

fn rate_from_log_accumulation(
    log_accumulation: f64,
    tau: f64,
    compounding: Compounding,
) -> Result<f64> {
    let rate = match compounding {
        Compounding::Continuous => log_accumulation / tau,
        Compounding::Simple => log_accumulation.exp_m1() / tau,
        _ => {
            let periods = f64::from(compounding.periods_per_year());
            periods * (log_accumulation / (periods * tau)).exp_m1()
        }
    };
    if !rate.is_finite() {
        return Err(Error::Numerical("discount-to-rate conversion".into()));
    }
    Ok(rate)
}

/// Equivalent rate over the same actual period, with no implicit annualization
/// period for Simple compounding. Requires positive tau even when conventions
/// agree; a zero-length equivalent rate is undefined.
pub fn convert_rate(rate: f64, tau: f64, from: Compounding, to: Compounding) -> Result<f64> {
    positive(tau, "rate conversion period")?;
    rate_from_log_accumulation(log_accumulation(rate, tau, from)?, tau, to)
}
