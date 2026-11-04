//! Checked risk arithmetic over explicit supplied prices and quote units.
//! Stencil arithmetic alone does not establish model bump convergence.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::yield_cash;
use crate::{BondError, BondResult};

pub(crate) fn finite_inputs(values: &[f64]) -> BondResult<()> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err(BondError::invalid_spec("finite risk inputs required"));
    }
    Ok(())
}

fn product(values: &[f64]) -> BondResult<f64> {
    finite_inputs(values)?;
    if values.contains(&0.0) {
        return Ok(0.0);
    }
    values
        .iter()
        .try_fold(1.0, |result, value| yield_cash::product(result, *value))
}

fn ratio(numerator: f64, denominator: f64) -> BondResult<f64> {
    finite_inputs(&[numerator, denominator])?;
    if denominator == 0.0 {
        return Err(BondError::invalid_spec("nonzero risk denominator required"));
    }
    yield_cash::quotient(numerator, denominator)
}

fn admit_stencil(up: f64, down: f64, base: f64, bump: f64) -> BondResult<()> {
    finite_inputs(&[up, down, base, bump])?;
    if base.abs() < 1e-10 {
        return Err(BondError::invalid_spec("zero base price"));
    }
    if bump == 0.0 {
        return Err(BondError::invalid_spec("nonzero bump required"));
    }
    Ok(())
}

/// Central first sensitivity `(down-up)/(2*base*bump)` in supplied price units.
/// Signed prices/bumps remain valid. Callers retain their own bump minima.
pub fn duration_stencil(up: f64, down: f64, base: f64, bump: f64) -> BondResult<f64> {
    admit_stencil(up, down, base, bump)?;
    if up == down {
        return Ok(0.0);
    }
    let numerator = down - up;
    let denominator = 2.0 * base * bump;
    if numerator.is_finite() && denominator.is_finite() && denominator != 0.0 {
        return ratio(numerator, denominator);
    }
    let scale = up.abs().max(down.abs()).max(base.abs());
    ratio(down / scale - up / scale, 2.0 * (base / scale) * bump)
}

/// Central second sensitivity `(down+up-2*base)/(base*bump^2)`.
/// Preserve ordinary arithmetic order; normalize prices for intermediate overflow.
pub fn convexity_stencil(up: f64, down: f64, base: f64, bump: f64) -> BondResult<f64> {
    admit_stencil(up, down, base, bump)?;
    if up == base && down == base {
        return Ok(0.0);
    }
    let numerator = down + up - 2.0 * base;
    if numerator == 0.0 {
        return Ok(0.0);
    }
    let denominator = base * bump.powi(2);
    if numerator.is_finite() && denominator.is_finite() && denominator != 0.0 {
        return ratio(numerator, denominator);
    }
    let scale = up.abs().max(down.abs()).max(base.abs());
    let scaled_base = base / scale;
    ratio(
        down / scale + up / scale - 2.0 * scaled_base,
        scaled_base * bump.powi(2),
    )
}

pub(crate) fn yield_bump(annual_yield: f64, bump_bps: f64) -> BondResult<f64> {
    finite_inputs(&[annual_yield, bump_bps])?;
    let bump = ratio(bump_bps, 10_000.0)?;
    let up = annual_yield + bump;
    let down = annual_yield - bump;
    finite_inputs(&[up, down])?;
    if bump == 0.0 || up == annual_yield || down == annual_yield {
        return Err(BondError::invalid_spec("yield bump is zero or unresolved"));
    }
    Ok(bump)
}

/// First-order absolute price change `-duration*price*annual_yield_change`.
pub fn duration_price_change(duration: f64, price: f64, change: f64) -> BondResult<f64> {
    product(&[-duration, price, change])
}

/// Dimensionless second-order adjustment `convexity*change^2/2`.
pub fn convexity_adjustment(convexity: f64, change: f64) -> BondResult<f64> {
    product(&[0.5, convexity, change, change])
}

/// Absolute duration-plus-convexity price approximation using the supplied price.
pub fn price_change(duration: f64, convexity: f64, price: f64, change: f64) -> BondResult<f64> {
    finite_inputs(&[duration, convexity, price, change])?;
    if price == 0.0 || change == 0.0 || (duration == 0.0 && convexity == 0.0) {
        return Ok(0.0);
    }
    let first = duration_price_change(duration, price, change)?;
    let second = product(&[convexity_adjustment(convexity, change)?, price])?;
    let result = first + second;
    finite_inputs(&[result])?;
    Ok(result)
}

/// Position DV01: supplied duration times per100 price times signed face/100 per bp.
pub fn dv01(duration: f64, price_per_100: f64, face: f64) -> BondResult<f64> {
    finite_inputs(&[duration, price_per_100, face])?;
    if duration == 0.0 || price_per_100 == 0.0 || face == 0.0 {
        return Ok(0.0);
    }
    product(&[duration, ratio(price_per_100, 100.0)?, face, 0.0001])
}

/// Same-unit DV01 from prices supplied at explicit minus/plus1bp shifts.
pub fn dv01_from_prices(up: f64, down: f64) -> BondResult<f64> {
    finite_inputs(&[up, down])?;
    let difference = down - up;
    if difference.is_finite() {
        return ratio(difference, 2.0);
    }
    let result = product(&[down, 0.5])? - product(&[up, 0.5])?;
    finite_inputs(&[result])?;
    Ok(result)
}

/// Signed face achieving target DV01; an undefined inverse returns an error.
/// Retain the original duration/price absolute1e-10 admission threshold.
pub fn notional_from_dv01(target: f64, duration: f64, price_per_100: f64) -> BondResult<f64> {
    finite_inputs(&[target, duration, price_per_100])?;
    if duration.abs() < 1e-10 || price_per_100.abs() < 1e-10 {
        return Err(BondError::invalid_spec("undefined DV01 notional inverse"));
    }
    ratio(
        target,
        product(&[duration, ratio(price_per_100, 100.0)?, 0.0001])?,
    )
}
