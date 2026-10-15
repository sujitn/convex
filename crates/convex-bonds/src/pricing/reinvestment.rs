//! Checked constant-rate account projection over original supplied cash units.
//! This does not infer a bond forward, complete instrument cash, receipts or P&L.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::{BondError, BondResult};
use convex_core::{cashflows::MAX_CONTRACT_DAYS, contracts::ExactDecimal, types::Date};
use convex_math::portable;
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde::{Deserialize, Serialize};

/// One original dated transfer, preserving its sign and actual input cash units.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProjectionCash {
    /// Actual transfer date, strictly after settlement.
    pub date: Date,
    /// Original signed amount; no face/per100 conversion is inferred.
    pub amount: ExactDecimal,
}
/// Explicit flat periodic ACT/365F model and simple return-reporting basis.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProjectionTerms {
    /// Original start date.
    pub settlement: Date,
    /// Explicit account close date; terminal cash must be supplied when reached.
    pub maturity: Date,
    /// Positive initial value in the SAME units as every transfer.
    pub initial_cash_value: ExactDecimal,
    /// Annual decimal model rate used to roll the residual account.
    pub valuation_rate: ExactDecimal,
    /// Independent annual decimal model rate applied to reinvested transfers.
    pub reinvestment_rate: ExactDecimal,
    /// Positive periodic compound frequency for both explicit rates.
    pub frequency: u32,
    /// Positive fixed days per reporting year; does not alter ACT/365F growth.
    pub reporting_days_per_year: u32,
    /// Positive calendar-day horizon, clamped to the supplied maturity.
    pub max_horizon_days: u32,
}
/// Model projection only; this is not recorded cash or realized P&L.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ProjectionResult {
    /// Actual effective horizon.
    pub horizon: Date,
    /// Rolled residual before maturity; zero at the explicitly supplied close.
    pub residual_balance: ExactDecimal,
    /// Original transfers compounded to the effective horizon.
    pub reinvested_cash: ExactDecimal,
    /// Residual plus reinvested cash in original input cash units.
    pub horizon_value: ExactDecimal,
    /// Simple annualized return as a decimal rate on the explicit reporting basis.
    pub annualized_return: ExactDecimal,
}
fn invalid(s: &str) -> BondError {
    BondError::invalid_spec(s)
}
fn arithmetic() -> BondError {
    BondError::pricing_failed(
        "projection arithmetic overflow/nonzero underflow or lost rate precision",
    )
}
fn interval(start: Date, end: Date, zero: bool) -> BondResult<i64> {
    let days = start.days_between(&end);
    if !(1..=9999).contains(&start.year())
        || !(1..=9999).contains(&end.year())
        || days < 0
        || (!zero && days == 0)
        || days > MAX_CONTRACT_DAYS
    {
        return Err(invalid("invalid or excessive projection interval"));
    }
    Ok(days)
}
fn mul(a: Decimal, b: Decimal) -> BondResult<Decimal> {
    let value = a.checked_mul(b).ok_or_else(arithmetic)?;
    if !a.is_zero() && !b.is_zero() && value.is_zero() {
        return Err(arithmetic());
    }
    Ok(value)
}
fn div(a: Decimal, b: Decimal) -> BondResult<Decimal> {
    let value = a.checked_div(b).ok_or_else(arithmetic)?;
    if !a.is_zero() && value.is_zero() {
        return Err(arithmetic());
    }
    Ok(value)
}
fn number(value: Decimal) -> BondResult<f64> {
    value
        .to_f64()
        .filter(|n| n.is_finite() && (value.is_zero() || *n != 0.0))
        .ok_or_else(arithmetic)
}
/// Explicit periodic compound growth over actual days/365. Equal dates have unit
/// growth, after dates, frequency and rate domain have all been validated.
pub fn periodic_growth(
    rate: Decimal,
    frequency: u32,
    start: Date,
    end: Date,
) -> BondResult<Decimal> {
    let days = interval(start, end, true)?;
    if frequency == 0 {
        return Err(invalid("positive projection compound frequency required"));
    }
    let per = div(rate, Decimal::from(frequency))?;
    let base = Decimal::ONE.checked_add(per).ok_or_else(arithmetic)?;
    if base <= Decimal::ZERO {
        return Err(invalid(
            "projection rate outside positive periodic base domain",
        ));
    }
    if !per.is_zero() && base == Decimal::ONE {
        return Err(arithmetic());
    }
    let b = number(base)?;
    if base != Decimal::ONE && b == 1.0 {
        return Err(arithmetic());
    }
    // Retain the original ordinary time/conversion/power operator order.
    let periods = (days as f64 / 365.0) * f64::from(frequency);
    let periods = Decimal::from_f64_retain(periods).ok_or_else(arithmetic)?;
    if days != 0 && periods.is_zero() {
        return Err(arithmetic());
    }
    let growth = portable::pow(b, number(periods)?);
    if !growth.is_finite() || growth <= 0.0 || (days != 0 && base != Decimal::ONE && growth == 1.0)
    {
        return Err(arithmetic());
    }
    let value = Decimal::from_f64_retain(growth).ok_or_else(arithmetic)?;
    if value <= Decimal::ZERO || (days != 0 && base != Decimal::ONE && value == Decimal::ONE) {
        return Err(arithmetic());
    }
    Ok(value)
}
/// Project 1..4096 sorted future transfers without manufacturing an instrument.
/// Equal dates retain their original component order. At maturity the original
/// terminal record is mandatory, including an explicitly supplied zero payout.
pub fn project(cash: &[ProjectionCash], terms: &ProjectionTerms) -> BondResult<ProjectionResult> {
    let remaining = interval(terms.settlement, terms.maturity, false)?;
    let initial = terms.initial_cash_value.decimal();
    if initial <= Decimal::ZERO
        || terms.reporting_days_per_year == 0
        || terms.max_horizon_days == 0
        || cash.is_empty()
        || cash.len() > 4096
    {
        return Err(invalid(
            "positive initial cash/reporting basis/horizon and1..4096 transfers required",
        ));
    }
    periodic_growth(
        terms.valuation_rate.decimal(),
        terms.frequency,
        terms.settlement,
        terms.settlement,
    )?;
    periodic_growth(
        terms.reinvestment_rate.decimal(),
        terms.frequency,
        terms.settlement,
        terms.settlement,
    )?;
    let days = i64::from(terms.max_horizon_days).min(remaining);
    let horizon = terms
        .settlement
        .checked_add_days(days)
        .map_err(|e| invalid(&e.to_string()))?;
    let mut previous = terms.settlement;
    for c in cash {
        interval(terms.settlement, c.date, false)?;
        if c.date < previous || c.date > terms.maturity {
            return Err(invalid(
                "projection cash must retain sorted dates through original maturity",
            ));
        }
        previous = c.date;
    }
    if horizon == terms.maturity && previous != terms.maturity {
        return Err(BondError::missing_field(
            "original terminal cash at the requested account close",
        ));
    }
    let mut balance = initial;
    let mut reinvested = Decimal::ZERO;
    let mut current = terms.settlement;
    for c in cash.iter().filter(|c| c.date <= horizon) {
        balance = mul(
            balance,
            periodic_growth(
                terms.valuation_rate.decimal(),
                terms.frequency,
                current,
                c.date,
            )?,
        )?
        .checked_sub(c.amount.decimal())
        .ok_or_else(arithmetic)?;
        reinvested = mul(
            reinvested,
            periodic_growth(
                terms.reinvestment_rate.decimal(),
                terms.frequency,
                current,
                c.date,
            )?,
        )?
        .checked_add(c.amount.decimal())
        .ok_or_else(arithmetic)?;
        current = c.date;
    }
    if current < horizon {
        balance = mul(
            balance,
            periodic_growth(
                terms.valuation_rate.decimal(),
                terms.frequency,
                current,
                horizon,
            )?,
        )?;
        reinvested = mul(
            reinvested,
            periodic_growth(
                terms.reinvestment_rate.decimal(),
                terms.frequency,
                current,
                horizon,
            )?,
        )?;
    }
    if horizon == terms.maturity {
        balance = Decimal::ZERO;
    }
    let value = balance.checked_add(reinvested).ok_or_else(arithmetic)?;
    let ratio = div(value, initial)?;
    if value != initial && ratio == Decimal::ONE {
        return Err(arithmetic());
    }
    let change = ratio.checked_sub(Decimal::ONE).ok_or_else(arithmetic)?;
    let time = div(
        Decimal::from(days),
        Decimal::from(terms.reporting_days_per_year),
    )?;
    let rate = div(change, time)?;
    Ok(ProjectionResult {
        horizon,
        residual_balance: ExactDecimal::new(balance),
        reinvested_cash: ExactDecimal::new(reinvested),
        horizon_value: ExactDecimal::new(value),
        annualized_return: ExactDecimal::new(rate),
    })
}
