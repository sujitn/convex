//! Checked explicit bill rate conversions; all results are decimal fractions.
//!
//! Bank discount uses face and 360 days; money market uses price and 360 days.
//! Simple fixed-365 yield is a distinct convention restricted to 1–182 days.
//! Treasury investment-rate quotations require their original dated owner.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rust_decimal::Decimal;

use crate::error::{BondError, BondResult};

fn arithmetic_error() -> BondError {
    BondError::pricing_failed("bill rate exceeds Decimal arithmetic domain")
}

fn cash_rate(
    price: Decimal,
    face: Decimal,
    days: i64,
    basis_days: u32,
    denominator: Decimal,
) -> BondResult<Decimal> {
    if price <= Decimal::ZERO {
        return Err(BondError::InvalidPrice {
            reason: "price must be positive".into(),
        });
    }
    if face <= Decimal::ZERO {
        return Err(BondError::invalid_spec("face value must be positive"));
    }
    if days <= 0 {
        return Err(BondError::invalid_spec("days to maturity must be positive"));
    }
    let gain = face
        .checked_sub(price)
        .ok_or_else(arithmetic_error)?
        .checked_div(denominator)
        .ok_or_else(arithmetic_error)?;
    let factor = Decimal::from(basis_days)
        .checked_div(Decimal::from(days))
        .ok_or_else(arithmetic_error)?;
    gain.checked_mul(factor).ok_or_else(arithmetic_error)
}

/// Checked bank-discount rate on positive original face and price, on 360 days.
/// Discount, par and premium prices are admitted. This result is a decimal rate.
pub fn bank_discount_yield(price: Decimal, face: Decimal, days: i64) -> BondResult<Decimal> {
    cash_rate(price, face, days, 360, face)
}

/// Checked simple money-market rate on positive original price, on 360 days.
/// This is a price-basis investment rate, not a face-basis bank-discount rate.
pub fn money_market_yield(price: Decimal, face: Decimal, days: i64) -> BondResult<Decimal> {
    cash_rate(price, face, days, 360, price)
}

/// Checked fixed-365 simple rate for 1 through 182 remaining days, inclusive.
/// Long terms need original issue, settlement and maturity through the dated
/// Treasury owner; no quadratic approximation or issue-year basis is inferred.
pub fn simple_bill_yield(price: Decimal, face: Decimal, days: i64) -> BondResult<Decimal> {
    short_term(days)?;
    cash_rate(price, face, days, 365, price)
}

fn short_term(days: i64) -> BondResult<()> {
    if days <= 0 {
        return Err(BondError::invalid_spec("days to maturity must be positive"));
    }
    if days > 182 {
        return Err(BondError::invalid_spec(
            "long-bill quotation requires original issue, settlement and maturity dates; use quote_treasury_bill",
        ));
    }
    Ok(())
}

/// Convert a decimal simple price-basis 360 money-market rate to fixed-365.
/// The implied face/price ratio must be positive; negative premium rates remain
/// valid inside that domain. Only 1 through 182 remaining days are admitted.
pub fn simple_bill_yield_from_money_market(rate: Decimal, days: i64) -> BondResult<Decimal> {
    short_term(days)?;
    // Compare rate * days > -360 exactly, without losing a positive residual
    // when a rounded Decimal addition to one would cancel near the rate pole.
    // A 96-bit mantissa times <=182 and 360*10^scale (scale<=28) fit in i128.
    let scaled_rate = rate
        .mantissa()
        .checked_mul(i128::from(days))
        .ok_or_else(arithmetic_error)?;
    let lower_bound = 10i128
        .checked_pow(rate.scale())
        .and_then(|scale| scale.checked_mul(-360))
        .ok_or_else(arithmetic_error)?;
    if scaled_rate <= lower_bound {
        return Err(BondError::invalid_spec(
            "money-market rate implies nonpositive face/price",
        ));
    }
    rate.checked_mul(Decimal::from(365))
        .and_then(|scaled| scaled.checked_div(Decimal::from(360)))
        .ok_or_else(arithmetic_error)
}
