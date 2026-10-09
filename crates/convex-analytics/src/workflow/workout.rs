//! Full fixed-cash workout quotations, separate from option valuation and events.
use super::*;
use convex_bonds::{
    contracts::{
        BondContract, BondProjection, BondTerms, ExerciseDates, ExerciseRight, ExerciseStyle,
        WorkoutProjection,
    },
    types::CalendarId,
};
use convex_core::{calendars::Calendar, cashflows::PaymentDateEntitlement};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Maximum examined future contractual exercise dates in one quotation.
pub const MAX_WORKOUT_DATES: u32 = 16_384;
/// Maximum reserved individual cash discount evaluations, including inverses.
pub const MAX_WORKOUT_CASH_EVALUATIONS: u64 = 67_108_864;

/// Full legal workout choice; a maturity date is not an arbitrary call date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkoutSelection {
    /// Original maturity cash, conditional on no early exercise.
    Maturity {},
    /// Exact original provision and permitted early exercise date.
    Exercise {
        /// Source index in the original full exercise terms.
        provision: u32,
        /// Exact contractual exercise date, not its later cash date.
        date: Date,
    },
}
/// Caller-supplied notice knowledge and policy, without a system clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkoutNotice {
    /// No outstanding notice; new notice cannot precede this explicit date.
    Unnotified {
        /// Policy date, e.g. the caller's applicable day-after-trade rule.
        earliest_notice: Date,
    },
    /// A supplied timely original notice forces this redemption path.
    Notified {
        /// Exact original provision.
        provision: u32,
        /// Exact future contractual exercise date.
        exercise: Date,
        /// Actual supplied date the notice was issued.
        notice_date: Date,
    },
}
/// Quotation ownership, information origin and explicit purchase cash boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WorkoutContext {
    /// Actual supplied information date, independent of purchase settlement.
    pub reference_date: Date,
    /// Original purchase trade date, checked under contractual settlement terms.
    pub trade_date: Date,
    /// Purchase settlement; every exercise must be strictly later.
    pub settlement: Date,
    /// Explicit ownership of same-day scheduled cash.
    pub on_payment: PaymentDateEntitlement,
    /// Original notice facts or future-notice policy.
    pub notice: WorkoutNotice,
}
/// All-date quotation bounds; native inverse controls are never relaxed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WorkoutSolveSettings {
    /// Original closed yield domain and native price/parameter/work tolerances.
    pub inverse: BondInverseSettings,
    /// Positive ceiling on examined future contractual exercise dates.
    pub max_dates: u32,
    /// Positive ceiling on reserved individual cash discount evaluations.
    pub max_cash_evaluations: u64,
}
/// Original exercise economics; complete scheduled source cash is retained once.
#[derive(Debug, Clone, Serialize)]
pub struct WorkoutCashDetails {
    /// Actual separate last notice, exercise and cash dates.
    pub dates: ExerciseDates,
    /// Call or put, retained independently from yield selection.
    pub right: ExerciseRight,
    /// Principal after same-day scheduled reductions, before early retirement.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub outstanding_principal: Decimal,
    /// Unrounded contracted redemption price times remaining principal.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub redemption: Decimal,
    /// Separate original partial coupon, zero for included-accrued terms.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub accrued_cash: Decimal,
    /// Once-rounded early redemption cash, excluding delayed completed claims.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub settlement_cash: Decimal,
}
/// Original native full-cash price or closed-domain yield inverse.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum WorkoutValue {
    /// Price at the caller's annualized dated-cash yield.
    Price(Box<BondValue>),
    /// Accepted original same-pricer inverse and final native residual.
    Yield(Box<BondInverse>),
}
/// Complete one-path conditional quotation, never an option price or event.
#[derive(Debug, Clone, Serialize)]
pub struct WorkoutQuotation {
    /// Full original contract, including all profiles and legal exercise terms.
    pub source: BondTerms,
    /// Original information/purchase/notice state.
    pub context: WorkoutContext,
    /// Exact source provision/date or original maturity.
    pub selection: WorkoutSelection,
    /// Original cash, entitlement and accrual before hypothetical early exercise.
    pub source_cash: BondProjection,
    /// Actual early redemption; absent for conditional maturity cash.
    pub exercise: Option<WorkoutCashDetails>,
    /// Original native price or inverse, including complete used workout cash.
    pub quotation: WorkoutValue,
}
pub(super) struct PreparedWorkout {
    pub(super) cash: PreparedBondCash,
    pub(super) source_cash: BondProjection,
    pub(super) exercise: Option<WorkoutCashDetails>,
}
/// Complete checked immutable native inputs; calendar identity is explicit.
pub struct WorkoutInputs<'a> {
    /// Original checked full bond contract.
    pub contract: &'a BondContract,
    /// Original information, purchase and notice state.
    pub context: &'a WorkoutContext,
    /// Exact original exercise-calendar identity and actual supplied calendar.
    pub exercise_calendar: (&'a CalendarId, &'a dyn Calendar),
    /// Original purchase settlement calendar identity and actual supplied data.
    pub purchase_calendar: (&'a CalendarId, &'a dyn Calendar),
}
pub(super) fn purchase(input: &WorkoutInputs<'_>) -> WorkflowResult<()> {
    let t = input.contract.terms();
    let c = input.context;
    if input.purchase_calendar.0 != &t.calendars.payment
        || c.trade_date < t.issue_date
        || c.trade_date > c.settlement
        || !(1..=9999).contains(&c.trade_date.year())
    {
        return Err(invalid(
            "workout.purchase",
            "original purchase calendar and valid trade-to-settlement ordering required",
        ));
    }
    let cal = input.purchase_calendar.1;
    let expected = cal
        .adjust(
            t.settlement_lag.apply(c.trade_date, cal)?,
            t.settlement_adjustment,
        )
        .map_err(|e| invalid("workout.purchase", e.to_string()))?;
    if expected != c.settlement {
        return Err(invalid(
            "workout.purchase",
            "supplied purchase settlement differs from original contractual lag/calendar",
        ));
    }
    Ok(())
}
pub(super) fn context(contract: &BondContract, c: &WorkoutContext) -> WorkflowResult<()> {
    if !(1..=9999).contains(&c.reference_date.year())
        || c.reference_date < contract.terms().issue_date
        || c.reference_date > c.settlement
        || c.settlement >= contract.terms().schedule.end_date
    {
        return Err(invalid("workout.context", "valid information date between issue and future purchase settlement, with live contractual maturity required"));
    }
    match c.notice {
        WorkoutNotice::Unnotified { earliest_notice } => {
            if !(1..=9999).contains(&earliest_notice.year()) || earliest_notice < c.reference_date {
                return Err(invalid(
                    "workout.notice",
                    "explicit earliest new notice cannot precede information date",
                ));
            }
        }
        WorkoutNotice::Notified { notice_date, .. } => {
            if notice_date < contract.terms().issue_date || notice_date > c.reference_date {
                return Err(invalid(
                    "workout.notice",
                    "supplied actual notice must be known at information date and after issue",
                ));
            }
        }
    }
    Ok(())
}
pub(super) fn prepare(
    contract: &BondContract,
    c: &WorkoutContext,
    choice: &WorkoutSelection,
    calendar_id: &CalendarId,
    calendar: &dyn Calendar,
) -> WorkflowResult<PreparedWorkout> {
    context(contract, c)?;
    if calendar_id != &contract.terms().calendars.exercise {
        return Err(invalid(
            "workout.calendar",
            "exact original exercise calendar required",
        ));
    }
    match choice {
        WorkoutSelection::Maturity {} => {
            if matches!(c.notice, WorkoutNotice::Notified { .. }) {
                return Err(invalid(
                    "workout.selection",
                    "supplied notice forces its exact early redemption path",
                ));
            }
            let source_cash = contract.fixed_projection(c.settlement, c.on_payment)?;
            Ok(PreparedWorkout {
                cash: PreparedBondCash::workout(contract, c.settlement, source_cash.clone()),
                source_cash,
                exercise: None,
            })
        }
        WorkoutSelection::Exercise { provision, date } => {
            let WorkoutProjection {
                original,
                cash,
                dates,
                outstanding_principal,
                redemption,
                accrued_cash,
                settlement_cash,
            } = contract.fixed_workout_projection(
                c.settlement,
                c.on_payment,
                *provision as usize,
                *date,
                calendar_id,
                calendar,
            )?;
            match c.notice {
                WorkoutNotice::Unnotified { earliest_notice } if dates.notice < earliest_notice => {
                    return Err(invalid(
                        "workout.notice",
                        "required notice deadline precedes explicit earliest new notice",
                    ));
                }
                WorkoutNotice::Notified {
                    provision: p,
                    exercise,
                    notice_date,
                } if p != *provision || exercise != *date || notice_date > dates.notice => {
                    return Err(invalid(
                        "workout.notice",
                        "exact original provision/exercise and timely supplied notice required",
                    ));
                }
                _ => {}
            }
            let right = contract.terms().exercise[*provision as usize].right;
            Ok(PreparedWorkout {
                cash: PreparedBondCash::workout(contract, c.settlement, cash),
                source_cash: original,
                exercise: Some(WorkoutCashDetails {
                    dates,
                    right,
                    outstanding_principal,
                    redemption,
                    accrued_cash,
                    settlement_cash,
                }),
            })
        }
    }
}
fn output(
    contract: &BondContract,
    c: &WorkoutContext,
    choice: &WorkoutSelection,
    p: PreparedWorkout,
    v: WorkoutValue,
) -> WorkoutQuotation {
    WorkoutQuotation {
        source: contract.terms().clone(),
        context: c.clone(),
        selection: choice.clone(),
        source_cash: p.source_cash,
        exercise: p.exercise,
        quotation: v,
    }
}
/// Quote complete permitted workout cash at an explicit dated yield. Optional
/// model valuation and normal workflow plain-pricer restrictions are unchanged.
pub fn quote_workout_price(
    input: &WorkoutInputs<'_>,
    choice: &WorkoutSelection,
    convention: DatedYieldConvention,
    decimal_yield: f64,
) -> WorkflowResult<WorkoutQuotation> {
    purchase(input)?;
    let WorkoutInputs {
        contract,
        context: c,
        exercise_calendar: (calendar_id, calendar),
        ..
    } = input;
    let p = prepare(contract, c, choice, calendar_id, *calendar)?;
    let v = p.cash.price_yield(decimal_yield, convention)?;
    Ok(output(
        contract,
        c,
        choice,
        p,
        WorkoutValue::Price(Box::new(v)),
    ))
}
/// Invert the same complete workout cash under unchanged native quote bounds.
pub fn quote_workout_yield(
    input: &WorkoutInputs<'_>,
    choice: &WorkoutSelection,
    convention: DatedYieldConvention,
    target: BondPriceTarget,
    settings: BondInverseSettings,
) -> WorkflowResult<WorkoutQuotation> {
    purchase(input)?;
    let WorkoutInputs {
        contract,
        context: c,
        exercise_calendar: (calendar_id, calendar),
        ..
    } = input;
    let p = prepare(contract, c, choice, calendar_id, *calendar)?;
    let v = solve_bond_yield(&p.cash, convention, target, settings)?;
    Ok(output(
        contract,
        c,
        choice,
        p,
        WorkoutValue::Yield(Box::new(v)),
    ))
}
/// Compact per-path diagnostics. Full source terms/cash, convention, observed
/// target and inverse settings are retained once; no full path is discarded.
#[derive(Debug, Clone, Serialize)]
pub struct WorkoutYieldSummary {
    /// Exact source provision/date or conditional maturity.
    pub selection: WorkoutSelection,
    /// Actual early exercise cash and legal dates.
    pub exercise: Option<WorkoutCashDetails>,
    /// Annualized decimal yield under the common declared convention.
    pub decimal_yield: f64,
    /// Original signed native dirty-price residual.
    pub native_residual: f64,
    /// Original-face clean price, after original purchase accrual/rebate.
    pub clean_per100: f64,
    /// Repriced original-face dirty value, preserving observed target separately.
    pub dirty_per100: f64,
    /// Original purchase accrued/rebate, in original-face units.
    pub accrued_per100: f64,
    /// Actual original bounded solver iterations.
    pub iterations: u32,
    /// Actual original pricer evaluations.
    pub price_evaluations: usize,
}
/// Complete all-date conditional quotation; puts remain separate holder choices.
#[derive(Debug, Clone, Serialize)]
pub struct WorkoutWorstQuotation {
    /// Full original contracts/profiles/legal terms and original unpaid cash.
    pub selected: WorkoutQuotation,
    /// Declared common annualized dated-cash convention.
    pub convention: DatedYieldConvention,
    /// Unchanged observed clean/dirty original-face target.
    pub target: BondPriceTarget,
    /// Original inverse/domain/date/cash-work bounds actually enforced.
    pub settings: WorkoutSolveSettings,
    /// Maturity first, then original provision order and actual date order.
    pub quotes: Vec<WorkoutYieldSummary>,
    /// Minimum among issuer calls and maturity, or forced supplied notice path.
    /// Holder puts are reported in quotes and never lower this worst-call yield.
    pub selected_index: usize,
    /// True only when an already supplied timely notice forced one path.
    pub forced_by_notice: bool,
    /// Actual future contractual dates inspected, including notice exclusions.
    pub examined_dates: u32,
    /// Actual original pricer work across ALL quoted paths.
    pub price_evaluations: usize,
    /// Actual individual cash discount operations across every original inverse.
    pub cash_discount_evaluations: u64,
    /// Preflight conservative upper work reservation; no late relaxed retry.
    pub reserved_cash_evaluations: u64,
}
/// Enumerate every permitted future calendar date in continuous windows. A full
/// fixed-price call/put quote is not a model exercise prediction or Street quote.
/// Any resource, path, inverse or final residual failure returns no partial result.
pub fn quote_workout_worst(
    input: &WorkoutInputs<'_>,
    convention: DatedYieldConvention,
    target: BondPriceTarget,
    settings: WorkoutSolveSettings,
) -> WorkflowResult<WorkoutWorstQuotation> {
    purchase(input)?;
    let WorkoutInputs {
        contract,
        context: c,
        ..
    } = input;
    context(contract, c)?;
    if settings.max_dates == 0
        || settings.max_dates > MAX_WORKOUT_DATES
        || settings.max_cash_evaluations == 0
        || settings.max_cash_evaluations > MAX_WORKOUT_CASH_EVALUATIONS
    {
        return Err(invalid(
            "workout.settings",
            "positive bounded contractual-date and cash-evaluation budgets required",
        ));
    }
    let (choices, examined, forced) = eligible_workout_paths(input, settings.max_dates)?;
    let original = contract.fixed_projection(c.settlement, c.on_payment)?;
    let max_cash = original.cashflows()?.len() as u64 + 2;
    let reserved = (choices.len() as u64)
        .checked_mul(max_cash)
        .and_then(|n| n.checked_mul(u64::from(settings.inverse.max_iterations) + 8))
        .ok_or(CashflowError::BoundExceeded("workout cash evaluations"))?;
    if reserved > settings.max_cash_evaluations {
        return Err(CashflowError::BoundExceeded("workout cash evaluations").into());
    }
    let mut quotes = Vec::with_capacity(choices.len());
    let mut selected: Option<WorkoutQuotation> = None;
    let mut selected_index = 0;
    let mut selected_yield = f64::INFINITY;
    let mut evaluations = 0usize;
    let mut cash_evaluations = 0u64;
    for (i, choice) in choices.iter().enumerate() {
        let q = quote_workout_yield(input, choice, convention, target, settings.inverse)?;
        let WorkoutValue::Yield(inverse) = &q.quotation else {
            return Err(invalid("workout.quotation", "yield dispatch mismatch"));
        };
        evaluations = evaluations
            .checked_add(inverse.price_evaluations)
            .ok_or(CashflowError::BoundExceeded("workout evaluations"))?;
        cash_evaluations = cash_evaluations
            .checked_add(
                (inverse.valuation.cash.cashflows()?.len() as u64)
                    .checked_mul(inverse.price_evaluations as u64)
                    .ok_or(CashflowError::BoundExceeded("workout cash evaluations"))?,
            )
            .ok_or(CashflowError::BoundExceeded("workout cash evaluations"))?;
        quotes.push(WorkoutYieldSummary {
            selection: choice.clone(),
            exercise: q.exercise.clone(),
            decimal_yield: inverse.decimal_parameter,
            native_residual: inverse.native_residual,
            clean_per100: inverse.valuation.clean_per100,
            dirty_per100: inverse.valuation.dirty_per100,
            accrued_per100: inverse.valuation.accrued_per100,
            iterations: inverse.iterations,
            price_evaluations: inverse.price_evaluations,
        });
        if (forced
            || q.exercise
                .as_ref()
                .is_none_or(|e| e.right == ExerciseRight::Call))
            && inverse.decimal_parameter < selected_yield
        {
            selected_yield = inverse.decimal_parameter;
            selected_index = i;
            selected = Some(q);
        }
    }
    if cash_evaluations > reserved {
        return Err(CashflowError::BoundExceeded("workout reserved cash evaluations").into());
    }
    Ok(WorkoutWorstQuotation {
        selected: selected.ok_or_else(|| {
            invalid(
                "workout.selection",
                "no complete eligible call or maturity quotation",
            )
        })?,
        convention,
        target,
        settings,
        quotes,
        selected_index,
        forced_by_notice: forced,
        examined_dates: examined,
        price_evaluations: evaluations,
        cash_discount_evaluations: cash_evaluations,
        reserved_cash_evaluations: reserved,
    })
}

// Shared original all-date legal-path enumeration, without choosing a market
// meaning for the resulting measures. Purchase/context admission runs first.
pub(super) fn eligible_workout_paths(
    input: &WorkoutInputs<'_>,
    max_dates: u32,
) -> WorkflowResult<(Vec<WorkoutSelection>, u32, bool)> {
    purchase(input)?;
    let contract = input.contract;
    let c = input.context;
    context(contract, c)?;
    let (id, calendar) = input.exercise_calendar;
    if id != &contract.terms().calendars.exercise || max_dates == 0 || max_dates > MAX_WORKOUT_DATES
    {
        return Err(invalid(
            "workout.paths",
            "original exercise calendar and bounded positive date budget required",
        ));
    }
    let mut choices = Vec::new();
    let mut examined = 0u32;
    let mut visit = |i: usize, date: Date| -> WorkflowResult<()> {
        if date <= c.settlement || date >= contract.terms().schedule.end_date {
            return Ok(());
        }
        examined = examined
            .checked_add(1)
            .ok_or(CashflowError::BoundExceeded("workout dates"))?;
        if examined > max_dates {
            return Err(CashflowError::BoundExceeded("workout dates").into());
        }
        let t = &contract.terms().exercise[i];
        if t.protection_end.is_some_and(|end| date < end) {
            return Ok(());
        }
        let d = t.dates(date, calendar)?;
        if let WorkoutNotice::Unnotified { earliest_notice } = c.notice {
            if d.notice < earliest_notice {
                return Ok(());
            }
        }
        choices.push(WorkoutSelection::Exercise {
            provision: i as u32,
            date,
        });
        Ok(())
    };
    let forced = matches!(c.notice, WorkoutNotice::Notified { .. });
    if let WorkoutNotice::Notified {
        provision,
        exercise,
        ..
    } = c.notice
    {
        choices.push(WorkoutSelection::Exercise {
            provision,
            date: exercise,
        });
        examined = 1;
    } else {
        // Count actual candidate dates before any inverse; no coupon-date proxy.
        for (i, t) in contract.terms().exercise.iter().enumerate() {
            match &t.style {
                ExerciseStyle::European { date } => visit(i, *date)?,
                ExerciseStyle::Bermudan { dates } => {
                    for date in dates {
                        visit(i, *date)?;
                    }
                }
                ExerciseStyle::American { start, end } => {
                    let next = c
                        .settlement
                        .checked_add_days(1)
                        .map_err(|e| invalid("workout.date", e.to_string()))?;
                    let last = contract
                        .terms()
                        .schedule
                        .end_date
                        .checked_add_days(-1)
                        .map_err(|e| invalid("workout.date", e.to_string()))?;
                    let mut date = (*start).max(next);
                    let end = (*end).min(last);
                    while date <= end {
                        visit(i, date)?;
                        if date == end {
                            break;
                        }
                        date = date
                            .checked_add_days(1)
                            .map_err(|e| invalid("workout.date", e.to_string()))?;
                    }
                }
            }
        }
        choices.insert(0, WorkoutSelection::Maturity {});
    }
    Ok((choices, examined, forced))
}
