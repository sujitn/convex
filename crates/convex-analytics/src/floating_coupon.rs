//! Supplied segment or explicit joint-normal whole-coupon rate bounds.
//! Original reset/profile/history/entitlement and model scope remain explicit.
//! Overnight/path and callable floating laws require their own complete models.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_bonds::{
    contracts::{
        BondContract, BondContractError, BondCoupon, BondProjectionContext, BondTerms,
        ProjectionRequest, MAX_BOND_TERMS,
    },
    options::floating_coupon::{
        bounded_floating_coupon, BoundedCouponValue, CouponOptionError, ForwardCouponModel,
    },
};
use convex_core::{
    cashflows::{
        BoundarySide, CashRounding, CashflowError, CouponPrincipal, DatedProfile, DatedValue,
        ExCouponAccrual, NegativeCouponPolicy, ProfileKind,
    },
    contracts::{IndexId, SourceId, UtcTimestamp, Version},
    fixings::{FixingSeries, ProjectionReason, RateOrigin, ResolvedRate, MAX_FIXING_REQUIREMENTS},
    Date,
};
use convex_curves::contracts::{BuiltCurve, CurveContractError, CurvePoint, CurveQuery, CurveRole};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod prepared;
mod whole_coupon;
pub use prepared::*;
use whole_coupon::{whole_coupon_value, WholeCouponTotals};
pub use whole_coupon::{
    WholeCouponAccrualPolicy, WholeCouponNormalLaw, WholeCouponNormalValue, WholeCouponRateFactors,
};

/// Explicit model for one future index observation and actual coupon payment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TermCouponModelPoint {
    /// Original provider-neutral index.
    pub index_id: IndexId,
    /// Actual fixing observation date, not reset start or payment date.
    pub observation_date: Date,
    /// Exact adjusted coupon payment.
    pub payment_date: Date,
    /// Annual forward volatility and explicit displacement.
    pub model: ForwardCouponModel,
    /// Supplied payment-measure mean adjustment to the original forward, decimal rate.
    pub forward_adjustment: f64,
    /// Actual model/volatility source identity.
    pub source_id: SourceId,
    /// Actual positive source revision.
    pub version: Version,
    /// Model information availability; must not exceed original information cutoff.
    pub known_at: UtcTimestamp,
}

/// Full covered rate bounds; no ignored or inferred optional cash terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TermCouponOptionInputs {
    /// Cap on each actual effective coupon-rate segment, with complete coverage.
    pub cap: Option<Vec<DatedValue>>,
    /// Floor on each actual effective coupon-rate segment, with complete coverage.
    pub floor: Option<Vec<DatedValue>>,
    /// Unique observation/payment model points; unused points fail.
    pub models: Vec<TermCouponModelPoint>,
    /// Joint terminal normal law for a bound on each entire coupon, if supplied.
    /// Omission retains the original segment payoff without changing its output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whole_coupon: Option<WholeCouponNormalLaw>,
}

/// All pricing inputs remain immutable and caller supplied.
pub struct TermCouponPricing<'a> {
    /// Checked original complete term bond and calendar-derived slices.
    pub bond: &'a BondContract,
    /// Actual settlement/entitlement and fixing information policy.
    pub context: &'a BondProjectionContext,
    /// Actual eligible fixing revisions.
    pub history: &'a FixingSeries,
    /// Separate actual same-currency collateral discount curve.
    pub discount: &'a BuiltCurve,
    /// Explicit original bounds and model revisions.
    pub options: &'a TermCouponOptionInputs,
}

/// Native failures preserve their original meaning; no successful partial price.
#[derive(Debug, thiserror::Error)]
pub enum TermCouponOptionError {
    /// Original supplied bond validation or economic restriction.
    #[error(transparent)]
    Bond(#[from] BondContractError),
    /// Original cash/profile/history failure, including full missing requirements.
    #[error(transparent)]
    Cash(#[from] CashflowError),
    /// Original actual curve query failure.
    #[error(transparent)]
    Curve(#[from] CurveContractError),
    /// Original stochastic option failure.
    #[error(transparent)]
    Option(#[from] CouponOptionError),
    /// Original observation/payment volatility is absent from supplied inputs.
    #[error(
        "missing term coupon model for {index_id} on {observation_date}, payment {payment_date}"
    )]
    MissingModel {
        /// Actual provider-neutral index.
        index_id: IndexId,
        /// Original observation date.
        observation_date: Date,
        /// Original adjusted payment date.
        payment_date: Date,
    },
    /// Valid full economics need an additional genuine stochastic model.
    #[error("unsupported term coupon option {feature}: {reason}")]
    Unsupported {
        /// Original unavailable feature.
        feature: &'static str,
        /// Why the current model cannot represent it.
        reason: &'static str,
    },
    /// Bounded native work/input ceiling.
    #[error("term coupon option work bound: {0}")]
    Bound(&'static str),
    /// Original incompatible or unavailable optional-coupon input.
    #[error("term coupon option {field}: {reason}")]
    Invalid {
        /// Exact original input or operation that cannot be admitted.
        field: &'static str,
        /// Original failure explanation without a substitute price.
        reason: &'static str,
    },
}
type Result<T> = std::result::Result<T, TermCouponOptionError>;
fn invalid(field: &'static str, reason: &'static str) -> TermCouponOptionError {
    TermCouponOptionError::Invalid { field, reason }
}
fn number(d: Decimal) -> Result<f64> {
    d.to_f64()
        .filter(|v| v.is_finite() && (d.is_zero() || *v != 0.0))
        .ok_or_else(|| invalid("cash/rate", "Decimal conversion overflow/underflow"))
}
fn finite(x: f64) -> Result<f64> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(invalid("model value", "nonfinite arithmetic"))
    }
}

/// Full original rate segment and model result; actual cash is never inferred.
#[derive(Debug, Serialize)]
pub struct TermCouponSegmentValue {
    /// Exact original coupon accrual subinterval.
    pub start: Date,
    /// Exact original coupon accrual subinterval end.
    pub end: Date,
    /// Native adjusted coupon payment.
    pub payment_date: Date,
    /// Original actual reset/projection request.
    pub request: ProjectionRequest,
    /// Original eligible known or projected rate and lineage.
    pub rate: ResolvedRate,
    /// Actual original principal convention amount.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub principal: Decimal,
    /// Full native reference-aware accrual fraction.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub year_fraction: Decimal,
    /// Segment model used for future uncertainty; joint scope uses normal_factors.
    pub model: Option<TermCouponModelPoint>,
    /// Actual future joint-model row for whole-coupon scope; omission retains
    /// the original segment output. Its cash is an unbounded component mean.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal_factors: Option<WholeCouponRateFactors>,
    /// Complete stochastic or deterministic bounded coupon-rate result.
    pub expectation: BoundedCouponValue,
    /// Absolute expected segment cash, unrounded model value.
    pub expected_cash: f64,
    /// Actual coupon entitlement, independent of principal.
    pub entitled: bool,
}

/// Full original term optional-coupon model quote in original-face units.
#[derive(Debug, Serialize)]
pub struct TermCouponOptionPrice {
    /// Original unchanged complete native economics.
    pub terms: BondTerms,
    /// Original actual context and information policy.
    pub context: BondProjectionContext,
    /// Original bounds/model source inputs consumed.
    pub options: TermCouponOptionInputs,
    /// All original rate/profile/reset segments and their actual proof.
    pub segments: Vec<TermCouponSegmentValue>,
    /// Whole-coupon joint-law proof, absent from original segment quotations.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub whole_coupons: Vec<WholeCouponNormalValue>,
    /// Absolute discounted entitled expected cash in original currency.
    pub present_value: f64,
    /// Dirty model quote per100 ORIGINAL face.
    pub dirty_price: f64,
    /// Explicit model accrued/rebate per100 ORIGINAL face; not recorded cash.
    pub accrued_model: f64,
    /// Dirty quote minus same-model accrued/rebate.
    pub clean_price: f64,
    /// Actual consumed discount curve key.
    pub discount_curve: convex_curves::contracts::CurveKey,
}

fn bound_profile(
    values: &Option<Vec<DatedValue>>,
    coverage: (Date, Date),
) -> Result<Option<DatedProfile>> {
    values
        .as_ref()
        .map(|v| {
            let p = DatedProfile::new(ProfileKind::Coupon, v.clone())?;
            if p.coverage() != coverage {
                return Err(invalid(
                    "bounds",
                    "complete original profile coverage required",
                ));
            }
            Ok(p)
        })
        .transpose()
}
fn profile(values: &[DatedValue], kind: ProfileKind) -> Result<DatedProfile> {
    Ok(DatedProfile::new(kind, values.to_vec())?)
}
fn future_policy(
    model: ForwardCouponModel,
    gearing: f64,
    additive: f64,
    forward: f64,
    floor: Option<f64>,
    cap: Option<f64>,
    policy: NegativeCouponPolicy,
) -> Result<()> {
    if policy == NegativeCouponPolicy::Allow {
        return Ok(());
    }
    if cap.is_some_and(|c| c < 0.0) {
        return Err(invalid(
            "negative coupon policy",
            "negative cap cannot satisfy Reject",
        ));
    }
    if floor.is_some_and(|f| f >= 0.0) {
        return Ok(());
    }
    let lowest = match model {
        ForwardCouponModel::Normal { volatility: 0.0 }
        | ForwardCouponModel::ShiftedLognormal {
            volatility: 0.0, ..
        } => gearing * forward + additive,
        _ if gearing == 0.0 => additive,
        ForwardCouponModel::ShiftedLognormal { displacement, .. } if gearing > 0.0 => {
            additive - gearing * displacement
        }
        _ => {
            return Err(invalid(
                "negative coupon policy",
                "model has negative support; Reject is not an implicit floor",
            ))
        }
    };
    if lowest < 0.0 {
        return Err(invalid(
            "negative coupon policy",
            "model has negative coupon support",
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct CouponTotals {
    known_cash: Decimal,
    model_cash: f64,
    known_accrued: Decimal,
    model_accrued: f64,
    entitled: bool,
    ex_coupon: bool,
    has_model: bool,
    whole: Option<WholeCouponTotals>,
}

struct PreparedTermCoupons<'a> {
    bond: &'a BondContract,
    context: &'a BondProjectionContext,
    discount: &'a BuiltCurve,
    options: &'a TermCouponOptionInputs,
    coupon: DatedProfile,
    spread: DatedProfile,
    gearing: DatedProfile,
    notional: DatedProfile,
    cap: Option<DatedProfile>,
    floor: Option<DatedProfile>,
    cuts: BTreeSet<Date>,
    models: BTreeMap<(IndexId, Date, Date), &'a TermCouponModelPoint>,
    rates: BTreeMap<(IndexId, Date), ResolvedRate>,
    factors: BTreeMap<(IndexId, Date, Date), &'a WholeCouponRateFactors>,
}

fn prepare_term_coupons<'a, P, F>(
    input: TermCouponPricing<'a>,
    mut publication: P,
    mut forecast: F,
) -> Result<PreparedTermCoupons<'a>>
where
    P: FnMut(&IndexId, Date) -> std::result::Result<UtcTimestamp, CashflowError>,
    F: FnMut(&ProjectionRequest) -> std::result::Result<Decimal, CashflowError>,
{
    let TermCouponPricing {
        bond,
        context,
        history,
        discount,
        options,
    } = input;
    bond.require_plain_valuation()?;
    if !matches!(bond.terms().coupon, BondCoupon::Term(_))
        || !context.overnight_accrual_cutoffs.is_empty()
    {
        return Err(TermCouponOptionError::Unsupported {
            feature: "coupon model",
            reason: "actual term coupon with no unused overnight inputs required",
        });
    }
    if options.cap.is_none() && options.floor.is_none() {
        return Err(invalid(
            "bounds",
            "at least one explicit cap/floor required",
        ));
    }
    if options.models.len() > MAX_BOND_TERMS {
        return Err(TermCouponOptionError::Bound("model points"));
    }
    if options.whole_coupon.is_some() && !options.models.is_empty() {
        return Err(invalid("model", "whole coupon requires its joint law only"));
    }
    let mut factors = BTreeMap::new();
    let mut dimensions = BTreeMap::new();
    if let Some(whole) = &options.whole_coupon {
        if whole.factors.len() > MAX_BOND_TERMS {
            return Err(TermCouponOptionError::Bound("joint normal points"));
        }
        for point in &whole.factors {
            if point.loadings.is_empty() || point.loadings.len() > 16 {
                return Err(TermCouponOptionError::Bound("1..16 common normal factors"));
            }
            if point.known_at > context.fixing.known_at
                || !point.forward_adjustment.is_finite()
                || point.forward_adjustment.abs() > 10.0
                || point
                    .loadings
                    .iter()
                    .any(|x| !x.is_finite() || x.abs() > 10.0)
            {
                return Err(invalid(
                    "joint model",
                    "future information or invalid rate loading",
                ));
            }
            if let Some(previous) = dimensions.insert(point.payment_date, point.loadings.len()) {
                if previous != point.loadings.len() {
                    return Err(invalid(
                        "joint model",
                        "inconsistent common factor dimension",
                    ));
                }
            }
            if factors
                .insert(
                    (
                        point.index_id.clone(),
                        point.observation_date,
                        point.payment_date,
                    ),
                    point,
                )
                .is_some()
            {
                return Err(invalid(
                    "joint model",
                    "duplicate observation/payment factors",
                ));
            }
        }
    }
    if discount.recipe().currency != bond.terms().currency
        || !matches!(&discount.recipe().role,CurveRole::Discount {collateral_currency} if *collateral_currency==bond.terms().currency)
    {
        return Err(invalid(
            "discount",
            "actual same-currency collateral discount role required",
        ));
    }
    let coupon = profile(&bond.terms().profiles.coupon, ProfileKind::Coupon)?;
    let spread = profile(&bond.terms().profiles.spread, ProfileKind::Spread)?;
    let gearing = profile(&bond.terms().profiles.gearing, ProfileKind::Gearing)?;
    let notional = profile(&bond.terms().profiles.notional, ProfileKind::Notional)?;
    let cap = bound_profile(&options.cap, coupon.coverage())?;
    let floor = bound_profile(&options.floor, coupon.coverage())?;
    let mut cuts = BTreeSet::new();
    for p in [&coupon, &spread, &gearing, &notional]
        .into_iter()
        .chain(cap.iter())
        .chain(floor.iter())
    {
        for e in p.entries() {
            cuts.insert(e.start);
            cuts.insert(e.end);
        }
    }
    for d in cuts.iter().copied().filter(|d| *d < coupon.coverage().1) {
        if let (Some(c), Some(f)) = (&cap, &floor) {
            if c.value_at(d, BoundarySide::After)? < f.value_at(d, BoundarySide::After)? {
                return Err(invalid(
                    "bounds",
                    "cap below floor on original profile interval",
                ));
            }
        }
    }
    let mut models = BTreeMap::new();
    for m in &options.models {
        m.model.validate()?;
        if m.known_at > context.fixing.known_at
            || !m.forward_adjustment.is_finite()
            || m.forward_adjustment.abs() > 10.0
        {
            return Err(invalid(
                "model",
                "future information or invalid payment-measure adjustment",
            ));
        }
        if models
            .insert((m.index_id.clone(), m.observation_date, m.payment_date), m)
            .is_some()
        {
            return Err(invalid("model", "duplicate observation/payment model"));
        }
    }
    let requirements =
        bond.fixing_requirements(context.settlement, context.on_payment, &mut publication)?;
    let mut resolutions = Vec::new();
    let mut missing = Vec::new();
    for batch in requirements.chunks(MAX_FIXING_REQUIREMENTS) {
        let r = history.resolve(batch, context.fixing)?;
        missing.extend_from_slice(r.missing());
        resolutions.push(r);
    }
    if !missing.is_empty() {
        return Err(CashflowError::MissingFixings(missing).into());
    }
    if resolutions.iter().any(|resolution| {
        resolution
            .projected_requirements()
            .any(|requirement| requirement.observation_date <= context.fixing.valuation_date)
    }) {
        return Err(TermCouponOptionError::Unsupported {
            feature: "unpublished fixing",
            reason: "awaiting-publication uncertainty requires its own conditional law",
        });
    }
    let mut requests = BTreeMap::new();
    for (period, _, _, request) in bond.term_reset_slices() {
        if period.payment_date() > context.settlement
            || (period.payment_date() == context.settlement
                && context.on_payment == convex_core::cashflows::PaymentDateEntitlement::Include)
        {
            requests.insert(
                (request.index_id.clone(), request.observation_date),
                request,
            );
        }
    }
    let mut rates = BTreeMap::new();
    for resolution in resolutions {
        for rate in resolution.project(|r| {
            let req = requests
                .get(&(r.index_id.clone(), r.observation_date))
                .ok_or_else(|| CashflowError::InvalidTerms {
                    field: "option projection",
                    reason: "original reset request absent".into(),
                })?;
            forecast(req)
        })? {
            rates.insert(
                (
                    rate.requirement.index_id.clone(),
                    rate.requirement.observation_date,
                ),
                rate,
            );
        }
    }
    Ok(PreparedTermCoupons {
        bond,
        context,
        discount,
        options,
        coupon,
        spread,
        gearing,
        notional,
        cap,
        floor,
        cuts,
        models,
        rates,
        factors,
    })
}

/// Price explicitly segmented term coupon bounds on every original native step.
/// All missing published history is collected before any forward callback runs.
/// Future rounded cash, awaiting-publication uncertainty, overnight and
/// exercise models remain explicit errors; no approximation or scope waiver.
pub fn price_term_coupon_options<P, F>(
    input: TermCouponPricing<'_>,
    publication: P,
    forecast: F,
) -> Result<TermCouponOptionPrice>
where
    P: FnMut(&IndexId, Date) -> std::result::Result<UtcTimestamp, CashflowError>,
    F: FnMut(&ProjectionRequest) -> std::result::Result<Decimal, CashflowError>,
{
    let PreparedTermCoupons {
        bond,
        context,
        discount,
        options,
        coupon,
        spread,
        gearing,
        notional,
        cap,
        floor,
        cuts,
        models,
        rates,
        factors,
    } = prepare_term_coupons(input, publication, forecast)?;
    let df = |date| -> Result<f64> {
        let value = discount
            .query(&CurveQuery::DiscountFactor {
                point: CurvePoint::Date(date),
            })?
            .value;
        if !value.is_finite() || value <= 0.0 {
            return Err(invalid("discount", "positive finite actual DF required"));
        }
        Ok(value)
    };
    let settlement_df = df(context.settlement)?;
    let mut values = Vec::new();
    let mut used = BTreeSet::new();
    let mut payments: BTreeMap<(Date, Date, Date), CouponTotals> = BTreeMap::new();
    for (period, start, end, request) in bond.term_reset_slices() {
        if period.payment_date() < context.settlement
            || (period.payment_date() == context.settlement
                && context.on_payment == convex_core::cashflows::PaymentDateEntitlement::Exclude)
        {
            continue;
        }
        let rate = rates
            .get(&(request.index_id.clone(), request.observation_date))
            .ok_or_else(|| invalid("fixing", "original resolved rate absent"))?;
        let mut edges = vec![start];
        edges.extend(
            cuts.range((
                std::ops::Bound::Excluded(start),
                std::ops::Bound::Excluded(end),
            ))
            .copied(),
        );
        edges.push(end);
        for pair in edges.windows(2) {
            if values.len() >= MAX_BOND_TERMS {
                return Err(TermCouponOptionError::Bound("profile/reset segments"));
            }
            let a = pair[0];
            let b = pair[1];
            let frac = period.year_fraction(a, b, bond.terms().day_count)?;
            let n = match bond.terms().coupon_principal {
                CouponPrincipal::AccrualProfile => notional.value_at(a, BoundarySide::After)?,
                CouponPrincipal::BeforeCouponBoundary => {
                    bond.outstanding_principal(period.contractual_dates().1, BoundarySide::Before)?
                }
                CouponPrincipal::AfterCouponBoundary => {
                    bond.outstanding_principal(period.contractual_dates().1, BoundarySide::After)?
                }
            };
            let g = gearing.value_at(a, BoundarySide::After)?;
            let additive = coupon
                .value_at(a, BoundarySide::After)?
                .checked_add(spread.value_at(a, BoundarySide::After)?)
                .ok_or(CashflowError::Arithmetic("optional additive rate"))?;
            let c = cap
                .as_ref()
                .map(|p| p.value_at(a, BoundarySide::After))
                .transpose()?;
            let f = floor
                .as_ref()
                .map(|p| p.value_at(a, BoundarySide::After))
                .transpose()?;
            let key = (
                request.index_id.clone(),
                request.observation_date,
                period.payment_date(),
            );
            let whole_scope = options.whole_coupon.is_some();
            let mut normal_factors = None;
            let (expectation, model, known_cash) = match &rate.origin {
                RateOrigin::Published(_) => {
                    let r = g
                        .checked_mul(rate.rate)
                        .and_then(|r| r.checked_add(additive))
                        .ok_or(CashflowError::Arithmetic("known bounded rate"))?;
                    let effective_cap = if whole_scope { None } else { c };
                    let effective_floor = if whole_scope { None } else { f };
                    let cap_deduction =
                        effective_cap.map_or(Decimal::ZERO, |c| (r - c).max(Decimal::ZERO));
                    let floor_addition =
                        effective_floor.map_or(Decimal::ZERO, |f| (f - r).max(Decimal::ZERO));
                    let r = effective_cap.map_or(r, |c| r.min(c));
                    let r = effective_floor.map_or(r, |f| r.max(f));
                    if bond.terms().negative_coupons == NegativeCouponPolicy::Reject
                        && r < Decimal::ZERO
                        && !whole_scope
                    {
                        return Err(invalid(
                            "negative coupon policy",
                            "actual known bounded rate rejected",
                        ));
                    }
                    let amount = n
                        .checked_mul(r)
                        .and_then(|v| v.checked_mul(frac))
                        .ok_or(CashflowError::Arithmetic("known bounded cash"))?;
                    (
                        BoundedCouponValue {
                            expected_rate: number(r)?,
                            cap_adjustment: number(cap_deduction)?,
                            floor_adjustment: number(floor_addition)?,
                            forward_delta: Some(0.0),
                            forward_gamma: Some(0.0),
                            annual_vol_vega: Some(0.0),
                        },
                        None,
                        Some((amount, r)),
                    )
                }
                RateOrigin::Projected(reason) => {
                    if *reason != ProjectionReason::FutureObservation {
                        return Err(TermCouponOptionError::Unsupported {
                            feature: "unpublished fixing",
                            reason:
                                "awaiting-publication uncertainty requires its own conditional law",
                        });
                    }
                    if bond.terms().rounding != CashRounding::Unrounded {
                        return Err(TermCouponOptionError::Unsupported {
                            feature: "stochastic cash rounding",
                            reason:
                                "expected rounded stochastic cash requires explicit integration",
                        });
                    }
                    if whole_scope {
                        let point = factors.get(&key).ok_or_else(|| {
                            TermCouponOptionError::MissingModel {
                                index_id: request.index_id.clone(),
                                observation_date: request.observation_date,
                                payment_date: period.payment_date(),
                            }
                        })?;
                        let mean = finite(number(rate.rate)? + point.forward_adjustment)?;
                        let g = number(g)?;
                        let expectation = BoundedCouponValue {
                            expected_rate: finite(g * mean + number(additive)?)?,
                            cap_adjustment: 0.0,
                            floor_adjustment: 0.0,
                            forward_delta: Some(g),
                            forward_gamma: Some(0.0),
                            annual_vol_vega: Some(0.0),
                        };
                        normal_factors = Some((*point).clone());
                        used.insert(key.clone());
                        (expectation, None, None)
                    } else {
                        let m = models.get(&key).ok_or_else(|| {
                            TermCouponOptionError::MissingModel {
                                index_id: request.index_id.clone(),
                                observation_date: request.observation_date,
                                payment_date: period.payment_date(),
                            }
                        })?;
                        let time = context
                            .fixing
                            .valuation_date
                            .days_between(&request.observation_date)
                            as f64
                            / 365.0;
                        if time <= 0.0 {
                            return Err(invalid(
                                "model",
                                "positive original future observation time required",
                            ));
                        }
                        let forward = finite(number(rate.rate)? + m.forward_adjustment)?;
                        let c = c.map(number).transpose()?;
                        let f = f.map(number).transpose()?;
                        let g = number(g)?;
                        let additive = number(additive)?;
                        future_policy(
                            m.model,
                            g,
                            additive,
                            forward,
                            f,
                            c,
                            bond.terms().negative_coupons,
                        )?;
                        let expectation =
                            bounded_floating_coupon(m.model, time, forward, g, additive, c, f)?;
                        used.insert(key.clone());
                        (expectation, Some((*m).clone()), None)
                    }
                }
            };
            let cash = match known_cash {
                Some((v, _)) => number(v)?,
                None => finite(number(n)? * number(frac)? * expectation.expected_rate)?,
            };
            let prefix_end = context.settlement.min(b);
            let prefix = if prefix_end > a {
                period.year_fraction(a, prefix_end, bond.terms().day_count)?
            } else {
                Decimal::ZERO
            };
            let entitled = period.is_entitled(context.settlement, context.on_payment);
            let ex = period.is_ex_coupon(context.settlement);
            let totals = payments
                .entry((
                    period.contractual_dates().0,
                    period.contractual_dates().1,
                    period.payment_date(),
                ))
                .or_insert(CouponTotals {
                    known_cash: Decimal::ZERO,
                    model_cash: 0.0,
                    known_accrued: Decimal::ZERO,
                    model_accrued: 0.0,
                    entitled,
                    ex_coupon: ex,
                    has_model: false,
                    whole: whole_scope.then_some(WholeCouponTotals {
                        notional_accrual: Decimal::ZERO,
                        elapsed_notional_accrual: Decimal::ZERO,
                        factor_loadings: Vec::new(),
                        cap: c,
                        floor: f,
                    }),
                });
            if let Some(whole) = &mut totals.whole {
                if whole.cap != c || whole.floor != f {
                    return Err(invalid(
                        "whole coupon bounds",
                        "cap/floor must be constant over the entire coupon",
                    ));
                }
                let weight = n
                    .checked_mul(frac)
                    .ok_or(CashflowError::Arithmetic("whole coupon weight"))?;
                let elapsed = n
                    .checked_mul(prefix)
                    .ok_or(CashflowError::Arithmetic("whole coupon elapsed weight"))?;
                whole.notional_accrual = whole
                    .notional_accrual
                    .checked_add(weight)
                    .ok_or(CashflowError::Arithmetic("whole coupon total weight"))?;
                whole.elapsed_notional_accrual = whole
                    .elapsed_notional_accrual
                    .checked_add(elapsed)
                    .ok_or(CashflowError::Arithmetic("whole coupon elapsed sum"))?;
                if let Some(point) = &normal_factors {
                    if whole.factor_loadings.is_empty() {
                        whole.factor_loadings.resize(point.loadings.len(), 0.0);
                    }
                    for (loading, rate_loading) in
                        whole.factor_loadings.iter_mut().zip(&point.loadings)
                    {
                        let coefficient = finite(number(weight)? * number(g)?)?;
                        let contribution = finite(coefficient * rate_loading)?;
                        if coefficient != 0.0 && *rate_loading != 0.0 && contribution == 0.0 {
                            return Err(invalid(
                                "joint loading",
                                "positive loading contribution underflow",
                            ));
                        }
                        *loading = finite(*loading + contribution)?;
                    }
                }
            }
            if let Some((known, known_rate)) = known_cash {
                totals.known_cash = totals
                    .known_cash
                    .checked_add(known)
                    .ok_or(CashflowError::Arithmetic("known bounded coupon sum"))?;
                let accrued = n
                    .checked_mul(known_rate)
                    .and_then(|v| v.checked_mul(prefix))
                    .ok_or(CashflowError::Arithmetic("known bounded accrued"))?;
                totals.known_accrued = totals
                    .known_accrued
                    .checked_add(accrued)
                    .ok_or(CashflowError::Arithmetic("known bounded accrued sum"))?;
            } else {
                totals.has_model = true;
                totals.model_cash = finite(totals.model_cash + cash)?;
                totals.model_accrued = finite(
                    totals.model_accrued + number(n)? * number(prefix)? * expectation.expected_rate,
                )?;
            }
            values.push(TermCouponSegmentValue {
                start: a,
                end: b,
                payment_date: period.payment_date(),
                request: request.clone(),
                rate: rate.clone(),
                principal: n,
                year_fraction: frac,
                model,
                normal_factors,
                expectation,
                expected_cash: cash,
                entitled,
            });
        }
    }
    if used.len() != models.len() + factors.len() {
        return Err(invalid("models", "unused historical/unknown model point"));
    }
    let mut pv = 0.0;
    let mut accrued = 0.0;
    let mut whole_coupons = Vec::new();
    for ((start, end, date), totals) in payments {
        let CouponTotals {
            known_cash: known,
            model_cash: model,
            known_accrued,
            model_accrued,
            entitled,
            ex_coupon: ex,
            has_model,
            whole,
        } = totals;
        if let Some(whole) = whole {
            let (amount, value) = whole_coupon_value(
                start,
                end,
                date,
                known,
                model,
                whole,
                bond.terms().negative_coupons,
                bond.terms().rounding,
            )?;
            if entitled {
                pv = finite(pv + amount * df(date)? / settlement_df)?;
            }
            let fraction =
                number(value.elapsed_notional_accrual)? / number(value.output.notional_accrual)?;
            let mut value_accrued = finite(amount * fraction)?;
            if ex {
                value_accrued = match bond.terms().ex_coupon_accrual {
                    ExCouponAccrual::SubtractCoupon => finite(value_accrued - amount)?,
                    ExCouponAccrual::Zero => 0.0,
                };
            }
            accrued = finite(accrued + value_accrued)?;
            whole_coupons.push(value.output);
            continue;
        }
        let amount = number(bond.terms().rounding.apply(known)?)? + model;
        if entitled {
            pv = finite(pv + amount * df(date)? / settlement_df)?;
        }
        let coupon_accrued = if has_model {
            let value = number(known_accrued)? + model_accrued;
            if ex {
                match bond.terms().ex_coupon_accrual {
                    ExCouponAccrual::SubtractCoupon => finite(value - (number(known)? + model))?,
                    ExCouponAccrual::Zero => 0.0,
                }
            } else {
                finite(value)?
            }
        } else {
            let value = if ex {
                match bond.terms().ex_coupon_accrual {
                    ExCouponAccrual::SubtractCoupon => known_accrued
                        .checked_sub(known)
                        .ok_or(CashflowError::Arithmetic("known bounded ex-coupon rebate"))?,
                    ExCouponAccrual::Zero => Decimal::ZERO,
                }
            } else {
                known_accrued
            };
            number(bond.terms().rounding.apply(value)?)?
        };
        accrued = finite(accrued + coupon_accrued)?;
    }
    for principal in bond.principal_cashflows() {
        if principal.is_entitled(context.settlement, context.on_payment) {
            pv = finite(
                pv + number(principal.amount)? * df(principal.payment_date)? / settlement_df,
            )?;
        }
    }
    let face = number(bond.terms().face_value)?;
    let dirty = finite(pv / face * 100.0)?;
    let accrued = finite(accrued / face * 100.0)?;
    Ok(TermCouponOptionPrice {
        terms: bond.terms().clone(),
        context: context.clone(),
        options: options.clone(),
        segments: values,
        whole_coupons,
        present_value: pv,
        dirty_price: dirty,
        accrued_model: accrued,
        clean_price: finite(dirty - accrued)?,
        discount_curve: discount.recipe().key.clone(),
    })
}
