//! Actual product preparation/history preflight and native implied quotations.

use convex_core::{
    cashflows::{CashRounding, NegativeCouponPolicy},
    fixings::{FixingSeries, RateOrigin, MAX_FIXING_REQUIREMENTS},
    futures::{EligibilityContext, FuturesSettlement, RateAccrual},
    types::Compounding,
};
use convex_curves::contracts::*;
use convex_rates::{
    basis::{price_basis_swap, BasisParQuotes, BasisQuotedLeg, BasisSwapContract},
    pricing::{price_swap, SwapParQuotes, SwapValuationInputs},
    FloatingLegTerms, SwapContract,
};
use rust_decimal::prelude::ToPrimitive;

use super::*;
use crate::futures::{
    price_rate_future, rate_futures_requirements, FuturesCurve, RateFuturesInputs,
};

pub(super) enum Product {
    MoneyMarket,
    Swap(Box<SwapContract>),
    RateFuture,
    BasisSwap(Box<BasisSwapContract>),
}
pub(super) struct PreparedHelper<'a> {
    pub helper: &'a RateCalibrationHelper,
    pub resource: &'a RateHelperResources<'a>,
    pub product: Product,
}
pub(super) struct Prepared<'a> {
    pub recipe: &'a RateCurveRecipe,
    pub helpers: Vec<PreparedHelper<'a>>,
}

// Decimal economic profiles and f64 native quotes have separate ingress paths.
// Require exact equality in either decimal-rate or original native units; the
// scale roundtrip can round once differently. No financial tolerance is used.
fn profile_matches_quote(value: rust_decimal::Decimal, quote: MarketQuote) -> bool {
    // Parse the exact base-ten representation with correctly rounded f64 ingress;
    // Decimal::to_f64 divides floating limbs and can add another rounding step.
    value.to_string().parse::<f64>().is_ok_and(|rate| {
        quote.decimal_rate().is_ok_and(|q| rate == q)
            || quote.model_rate_quote(rate).is_ok_and(|q| q == quote)
    })
}

fn role_index(
    recipe: &RateCurveRecipe,
    key: &CurveKey,
    index: &str,
) -> Result<(), RateCalibrationError> {
    let fits = if key == &recipe.curve.key {
        matches!(&recipe.curve.role, CurveRole::Projection { index_id } if index_id == index)
    } else {
        recipe
            .overnight_projection
            .as_ref()
            .is_some_and(|p| key == &p.key && p.index_id == index)
    };
    if !fits {
        return Err(invalid(
            "projection",
            "exact fitted index role or explicitly named overnight view required",
        ));
    }
    Ok(())
}
fn swap_mapping(
    recipe: &RateCurveRecipe,
    terms: &convex_rates::SwapTerms,
) -> Result<(), RateCalibrationError> {
    if terms.currency != recipe.curve.currency {
        return Err(invalid("currency", "helper/curve mismatch"));
    }
    match &recipe.curve.role {
        CurveRole::Projection { .. } => role_index(
            recipe,
            terms.floating.projection_curve(),
            terms.floating.index_id().as_str(),
        ),
        CurveRole::Discount { .. } if terms.collateral.discount_curve == recipe.curve.key => {
            if !matches!(terms.floating, FloatingLegTerms::Overnight(_)) {
                return Err(unsupported("discount fitting with a swap requires full OIS and an explicit overnight projection view"));
            }
            role_index(
                recipe,
                terms.floating.projection_curve(),
                terms.floating.index_id().as_str(),
            )
        }
        _ => Err(invalid(
            "mapping",
            "helper must calibrate its declared fitted discount/index role",
        )),
    }
}

pub(super) fn prepare<'a, P>(
    input: &RateCurveBuildInput<'a>,
    history: &FixingSeries,
    context: RateCalibrationContext,
    publications: &mut Publications,
    missing: &mut Publications,
    callback: &mut P,
) -> Result<Prepared<'a>, RateCalibrationError>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    let recipe = input.recipe;
    let mut dependencies = std::collections::BTreeSet::new();
    let mut helpers = Vec::new();
    let mut used_alias = false;
    for (helper, resource) in recipe.helpers.iter().zip(input.resources) {
        let mut requirements = Vec::new();
        let product = match (&helper.terms, resource) {
            (RateCalibrationTerms::MoneyMarket(t), RateHelperResources::MoneyMarket(calendar)) => {
                if calendar.pin != &t.calendar || !same_output(&t.curve, recipe) {
                    return Err(invalid(
                        "money market",
                        "exact supplied calendar and fitted curve mapping required",
                    ));
                }
                helper.quote.decimal_rate()?;
                let (start, end, dc) = t.terms.period();
                if start < recipe.curve.reference_date
                    || start >= end
                    || end <= context.cashflows.fixings.valuation_date
                    || end > helper.pillar
                    || !calendar.calendar.is_business_day(start)
                    || !calendar.calendar.is_business_day(end)
                {
                    return Err(invalid("money market dates", "actual business value/end dates, positive future life and sufficient pillar required"));
                }
                // Date-contextual day counts require a full coupon reference model.
                if !matches!(
                    dc,
                    convex_core::daycounts::DayCountConvention::Act360
                        | convex_core::daycounts::DayCountConvention::Act365Fixed
                        | convex_core::daycounts::DayCountConvention::ActActIsda
                        | convex_core::daycounts::DayCountConvention::Thirty360US
                        | convex_core::daycounts::DayCountConvention::Thirty360E
                ) {
                    return Err(unsupported(
                        "money-market quotation requires a context-free date day count",
                    ));
                }
                if let MoneyMarketTerms::Fra {
                    index_id,
                    fixing_date,
                    settlement_date,
                    settlement,
                    ..
                } = &t.terms
                {
                    role_index(recipe, &t.curve, index_id)?;
                    let expected = match settlement {
                        FraSettlement::AtStart => start,
                        FraSettlement::AtEnd => end,
                    };
                    if *fixing_date > start
                        || *settlement_date != expected
                        || !calendar.calendar.is_business_day(*fixing_date)
                    {
                        return Err(invalid("FRA dates", "actual fixing before value date and settlement on its declared boundary required"));
                    }
                    let index = IndexId::new(index_id.clone())
                        .map_err(|e| invalid("FRA index", e.to_string()))?;
                    let expected_publication =
                        freeze(publications, &index, *fixing_date, callback)?;
                    requirements.push(FixingRequirement {
                        index_id: index,
                        observation_date: *fixing_date,
                        expected_publication,
                    });
                }
                dependencies.insert(t.curve.clone());
                Product::MoneyMarket
            }
            (RateCalibrationTerms::Swap(t), RateHelperResources::Swap(calendars)) => {
                swap_mapping(recipe, t)?;
                helper.quote.decimal_rate()?;
                if t.fixed.rates.is_empty()
                    || t.fixed
                        .rates
                        .iter()
                        .any(|v| !profile_matches_quote(v.value, helper.quote))
                    || t.fixed.leg.rounding != CashRounding::Unrounded
                    || t.floating.leg().rounding != CashRounding::Unrounded
                    || t.fixed.leg.negative_coupons != NegativeCouponPolicy::Allow
                    || t.floating.leg().negative_coupons != NegativeCouponPolicy::Allow
                {
                    return Err(unsupported("par-fixed helper requires its original uniform quote, unrounded Allow-negative leg cash"));
                }
                let contract = t.as_ref().clone().build(calendars)?;
                requirements = contract.fixing_requirements(context.cashflows, |i, d| {
                    freeze(publications, i, d, callback)
                })?;
                dependencies.insert(t.collateral.discount_curve.clone());
                dependencies.insert(t.floating.projection_curve().clone());
                Product::Swap(Box::new(contract))
            }
            (RateCalibrationTerms::RateFuture(t), RateHelperResources::RateFuture { .. }) => {
                if helper.quote.unit != QuoteUnit::FuturePrice100MinusRate
                    || t.contract.currency != recipe.curve.currency
                {
                    return Err(invalid(
                        "future quote",
                        "native price points and exact cash currency required",
                    ));
                }
                let FuturesSettlement::Rate { accrual, .. } = &t.contract.settlement else {
                    return Err(unsupported(
                        "bond futures cannot calibrate a rate projection helper",
                    ));
                };
                let (index, end) = match accrual {
                    RateAccrual::Term { index, end, .. }
                    | RateAccrual::Overnight { index, end, .. } => (index, end.date()),
                };
                if end <= context.cashflows.fixings.valuation_date || end > helper.pillar {
                    return Err(invalid("future pillar", "future economic end and all quotation horizons must fit the supplied pillar"));
                }
                role_index(recipe, &t.projection_curve, index.as_str())?;
                let inputs = future_inputs(t, resource, None, context)?;
                requirements = rate_futures_requirements(&inputs, |i, d| {
                    freeze(publications, i, d, callback)
                })?;
                if let RateHelperResources::RateFuture { fixings, .. } = resource {
                    for chunk in requirements.chunks(MAX_FIXING_REQUIREMENTS) {
                        for r in fixings
                            .resolve_requirements(chunk, context.cashflows.fixings)?
                            .missing()
                        {
                            missing.insert((r.index_id.clone(), r.observation_date), r.clone());
                        }
                    }
                }
                dependencies.insert(t.projection_curve.clone());
                Product::RateFuture
            }
            (
                RateCalibrationTerms::BasisSwap {
                    terms: t,
                    quoted_leg,
                },
                RateHelperResources::BasisSwap(calendars),
            ) => {
                if t.currency != recipe.curve.currency
                    || t.collateral.discount_curve == recipe.curve.key
                    || !matches!(recipe.curve.role, CurveRole::Projection { .. })
                {
                    return Err(unsupported("tenor basis calibrates an independent projection against an external discount curve"));
                }
                let fitted = [&t.pay, &t.receive]
                    .into_iter()
                    .filter(|leg| leg.projection_curve() == &recipe.curve.key)
                    .collect::<Vec<_>>();
                if fitted.len() != 1 {
                    return Err(invalid(
                        "basis mapping",
                        "exactly one independently projected leg must use the fitted index",
                    ));
                }
                role_index(
                    recipe,
                    fitted[0].projection_curve(),
                    fitted[0].index_id().as_str(),
                )?;
                let quoted = match quoted_leg {
                    BasisQuotedLeg::Pay => &t.pay,
                    BasisQuotedLeg::Receive => &t.receive,
                };
                helper.quote.decimal_rate()?;
                if quoted.profiles().spreads.is_empty()
                    || quoted
                        .profiles()
                        .spreads
                        .iter()
                        .any(|v| !profile_matches_quote(v.value, helper.quote))
                    || [&t.pay, &t.receive].iter().any(|l| {
                        l.leg().rounding != CashRounding::Unrounded
                            || l.leg().negative_coupons != NegativeCouponPolicy::Allow
                    })
                {
                    return Err(unsupported("basis helper requires original uniform quoted-leg spread and unrounded Allow-negative cash"));
                }
                let contract = t.as_ref().clone().build(calendars)?;
                requirements = contract.fixing_requirements(context.cashflows, |i, d| {
                    freeze(publications, i, d, callback)
                })?;
                for key in [
                    &t.collateral.discount_curve,
                    t.pay.projection_curve(),
                    t.receive.projection_curve(),
                ] {
                    dependencies.insert(key.clone());
                }
                Product::BasisSwap(Box::new(contract))
            }
            _ => return Err(invalid(
                "resources",
                "one exact product-matching calendar/history binding per original helper required",
            )),
        };
        if !matches!(product, Product::RateFuture) {
            for chunk in requirements.chunks(MAX_FIXING_REQUIREMENTS) {
                for r in history.resolve(chunk, context.cashflows.fixings)?.missing() {
                    missing.insert((r.index_id.clone(), r.observation_date), r.clone());
                }
            }
        }
        used_alias |= recipe
            .overnight_projection
            .as_ref()
            .is_some_and(|p| dependencies.contains(&p.key));
        helpers.push(PreparedHelper {
            helper,
            resource,
            product,
        });
    }
    dependencies.remove(&recipe.curve.key);
    if let CalibrationMode::Ufr(settings) = &recipe.curve.calibration {
        dependencies.insert(settings.base.clone());
    }
    if let Some(p) = &recipe.overnight_projection {
        dependencies.remove(&p.key);
        if !used_alias {
            return Err(invalid(
                "overnight projection",
                "unused zero-basis assumption is not a consumed dependency",
            ));
        }
    }
    let declared: std::collections::BTreeSet<_> =
        recipe.curve.dependencies.iter().cloned().collect();
    if declared.len() != recipe.curve.dependencies.len() || declared != dependencies {
        return Err(invalid(
            "dependencies",
            "must exactly match all external helper discount/index versions",
        ));
    }
    Ok(Prepared { recipe, helpers })
}

fn future_inputs<'a>(
    terms: &'a RateFutureCalibrationTerms,
    resource: &'a RateHelperResources<'a>,
    curves: Option<&'a CurveSet>,
    context: RateCalibrationContext,
) -> Result<RateFuturesInputs<'a>, RateCalibrationError> {
    let RateHelperResources::RateFuture { calendar, fixings } = resource else {
        return Err(invalid("resources", "missing exact futures resources"));
    };
    let projection = match curves {
        Some(c) => Some(FuturesCurve {
            curve: c.get(&terms.projection_curve)?,
            stamp: &terms.projection_stamp,
        }),
        None => None,
    };
    Ok(RateFuturesInputs {
        contract: &terms.contract,
        calendar: *calendar,
        clearing: &terms.clearing,
        fixings,
        fixing_stamp: &terms.fixing_stamp,
        projection,
        convexity_model: terms.convexity_model.as_ref(),
        context: EligibilityContext {
            as_of: context.as_of,
            known_at: context.cashflows.fixings.known_at,
        },
        unpublished: context.cashflows.fixings.unpublished,
        final_only: context.cashflows.fixings.final_only,
    })
}

pub(super) fn value(
    prepared: &PreparedHelper<'_>,
    curves: &CurveSet,
    history: &FixingSeries,
    context: RateCalibrationContext,
    publications: &Publications,
    evaluations: &mut usize,
) -> Result<(MarketQuote, RateHelperValuation), RateCalibrationError> {
    if *evaluations >= MAX_RATE_PRICE_EVALUATIONS {
        return Err(CurveContractError::BoundExceeded("mixed rate helper valuations").into());
    }
    *evaluations += 1;
    let helper = prepared.helper;
    let quote = helper.quote;
    let pubtime = |i: &IndexId, d: Date| publication(publications, i, d);
    let (model, priced) = match (&prepared.product, &helper.terms) {
        (Product::MoneyMarket, RateCalibrationTerms::MoneyMarket(t)) => {
            let (start, end, dc) = t.terms.period();
            let mut origin = None;
            let mut query = None;
            let mut forecast = || -> Result<rust_decimal::Decimal, CashflowError> {
                let q = curves
                    .get(&t.curve)
                    .and_then(|c| {
                        c.query(&CurveQuery::ForwardRate {
                            start: CurvePoint::Date(start),
                            end: CurvePoint::Date(end),
                            compounding: Compounding::Simple,
                            day_count: dc,
                        })
                    })
                    .map_err(|e| CashflowError::InvalidTerms {
                        field: "money-market projection",
                        reason: e.to_string(),
                    })?;
                let value = rust_decimal::Decimal::from_f64_retain(q.value).ok_or(
                    CashflowError::Arithmetic("money-market forecast conversion"),
                )?;
                if q.value != 0.0 && value.is_zero() {
                    return Err(CashflowError::Arithmetic("money-market forecast underflow"));
                }
                query = Some(q);
                Ok(value)
            };
            let rate = if let MoneyMarketTerms::Fra {
                index_id,
                fixing_date,
                ..
            } = &t.terms
            {
                let index = IndexId::new(index_id.clone())
                    .map_err(|e| invalid("FRA index", e.to_string()))?;
                let requirements = [FixingRequirement {
                    index_id: index.clone(),
                    observation_date: *fixing_date,
                    expected_publication: pubtime(&index, *fixing_date)?,
                }];
                let r = history
                    .resolve(&requirements, context.cashflows.fixings)?
                    .project(|_| forecast())?
                    .into_iter()
                    .next()
                    .ok_or_else(|| invalid("FRA fixing", "missing resolved observation"))?;
                origin = Some(r.origin);
                r.rate
                    .to_f64()
                    .ok_or_else(|| invalid("FRA quote", "Decimal conversion failed"))?
            } else {
                forecast()?
                    .to_f64()
                    .ok_or_else(|| invalid("deposit quote", "Decimal conversion failed"))?
            };
            let model = quote.model_rate_quote(rate)?;
            (
                model,
                RateHelperValuation::MoneyMarket(Box::new(DatedRateValuation {
                    terms: t.clone(),
                    model,
                    origin,
                    query,
                })),
            )
        }
        (Product::Swap(contract), RateCalibrationTerms::Swap(_)) => {
            let v = price_swap(
                contract,
                &SwapValuationInputs {
                    curves,
                    fixings: history,
                },
                context.cashflows,
                pubtime,
            )?;
            let SwapParQuotes::Available { fixed_rate, .. } = &v.par else {
                return Err(unsupported(
                    "swap helper has no remaining linear par exposure",
                ));
            };
            (
                quote.model_rate_quote(fixed_rate.value().get())?,
                RateHelperValuation::Swap(Box::new(v)),
            )
        }
        (Product::RateFuture, RateCalibrationTerms::RateFuture(t)) => {
            let v = price_rate_future(
                &future_inputs(t, prepared.resource, Some(curves), context)?,
                pubtime,
            )?;
            let model = MarketQuote {
                value: v
                    .model_price_points
                    .decimal()
                    .to_f64()
                    .ok_or_else(|| invalid("future price", "Decimal conversion failed"))?,
                unit: quote.unit,
            };
            model.validate()?;
            (model, RateHelperValuation::RateFuture(Box::new(v)))
        }
        (Product::BasisSwap(contract), RateCalibrationTerms::BasisSwap { quoted_leg, .. }) => {
            let v = price_basis_swap(
                contract,
                &SwapValuationInputs {
                    curves,
                    fixings: history,
                },
                context.cashflows,
                pubtime,
            )?;
            let BasisParQuotes::Available {
                pay_spread,
                receive_spread,
                ..
            } = &v.par
            else {
                return Err(unsupported(
                    "basis helper has no remaining linear spread exposure",
                ));
            };
            let rate = match quoted_leg {
                BasisQuotedLeg::Pay => pay_spread,
                BasisQuotedLeg::Receive => receive_spread,
            };
            (
                quote.model_rate_quote(rate.value().get())?,
                RateHelperValuation::BasisSwap(Box::new(v)),
            )
        }
        _ => return Err(invalid("prepared product", "inconsistent admitted helper")),
    };
    Ok((model, priced))
}

pub(super) fn horizon(
    v: &RateHelperValuation,
    helper: &RateCalibrationHelper,
    recipe: &RateCurveRecipe,
) -> Result<(), RateCalibrationError> {
    let mut latest = recipe.curve.reference_date;
    let mut rates = |uses: &[convex_rates::RateUse]| {
        for u in uses {
            if same_output(&u.request.projection_curve, recipe)
                && matches!(u.resolved.origin, RateOrigin::Projected { .. })
            {
                latest = latest.max(u.request.rate_end);
            }
        }
    };
    match v {
        RateHelperValuation::MoneyMarket(v) => {
            if v.query.is_some() {
                latest = latest.max(v.terms.terms.period().1);
            }
        }
        RateHelperValuation::Swap(v) => {
            rates(&v.cashflows.rates);
            for c in v.fixed.iter().chain(&v.floating) {
                if same_output(&c.query.curve, recipe) {
                    latest = latest.max(c.payment_date);
                }
            }
        }
        RateHelperValuation::RateFuture(v) => {
            for r in &v.observations {
                if !matches!(
                    r.origin,
                    crate::futures::FuturesRateOrigin::Published { .. }
                ) {
                    latest = latest.max(r.rate_end);
                }
            }
        }
        RateHelperValuation::BasisSwap(v) => {
            rates(&v.pay.rates);
            rates(&v.receive.rates);
            for c in v.pay.discounted.iter().chain(&v.receive.discounted) {
                if same_output(&c.query.curve, recipe) {
                    latest = latest.max(c.payment_date);
                }
            }
        }
    }
    if latest > helper.pillar {
        return Err(invalid(
            "helper.pillar",
            format!(
                "{} queries fitted curve through {latest}, after {}",
                helper.id, helper.pillar
            ),
        ));
    }
    Ok(())
}
