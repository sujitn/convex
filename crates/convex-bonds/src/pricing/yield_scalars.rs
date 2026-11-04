//! Checked scalar reporting and explicitly supplied current/simple-yield arithmetic.
//! These measures do not identify a dated market convention or solve an instrument.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::{BondError, BondResult};
use rust_decimal::{prelude::ToPrimitive, Decimal};

fn invalid(reason: &str) -> BondError {
    BondError::invalid_spec(reason)
}
fn numerical() -> BondError {
    BondError::PricingFailed {
        reason: "yield scalar arithmetic overflow or nonzero underflow".into(),
    }
}
fn finite(value: f64) -> BondResult<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(numerical())
    }
}
fn divide(numerator: f64, denominator: f64) -> BondResult<f64> {
    let result = finite(numerator / denominator)?;
    if numerator != 0.0 && result == 0.0 {
        return Err(numerical());
    }
    Ok(result)
}
fn positive(value: f64, reason: &str) -> BondResult<()> {
    finite(value)?;
    if value <= 0.0 {
        return Err(invalid(reason));
    }
    Ok(())
}
fn decimal_divide(numerator: Decimal, denominator: Decimal) -> BondResult<Decimal> {
    let result = numerator.checked_div(denominator).ok_or_else(numerical)?;
    if numerator != Decimal::ZERO && result == Decimal::ZERO {
        return Err(numerical());
    }
    Ok(result)
}
fn decimal_multiply(left: Decimal, right: Decimal) -> BondResult<Decimal> {
    let result = left.checked_mul(right).ok_or_else(numerical)?;
    if left != Decimal::ZERO && right != Decimal::ZERO && result == Decimal::ZERO {
        return Err(numerical());
    }
    Ok(result)
}
fn float(value: Decimal) -> BondResult<f64> {
    let result = value
        .to_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(numerical)?;
    if value != Decimal::ZERO && result == 0.0 {
        return Err(numerical());
    }
    Ok(result)
}

/// Annual decimal-rate scalar to percentage points; nonfinite/overflow is an error.
pub fn to_percent(rate: f64) -> BondResult<f64> {
    finite(rate)?;
    finite(rate * 100.0)
}
/// Retain the supplied binary64 in Decimal, rejecting loss of a nonzero value.
/// This does not reconstruct an original decimal market quotation.
pub fn to_decimal(rate: f64) -> BondResult<Decimal> {
    finite(rate)?;
    let result = Decimal::from_f64_retain(rate).ok_or_else(numerical)?;
    if rate != 0.0 && result == Decimal::ZERO {
        return Err(numerical());
    }
    Ok(result)
}
/// Annual coupon cash divided by a strictly positive price in the SAME money basis.
pub fn current_binary64(annual_coupon: f64, clean_price: f64) -> BondResult<f64> {
    finite(annual_coupon)?;
    positive(clean_price, "clean price must be positive")?;
    divide(annual_coupon, clean_price)
}
/// Arithmetic simple-yield estimate with coupon and price per100 face.
/// Annual decimal-rate output; no dated Japanese/Street convention is inferred.
pub fn simple_binary64(
    annual_coupon: f64,
    clean_price: f64,
    years_to_maturity: f64,
) -> BondResult<f64> {
    finite(annual_coupon)?;
    positive(clean_price, "clean price must be positive")?;
    positive(years_to_maturity, "years to maturity must be positive")?;
    let capital_gain = divide(finite(100.0 - clean_price)?, years_to_maturity)?;
    divide(finite(annual_coupon + capital_gain)?, clean_price)
}
fn current_ratio(annual_coupon: Decimal, clean_price: Decimal) -> BondResult<Decimal> {
    if clean_price <= Decimal::ZERO {
        return Err(invalid("clean price must be positive"));
    }
    decimal_divide(annual_coupon, clean_price)
}
/// Exact supplied coupon/price Decimal arithmetic, reported as an annual binary64 rate.
pub fn current_decimal_rate(annual_coupon: Decimal, clean_price: Decimal) -> BondResult<f64> {
    float(current_ratio(annual_coupon, clean_price)?)
}
/// Annual coupon RATE and per100 price, reported in Decimal percentage points.
pub fn current_percent_from_rate(
    annual_rate: Decimal,
    clean_price_per100: Decimal,
) -> BondResult<Decimal> {
    if clean_price_per100 <= Decimal::ZERO {
        return Err(invalid("clean price must be positive"));
    }
    current_percent_from_amount(
        decimal_multiply(annual_rate, Decimal::ONE_HUNDRED)?,
        clean_price_per100,
    )
}
/// Annual coupon CASH per100 and per100 price, reported in Decimal percentage points.
pub fn current_percent_from_amount(
    annual_coupon_per100: Decimal,
    clean_price_per100: Decimal,
) -> BondResult<Decimal> {
    decimal_multiply(
        current_ratio(annual_coupon_per100, clean_price_per100)?,
        Decimal::ONE_HUNDRED,
    )
}
fn coupon_per100(annual_rate: Decimal, original_face: Decimal) -> BondResult<Decimal> {
    if original_face <= Decimal::ZERO {
        return Err(invalid("original face must be positive"));
    }
    // Price is explicitly per100; native full-face cash must not be divided by it.
    decimal_multiply(annual_rate, Decimal::ONE_HUNDRED)
}
/// Constant coupon rate, actual positive native face and per100 price; decimal-rate output.
pub fn fixed_current_decimal_rate(
    annual_rate: Decimal,
    original_face: Decimal,
    clean_price_per100: Decimal,
) -> BondResult<f64> {
    current_decimal_rate(
        coupon_per100(annual_rate, original_face)?,
        clean_price_per100,
    )
}
/// Same original face/per100 normalization, reported in Decimal percentage points.
pub fn fixed_current_percent(
    annual_rate: Decimal,
    original_face: Decimal,
    clean_price_per100: Decimal,
) -> BondResult<Decimal> {
    if clean_price_per100 <= Decimal::ZERO {
        return Err(invalid("clean price must be positive"));
    }
    current_percent_from_amount(
        coupon_per100(annual_rate, original_face)?,
        clean_price_per100,
    )
}
/// Arithmetic simple-yield estimate in percentage points, with all money in one basis.
pub fn simple_decimal_percent(
    annual_coupon: Decimal,
    clean_price: Decimal,
    par_value: Decimal,
    years_to_maturity: Decimal,
) -> BondResult<Decimal> {
    if clean_price <= Decimal::ZERO {
        return Err(invalid("clean price must be positive"));
    }
    if years_to_maturity <= Decimal::ZERO {
        return Err(invalid("years to maturity must be positive"));
    }
    if par_value <= Decimal::ZERO {
        return Err(invalid("par value must be positive"));
    }
    let gain = par_value.checked_sub(clean_price).ok_or_else(numerical)?;
    let capital_gain = decimal_divide(gain, years_to_maturity)?;
    let annual = annual_coupon
        .checked_add(capital_gain)
        .ok_or_else(numerical)?;
    decimal_multiply(decimal_divide(annual, clean_price)?, Decimal::ONE_HUNDRED)
}
