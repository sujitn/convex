//! Whole-coupon Gaussian payment law and exact original cash-weight aggregation.
use super::*;

/// Explicit model accrued policy for an entire stochastic coupon.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WholeCouponAccrualPolicy {
    /// Expected full bounded coupon times elapsed notional-accrual / full weight.
    ExpectedCouponProRataNotionalAccrual,
}

/// One future rate is its payment-measure mean plus loadings times independent
/// unit normal factors shared by all observations of the same coupon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WholeCouponRateFactors {
    /// Original provider-neutral index.
    pub index_id: IndexId,
    /// Original observation date.
    pub observation_date: Date,
    /// Actual coupon payment; factors are local to this complete coupon.
    pub payment_date: Date,
    /// Explicit payment-measure mean adjustment in annual decimal rate.
    pub forward_adjustment: f64,
    /// Terminal decimal-rate loadings, including the actual remaining expiry.
    /// No volatility clock, independence or correlation is inferred.
    pub loadings: Vec<f64>,
    /// Actual joint-model source.
    pub source_id: SourceId,
    /// Actual positive source revision.
    pub version: Version,
    /// Information availability, bounded by the original fixing cutoff.
    pub known_at: UtcTimestamp,
}

/// Supplied Gaussian joint law. Covariance is exactly the common factor Gram
/// matrix; a sum of shifted-lognormal rates is never substituted with this law.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WholeCouponNormalLaw {
    /// Explicit model accrued convention.
    pub accrual_policy: WholeCouponAccrualPolicy,
    /// Unique original observation/payment points; unused points fail.
    pub factors: Vec<WholeCouponRateFactors>,
}

/// Complete coupon expectation after aggregating every shared normal factor.
#[derive(Debug, Serialize)]
pub struct WholeCouponNormalValue {
    /// Actual contractual accrual start.
    pub start: Date,
    /// Actual contractual accrual end.
    pub end: Date,
    /// Original adjusted payment.
    pub payment_date: Date,
    /// Sum of original notional times native reference-aware accrual fractions.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub notional_accrual: Decimal,
    /// Absolute unbounded payment-measure mean cash.
    pub unbounded_cash_mean: f64,
    /// Aggregated cash loading per common unit normal factor.
    pub cash_factor_loadings: Vec<f64>,
    /// Root sum of squared common cash loadings.
    pub cash_standard_deviation: f64,
    /// True bounded whole-coupon rate expectation and effective-mean derivatives.
    pub expectation: BoundedCouponValue,
    /// Absolute currency expected cash before discount/entitlement.
    pub expected_cash: f64,
}

#[derive(Debug)]
pub(super) struct WholeCouponTotals {
    pub(super) notional_accrual: Decimal,
    pub(super) elapsed_notional_accrual: Decimal,
    pub(super) factor_loadings: Vec<f64>,
    pub(super) cap: Option<Decimal>,
    pub(super) floor: Option<Decimal>,
}

pub(super) struct WholeCouponResult {
    pub(super) output: WholeCouponNormalValue,
    pub(super) elapsed_notional_accrual: Decimal,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn whole_coupon_value(
    start: Date,
    end: Date,
    payment_date: Date,
    known: Decimal,
    model: f64,
    whole: WholeCouponTotals,
    policy: NegativeCouponPolicy,
    rounding: CashRounding,
) -> Result<(f64, WholeCouponResult)> {
    if whole.notional_accrual <= Decimal::ZERO {
        return Err(invalid(
            "whole coupon weight",
            "positive notional accrual required",
        ));
    }
    let weight = number(whole.notional_accrual)?;
    let mean = finite(number(known)? + model)?;
    // Summing common factor loadings first retains exact shared shocks and
    // offsetting/negative gearing. Covariance is a Gram matrix by construction.
    let variance = whole.factor_loadings.iter().try_fold(0.0, |sum, x| {
        if *x != 0.0 && *x * *x == 0.0 {
            return Err(invalid(
                "joint variance",
                "positive loading square underflow",
            ));
        }
        finite(sum + x * x)
    })?;
    let deviation = variance.sqrt();
    let rate_deviation = deviation / weight;
    if deviation > 0.0 && rate_deviation == 0.0 {
        return Err(invalid("joint variance", "rate deviation underflow"));
    }
    let law = ForwardCouponModel::Normal {
        volatility: rate_deviation,
    };
    let rate_mean = finite(mean / weight)?;
    future_policy(
        law,
        1.0,
        0.0,
        rate_mean,
        whole.floor.map(number).transpose()?,
        whole.cap.map(number).transpose()?,
        policy,
    )?;
    let expectation = bounded_floating_coupon(
        law,
        1.0,
        rate_mean,
        1.0,
        0.0,
        whole.cap.map(number).transpose()?,
        whole.floor.map(number).transpose()?,
    )?;
    let amount = if whole.factor_loadings.is_empty() {
        // With no future observations retain exact deterministic bounded cash.
        let mut cash = known;
        if let Some(cap) = whole.cap {
            cash = cash.min(
                cap.checked_mul(whole.notional_accrual)
                    .ok_or(CashflowError::Arithmetic("whole known cap"))?,
            );
        }
        if let Some(floor) = whole.floor {
            cash = cash.max(
                floor
                    .checked_mul(whole.notional_accrual)
                    .ok_or(CashflowError::Arithmetic("whole known floor"))?,
            );
        }
        number(rounding.apply(cash)?)?
    } else {
        finite(weight * expectation.expected_rate)?
    };
    Ok((
        amount,
        WholeCouponResult {
            output: WholeCouponNormalValue {
                start,
                end,
                payment_date,
                notional_accrual: whole.notional_accrual,
                unbounded_cash_mean: mean,
                cash_factor_loadings: whole.factor_loadings,
                cash_standard_deviation: deviation,
                expectation,
                expected_cash: amount,
            },
            elapsed_notional_accrual: whole.elapsed_notional_accrual,
        },
    ))
}
