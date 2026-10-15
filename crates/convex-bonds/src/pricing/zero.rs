//! Checked absolute zero-coupon cash and decimal-rate conversions.
//! No date, face-normalization, convention or successful error fallback is inferred.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_core::types::Compounding as CoreCompounding;
use convex_curves::contracts::{convert_rate, discount_from_rate};
use convex_math::portable;
use rust_decimal::{prelude::ToPrimitive, Decimal};

use crate::{
    error::{BondError, BondResult},
    instruments::Compounding,
};

fn numerical() -> BondError {
    BondError::pricing_failed("zero-coupon calculation exceeds its numerical representation domain")
}

fn core(compounding: Compounding) -> CoreCompounding {
    match compounding {
        Compounding::Annual => CoreCompounding::Annual,
        Compounding::SemiAnnual => CoreCompounding::SemiAnnual,
        Compounding::Quarterly => CoreCompounding::Quarterly,
        Compounding::Monthly => CoreCompounding::Monthly,
        Compounding::Continuous => CoreCompounding::Continuous,
    }
}

fn fraction(years: f64) -> BondResult<()> {
    if !years.is_finite() || years <= 0.0 || years > 150.0 {
        return Err(BondError::invalid_spec(
            "zero-coupon year fraction must be finite in (0,150]",
        ));
    }
    Ok(())
}

fn positive(value: Decimal, field: &str) -> BondResult<f64> {
    if value <= Decimal::ZERO {
        return Err(BondError::invalid_spec(format!("{field} must be positive")));
    }
    value
        .to_f64()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(numerical)
}

fn decimal(value: f64) -> BondResult<Decimal> {
    if !value.is_finite() {
        return Err(numerical());
    }
    let result = Decimal::from_f64_retain(value).ok_or_else(numerical)?;
    if value != 0.0 && result == Decimal::ZERO {
        return Err(numerical());
    }
    Ok(result)
}

fn validate_rate(rate: Decimal, compounding: Compounding) -> BondResult<()> {
    if compounding
        .periods_per_year_opt()
        .is_some_and(|m| rate <= -Decimal::from(m))
    {
        return Err(BondError::invalid_spec(
            "periodic compounding requires a positive base",
        ));
    }
    Ok(())
}

fn continuous(rate: Decimal, compounding: Compounding) -> BondResult<f64> {
    validate_rate(rate, compounding)?;
    let raw = rate
        .to_f64()
        .filter(|r| r.is_finite())
        .ok_or_else(numerical)?;
    if let Some(m) = compounding.periods_per_year_opt() {
        if raw / f64::from(m) <= -1.0 {
            // Preserve the original positive Decimal residual if projecting the
            // rate alone to binary64 would round it onto the periodic pole.
            let residual = rate.checked_add(Decimal::from(m)).ok_or_else(numerical)?;
            let base = positive(residual, "periodic residual")? / f64::from(m);
            let result = f64::from(m) * portable::log(base);
            return if result.is_finite() {
                Ok(result)
            } else {
                Err(numerical())
            };
        }
    }
    convert_rate(raw, 1.0, core(compounding), CoreCompounding::Continuous).map_err(|_| numerical())
}

fn from_continuous(rate: f64, compounding: Compounding) -> BondResult<Decimal> {
    let raw = convert_rate(rate, 1.0, CoreCompounding::Continuous, core(compounding))
        .map_err(|_| numerical())?;
    let result = if let Some(m) = compounding.periods_per_year_opt() {
        if raw <= -f64::from(m) {
            // exp_m1 can round a representable positive tail onto -1. Retain
            // that tail in Decimal before subtracting one and scaling by m.
            let tail = decimal(f64::from(m) * portable::exp(rate / f64::from(m)))?;
            if tail <= Decimal::ZERO {
                return Err(numerical());
            }
            tail.checked_sub(Decimal::from(m)).ok_or_else(numerical)?
        } else {
            decimal(raw)?
        }
    } else {
        decimal(raw)?
    };
    validate_rate(result, compounding).map_err(|_| numerical())?;
    Ok(result)
}

/// Actual instrument dates have already validated zero discount time.
/// Retain absolute original Decimal cash rather than a floating conversion.
pub(crate) fn dated_price_from_yield(
    redemption: Decimal,
    rate: Decimal,
    years: f64,
    compounding: Compounding,
) -> BondResult<Decimal> {
    if years != 0.0 {
        return price_from_yield(redemption, rate, years, compounding);
    }
    positive(redemption, "redemption")?;
    continuous(rate, compounding)?;
    Ok(redemption)
}

/// Constant cash has either no yield solution or no uniquely identified yield.
pub(crate) fn dated_yield_from_price(
    redemption: Decimal,
    price: Decimal,
    years: f64,
    compounding: Compounding,
) -> BondResult<Decimal> {
    if years != 0.0 {
        return yield_from_price(redemption, price, years, compounding);
    }
    positive(redemption, "redemption")?;
    positive(price, "price")?;
    Err(if price == redemption {
        BondError::YieldNonUnique
    } else {
        BondError::YieldNoRoot
    })
}

/// Equivalent annualized decimal rate. Same-convention inputs are validated
/// and returned exactly; negative rates require an exactly positive source base.
pub fn convert_yield(rate: Decimal, from: Compounding, to: Compounding) -> BondResult<Decimal> {
    let continuous = continuous(rate, from)?;
    if from == to {
        return Ok(rate);
    }
    from_continuous(continuous, to)
}

/// Absolute cash price of the supplied positive redemption over an explicit
/// checked positive year fraction; no per100 face scaling or settlement lag.
pub fn price_from_yield(
    redemption: Decimal,
    rate: Decimal,
    years: f64,
    compounding: Compounding,
) -> BondResult<Decimal> {
    fraction(years)?;
    let cash = positive(redemption, "redemption")?;
    let annual = continuous(rate, compounding)?;
    let log_discount = -annual * years;
    let discount =
        discount_from_rate(annual, years, CoreCompounding::Continuous).map_err(|_| numerical())?;
    let price = if log_discount.abs() < 0.5 {
        // Apply the actual small cash change before Decimal addition, avoiding
        // exp(x) rounding to one and silently returning redemption.
        let change = decimal(cash * portable::expm1(log_discount))?;
        let result = redemption.checked_add(change).ok_or_else(numerical)?;
        if log_discount != 0.0 && result == redemption {
            return Err(numerical());
        }
        result
    } else {
        decimal(cash * discount)?
    };
    if price <= Decimal::ZERO {
        return Err(numerical());
    }
    Ok(price)
}

/// Annualized decimal yield from absolute positive original redemption and
/// price. Exact cash differences retain small nonzero yields near par.
pub fn yield_from_price(
    redemption: Decimal,
    price: Decimal,
    years: f64,
    compounding: Compounding,
) -> BondResult<Decimal> {
    fraction(years)?;
    let cash = positive(redemption, "redemption")?;
    let paid = positive(price, "price")?;
    let difference = redemption.checked_sub(price).ok_or_else(numerical)?;
    let relative = difference.to_f64().ok_or_else(numerical)? / paid;
    let log_ratio = if relative > -0.5 {
        portable::log1p(relative)
    } else {
        portable::log(cash) - portable::log(paid)
    };
    let annual = log_ratio / years;
    if difference != Decimal::ZERO && annual == 0.0 {
        return Err(numerical());
    }
    from_continuous(annual, compounding)
}
