//! Checked dated-cash yield PV and analytical derivatives on one discount basis.
//! Cash is already entitled and absolute; this operation does not infer a market
//! quotation convention or manufacture ICMA reference periods.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{error::BondResult, traits::BondCashFlow, BondError};
use convex_core::{
    daycounts::DayCountConvention,
    types::{Compounding, Date},
};
use convex_math::portable;
use rust_decimal::{prelude::ToPrimitive, Decimal};

/// All monetary fields use the exact input cash currency and scale.
#[derive(Debug, Clone, Copy)]
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

fn number(value: Decimal) -> BondResult<f64> {
    value
        .to_f64()
        .filter(|n| n.is_finite() && (value.is_zero() || *n != 0.0))
        .ok_or_else(|| {
            BondError::pricing_failed("dated cash Decimal conversion overflow/underflow")
        })
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
/// ACT/ACT ICMA and maturity-dependent 30E/360 ISDA require full contextual
/// quotation inputs elsewhere. A valid enum is not a market convention model.
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
    if matches!(
        day_count,
        DayCountConvention::ActActIcma | DayCountConvention::Thirty360EIsda
    ) {
        return Err(BondError::invalid_spec(
            "dated cash day count requires explicit reference/maturity context",
        ));
    }
    if cash
        .iter()
        .any(|c| c.date <= settlement || c.amount < Decimal::ZERO)
        || cash.windows(2).any(|c| c[1].date < c[0].date)
        || !cash.iter().any(|c| c.amount > Decimal::ZERO)
    {
        return Err(BondError::invalid_spec(
            "sorted future nonnegative cash with positive exposure required",
        ));
    }
    let dc = day_count.to_day_count();
    let mut pv = 0.0;
    let mut first = 0.0;
    let mut second = 0.0;
    let mut weighted = 0.0;
    for c in cash {
        let time = number(dc.year_fraction(settlement, c.date))?;
        let amount = number(c.amount)?;
        if time <= 0.0 {
            return Err(BondError::invalid_spec(
                "positive dated cash year fraction required",
            ));
        }
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
