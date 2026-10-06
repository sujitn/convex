//! Multi-curve expected coupon valuation with explicit sign, units and lineage.
//! No par-floater shortcut or inferred projection/discount mapping is used.

use crate::{
    contract::calculation, invalid, CashflowContext, CouponEvidence, ForecastRate, LegTerms,
    ProjectionRequest, SwapCashflows, SwapContract, SwapError,
};
use convex_core::{
    cashflows::{
        BoundarySide, CashRounding, CashflowError, CouponPeriod, CouponProfiles, DatedProfile,
        DatedValue, NegativeCouponPolicy, ProfileKind,
    },
    contracts::{
        ExactDecimal, FiniteF64, IndexId, ModelDecimalRate, ModelMoney, Money, UtcTimestamp,
    },
    types::Compounding,
    Currency, Date,
};
use convex_curves::contracts::{
    BuiltCurve, CurvePoint, CurveQuery, CurveQueryValue, CurveRecipe, CurveRole, CurveSet,
    MAX_CURVES,
};
use rust_decimal::{prelude::ToPrimitive, Decimal};

/// Resolved immutable market values. Contract construction has already bound
/// every actual independent calendar; valuation needs no provider or clock.
pub struct SwapValuationInputs<'a> {
    /// Exact supplied curve versions, with currency and semantic roles.
    pub curves: &'a CurveSet,
    /// Eligible published history; a missing due fixing is never forecast.
    pub fixings: &'a convex_core::fixings::FixingSeries,
}

/// Analytic par quotes are valid for unrounded, signed linear coupon contracts.
#[derive(Debug, Clone, PartialEq)]
pub enum SwapParQuotes {
    /// Uniform replacement rates and parallel profile shifts, in decimal units.
    Available {
        /// Uniform fixed rate that replaces every fixed-rate profile level.
        fixed_rate: ModelDecimalRate,
        /// Uniform floating spread that replaces every spread profile level.
        floating_spread: ModelDecimalRate,
        /// Parallel addition to the existing fixed-rate profile to reach zero PV.
        fixed_rate_shift: ModelDecimalRate,
        /// Parallel addition to the existing floating spread profile to reach zero PV.
        floating_spread_shift: ModelDecimalRate,
        /// Positive unsigned fixed-leg PV per unit decimal rate.
        fixed_annuity: ModelMoney,
        /// Positive unsigned floating-leg PV per unit decimal simple spread.
        floating_annuity: ModelMoney,
    },
    /// No remaining rate exposure or a nonlinear contract has no admitted inverse.
    Unavailable {
        /// Stable economic explanation; never a manufactured zero rate.
        reason: &'static str,
    },
}

/// Per-coupon expected PV in absolute contract currency, normalized at valuation.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscountedCoupon {
    /// Actual cash payment date, distinct from accrual maturity.
    pub payment_date: Date,
    /// P(payment)/P(valuation), not a discount-to-projection substitution.
    pub discount_ratio: FiniteF64,
    /// Signed PV of this exact contract-rounded expected coupon.
    pub present_value: ModelMoney,
    /// Exact query conventions/version/extrapolation evidence.
    pub query: CurveQueryValue,
}

/// Complete native valuation, including cash and actually consumed market lineage.
#[derive(Debug, Clone)]
pub struct SwapValuation {
    /// Signed net PV in absolute currency units; negative PV remains valid.
    pub present_value: ModelMoney,
    /// Signed fixed leg PV.
    pub fixed_present_value: ModelMoney,
    /// Signed floating leg PV.
    pub floating_present_value: ModelMoney,
    /// Expected accrued cash in trade direction, respecting independent profiles.
    pub fixed_accrued: Money,
    /// Term slices or actual overnight observation prefix accrued.
    pub floating_accrued: Money,
    /// Par quotes with explicit admissibility, not a rounded-cash approximation.
    pub par: SwapParQuotes,
    /// Exact original terms/context and published/projected cash evidence.
    pub cashflows: SwapCashflows,
    /// Discounted fixed coupons in the original chronological order.
    pub fixed: Vec<DiscountedCoupon>,
    /// Discounted floating coupons in the original chronological order.
    pub floating: Vec<DiscountedCoupon>,
    /// Actual projection queries, once for every projected unique observation.
    pub projection_queries: Vec<CurveQueryValue>,
    /// Actually consumed valuation-date discount; absent after last cash payment.
    pub valuation_discount: Option<CurveQueryValue>,
    /// Exact actually consumed curve recipes, including transitive model bases.
    /// Unused supplied curves and missing historical projection sources are absent.
    pub consumed_curves: Vec<CurveRecipe>,
}

/// Value every outstanding contractual coupon on separate exact discount/index
/// roles. The whole required history is checked before any curve projection.
/// Fully paid/mature projection has zero PV without invented market inputs.
pub fn price_swap<P>(
    contract: &SwapContract,
    inputs: &SwapValuationInputs<'_>,
    context: CashflowContext,
    publication: P,
) -> Result<SwapValuation, SwapError>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    let mut projection_queries = Vec::new();
    let mut projection_error = None;
    let cash =
        contract.cashflows(
            context,
            inputs.fixings,
            publication,
            |request| match forecast_from_curve(request, contract.terms().currency, inputs.curves) {
                Ok((rate, query)) => {
                    projection_queries.push(query);
                    Ok(rate)
                }
                Err(error) => {
                    projection_error = Some(error);
                    Err(CashflowError::InvalidTerms {
                        field: "projection_curve",
                        reason: "exact projection query failed".into(),
                    })
                }
            },
        );
    let cash = match cash {
        Ok(value) => value,
        Err(error) => return Err(projection_error.unwrap_or(error)),
    };
    let currency = contract.terms().currency;
    let mut fixed = Vec::new();
    let mut floating = Vec::new();
    let mut valuation_discount = None;
    let mut fixed_pv = 0.0;
    let mut float_pv = 0.0;
    let mut fixed_annuity = 0.0;
    let mut float_annuity = 0.0;
    let mut spread_pv = 0.0;
    if !cash.fixed.is_empty() || !cash.floating.is_empty() {
        let discount = inputs
            .curves
            .get(&contract.terms().collateral.discount_curve)?;
        if discount.recipe().currency != currency
            || !matches!(discount.recipe().role, CurveRole::Discount { collateral_currency } if collateral_currency == contract.terms().collateral.currency)
        {
            return Err(invalid(
                "discount_curve",
                "exact currency/collateral discount role required",
            ));
        }
        let anchor = discount.query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Date(context.fixings.valuation_date),
        })?;
        let fixed_units = unit_profiles(&contract.terms().fixed.leg)?;
        let float_units = unit_profiles(contract.terms().floating.leg())?;
        let spreads = spread_profiles(
            contract.terms().floating.leg(),
            &contract.terms().floating.profiles().spreads,
        )?;
        for coupon in &cash.fixed {
            let flow = discounted(
                &coupon.period,
                coupon.signed_amount,
                currency,
                discount,
                anchor.value,
            )?;
            fixed_pv = total(fixed_pv, flow.present_value.value().get())?;
            fixed_annuity = total(
                fixed_annuity,
                number(fixed_units.coupon_amount(
                    &coupon.period,
                    unrounded(&contract.terms().fixed.leg, Decimal::ZERO),
                )?)? * flow.discount_ratio.get(),
            )?;
            fixed.push(flow);
        }
        for coupon in &cash.floating {
            let flow = discounted(
                &coupon.period,
                coupon.signed_amount,
                currency,
                discount,
                anchor.value,
            )?;
            float_pv = total(float_pv, flow.present_value.value().get())?;
            float_annuity = total(
                float_annuity,
                number(float_units.coupon_amount(
                    &coupon.period,
                    unrounded(contract.terms().floating.leg(), Decimal::ZERO),
                )?)? * flow.discount_ratio.get(),
            )?;
            spread_pv = total(
                spread_pv,
                number(spreads.coupon_amount(
                    &coupon.period,
                    unrounded(contract.terms().floating.leg(), Decimal::ZERO),
                )?)? * flow.discount_ratio.get(),
            )?;
            floating.push(flow);
        }
        valuation_discount = Some(anchor);
    }
    let sign = number(contract.terms().side.fixed_sign())?;
    let unsigned_fixed = fixed_pv * sign;
    let unsigned_float = float_pv * -sign;
    let par = if [
        contract.terms().fixed.leg.rounding,
        contract.terms().floating.leg().rounding,
    ]
    .iter()
    .any(|x| *x != CashRounding::Unrounded)
        || [
            contract.terms().fixed.leg.negative_coupons,
            contract.terms().floating.leg().negative_coupons,
        ]
        .iter()
        .any(|x| *x != NegativeCouponPolicy::Allow)
    {
        SwapParQuotes::Unavailable {
            reason: "analytic inverse requires unrounded cash and Allow negative-coupon policies",
        }
    } else if fixed_annuity <= 0.0 || float_annuity <= 0.0 {
        SwapParQuotes::Unavailable {
            reason: "no positive remaining fixed and floating rate exposure",
        }
    } else {
        SwapParQuotes::Available {
            fixed_rate: model_rate(unsigned_float / fixed_annuity)?,
            floating_spread: model_rate(
                (unsigned_fixed - unsigned_float + spread_pv) / float_annuity,
            )?,
            fixed_rate_shift: model_rate((unsigned_float - unsigned_fixed) / fixed_annuity)?,
            floating_spread_shift: model_rate((unsigned_fixed - unsigned_float) / float_annuity)?,
            fixed_annuity: money(fixed_annuity, currency)?,
            floating_annuity: money(float_annuity, currency)?,
        }
    };
    let fixed_accrued = accrued(contract, &cash, true)?;
    let floating_accrued = accrued(contract, &cash, false)?;
    let consumed_curves = consumed_curves(
        inputs.curves,
        projection_queries
            .iter()
            .chain(valuation_discount.iter())
            .map(|query| &query.curve),
    )?;
    Ok(SwapValuation {
        present_value: money(total(fixed_pv, float_pv)?, currency)?,
        fixed_present_value: money(fixed_pv, currency)?,
        floating_present_value: money(float_pv, currency)?,
        fixed_accrued,
        floating_accrued,
        par,
        cashflows: cash,
        fixed,
        floating,
        projection_queries,
        valuation_discount,
        consumed_curves,
    })
}

fn consumed_curves<'a>(
    curves: &CurveSet,
    roots: impl Iterator<Item = &'a convex_curves::contracts::CurveKey>,
) -> Result<Vec<CurveRecipe>, SwapError> {
    let mut pending: Vec<_> = roots.cloned().collect();
    let mut used = std::collections::BTreeMap::new();
    while let Some(key) = pending.pop() {
        if used.contains_key(&key) {
            continue;
        }
        if used.len() >= MAX_CURVES {
            return Err(convex_curves::contracts::CurveContractError::BoundExceeded(
                "consumed curve recipes",
            )
            .into());
        }
        let recipe = curves.get(&key)?.recipe().clone();
        pending.extend(
            recipe
                .dependencies
                .iter()
                .filter(|key| !used.contains_key(*key))
                .cloned(),
        );
        used.insert(key, recipe);
    }
    Ok(used.into_values().collect())
}

/// Query the full actual index interval in simple annualized units, validating
/// exact source, index and currency. A discount-role curve cannot be substituted.
pub fn forecast_from_curve(
    request: &ProjectionRequest,
    currency: Currency,
    curves: &CurveSet,
) -> Result<(ForecastRate, CurveQueryValue), SwapError> {
    let projection = curves.get(&request.projection_curve)?;
    if projection.recipe().currency != currency
        || !matches!(&projection.recipe().role, CurveRole::Projection { index_id } if index_id == request.requirement.index_id.as_str())
    {
        return Err(invalid(
            "projection_curve",
            "exact index/projection role and cash currency required",
        ));
    }
    let query = projection.query(&CurveQuery::ForwardRate {
        start: CurvePoint::Date(request.rate_start),
        end: CurvePoint::Date(request.rate_end),
        compounding: Compounding::Simple,
        day_count: request.day_count,
    })?;
    let rate = Decimal::from_f64_retain(query.value)
        .ok_or_else(|| invalid("projection_rate", "Decimal conversion overflow"))?;
    if query.value != 0.0 && rate.is_zero() {
        return Err(invalid(
            "projection_rate",
            "nonzero rate underflows Decimal",
        ));
    }
    Ok((
        ForecastRate {
            rate,
            projection_curve: request.projection_curve.clone(),
        },
        query,
    ))
}

fn discounted(
    period: &CouponPeriod,
    amount: Decimal,
    currency: Currency,
    curve: &BuiltCurve,
    anchor: f64,
) -> Result<DiscountedCoupon, SwapError> {
    let query = curve.query(&CurveQuery::DiscountFactor {
        point: CurvePoint::Date(period.payment_date()),
    })?;
    let ratio = query.value / anchor;
    if !ratio.is_finite() || ratio <= 0.0 {
        return Err(invalid("discount_ratio", "positive finite ratio required"));
    }
    Ok(DiscountedCoupon {
        payment_date: period.payment_date(),
        discount_ratio: finite(ratio)?,
        present_value: money(number(amount)? * ratio, currency)?,
        query,
    })
}
fn number(amount: Decimal) -> Result<f64, SwapError> {
    let value = amount
        .to_f64()
        .ok_or_else(|| invalid("model_number", "Decimal conversion failed"))?;
    if !value.is_finite() || (amount != Decimal::ZERO && value == 0.0) {
        return Err(invalid("model_number", "nonfinite or underflowed amount"));
    }
    Ok(value)
}
fn finite(value: f64) -> Result<FiniteF64, SwapError> {
    FiniteF64::new(value).map_err(|_| invalid("model_value", "nonfinite calculation"))
}
fn money(value: f64, currency: Currency) -> Result<ModelMoney, SwapError> {
    Ok(ModelMoney::new(finite(value)?, currency))
}
fn model_rate(value: f64) -> Result<ModelDecimalRate, SwapError> {
    Ok(ModelDecimalRate::new(finite(value)?))
}
fn total(previous: f64, value: f64) -> Result<f64, SwapError> {
    let sum = previous + value;
    finite(value)?;
    finite(sum)?;
    Ok(sum)
}
fn unrounded(leg: &LegTerms, index: Decimal) -> convex_core::cashflows::CouponCalculation {
    let mut value = calculation(leg, index);
    value.rounding = CashRounding::Unrounded;
    value.negative_coupons = NegativeCouponPolicy::Allow;
    value
}
fn single(leg: &LegTerms, kind: ProfileKind, value: Decimal) -> Result<DatedProfile, SwapError> {
    let a = leg
        .notionals
        .first()
        .ok_or_else(|| invalid("notionals", "empty"))?
        .start;
    let b = leg
        .notionals
        .last()
        .ok_or_else(|| invalid("notionals", "empty"))?
        .end;
    Ok(DatedProfile::new(
        kind,
        vec![DatedValue {
            start: a,
            end: b,
            value,
        }],
    )?)
}
fn unit_profiles(leg: &LegTerms) -> Result<CouponProfiles, SwapError> {
    Ok(CouponProfiles::new(
        single(leg, ProfileKind::Coupon, Decimal::ONE)?,
        single(leg, ProfileKind::Spread, Decimal::ZERO)?,
        single(leg, ProfileKind::Gearing, Decimal::ZERO)?,
        DatedProfile::new(ProfileKind::Notional, leg.notionals.clone())?,
    )?)
}
fn spread_profiles(leg: &LegTerms, spreads: &[DatedValue]) -> Result<CouponProfiles, SwapError> {
    Ok(CouponProfiles::new(
        single(leg, ProfileKind::Coupon, Decimal::ZERO)?,
        DatedProfile::new(ProfileKind::Spread, spreads.to_vec())?,
        single(leg, ProfileKind::Gearing, Decimal::ZERO)?,
        DatedProfile::new(ProfileKind::Notional, leg.notionals.clone())?,
    )?)
}

fn accrued(contract: &SwapContract, cash: &SwapCashflows, fixed: bool) -> Result<Money, SwapError> {
    let leg = if fixed {
        &contract.terms().fixed.leg
    } else {
        contract.terms().floating.leg()
    };
    let coupons = if fixed { &cash.fixed } else { &cash.floating };
    let at = cash.context.fixings.valuation_date;
    let mut amount = Decimal::ZERO;
    for coupon in coupons {
        let (start, end) = coupon.period.accrual_dates();
        let cutoff = at.min(end);
        if cutoff <= start {
            continue;
        }
        let value = if cutoff == end {
            coupon.amount
        } else {
            match &coupon.evidence {
                CouponEvidence::Fixed => contract.fixed_profiles.segment_amount(
                    &coupon.period,
                    start,
                    cutoff,
                    unrounded(leg, Decimal::ZERO),
                )?,
                CouponEvidence::Term(resets) => {
                    let mut sum = Decimal::ZERO;
                    for reset in resets {
                        let stop = cutoff.min(reset.dates.accrual_end);
                        if stop > reset.dates.accrual_start {
                            sum = sum
                                .checked_add(contract.floating_profiles.segment_amount(
                                    &coupon.period,
                                    reset.dates.accrual_start,
                                    stop,
                                    unrounded(leg, reset.rate.rate),
                                )?)
                                .ok_or(CashflowError::Arithmetic("reset accrued sum"))?;
                        }
                    }
                    sum
                }
                CouponEvidence::Overnight(compound) => {
                    // Prefix the actual represented interest-weight windows at the
                    // valuation date. Observation shift has already shifted those
                    // windows; shifting the cutoff again would double the lookback.
                    let factor = compound.factor_through(cutoff, leg.day_count)?;
                    let notional = contract
                        .floating_profiles
                        .notional()
                        .value_at(start, BoundarySide::After)?;
                    let gearing = DatedProfile::new(
                        ProfileKind::Gearing,
                        contract.terms().floating.profiles().gearings.clone(),
                    )?
                    .value_at(start, BoundarySide::After)?;
                    let interest = notional
                        .checked_mul(gearing)
                        .and_then(|n| {
                            factor
                                .checked_sub(Decimal::ONE)
                                .and_then(|f| n.checked_mul(f))
                        })
                        .ok_or(CashflowError::Arithmetic("overnight accrued interest"))?;
                    interest
                        .checked_add(
                            spread_profiles(leg, &contract.terms().floating.profiles().spreads)?
                                .segment_amount(
                                    &coupon.period,
                                    start,
                                    cutoff,
                                    unrounded(leg, Decimal::ZERO),
                                )?,
                        )
                        .ok_or(CashflowError::Arithmetic("overnight accrued spread"))?
                }
            }
        };
        amount = amount
            .checked_add(leg.rounding.apply(value)?)
            .ok_or(CashflowError::Arithmetic("accrued coupon sum"))?;
    }
    let sign = contract.terms().side.fixed_sign();
    let sign = if fixed { sign } else { -sign };
    Ok(Money::new(
        ExactDecimal::new(
            amount
                .checked_mul(sign)
                .ok_or(CashflowError::Arithmetic("signed accrued"))?,
        ),
        contract.terms().currency,
    ))
}
