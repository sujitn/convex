//! Checked numerical discount laws over explicitly supplied times and cash.
//! No dates, entitlement, market convention or cash normalization are inferred.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{types::CompoundingMethod, BondError, BondResult};
use convex_math::portable;
use serde::{Deserialize, Serialize};

/// Mathematical annual-rate basis for supplied positive discount years.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CashYieldCompounding {
    /// `(1+y/frequency)^(-time*frequency)`; frequency is explicitly supplied.
    Periodic {
        /// Strictly positive finite periods per year.
        frequency: f64,
    },
    /// `(1+y)^(-time)`; no market-specific date convention is implied.
    Annual,
    /// `exp(-y*time)`.
    Continuous,
    /// `1/(1+y*time)`.
    Simple,
}

fn invalid(reason: &str) -> BondError {
    BondError::invalid_spec(reason)
}
fn numerical() -> BondError {
    BondError::pricing_failed("supplied-cash yield arithmetic overflow/nonzero underflow")
}
fn finite(value: f64) -> BondResult<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(numerical())
    }
}
fn product(left: f64, right: f64) -> BondResult<f64> {
    let result = finite(left * right)?;
    if left != 0.0 && right != 0.0 && result == 0.0 {
        return Err(numerical());
    }
    Ok(result)
}
fn quotient(left: f64, right: f64) -> BondResult<f64> {
    let result = finite(left / right)?;
    if left != 0.0 && result == 0.0 {
        return Err(numerical());
    }
    Ok(result)
}
pub(crate) fn validate_cash(cash: &[(f64, f64)], inverse: bool) -> BondResult<()> {
    if cash.is_empty()
        || cash.len() > 4096
        || cash
            .iter()
            .any(|(t, a)| !t.is_finite() || *t <= 0.0 || !a.is_finite() || (inverse && *a < 0.0))
    {
        return Err(invalid("1..4096 finite cash entries with positive times required; inverse requires nonnegative cash"));
    }
    if !cash.iter().any(|(_, a)| *a > 0.0) {
        return Err(invalid("positive supplied cash exposure required"));
    }
    Ok(())
}
/// Price in the original input cash basis; signed cash components may be priced.
/// Input order is retained. A positive finite net PV is required.
pub fn price_cash(
    cash: &[(f64, f64)],
    annual_yield: f64,
    compounding: CashYieldCompounding,
) -> BondResult<f64> {
    value_cash(cash, annual_yield, compounding).map(|v| v.0)
}
pub(crate) fn value_cash(
    cash: &[(f64, f64)],
    annual_yield: f64,
    compounding: CashYieldCompounding,
) -> BondResult<(f64, f64)> {
    evaluate(cash, annual_yield, compounding)
}
pub(crate) fn standard_value(
    cash: &[(f64, f64)],
    annual_yield: f64,
    method: CompoundingMethod,
) -> BondResult<(f64, f64)> {
    let basis = match method {
        CompoundingMethod::Periodic { frequency }
        | CompoundingMethod::ActualPeriod { frequency } => CashYieldCompounding::Periodic {
            frequency: f64::from(frequency),
        },
        CompoundingMethod::Continuous => CashYieldCompounding::Continuous,
        CompoundingMethod::Simple => CashYieldCompounding::Simple,
        CompoundingMethod::Discount => {
            return Err(invalid(
                "discount quotation requires an actual dated bill contract",
            ))
        }
    };
    evaluate(cash, annual_yield, basis)
}
fn evaluate(cash: &[(f64, f64)], rate: f64, basis: CashYieldCompounding) -> BondResult<(f64, f64)> {
    validate_cash(cash, false)?;
    finite(rate)?;
    if let CashYieldCompounding::Periodic { frequency } = basis {
        if !frequency.is_finite() || frequency <= 0.0 {
            return Err(invalid("positive finite compounding frequency required"));
        }
    }
    let mut pv = 0.0;
    let mut first = 0.0;
    for &(time, amount) in cash {
        let (df, derivative) = match basis {
            CashYieldCompounding::Periodic { frequency } => {
                if rate <= -frequency {
                    return Err(invalid("yield outside periodic compounding domain"));
                }
                let periodic_rate = quotient(rate, frequency)?;
                let (base, logarithm) = if periodic_rate < -0.5 {
                    // Subtraction near the domain edge retains the actual
                    // positive base even when rate/frequency rounds to -1.
                    let base = quotient(finite(frequency + rate)?, frequency)?;
                    (base, finite(portable::log(base))?)
                } else {
                    (
                        finite(1.0 + periodic_rate)?,
                        finite(portable::log1p(periodic_rate))?,
                    )
                };
                // Multiply the smallest and largest magnitudes first. Either
                // time*frequency or frequency*logarithm can overflow although
                // the complete exponent is representable.
                let mut factors = [-time, frequency, logarithm];
                factors.sort_by(|left, right| left.abs().total_cmp(&right.abs()));
                let exponent = product(product(factors[0], factors[2])?, factors[1])?;
                let df = finite(portable::exp(exponent))?;
                (df, quotient(product(-time, df)?, base)?)
            }
            CashYieldCompounding::Annual => {
                let base = finite(1.0 + rate)?;
                if base <= 0.0 {
                    return Err(invalid("yield outside annual compounding domain"));
                }
                let df = finite(portable::exp(product(
                    -time,
                    finite(portable::log1p(rate))?,
                )?))?;
                (df, quotient(product(-time, df)?, base)?)
            }
            CashYieldCompounding::Continuous => {
                let df = finite(portable::exp(product(-rate, time)?))?;
                (df, product(-time, df)?)
            }
            CashYieldCompounding::Simple => {
                let base = finite(1.0 + product(rate, time)?)?;
                if base <= 0.0 {
                    return Err(invalid("yield outside simple interest domain"));
                }
                (quotient(1.0, base)?, quotient(-time, product(base, base)?)?)
            }
        };
        if df <= 0.0 || derivative == 0.0 {
            return Err(numerical());
        }
        pv = finite(pv + product(amount, df)?)?;
        first = finite(first + product(amount, derivative)?)?;
    }
    if pv <= 0.0 {
        return Err(invalid("positive finite supplied-cash price required"));
    }
    Ok((pv, first))
}
