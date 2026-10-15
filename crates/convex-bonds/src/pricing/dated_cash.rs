//! Checked dated-cash yield PV and analytical derivatives on one discount basis.
//! Cash is already entitled and absolute; this operation does not infer a market
//! quotation convention or manufacture ICMA reference periods.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::discount_time::{DatedYieldCash, DiscountTimeBasis, PreparedDatedCash};
use super::yield_cash;
use crate::{error::BondResult, traits::BondCashFlow, BondError};
use convex_core::{
    daycounts::DayCountConvention,
    types::{Compounding, Date},
};
use convex_math::portable;

/// All monetary fields use the exact input cash currency and scale.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct DatedCashYieldRisk {
    /// Absolute entitled-cash PV at the explicit annual decimal yield.
    pub present_value: f64,
    /// Currency per annual decimal yield, preserving its sign.
    pub first_derivative: f64,
    /// Currency per annual decimal yield squared.
    pub second_derivative: f64,
    /// Cash-PV-weighted settlement-to-payment time in years.
    pub macaulay_duration: f64,
    /// Minus first derivative divided by actual PV.
    pub modified_duration: f64,
    /// Second derivative divided by actual PV.
    pub convexity: f64,
    /// Positive currency loss for a +1bp annual yield change, to first order.
    pub dv01_loss: f64,
}

fn finite(value: f64) -> BondResult<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(BondError::pricing_failed(
            "nonfinite dated cash yield result",
        ))
    }
}

/// Same-cash price and exact first/second annual-yield derivatives.
///
/// At most 4096 sorted future nonnegative amounts are admitted; at least one
/// positive amount and positive PV are required. Equal payment dates remain
/// separate cash components. Past/settlement cash is rejected, never filtered.
/// ICMA, ACT365L and maturity-dependent 30E/360 ISDA require
/// [`dated_cash_yield_risk_with_basis`]. This does not infer a market convention.
pub fn dated_cash_yield_risk(
    cash: &[BondCashFlow],
    settlement: Date,
    annual_yield: f64,
    day_count: DayCountConvention,
    compounding: Compounding,
) -> BondResult<DatedCashYieldRisk> {
    if cash.is_empty() || cash.len() > 4096 || !annual_yield.is_finite() {
        return Err(BondError::invalid_spec(
            "1..4096 cash entries and finite annual yield required",
        ));
    }
    let original: Vec<_> = cash.iter().map(DatedYieldCash::from).collect();
    dated_cash_yield_risk_with_basis(
        &original,
        settlement,
        annual_yield,
        &DiscountTimeBasis::Calendar { day_count },
        compounding,
    )
}

/// Same native price and derivatives using the complete supplied discount context.
/// Actual entitled payment dates and original cash units are consumed directly.
/// No payment adjustment, schedule construction or entitlement decision is inferred.
pub fn dated_cash_yield_risk_with_basis(
    cash: &[DatedYieldCash],
    settlement: Date,
    annual_yield: f64,
    basis: &DiscountTimeBasis,
    compounding: Compounding,
) -> BondResult<DatedCashYieldRisk> {
    if cash.is_empty() || cash.len() > 4096 || !annual_yield.is_finite() {
        return Err(BondError::invalid_spec(
            "1..4096 cash entries and finite annual yield required",
        ));
    }
    let prepared = PreparedDatedCash::new(cash, settlement, basis)?;
    yield_cash::validate_dated_cash(prepared.discount_cash(), true)?;
    let mut pv = 0.0;
    let mut first = 0.0;
    let mut second = 0.0;
    let mut weighted = 0.0;
    for &(time, amount) in prepared.discount_cash() {
        let (df, d1, d2) = match compounding {
            Compounding::Continuous => {
                let df = portable::exp(-annual_yield * time);
                (df, -time * df, time * time * df)
            }
            Compounding::Simple => {
                let denominator = finite(1.0 + annual_yield * time)?;
                if denominator <= 0.0 {
                    return Err(BondError::invalid_spec(
                        "yield outside simple interest domain",
                    ));
                }
                let df = 1.0 / denominator;
                (df, -time * df * df, 2.0 * time * time * df * df * df)
            }
            periodic => {
                let frequency = periodic.periods_per_year() as f64;
                let denominator = finite(1.0 + annual_yield / frequency)?;
                if denominator <= 0.0 {
                    return Err(BondError::invalid_spec(
                        "yield outside periodic compounding domain",
                    ));
                }
                // log1p retains the small periodic rate before exponentiation.
                let df =
                    portable::exp(-frequency * time * portable::log1p(annual_yield / frequency));
                (
                    df,
                    -time * df / denominator,
                    time * (time + 1.0 / frequency) * df / (denominator * denominator),
                )
            }
        };
        if !df.is_finite() || df <= 0.0 {
            return Err(BondError::pricing_failed(
                "positive finite dated cash discount factor required",
            ));
        }
        pv = finite(pv + amount * df)?;
        first = finite(first + amount * finite(d1)?)?;
        second = finite(second + amount * finite(d2)?)?;
        weighted = finite(weighted + amount * df * time)?;
    }
    if pv <= 0.0 {
        return Err(BondError::pricing_failed("positive dated cash PV required"));
    }
    Ok(DatedCashYieldRisk {
        present_value: pv,
        first_derivative: first,
        second_derivative: second,
        macaulay_duration: finite(weighted / pv)?,
        modified_duration: finite(-first / pv)?,
        convexity: finite(second / pv)?,
        dv01_loss: finite(-first * 0.0001)?,
    })
}
