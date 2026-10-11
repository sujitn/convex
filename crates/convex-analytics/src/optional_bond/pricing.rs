//! Event-aligned fixed and restricted joint term-FRN optimal-stopping valuation.

use super::*;
use convex_bonds::{
    contracts::{
        BondContract, BondCoupon, BondProjectionContext, BondProjectionPolicy, ExerciseAccrued,
        ExerciseDates, ExerciseRight, ExerciseStyle, ExerciseTerms,
    },
    options::hw1f::{event_grid, Hw1fTree, TreeDiagnostics, MAX_TREE_STEPS},
};
use convex_core::{
    calendars::Calendar,
    cashflows::{
        BoundarySide, CashRounding, CouponPrincipal, DateOffset, DatedValue, NegativeCouponPolicy,
        PaymentDateEntitlement,
    },
    contracts::{IndexId, UtcTimestamp},
    fixings::{FixingRequirement, FixingSeries, RateOrigin, ResolvedRate},
};
use serde::{Deserialize, Serialize};

mod model_risk;
pub use model_risk::*;

const MAX_EXERCISES: usize = 128;
const MAX_PRICE_WORK: usize = 32_000_000;

/// Caller-supplied economic state; historical notice/exercise is not inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExerciseState {
    /// The bond remains alive with no accepted outstanding exercise notice.
    AliveUnexercised,
}
/// Cash-date inclusion is independent of model/valuation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OptionalBondContext {
    /// Root of the model and price, using ACT/365F elapsed time.
    pub settlement: Date,
    /// Include/exclude already due cash on this root date.
    pub on_payment: PaymentDateEntitlement,
    /// Explicit supplied unexercised state.
    pub exercise_state: ExerciseState,
}
/// Ordering of simultaneous rights, especially when put price exceeds call price.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExercisePriority {
    /// Issuer can override the holder's decision: min(max(V, put), call).
    IssuerLast,
    /// Holder can override the issuer's decision: max(min(V, call), put).
    HolderLast,
}
/// Explicit model-specific price refinement policy; no universal callable bound.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GridSettings {
    /// Desired coarse intervals, before exact event insertion.
    pub base_steps: usize,
    /// 2..4 doublings. Both of the last two successive differences must pass.
    pub refinements: usize,
    /// Absolute tolerance in price per 100 ORIGINAL face.
    pub absolute_price_tolerance: f64,
    /// Relative bound, multiplied by the finer price's absolute magnitude.
    pub relative_price_tolerance: f64,
}
impl GridSettings {
    fn validate(self) -> Result<(), OptionalBondError> {
        if self.base_steps == 0
            || !(2..=4).contains(&self.refinements)
            || self
                .base_steps
                .checked_mul(1 << self.refinements)
                .is_none_or(|n| n > MAX_TREE_STEPS)
            || !self.absolute_price_tolerance.is_finite()
            || self.absolute_price_tolerance <= 0.0
            || !self.relative_price_tolerance.is_finite()
            || self.relative_price_tolerance < 0.0
        {
            return Err(invalid(
                "grid settings",
                "bounded intervals, 2..4 refinements and finite price tolerances required",
            ));
        }
        Ok(())
    }
}
/// Complete optional model policy, separate from instrument terms.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OptionalBondSettings {
    /// Checked refinement policy.
    pub grid: GridSettings,
    /// Supplied simultaneous call/put ordering.
    pub exercise_priority: ExercisePriority,
}
/// Resolved immutable inputs; no repositories, clocks or provider handles.
pub struct OptionalBondInputs<'a> {
    /// Currency/collateral discount role.
    pub discount: &'a BuiltCurve,
    /// Explicit index role for term FRNs; absent for fixed cash.
    pub projection: Option<&'a BuiltCurve>,
    /// Assumed or successfully calibrated parameters, never a volatility default.
    pub model: &'a OptionalBondModel,
    /// Actual caller-resolved exercise/notice calendar.
    pub exercise_calendar: &'a dyn Calendar,
}
/// A declared parallel continuous-zero scenario, in decimal rate units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CurveShift {
    /// Shift of currency discount zero rates on ACT/365F model time.
    pub discount: f64,
    /// Shift of index projection zeros; known fixings never move.
    pub projection: f64,
}
impl CurveShift {
    /// Explicit base scenario.
    pub const BASE: Self = Self {
        discount: 0.0,
        projection: 0.0,
    };
    fn validate(self, floating: bool) -> Result<(), OptionalBondError> {
        if !self.discount.is_finite()
            || self.discount.abs() > 1.0
            || !self.projection.is_finite()
            || self.projection.abs() > 1.0
        {
            return Err(invalid(
                "curve shift",
                "finite decimal shifts in [-1,1] required",
            ));
        }
        if !floating && self.projection != 0.0 {
            return Err(unsupported(
                "projection risk",
                "fixed cash has no index projection role",
            ));
        }
        Ok(())
    }
}
/// One actual grid result, including the initial-curve fit and resources.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GridPrice {
    /// Desired resolution before actual dated event insertion.
    pub desired_steps: usize,
    /// Measured price per 100 original face.
    pub dirty_price: f64,
    /// Absolute difference from the preceding grid; absent on the first grid.
    pub successive_difference: Option<f64>,
    /// Actual allocation/curve-fit/probability diagnostics.
    pub tree: TreeDiagnostics,
}
/// Exercise decisions on the finest grid; node counts are not probabilities.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ExerciseDiagnostic {
    /// Complete notice/exercise/settlement dates consumed.
    pub dates: ExerciseDates,
    /// Issuer call or holder put.
    pub right: ExerciseRight,
    /// Nodes at which this right strictly changes the holder's value.
    pub exercise_nodes: usize,
    /// Number of nodes at this decision layer.
    pub decision_nodes: usize,
}
/// Successful model quote with full assumptions and numerical convergence.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OptionalBondPrice {
    /// Exact supplied settlement, cash-date and alive state.
    pub context: OptionalBondContext,
    /// Absolute currency PV at settlement.
    pub present_value: f64,
    /// Dirty per 100 original face.
    pub dirty_price: f64,
    /// Clean per 100 original face.
    pub clean_price: f64,
    /// Known accrued/rebate per 100 original face.
    pub accrued: f64,
    /// Constant additive discount spread, decimal; never rounded to whole bp.
    pub oas_decimal: f64,
    /// Exact supplied model and assumption/calibration provenance.
    pub model: OptionalBondModel,
    /// Base input curve identity; scenario is reported separately.
    pub discount_curve: CurveKey,
    /// Explicit projection input identity, if floating.
    pub projection_curve: Option<CurveKey>,
    /// Eligible published revisions only; future forecasts are not fixing values.
    pub fixings: Vec<ResolvedRate>,
    /// Future stochastic reset requirements, without a fabricated scalar coupon.
    pub future_reset_requirements: Vec<FixingRequirement>,
    /// Actual modeled parallel rate shifts.
    pub scenario: CurveShift,
    /// Every declared resolution and its measured price.
    pub grids: Vec<GridPrice>,
    /// Optimal-stopping node diagnostics; not recorded exercise events.
    pub exercises: Vec<ExerciseDiagnostic>,
    /// Floating basis is the supplied time-zero projection/discount ratio, with
    /// common HW1F state and deterministic multiplicative basis dynamics.
    pub deterministic_projection_basis: bool,
    /// Supplied holder/issuer priority retained in the result.
    pub exercise_priority: ExercisePriority,
}

#[derive(Debug, Clone)]
struct DatedCash {
    date: Date,
    amount: f64,
}
#[derive(Debug, Clone)]
struct PreparedExercise {
    dates: ExerciseDates,
    right: ExerciseRight,
    cash: Vec<DatedCash>,
}
#[derive(Debug, Clone)]
struct FloatingCoupon {
    start: Date,
    end: Date,
    notional: f64,
    gearing: f64,
    deterministic_cash: f64,
    accrual_ratio: f64,
    basis_ratio: f64,
}
#[derive(Debug, Clone)]
struct Grid {
    desired_steps: usize,
    tree: Hw1fTree,
    base_discounts: Vec<f64>,
    cash: Vec<(usize, f64)>,
    exercise: Vec<(usize, Vec<(usize, f64)>)>,
    floating: Vec<(usize, usize)>,
}
/// Prepared immutable pure pricer. OAS inversion reuses the same validated trees.
#[derive(Debug, Clone)]
pub struct PreparedOptionalBond {
    currency: convex_core::Currency,
    context: OptionalBondContext,
    face: f64,
    face_value: Decimal,
    accrued: f64,
    model: OptionalBondModel,
    discount_curve: CurveKey,
    projection_curve: Option<CurveKey>,
    discount_recipe: convex_curves::contracts::CurveRecipe,
    projection_recipe: Option<convex_curves::contracts::CurveRecipe>,
    settings: OptionalBondSettings,
    exercises: Vec<PreparedExercise>,
    floating: Vec<FloatingCoupon>,
    fixings: Vec<ResolvedRate>,
    future_reset_requirements: Vec<FixingRequirement>,
    grids: Vec<Grid>,
}

fn validate_inputs(
    bond: &BondContract,
    context: OptionalBondContext,
    inputs: &OptionalBondInputs<'_>,
    settings: OptionalBondSettings,
) -> Result<(), OptionalBondError> {
    settings.grid.validate()?;
    time(context.settlement, context.settlement)?;
    if context.settlement < bond.terms().issue_date {
        return Err(invalid("settlement", "before issue"));
    }
    if bond.terms().exercise.is_empty() {
        return Err(unsupported(
            "optional price",
            "requires explicit exercise terms; use the plain bond route otherwise",
        ));
    }
    if bond.periods().any(|p| p.ex_coupon().is_some()) {
        return Err(unsupported("optional ex-coupon", "requires detached-coupon ownership across exercise; plain entitlement remains available"));
    }
    discount_role(inputs.discount, bond.terms().currency)?;
    if let ModelOrigin::SwaptionCalibration {
        discount_curve,
        valuation_date,
        ..
    } = inputs.model.origin()
    {
        if *discount_curve != inputs.discount.recipe().key || *valuation_date != context.settlement
        {
            return Err(invalid("calibrated model", "model root and exact calibration curve must match; explicit scenario risk freezes this base model"));
        }
    }
    Ok(())
}
fn exercise_dates(
    bond: &BondContract,
    root: Date,
    calendar: &dyn Calendar,
) -> Result<Vec<(ExerciseTerms, ExerciseDates)>, OptionalBondError> {
    let mut result = vec![];
    for terms in &bond.terms().exercise {
        let dates=match &terms.style {
            ExerciseStyle::European { date } => vec![*date],
            ExerciseStyle::Bermudan { dates } => dates.clone(),
            ExerciseStyle::American { .. } => return Err(unsupported("American exercise", "continuous exercise needs a separately validated obstacle/discretization model; a daily/coupon-date substitute is not admitted")),
        };
        for date in dates {
            if date < root || terms.protection_end.is_some_and(|end| date < end) {
                continue;
            }
            let consequences = terms.dates(date, calendar)?;
            if consequences.notice < root {
                return Err(unsupported("outstanding notice state", "future redemption with elapsed notice requires an explicit accepted/declined notice-state model"));
            }
            result.push((terms.clone(), consequences));
            if result.len() > MAX_EXERCISES {
                return Err(invalid("exercise count", "at most 128 live dated rights"));
            }
        }
    }
    result.sort_by_key(|(_, d)| (d.notice, d.exercise, d.settlement));
    Ok(result)
}
fn fixed_exercises(
    bond: &BondContract,
    context: OptionalBondContext,
    calendar: &dyn Calendar,
) -> Result<Vec<PreparedExercise>, OptionalBondError> {
    let base = bond.fixed_projection(context.settlement, context.on_payment)?;
    exercise_dates(bond, context.settlement, calendar)?
        .into_iter()
        .map(|(terms, dates)| {
            let mut cash = vec![];
            for c in &base.coupons {
                if c.entitled && c.accrual_end <= dates.exercise && c.payment_date >= dates.notice {
                    cash.push(DatedCash {
                        date: c.payment_date,
                        amount: number(c.amount)?,
                    });
                }
            }
            for p in &base.principal {
                if p.effective_date <= dates.exercise && p.payment_date >= dates.notice {
                    cash.push(DatedCash {
                        date: p.payment_date,
                        amount: number(p.amount)?,
                    });
                }
            }
            let outstanding = bond.outstanding_principal(dates.exercise, BoundarySide::After)?;
            let price = outstanding
                .checked_mul(terms.price_per_100)
                .and_then(|v| v.checked_div(Decimal::from(100)))
                .ok_or_else(|| invalid("exercise cash", "Decimal arithmetic"))?;
            let mut retirement = bond.terms().rounding.apply(price)?;
            if terms.accrued == ExerciseAccrued::PayAccrued {
                let at_exercise =
                    bond.fixed_projection(dates.exercise, PaymentDateEntitlement::Exclude)?;
                for c in at_exercise.coupons {
                    // Fully accrued delayed coupons are already owed in `cash`.
                    if c.accrual_end > dates.exercise {
                        retirement = retirement
                            .checked_add(c.accrued)
                            .ok_or_else(|| invalid("exercise accrued", "Decimal arithmetic"))?;
                    }
                }
            }
            cash.push(DatedCash {
                date: dates.settlement,
                amount: number(retirement)?,
            });
            Ok(PreparedExercise {
                dates,
                right: terms.right,
                cash,
            })
        })
        .collect()
}

/// Fixed/stepped/amortizing pure optional price setup. Notice decisions compare
/// continuation with the discounted full contractual exercise branch, including
/// intervening and owed delayed cash. Principal uses the AFTER boundary.
pub fn prepare_fixed(
    bond: &BondContract,
    context: OptionalBondContext,
    inputs: OptionalBondInputs<'_>,
    settings: OptionalBondSettings,
) -> Result<PreparedOptionalBond, OptionalBondError> {
    validate_inputs(bond, context, &inputs, settings)?;
    if !matches!(bond.terms().coupon, BondCoupon::Fixed) || inputs.projection.is_some() {
        return Err(unsupported(
            "fixed optional price",
            "requires actual fixed cash and no projection role",
        ));
    }
    let projection = bond.fixed_projection(context.settlement, context.on_payment)?;
    let cash = projection
        .cashflows()?
        .into_iter()
        .map(|c| {
            Ok(DatedCash {
                date: c.date,
                amount: number(c.amount)?,
            })
        })
        .collect::<Result<Vec<_>, OptionalBondError>>()?;
    let exercises = fixed_exercises(bond, context, inputs.exercise_calendar)?;
    assemble(
        context,
        bond.terms().face_value,
        number(projection.accrued)?,
        inputs,
        settings,
        cash,
        exercises,
        vec![],
        vec![],
        vec![],
    )
}
fn constant_profile(
    profile: &[DatedValue],
    start: Date,
    end: Date,
    field: &'static str,
) -> Result<f64, OptionalBondError> {
    let term = profile
        .iter()
        .find(|p| p.start <= start && p.end >= end)
        .ok_or_else(|| {
            unsupported(
                field,
                "joint FRN model requires constant terms within each reset/coupon interval",
            )
        })?;
    number(term.value)
}
fn term_periods(
    bond: &BondContract,
) -> Result<Vec<convex_core::cashflows::CouponPeriod>, OptionalBondError> {
    let BondCoupon::Term(rate) = &bond.terms().coupon else {
        return Err(unsupported("joint floating option", "only aligned term resets are supported; overnight/path-dependent coupons require additional state"));
    };
    if !matches!(
        rate.fixing_offset,
        DateOffset::CalendarDays(0) | DateOffset::BusinessDays(0)
    ) || bond.terms().rounding != CashRounding::Unrounded
        || bond.terms().negative_coupons != NegativeCouponPolicy::Allow
        || bond.terms().coupon_principal == CouponPrincipal::AfterCouponBoundary
    {
        return Err(unsupported("joint FRN terms", "requires zero fixing offset, unrounded linear coupons, allowed negative coupons and before/accrual principal; cap/floor/clipping and nonlinear rounding need additional state"));
    }
    let periods: Vec<_> = bond.periods().cloned().collect();
    let resets: Vec<_> = bond.term_reset_slices().collect();
    if periods.len() != resets.len()
        || periods
            .iter()
            .zip(&resets)
            .any(|(p, (original, start, end, request))| {
                p != *original
                    || p.accrual_dates() != (*start, *end)
                    || request.rate_start != *start
                    || request.rate_end != *end
                    || request.observation_date != *start
                    || p.payment_date() != *end
            })
    {
        return Err(unsupported("joint FRN dates", "one actual prepared reset per coupon, payment at accrual end and exercise after coupons are required"));
    }
    if bond.principal_cashflows().iter().any(|p| {
        !periods
            .iter()
            .any(|c| c.accrual_dates().1 == p.effective_date)
            || p.payment_date != p.effective_date
    }) {
        return Err(unsupported(
            "joint FRN principal",
            "reductions/payments must coincide with reset/coupon boundaries",
        ));
    }
    Ok(periods)
}

/// Restricted genuine joint term-FRN model. Future reset coupons use the same
/// stochastic discount state and explicit deterministic multiplicative index
/// basis. Eligible published coupons remain fixed. No coupon-date optionality
/// is evaluated on deterministic projected coupon amounts.
pub fn prepare_floating<P>(
    bond: &BondContract,
    context: OptionalBondContext,
    projection_context: &BondProjectionContext,
    history: &FixingSeries,
    publication: P,
    inputs: OptionalBondInputs<'_>,
    settings: OptionalBondSettings,
) -> Result<PreparedOptionalBond, OptionalBondError>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    if context.settlement != projection_context.settlement
        || context.on_payment != projection_context.on_payment
    {
        return Err(invalid(
            "floating context",
            "optional and fixing/cash settlement policies must match",
        ));
    }
    validate_inputs(bond, context, &inputs, settings)?;
    if projection_context.fixing.valuation_date != context.settlement
        || !projection_context.overnight_accrual_cutoffs.is_empty()
    {
        return Err(unsupported(
            "joint FRN information time",
            "model root must equal supplied fixing valuation date, with no overnight cutoffs",
        ));
    }
    let periods = term_periods(bond)?;
    let projection_curve = inputs
        .projection
        .ok_or_else(|| invalid("projection curve", "required exact index curve"))?;
    let mut forecast = |request: &convex_bonds::contracts::ProjectionRequest| {
        crate::bond_contract::project_from_curve(request, bond.terms().currency, projection_curve)
    };
    let projection = bond.project(
        projection_context,
        history,
        publication,
        BondProjectionPolicy::Supplied(&mut forecast),
    )?;
    let BondCoupon::Term(rate) = &bond.terms().coupon else {
        return Err(unsupported("joint FRN", "requires term rate"));
    };
    // Role admission occurs even for all-known or fully matured coupons.
    if projection_curve.recipe().currency != bond.terms().currency
        || !matches!(&projection_curve.recipe().role,CurveRole::Projection { index_id } if *index_id==rate.index_id.as_str())
    {
        return Err(invalid(
            "projection curve",
            "currency/index/projection role mismatch",
        ));
    }
    let mut cash = vec![];
    let mut floating = vec![];
    let mut fixings = vec![];
    let mut future_reset_requirements = vec![];
    for coupon in &projection.coupons {
        if !coupon.entitled || coupon.rates.len() != 1 {
            return Err(unsupported(
                "joint FRN coupon",
                "requires one entitled term reset per coupon",
            ));
        }
        let period = periods
            .iter()
            .find(|p| p.accrual_dates() == (coupon.accrual_start, coupon.accrual_end))
            .ok_or_else(|| invalid("coupon", "prepared period missing"))?;
        let known = matches!(coupon.rates[0].origin, RateOrigin::Published(_));
        if known {
            fixings.extend(coupon.rates.iter().cloned());
            cash.push(DatedCash {
                date: coupon.payment_date,
                amount: number(coupon.amount)?,
            });
            continue;
        }
        if coupon.accrual_start <= context.settlement {
            return Err(unsupported("unpublished current FRN reset","eligible published fixing is required at/before the model root; it cannot become a future stochastic reset"));
        }
        future_reset_requirements.push(coupon.rates[0].requirement.clone());
        let start = coupon.accrual_start;
        let end = coupon.accrual_end;
        let n = constant_profile(
            &bond.terms().profiles.notional,
            start,
            end,
            "FRN principal profile",
        )?;
        let gearing = constant_profile(
            &bond.terms().profiles.gearing,
            start,
            end,
            "FRN gearing profile",
        )?;
        let component = constant_profile(
            &bond.terms().profiles.coupon,
            start,
            end,
            "FRN coupon profile",
        )?;
        let spread = constant_profile(
            &bond.terms().profiles.spread,
            start,
            end,
            "FRN spread profile",
        )?;
        let accrual = number(period.year_fraction(start, end, bond.terms().day_count)?)?;
        let index_accrual = number(
            rate.projection_day_count
                .to_day_count()
                .year_fraction(start, end),
        )?;
        if index_accrual <= 0.0 {
            return Err(invalid("index accrual", "nonpositive"));
        }
        let basis_ratio = finite(
            (df(projection_curve, end)? / df(projection_curve, start)?)
                / (df(inputs.discount, end)? / df(inputs.discount, start)?),
            "FRN basis",
        )?;
        if basis_ratio <= 0.0 {
            return Err(invalid("FRN basis", "must be positive"));
        }
        floating.push(FloatingCoupon {
            start,
            end,
            notional: n,
            gearing,
            deterministic_cash: n * (component + spread) * accrual,
            accrual_ratio: accrual / index_accrual,
            basis_ratio,
        });
    }
    for p in &projection.principal {
        cash.push(DatedCash {
            date: p.payment_date,
            amount: number(p.amount)?,
        });
    }
    let mut exercises = vec![];
    for (terms, dates) in exercise_dates(bond, context.settlement, inputs.exercise_calendar)? {
        if dates.notice != dates.exercise
            || dates.settlement != dates.exercise
            || !periods
                .iter()
                .any(|p| p.accrual_dates().1 == dates.exercise)
        {
            return Err(unsupported("joint FRN exercise","requires zero notice/settlement delay and exercise at actual coupon/reset boundary; off-cycle known coupons need additional state"));
        }
        let outstanding = number(bond.outstanding_principal(dates.exercise, BoundarySide::After)?)?;
        let mut branch: Vec<_> = cash
            .iter()
            .filter(|c| c.date == dates.exercise)
            .cloned()
            .collect();
        branch.push(DatedCash {
            date: dates.exercise,
            amount: outstanding * number(terms.price_per_100)? / 100.0,
        });
        exercises.push(PreparedExercise {
            dates,
            right: terms.right,
            cash: branch,
        });
    }
    assemble(
        context,
        bond.terms().face_value,
        number(projection.accrued)?,
        inputs,
        settings,
        cash,
        exercises,
        floating,
        fixings,
        future_reset_requirements,
    )
}

fn grid_discount(
    curve: &BuiltCurve,
    root: Date,
    model_time: f64,
    root_df: f64,
) -> Result<f64, OptionalBondError> {
    let days = model_time * 365.0;
    // Recover exact dated anchors within binary arithmetic precision only.
    if (days - days.round()).abs() < 1e-9 {
        return Ok(df(
            curve,
            root.checked_add_days(days.round() as i64)
                .map_err(|e| invalid("grid date", e.to_string()))?,
        )? / root_df);
    }
    let floor = days.floor() as i64;
    let date = root
        .checked_add_days(floor)
        .map_err(|e| invalid("grid date", e.to_string()))?;
    let next = date
        .checked_add_days(1)
        .map_err(|e| invalid("grid date", e.to_string()))?;
    let left = curve.date_to_tenor(date)?;
    let right = curve.date_to_tenor(next)?;
    let tenor = left + (days - floor as f64) * (right - left);
    Ok(curve
        .query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(tenor),
        })?
        .value
        / root_df)
}
#[allow(clippy::too_many_arguments)] // One internal assembly of validated components.
fn assemble(
    context: OptionalBondContext,
    face_value: Decimal,
    accrued: f64,
    inputs: OptionalBondInputs<'_>,
    settings: OptionalBondSettings,
    cash: Vec<DatedCash>,
    exercises: Vec<PreparedExercise>,
    floating: Vec<FloatingCoupon>,
    fixings: Vec<ResolvedRate>,
    future_reset_requirements: Vec<FixingRequirement>,
) -> Result<PreparedOptionalBond, OptionalBondError> {
    let face = number(face_value)?;
    if cash.len() > MAX_TREE_STEPS || floating.len() > 512 || face <= 0.0 {
        return Err(invalid(
            "optional cash dimensions",
            "bounded positive-face cash required",
        ));
    }
    let mut events: Vec<f64> = cash
        .iter()
        .map(|c| time(context.settlement, c.date))
        .collect::<Result<_, _>>()?;
    for exercise in &exercises {
        events.push(time(context.settlement, exercise.dates.notice)?);
        for c in &exercise.cash {
            events.push(time(context.settlement, c.date)?);
        }
    }
    for c in &floating {
        events.extend([
            time(context.settlement, c.start)?,
            time(context.settlement, c.end)?,
        ]);
    }
    if !events.iter().any(|t| *t > 0.0) {
        return Err(unsupported(
            "matured optional price",
            "no positive remaining model horizon; value remaining plain cash explicitly",
        ));
    }
    let root_df = df(inputs.discount, context.settlement)?;
    let mut grids = vec![];
    for refinement in 0..=settings.grid.refinements {
        let desired_steps = settings.grid.base_steps * (1 << refinement);
        let times = event_grid(&events, desired_steps)?;
        let mut discounts = times
            .iter()
            .map(|t| grid_discount(inputs.discount, context.settlement, *t, root_df))
            .collect::<Result<Vec<_>, _>>()?;
        discounts[0] = 1.0;
        let tree = Hw1fTree::build(inputs.model.parameters(), &times, &discounts)?;
        let factor = 1 + exercises.len() + 3 * floating.len();
        if tree
            .diagnostics()
            .nodes
            .checked_mul(factor)
            .is_none_or(|work| work > MAX_PRICE_WORK)
        {
            return Err(invalid(
                "optional rollback work",
                "32 million node-visit conservative budget per grid exceeded",
            ));
        }
        let layer =
            |date| Ok::<_, OptionalBondError>(tree.layer_at(time(context.settlement, date)?)?);
        let grid_cash = cash
            .iter()
            .map(|c| Ok((layer(c.date)?, c.amount)))
            .collect::<Result<_, OptionalBondError>>()?;
        let grid_exercise = exercises
            .iter()
            .map(|e| {
                Ok((
                    layer(e.dates.notice)?,
                    e.cash
                        .iter()
                        .map(|c| Ok((layer(c.date)?, c.amount)))
                        .collect::<Result<_, OptionalBondError>>()?,
                ))
            })
            .collect::<Result<_, OptionalBondError>>()?;
        let grid_floating = floating
            .iter()
            .map(|c| Ok((layer(c.start)?, layer(c.end)?)))
            .collect::<Result<_, OptionalBondError>>()?;
        grids.push(Grid {
            desired_steps,
            tree,
            base_discounts: discounts,
            cash: grid_cash,
            exercise: grid_exercise,
            floating: grid_floating,
        });
    }
    Ok(PreparedOptionalBond {
        currency: inputs.discount.recipe().currency,
        context,
        face,
        face_value,
        accrued,
        model: inputs.model.clone(),
        discount_curve: inputs.discount.recipe().key.clone(),
        projection_curve: inputs.projection.map(|p| p.recipe().key.clone()),
        discount_recipe: inputs.discount.recipe().clone(),
        projection_recipe: inputs.projection.map(|p| p.recipe().clone()),
        settings,
        exercises,
        floating,
        fixings,
        future_reset_requirements,
        grids,
    })
}

impl PreparedOptionalBond {
    /// Exact original face, retained without an f64 or per100 reconstruction.
    pub fn face_value(&self) -> Decimal {
        self.face_value
    }
    /// Original model root and price settlement.
    pub fn settlement(&self) -> Date {
        self.context.settlement
    }
    /// Last actual tree query relative to the original settlement, in ACT/365F.
    /// All remaining cash, exercises and future reset endpoints are inserted.
    pub fn curve_query_horizon(&self) -> Result<f64, OptionalBondError> {
        self.grids
            .last()
            .and_then(|g| g.tree.times().last())
            .copied()
            .ok_or_else(|| invalid("curve query horizon", "empty original tree"))
    }
    /// Complete actual input recipes; graph composition must use these exact inputs.
    pub fn curve_recipes(
        &self,
    ) -> (
        &convex_curves::contracts::CurveRecipe,
        Option<&convex_curves::contracts::CurveRecipe>,
    ) {
        (&self.discount_recipe, self.projection_recipe.as_ref())
    }
    /// Original assumed or calibrated model; this does not expose mutable state.
    pub fn model(&self) -> &OptionalBondModel {
        &self.model
    }
    /// Contract currency, retained from the checked original discount mapping.
    pub fn currency(&self) -> convex_core::Currency {
        self.currency
    }
    /// Price at a declared constant OAS, checking both final successive refinements.
    pub fn price(&self, oas_decimal: f64) -> Result<OptionalBondPrice, OptionalBondError> {
        self.price_scenario(oas_decimal, CurveShift::BASE)
    }
    /// Bump/reprice the same immutable model/fixings/cash/grid policy. Model
    /// parameters are frozen; shifted input curves are refit by state prices.
    pub fn price_scenario(
        &self,
        oas_decimal: f64,
        scenario: CurveShift,
    ) -> Result<OptionalBondPrice, OptionalBondError> {
        scenario.validate(self.projection_curve.is_some())?;
        let mut grids: Vec<GridPrice> = vec![];
        let mut diagnostics = vec![];
        let mut pv = 0.0;
        for grid in &self.grids {
            let (price, exercises, tree) = self.price_on_grid(grid, oas_decimal, scenario)?;
            pv = price;
            let dirty = finite(price / self.face * 100.0, "dirty price")?;
            let difference = grids.last().map(|p| (dirty - p.dirty_price).abs());
            grids.push(GridPrice {
                desired_steps: grid.desired_steps,
                dirty_price: dirty,
                successive_difference: difference,
                tree,
            });
            diagnostics = exercises;
        }
        for p in &grids[grids.len() - 2..] {
            if p.successive_difference.is_none_or(|d| {
                d > self.settings.grid.absolute_price_tolerance
                    + self.settings.grid.relative_price_tolerance * p.dirty_price.abs()
            }) {
                return Err(OptionalBondError::GridConvergence { diagnostics: grids });
            }
        }
        let dirty_price = pv / self.face * 100.0;
        let accrued = self.accrued / self.face * 100.0;
        Ok(OptionalBondPrice {
            context: self.context,
            present_value: pv,
            dirty_price,
            clean_price: finite(dirty_price - accrued, "clean price")?,
            accrued,
            oas_decimal,
            model: self.model.clone(),
            discount_curve: self.discount_curve.clone(),
            projection_curve: self.projection_curve.clone(),
            fixings: self.fixings.clone(),
            future_reset_requirements: self.future_reset_requirements.clone(),
            scenario,
            grids,
            exercises: diagnostics,
            deterministic_projection_basis: self.projection_curve.is_some(),
            exercise_priority: self.settings.exercise_priority,
        })
    }
    fn price_on_grid(
        &self,
        grid: &Grid,
        oas: f64,
        scenario: CurveShift,
    ) -> Result<(f64, Vec<ExerciseDiagnostic>, TreeDiagnostics), OptionalBondError> {
        if !oas.is_finite() || oas.abs() > 1.0 {
            return Err(invalid("OAS", "finite decimal spread in [-1,1] required"));
        }
        let shifted_tree;
        let tree = if scenario.discount == 0.0 {
            &grid.tree
        } else {
            let discounts = grid
                .base_discounts
                .iter()
                .zip(grid.tree.times())
                .map(|(df, t)| df * (-scenario.discount * t).exp())
                .collect::<Vec<_>>();
            shifted_tree = Hw1fTree::build(self.model.parameters(), grid.tree.times(), &discounts)?;
            &shifted_tree
        };
        let mut amounts = vec![0.0; tree.times().len()];
        for &(i, cash) in &grid.cash {
            amounts[i] = finite(amounts[i] + cash, "cash sum")?;
        }
        let branches = grid
            .exercise
            .iter()
            .map(|(decision, cash)| tree.cash_value_at(*decision, cash, oas))
            .collect::<Result<Vec<_>, _>>()?;
        let mut floating_values = Vec::with_capacity(self.floating.len());
        for (coupon, &(start, end)) in self.floating.iter().zip(&grid.floating) {
            let unspread = tree.cash_value_at(start, &[(end, 1.0)], 0.0)?;
            let spread = tree.cash_value_at(start, &[(end, 1.0)], oas)?;
            let duration = time(coupon.start, coupon.end)?;
            let basis =
                coupon.basis_ratio * (-(scenario.projection - scenario.discount) * duration).exp();
            let values = unspread
                .iter()
                .zip(spread)
                .map(|(p, ps)| {
                    if *p <= 0.0 || basis <= 0.0 {
                        return Err(invalid("conditional projection DF", "must be positive"));
                    }
                    finite(
                        (coupon.notional
                            * coupon.gearing
                            * coupon.accrual_ratio
                            * (1.0 / (*p * basis) - 1.0)
                            + coupon.deterministic_cash)
                            * ps,
                        "stochastic FRN coupon",
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            floating_values.push(values);
        }
        let mut diagnostics: Vec<_> = self
            .exercises
            .iter()
            .map(|e| ExerciseDiagnostic {
                dates: e.dates.clone(),
                right: e.right,
                exercise_nodes: 0,
                decision_nodes: 0,
            })
            .collect();
        let end = tree.times().len() - 1;
        let mut values = vec![0.0; tree.width(end)?];
        for layer in (0..=end).rev() {
            if layer < end {
                values = tree.rollback(layer, &values, oas)?;
            }
            let resetting: Vec<_> = grid
                .floating
                .iter()
                .enumerate()
                .filter_map(|(k, &(start, _))| (start == layer).then_some(k))
                .collect();
            for (index, value) in values.iter_mut().enumerate() {
                *value = finite(*value + amounts[layer], "holder value")?;
                for &k in &resetting {
                    *value = finite(*value + floating_values[k][index], "reset coupon value")?;
                }
            }
            let order = match self.settings.exercise_priority {
                ExercisePriority::IssuerLast => [ExerciseRight::Put, ExerciseRight::Call],
                ExercisePriority::HolderLast => [ExerciseRight::Call, ExerciseRight::Put],
            };
            for right in order {
                for (k, (decision, _)) in grid.exercise.iter().enumerate() {
                    if *decision != layer || self.exercises[k].right != right {
                        continue;
                    }
                    diagnostics[k].decision_nodes = values.len();
                    for (index, value) in values.iter_mut().enumerate() {
                        let candidate = branches[k][index];
                        let exercise = match right {
                            ExerciseRight::Call => candidate < *value,
                            ExerciseRight::Put => candidate > *value,
                        };
                        if exercise {
                            *value = candidate;
                            diagnostics[k].exercise_nodes += 1;
                        }
                    }
                }
            }
        }
        Ok((
            finite(values[0], "optional PV")?,
            diagnostics,
            tree.diagnostics().clone(),
        ))
    }
    /// OAS price inversion on the fixed finest grid, followed by all declared
    /// convergence checks. Width alone and rounded bp never establish success.
    pub fn solve_oas(
        &self,
        target_dirty_price: f64,
        settings: OasSolveSettings,
    ) -> Result<OasResult, OptionalBondError> {
        if !target_dirty_price.is_finite()
            || target_dirty_price <= 0.0
            || !settings.lower.is_finite()
            || !settings.upper.is_finite()
            || settings.lower >= settings.upper
            || settings.lower < -1.0
            || settings.upper > 1.0
            || !settings.price_tolerance.is_finite()
            || settings.price_tolerance <= 0.0
            || settings.max_iterations == 0
            || settings.max_iterations > 10000
        {
            return Err(invalid("OAS inverse","positive target, closed spread bracket and bounded native price tolerance required"));
        }
        let grid = self.grids.last().ok_or_else(|| invalid("grid", "empty"))?;
        let residual = |spread| {
            Ok::<_, OptionalBondError>(
                self.price_on_grid(grid, spread, CurveShift::BASE)?.0 / self.face * 100.0
                    - target_dirty_price,
            )
        };
        let mut low = settings.lower;
        let mut high = settings.upper;
        let mut f_low = residual(low)?;
        let f_high = residual(high)?;
        if (f_low - f_high).abs() <= settings.price_tolerance
            || (f_low.signum() == f_high.signum()
                && f_low.abs() > settings.price_tolerance
                && f_high.abs() > settings.price_tolerance)
        {
            return Err(OptionalBondError::OasBracket {
                low_residual: f_low,
                high_residual: f_high,
            });
        }
        for (spread, value) in [(low, f_low), (high, f_high)] {
            if value.abs() <= settings.price_tolerance {
                let price = self.price(spread)?;
                return Ok(OasResult {
                    oas_decimal: spread,
                    oas_bps: spread * 10000.0,
                    iterations: 0,
                    residual_per_100: price.dirty_price - target_dirty_price,
                    price,
                });
            }
        }
        for iteration in 1..=settings.max_iterations {
            let spread = f64::midpoint(low, high);
            let value = residual(spread)?;
            if value.abs() <= settings.price_tolerance {
                let price = self.price(spread)?;
                return Ok(OasResult {
                    oas_decimal: spread,
                    oas_bps: spread * 10000.0,
                    iterations: iteration,
                    residual_per_100: price.dirty_price - target_dirty_price,
                    price,
                });
            }
            if iteration == settings.max_iterations {
                return Err(OptionalBondError::OasConvergence {
                    iterations: iteration,
                    residual: value,
                });
            }
            if value.signum() == f_low.signum() {
                low = spread;
                f_low = value;
            } else {
                high = spread;
            }
        }
        Err(invalid("OAS inverse", "unreachable bounded loop end"))
    }
    /// Effective curve risk freezes OAS/parameters and eligible observations.
    /// Spread risk freezes input curves/parameters and changes only OAS.
    pub fn risk(
        &self,
        oas: f64,
        axis: RiskAxis,
        bump: f64,
        settings: RiskSettings,
    ) -> Result<OptionalBondRisk, OptionalBondError> {
        let minimum_bump = self.risk_resolution(bump, settings)?;
        let base = self.price(oas)?;
        let shifted = |sign: f64| match axis {
            RiskAxis::Discount => self.price_scenario(
                oas,
                CurveShift {
                    discount: sign * bump,
                    projection: 0.0,
                },
            ),
            RiskAxis::Projection => self.price_scenario(
                oas,
                CurveShift {
                    discount: 0.0,
                    projection: sign * bump,
                },
            ),
            RiskAxis::DiscountAndProjection => self.price_scenario(
                oas,
                CurveShift {
                    discount: sign * bump,
                    projection: if self.projection_curve.is_some() {
                        sign * bump
                    } else {
                        0.0
                    },
                },
            ),
            RiskAxis::Oas => self.price(oas + sign * bump),
        };
        let up = shifted(1.0)?;
        let down = shifted(-1.0)?;
        if base.dirty_price.abs() < 1e-12 {
            return Err(invalid("risk normalization", "zero base price"));
        }
        let mut grids = Vec::with_capacity(base.grids.len());
        for ((b, u), d) in base.grids.iter().zip(&up.grids).zip(&down.grids) {
            if b.dirty_price.abs() < 1e-12 {
                return Err(invalid("risk grid normalization", "zero price"));
            }
            grids.push(GridRisk {
                desired_steps: b.desired_steps,
                duration: finite(
                    (d.dirty_price - u.dirty_price) / (2.0 * b.dirty_price * bump),
                    "grid duration",
                )?,
                convexity: finite(
                    (d.dirty_price + u.dirty_price - 2.0 * b.dirty_price)
                        / (b.dirty_price * bump * bump),
                    "grid convexity",
                )?,
                pv01: finite(
                    (d.dirty_price - u.dirty_price) / (2.0 * bump) * 1e-4 * self.face / 100.0,
                    "grid PV01",
                )?,
            });
        }
        for pair in grids[grids.len() - 3..].windows(2) {
            if (pair[1].duration - pair[0].duration).abs()
                > settings.absolute_duration_tolerance
                    + settings.relative_duration_tolerance * pair[1].duration.abs()
                || (pair[1].convexity - pair[0].convexity).abs()
                    > settings.absolute_convexity_tolerance
                        + settings.relative_convexity_tolerance * pair[1].convexity.abs()
            {
                return Err(OptionalBondError::RiskConvergence { diagnostics: grids });
            }
        }
        Ok(OptionalBondRisk {
            settings,
            grids,
            minimum_resolved_bump: minimum_bump,
            duration: finite(
                (down.dirty_price - up.dirty_price) / (2.0 * base.dirty_price * bump),
                "effective duration",
            )?,
            convexity: finite(
                (down.dirty_price + up.dirty_price - 2.0 * base.dirty_price)
                    / (base.dirty_price * bump * bump),
                "effective convexity",
            )?,
            pv01: finite(
                (down.present_value - up.present_value) / (2.0 * bump) * 1e-4,
                "currency PV01",
            )?,
            axis,
            bump,
            base,
            up,
            down,
        })
    }
}

/// Native inverse controls; no bracket expansion or hidden price tolerance.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OasSolveSettings {
    /// Lower decimal OAS, at least -1.
    pub lower: f64,
    /// Upper decimal OAS, at most 1.
    pub upper: f64,
    /// Absolute dirty-price residual per 100 original face.
    pub price_tolerance: f64,
    /// Bounded bisection attempts, 1..10000.
    pub max_iterations: usize,
}
/// Successful unrounded inverse and its repriced model inputs.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OasResult {
    /// Constant decimal discount spread.
    pub oas_decimal: f64,
    /// Basis points, preserving fractional bp.
    pub oas_bps: f64,
    /// Actual bisection attempts.
    pub iterations: usize,
    /// Model minus target dirty price per 100 original face.
    pub residual_per_100: f64,
    /// Repriced result with actual refinement/assumption diagnostics.
    pub price: OptionalBondPrice,
}
/// Curve factor changed by a finite difference; not a quote bump/recalibration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RiskAxis {
    /// Discount only, with projection basis adjusted to hold index zeros fixed.
    Discount,
    /// Index projection only; known fixings do not move.
    Projection,
    /// Discount and index projection together, where applicable.
    DiscountAndProjection,
    /// Constant spread only, with both input curves fixed.
    Oas,
}
/// Complete bump/reprice outcomes; missing/unstable risk is never zero-filled.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OptionalBondRisk {
    /// Native sensitivity refinement policy actually enforced.
    pub settings: RiskSettings,
    /// Measured sensitivities at every grid, independently from price convergence.
    pub grids: Vec<GridRisk>,
    /// Two finest-grid short-rate state cells; a conservative resolution floor,
    /// not an accuracy guarantee or substitute for independent model validation.
    pub minimum_resolved_bump: f64,
    /// Central dirty-price duration, units years under the declared factor.
    pub duration: f64,
    /// Central convexity, units years squared under the declared factor.
    pub convexity: f64,
    /// Signed long currency PV reduction per positive 1bp factor move.
    pub pv01: f64,
    /// Explicit bumped factor.
    pub axis: RiskAxis,
    /// Decimal bump actually consumed.
    pub bump: f64,
    /// Complete base price with OAS/model/grid provenance.
    pub base: OptionalBondPrice,
    /// Complete positive-bump price.
    pub up: OptionalBondPrice,
    /// Complete negative-bump price.
    pub down: OptionalBondPrice,
}

/// Separate sensitivity postconditions; price convergence cannot substitute.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RiskSettings {
    /// Absolute duration difference across final successive grids, in years.
    pub absolute_duration_tolerance: f64,
    /// Relative duration bound against the finer sensitivity.
    pub relative_duration_tolerance: f64,
    /// Absolute convexity difference across final successive grids, years squared.
    pub absolute_convexity_tolerance: f64,
    /// Relative convexity bound against the finer sensitivity.
    pub relative_convexity_tolerance: f64,
}
impl RiskSettings {
    /// Check explicit finite separate sensitivity refinement bounds.
    pub fn validate(self) -> Result<(), OptionalBondError> {
        if [
            self.absolute_duration_tolerance,
            self.absolute_convexity_tolerance,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0)
            || [
                self.relative_duration_tolerance,
                self.relative_convexity_tolerance,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(invalid(
                "risk settings",
                "positive finite absolute and nonnegative relative sensitivity bounds required",
            ));
        }
        Ok(())
    }
}
/// Actual fixed-model finite differences at one grid resolution.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GridRisk {
    /// Desired resolution before exact event insertion.
    pub desired_steps: usize,
    /// Central duration in years.
    pub duration: f64,
    /// Central convexity in years squared.
    pub convexity: f64,
    /// Signed currency PV reduction per positive 1bp factor move.
    pub pv01: f64,
}
