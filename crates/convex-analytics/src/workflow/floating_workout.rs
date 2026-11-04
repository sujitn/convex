//! Conditional floating exercise cash and frozen-projection discount margins.
use super::*;
use crate::{cash_quotation as invoice, quote_risk::RiskPosition};
use convex_bonds::contracts::{
    BondCoupon, BondProjection, BondProjectionContext, BondProjectionPolicy, BondTerms,
    PreparedFloatingWorkout, WorkoutExercise,
};
use convex_core::contracts::ModelMoney;
use convex_curves::contracts::{BuiltCurve, CurveRole};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn required_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Date>, D::Error> {
    Option::<Date>::deserialize(d)
}
#[cfg(feature = "schemars")]
fn nullable_date(g: &mut schemars::SchemaGenerator) -> schemars::Schema {
    g.subschema_for::<Option<Date>>()
}

/// Original legal path and explicit partial shifted observation-weight choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FloatingWorkoutSelection {
    /// Maturity or exact original provision/date.
    pub path: WorkoutSelection,
    /// Null unless a partial shifted overnight PayAccrued coupon needs a cutoff.
    /// Absence cannot choose an implicit null or infer a shifted economic date.
    #[serde(deserialize_with = "required_option")]
    #[cfg_attr(feature = "schemars", schemars(schema_with = "nullable_date"))]
    pub weight_end: Option<Date>,
}

/// Actual immutable native inputs; market resources contain values, not handles.
pub struct FloatingWorkoutInputs<'a> {
    /// Complete original contract, purchase, information and legal notice state.
    pub workout: WorkoutInputs<'a>,
    /// Original eligible fixing policy and purchase-date accrued prefix choices.
    pub projection_context: &'a BondProjectionContext,
    /// Full original history, publication decisions and exact curves.
    pub market: SuppliedMarket<'a>,
    /// Exact index-projection revision, even when all observations are known.
    pub projection: &'a CurveKey,
    /// Separate exact same-currency collateral discount revision.
    pub discount: &'a CurveKey,
    /// Signed original-face notional/reference quantity, scaled once.
    pub position: &'a RiskPosition,
}

/// Complete original source and frozen cash retained independently from paths.
#[derive(Debug, Clone, Serialize)]
pub struct FloatingWorkoutSource {
    /// Original full contractual terms, including all profiles and legal paths.
    pub terms: BondTerms,
    /// Actual original purchase/information/notice state.
    pub context: WorkoutContext,
    /// Original settlement/fixing/known-at/publication policy and accrued cutoffs.
    pub projection_context: BondProjectionContext,
    /// Complete original projected cash and consumed known/forecast origins.
    pub cash: BondProjection,
    /// Actual exact frozen index-projection dependency.
    pub projection: CurveKey,
    /// Actual exact independent collateral discount dependency.
    pub discount: CurveKey,
    /// Signed original-face quantity/reference size.
    pub position: RiskPosition,
}

/// Projected dated cash is distinct from contractual YTM and model OAS.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FloatingWorkoutValue {
    /// Full conditional cash at an explicit frozen-projection decimal DM.
    Price(Box<BondValue>),
    /// Same-pricer inverse with unchanged domain/residual/work postconditions.
    DiscountMargin(Box<BondInverse>),
}
/// One conditional legal path; it does not select optimal exercise or post cash.
#[derive(Debug, Clone, Serialize)]
pub struct FloatingWorkoutPathQuote {
    /// Actual original provision/date and partial observation-weight cutoff.
    pub selection: FloatingWorkoutSelection,
    /// Separate notice/exercise/cash dates and full retirement economics.
    pub exercise: Option<WorkoutCashDetails>,
    /// Full native cash, price or DM inverse, including original diagnostics.
    pub quotation: FloatingWorkoutValue,
    /// Signed unrounded purchase consideration. Inverse uses original observed
    /// clean/dirty target, independently of approximate final solver repricing.
    pub consideration: ModelMoney,
}
/// Single complete floating conditional quotation over an immutable projection.
#[derive(Debug, Clone, Serialize)]
pub struct FloatingWorkoutQuotation {
    /// Original terms, context, exact dependencies and complete frozen cash.
    pub source: FloatingWorkoutSource,
    /// Original legal path and complete computed quotation.
    pub quote: FloatingWorkoutPathQuote,
}
/// Every eligible original path, with no invented market worst-DM selection.
#[derive(Debug, Clone, Serialize)]
pub struct FloatingWorkoutPaths {
    /// Complete original frozen input cash retained once.
    pub source: FloatingWorkoutSource,
    /// Original observed target applied to every path; not an OAS observation.
    pub target: BondPriceTarget,
    /// Unchanged original inverse and total date/cash-work bounds.
    pub settings: WorkoutSolveSettings,
    /// Maturity then original provision/date order, including holder puts.
    pub quotes: Vec<FloatingWorkoutPathQuote>,
    /// True when an already known timely notice forces its one legal path.
    pub forced_by_notice: bool,
    /// All actual future dates examined, including excluded notice/protection.
    pub examined_dates: u32,
    /// Actual full native same-cash price evaluations across all inverses.
    pub price_evaluations: usize,
    /// Actual individual cash discount evaluations across all complete paths.
    pub cash_discount_evaluations: u64,
    /// Original preflight reservation; no truncation or relaxed retry.
    pub reserved_cash_evaluations: u64,
    /// Original coupon/rate/observation entries reserved for both path admission
    /// and composition, sharing the supplied budget with cash discounting.
    /// Primitive-specific work bounds still apply; this is not a CPU-operation count.
    pub reserved_projection_terms: u64,
}

struct Prepared<'a> {
    native: PreparedFloatingWorkout,
    discount: &'a BuiltCurve,
    source: FloatingWorkoutSource,
    scale: f64,
}
fn prepare<'a>(input: &FloatingWorkoutInputs<'a>) -> invoice::CashQuotationResult<Prepared<'a>> {
    workout::purchase(&input.workout)?;
    let w = &input.workout;
    workout::context(w.contract, w.context)?;
    if w.exercise_calendar.0 != &w.contract.terms().calendars.exercise
        || input.projection_context.settlement != w.context.settlement
        || input.projection_context.on_payment != w.context.on_payment
        || input.projection_context.fixing.valuation_date != w.context.reference_date
    {
        return Err(invalid(
            "floating workout.context",
            "original calendar, purchase boundary and information date must agree",
        )
        .into());
    }
    let index = match &w.contract.terms().coupon {
        BondCoupon::Term(t) => &t.index_id,
        BondCoupon::Overnight(t) => &t.index_id,
        BondCoupon::Fixed => {
            return Err(WorkflowError::NotApplicable(
                "floating workout requires actual term or overnight cash",
            )
            .into())
        }
    };
    let projection = input
        .market
        .curves
        .get(input.projection)
        .map_err(WorkflowError::from)?;
    let discount = input
        .market
        .curves
        .get(input.discount)
        .map_err(WorkflowError::from)?;
    if projection.recipe().currency != w.contract.terms().currency
        || projection.recipe().reference_date != w.context.reference_date
        || !matches!(&projection.recipe().role, CurveRole::Projection { index_id } if index_id == index.as_str())
        || discount.recipe().reference_date != w.context.reference_date
        || discount.recipe().currency != w.contract.terms().currency
        || !matches!(&discount.recipe().role, CurveRole::Discount { collateral_currency } if *collateral_currency == w.contract.terms().currency)
    {
        return Err(invalid("floating workout.curves", "exact information date, currency, index projection and separate collateral discount roles required").into());
    }
    let scale = invoice::invoice_scale(w.contract.terms(), input.position)?;
    let requirements = with_publications(input.market.publications, |p| {
        w.contract
            .fixing_requirements(w.context.settlement, w.context.on_payment, p)
    })?;
    input.market.publications.require(&requirements)?;
    resolve_requirements(
        &requirements,
        input.market.fixings,
        input.projection_context.fixing,
    )?;
    let mut forecast = |request: &convex_bonds::contracts::ProjectionRequest| {
        crate::bond_contract::project_from_curve(request, w.contract.terms().currency, projection)
    };
    let native = w
        .contract
        .floating_workouts(
            input.projection_context,
            input.market.fixings,
            input.market.publications.callback(),
            BondProjectionPolicy::Supplied(&mut forecast),
        )
        .map_err(WorkflowError::from)?;
    let source = FloatingWorkoutSource {
        terms: w.contract.terms().clone(),
        context: w.context.clone(),
        projection_context: input.projection_context.clone(),
        cash: native.original().clone(),
        projection: input.projection.clone(),
        discount: input.discount.clone(),
        position: input.position.clone(),
    };
    Ok(Prepared {
        native,
        discount,
        source,
        scale,
    })
}
fn path(
    input: &FloatingWorkoutInputs<'_>,
    prepared: &Prepared<'_>,
    selection: &FloatingWorkoutSelection,
) -> WorkflowResult<(PreparedBondCash, Option<WorkoutCashDetails>)> {
    let w = &input.workout;
    let (cash, details) = match selection.path {
        WorkoutSelection::Maturity {} => {
            if selection.weight_end.is_some()
                || matches!(w.context.notice, WorkoutNotice::Notified { .. })
            {
                return Err(invalid(
                    "floating workout.selection",
                    "maturity cannot carry a partial cutoff or override supplied notice",
                ));
            }
            (prepared.native.original().clone(), None)
        }
        WorkoutSelection::Exercise { provision, date } => {
            let p = prepared.native.exercise(&WorkoutExercise {
                provision: provision as usize,
                date,
                calendar_id: w.exercise_calendar.0,
                calendar: w.exercise_calendar.1,
                weight_end: selection.weight_end,
            })?;
            match w.context.notice {
                WorkoutNotice::Unnotified { earliest_notice }
                    if p.dates.notice < earliest_notice =>
                {
                    return Err(invalid(
                        "workout.notice",
                        "required notice deadline precedes explicit earliest new notice",
                    ));
                }
                WorkoutNotice::Notified {
                    provision: other,
                    exercise,
                    notice_date,
                } if other != provision || exercise != date || notice_date > p.dates.notice => {
                    return Err(invalid(
                        "workout.notice",
                        "exact original provision/exercise and timely supplied notice required",
                    ));
                }
                _ => {}
            }
            let details = WorkoutCashDetails {
                dates: p.dates,
                right: w.contract.terms().exercise[provision as usize].right,
                outstanding_principal: p.outstanding_principal,
                redemption: p.redemption,
                accrued_cash: p.accrued_cash,
                settlement_cash: p.settlement_cash,
            };
            (p.cash, Some(details))
        }
    };
    Ok((
        PreparedBondCash::workout(w.contract, w.context.settlement, cash),
        details,
    ))
}
fn quote(
    input: &FloatingWorkoutInputs<'_>,
    prepared: &Prepared<'_>,
    selection: &FloatingWorkoutSelection,
    value: FloatingWorkoutValue,
    details: Option<WorkoutCashDetails>,
    dirty: f64,
) -> invoice::CashQuotationResult<FloatingWorkoutPathQuote> {
    Ok(FloatingWorkoutPathQuote {
        selection: selection.clone(),
        exercise: details,
        quotation: value,
        consideration: invoice::invoice_consideration(
            input.workout.contract.terms(),
            dirty,
            prepared.scale,
        )?,
    })
}
fn inverse_quote(
    input: &FloatingWorkoutInputs<'_>,
    prepared: &Prepared<'_>,
    selection: &FloatingWorkoutSelection,
    target: BondPriceTarget,
    settings: BondInverseSettings,
) -> invoice::CashQuotationResult<FloatingWorkoutPathQuote> {
    let (cash, details) = path(input, prepared, selection)?;
    let inverse = solve_bond_spread(
        &cash,
        prepared.discount,
        BondDiscountRole::Collateral,
        target,
        settings,
    )?;
    let dirty = match target {
        BondPriceTarget::Dirty(d) => d,
        BondPriceTarget::Clean(c) => c + inverse.valuation.accrued_per100,
    };
    quote(
        input,
        prepared,
        selection,
        FloatingWorkoutValue::DiscountMargin(Box::new(inverse)),
        details,
        dirty,
    )
}
/// Price actual conditional projected cash with ACT/365F continuous discount
/// margin on an exact collateral curve. Projection and eligible history freeze.
pub fn quote_floating_workout_price(
    input: &FloatingWorkoutInputs<'_>,
    selection: &FloatingWorkoutSelection,
    decimal_dm: f64,
) -> invoice::CashQuotationResult<FloatingWorkoutQuotation> {
    let prepared = prepare(input)?;
    let (cash, details) = path(input, &prepared, selection)?;
    let value = cash.price_spread(prepared.discount, BondDiscountRole::Collateral, decimal_dm)?;
    let dirty = value.dirty_per100;
    let quote = quote(
        input,
        &prepared,
        selection,
        FloatingWorkoutValue::Price(Box::new(value)),
        details,
        dirty,
    )?;
    Ok(FloatingWorkoutQuotation {
        source: prepared.source,
        quote,
    })
}
/// Invert the same frozen complete cash with original native closed-domain,
/// identification, mixed-sign, residual and work guards. This is not OAS/YTM.
pub fn quote_floating_workout_dm(
    input: &FloatingWorkoutInputs<'_>,
    selection: &FloatingWorkoutSelection,
    target: BondPriceTarget,
    settings: BondInverseSettings,
) -> invoice::CashQuotationResult<FloatingWorkoutQuotation> {
    let prepared = prepare(input)?;
    let quote = inverse_quote(input, &prepared, selection, target, settings)?;
    Ok(FloatingWorkoutQuotation {
        source: prepared.source,
        quote,
    })
}
/// Invert every eligible legal path over one original frozen projection. Only
/// shifted partial PayAccrued paths carry supplied weight choices; duplicates,
/// omissions and unused choices fail. Any path failure returns no accepted set.
pub fn quote_floating_workout_paths(
    input: &FloatingWorkoutInputs<'_>,
    target: BondPriceTarget,
    settings: WorkoutSolveSettings,
    shifted_prefixes: &[FloatingWorkoutSelection],
) -> invoice::CashQuotationResult<FloatingWorkoutPaths> {
    let (choices, examined, forced) =
        workout::eligible_workout_paths(&input.workout, settings.max_dates)?;
    if settings.max_cash_evaluations == 0
        || settings.max_cash_evaluations > MAX_WORKOUT_CASH_EVALUATIONS
        || shifted_prefixes.len() > choices.len()
    {
        return Err(invalid(
            "floating workout.settings",
            "bounded positive cash work and complete original cutoff choices required",
        )
        .into());
    }
    let prepared = prepare(input)?;
    let max_cash = prepared
        .native
        .original()
        .cashflows()
        .map_err(WorkflowError::from)?
        .len() as u64
        + 2;
    let reserved = (choices.len() as u64)
        .checked_mul(max_cash)
        .and_then(|n| n.checked_mul(u64::from(settings.inverse.max_iterations) + 8))
        .ok_or(CashflowError::BoundExceeded("workout cash evaluations"))
        .map_err(WorkflowError::from)?;
    if reserved > settings.max_cash_evaluations {
        return Err(
            WorkflowError::from(CashflowError::BoundExceeded("workout cash evaluations")).into(),
        );
    }
    let evidence_terms = prepared
        .native
        .original()
        .coupons
        .iter()
        .try_fold(0u64, |n, c| {
            n.checked_add(
                1 + c.rates.len() as u64
                    + c.overnight
                        .as_ref()
                        .map_or(0, |o| o.observations.len() as u64),
            )
        })
        .ok_or(CashflowError::BoundExceeded("workout projection terms"))
        .map_err(WorkflowError::from)?;
    let projection_terms = evidence_terms
        .checked_mul(choices.len() as u64)
        .and_then(|n| n.checked_mul(2))
        .ok_or(CashflowError::BoundExceeded("workout projection terms"))
        .map_err(WorkflowError::from)?;
    if reserved
        .checked_add(projection_terms)
        .is_none_or(|n| n > settings.max_cash_evaluations)
    {
        return Err(
            WorkflowError::from(CashflowError::BoundExceeded("workout total evaluations")).into(),
        );
    }
    let mut prefixes = BTreeMap::new();
    for prefix in shifted_prefixes {
        let WorkoutSelection::Exercise { provision, date } = prefix.path else {
            return Err(
                invalid("floating workout.prefixes", "unused maturity prefix choice").into(),
            );
        };
        let cutoff = prefix
            .weight_end
            .ok_or_else(|| invalid("floating workout.prefixes", "null shifted prefix choice"))?;
        if prefixes.insert((provision, date), cutoff).is_some() {
            return Err(invalid(
                "floating workout.prefixes",
                "duplicate shifted prefix choice",
            )
            .into());
        }
    }
    let mut selections = Vec::with_capacity(choices.len());
    // Admit every legal path and original partial prefix before any inverse.
    // Bad later/holder paths cannot hide behind an earlier no-root outcome.
    for choice in choices {
        let weight_end = match choice {
            WorkoutSelection::Maturity {} => None,
            WorkoutSelection::Exercise { provision, date } => prefixes.remove(&(provision, date)),
        };
        let selection = FloatingWorkoutSelection {
            path: choice,
            weight_end,
        };
        path(input, &prepared, &selection)?;
        selections.push(selection);
    }
    if !prefixes.is_empty() {
        return Err(invalid("floating workout.prefixes", "unused original prefix choice").into());
    }
    let mut quotes = Vec::with_capacity(selections.len());
    let mut evaluations = 0usize;
    let mut cash_evaluations = 0u64;
    for selection in selections {
        let q = inverse_quote(input, &prepared, &selection, target, settings.inverse)?;
        let FloatingWorkoutValue::DiscountMargin(v) = &q.quotation else {
            return Err(invalid("floating workout.quotation", "DM dispatch mismatch").into());
        };
        evaluations = evaluations
            .checked_add(v.price_evaluations)
            .ok_or(CashflowError::BoundExceeded("workout evaluations"))
            .map_err(WorkflowError::from)?;
        cash_evaluations = cash_evaluations
            .checked_add(
                (v.valuation
                    .cash
                    .cashflows()
                    .map_err(WorkflowError::from)?
                    .len() as u64)
                    .checked_mul(v.price_evaluations as u64)
                    .ok_or(CashflowError::BoundExceeded("workout cash evaluations"))
                    .map_err(WorkflowError::from)?,
            )
            .ok_or(CashflowError::BoundExceeded("workout cash evaluations"))
            .map_err(WorkflowError::from)?;
        quotes.push(q);
    }
    if cash_evaluations > reserved {
        return Err(invalid("floating workout.work", "original reserved work exceeded").into());
    }
    Ok(FloatingWorkoutPaths {
        source: prepared.source,
        target,
        settings,
        quotes,
        forced_by_notice: forced,
        examined_dates: examined,
        price_evaluations: evaluations,
        cash_discount_evaluations: cash_evaluations,
        reserved_cash_evaluations: reserved,
        reserved_projection_terms: projection_terms,
    })
}
