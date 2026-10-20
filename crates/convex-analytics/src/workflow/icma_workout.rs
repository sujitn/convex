//! Complete contextual fixed-workout quotations over the original legal paths.
use super::*;
use convex_bonds::contracts::ExerciseRight;
use serde::{Deserialize, Serialize};

/// Explicit whole-request preparation and cash-work ceilings, with no defaults.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaWorkoutSettings {
    /// Original examined legal-date ceiling, including excluded notice dates.
    pub max_dates: u32,
    /// Total of the original per-path ICMA reference-entry reservations.
    pub max_reference_evaluations: u64,
    /// Original conservative whole-request cash-discount reservation ceiling.
    pub max_cash_evaluations: u64,
}

/// Same-pricer inverse controls plus explicit whole-request admission bounds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaWorkoutSolveSettings {
    /// Original closed domain, native price residual and iteration ceiling.
    pub inverse: BondInverseSettings,
    /// Complete enumeration, preparation and cash-work bounds.
    pub paths: IcmaWorkoutSettings,
}

/// Full price for every original eligible maturity/call/put path, in legal order.
#[derive(Debug, Clone, Serialize)]
pub struct IcmaWorkoutPrices {
    /// Original purchase, information and actual notice facts.
    pub context: WorkoutContext,
    /// Supplied common annual decimal yield, independent of contractual coupon.
    pub decimal_yield: f64,
    /// Original whole-request bounds actually enforced.
    pub settings: IcmaWorkoutSettings,
    /// Every complete original cash/time/price/source/position/invoice result.
    pub quotes: Vec<IcmaQuotation>,
    /// True only for the one path forced by an actual supplied timely notice.
    pub forced_by_notice: bool,
    /// Original future contractual dates examined, including legal exclusions.
    pub examined_dates: u32,
    /// Actual complete cash valuations, one for each admitted path.
    pub price_evaluations: usize,
    /// Actual entitled cash discount operations across every path.
    pub cash_discount_evaluations: u64,
    /// Conservative original source-cash-plus-two reservation across all paths.
    pub reserved_cash_evaluations: u64,
    /// Total original per-path one-time ICMA preparation reservations.
    pub reserved_reference_evaluations: u64,
}

/// Complete all-path yields and the original defined worst-call selection.
#[derive(Debug, Clone, Serialize)]
pub struct IcmaWorkoutWorst {
    /// Original purchase, information and actual notice facts.
    pub context: WorkoutContext,
    /// ORIGINAL observed clean/dirty original-face price shared by every inverse.
    pub target: BondPriceTarget,
    /// Original closed inverse and whole-request bounds, without retries.
    pub settings: IcmaWorkoutSolveSettings,
    /// Every complete quotation, including unselected puts and delayed claims.
    pub quotes: Vec<IcmaQuotation>,
    /// Minimum call/maturity yield, retaining the first exact numerical tie.
    /// An actual notice instead forces its one complete original path.
    pub selected_index: usize,
    /// True only for an actual supplied timely notice, including a forced put.
    pub forced_by_notice: bool,
    /// Original future contractual dates examined, including legal exclusions.
    pub examined_dates: u32,
    /// Actual original inverse valuations across ALL eligible paths.
    pub price_evaluations: usize,
    /// Actual cash discount operations across every original inverse.
    pub cash_discount_evaluations: u64,
    /// Conservative original cash-count and unchanged inverse-work reservation.
    pub reserved_cash_evaluations: u64,
    /// Total original per-path one-time ICMA preparation reservations.
    pub reserved_reference_evaluations: u64,
}

// Private checked state: every legal path and its full source/time context is
// admitted before the first numerical price or inverse endpoint is evaluated.
struct PreparedPaths {
    paths: Vec<PreparedIcmaQuotation>,
    examined_dates: u32,
    forced_by_notice: bool,
    reserved_cash_evaluations: u64,
    reserved_reference_evaluations: u64,
}

fn prepare(
    input: &IcmaQuotationInputs<'_>,
    context: &WorkoutContext,
    settings: IcmaWorkoutSettings,
    evaluations_per_path: u64,
) -> WorkflowResult<PreparedPaths> {
    if settings.max_dates == 0
        || settings.max_dates > MAX_WORKOUT_DATES
        || settings.max_cash_evaluations == 0
        || settings.max_cash_evaluations > MAX_WORKOUT_CASH_EVALUATIONS
        || settings.max_reference_evaluations == 0
        || settings.max_reference_evaluations > MAX_ICMA_REFERENCE_EVALUATIONS
    {
        return Err(invalid(
            "ICMA workout settings",
            "original positive bounded date, total cash and total reference ceilings required",
        ));
    }
    let workout = WorkoutInputs {
        contract: input.contract,
        context,
        exercise_calendar: input.exercise_calendar,
        purchase_calendar: input.purchase_calendar,
    };
    let (choices, examined_dates, forced_by_notice) =
        super::workout::eligible_workout_paths(&workout, settings.max_dates)?;
    // Same original source cash count and two possible exercise components as
    // quote_workout_worst; no coupon-date approximation or trimmed path count.
    let original = input
        .contract
        .fixed_projection(context.settlement, context.on_payment)?;
    let max_cash = original.cashflows()?.len() as u64 + 2;
    let reserved_cash_evaluations = (choices.len() as u64)
        .checked_mul(max_cash)
        .and_then(|n| n.checked_mul(evaluations_per_path))
        .ok_or(CashflowError::BoundExceeded(
            "ICMA workout cash evaluations",
        ))?;
    if reserved_cash_evaluations > settings.max_cash_evaluations {
        return Err(CashflowError::BoundExceeded("ICMA workout cash evaluations").into());
    }
    let mut paths = Vec::with_capacity(choices.len());
    let mut reserved_reference_evaluations = 0_u64;
    for selection in choices {
        let path = PreparedIcmaQuotation::new(
            input,
            &IcmaCashSelection::Workout {
                context: context.clone(),
                selection,
            },
        )?;
        reserved_reference_evaluations = reserved_reference_evaluations
            .checked_add(path.reserved_reference_evaluations())
            .ok_or(CashflowError::BoundExceeded(
                "ICMA workout reference evaluations",
            ))?;
        if reserved_reference_evaluations > settings.max_reference_evaluations {
            return Err(CashflowError::BoundExceeded("ICMA workout reference evaluations").into());
        }
        paths.push(path);
    }
    Ok(PreparedPaths {
        paths,
        examined_dates,
        forced_by_notice,
        reserved_cash_evaluations,
        reserved_reference_evaluations,
    })
}

/// Price every eligible original legal fixed path under one contextual yield.
/// Any source, path, reference, work or price failure exposes no accepted subset.
pub fn quote_icma_workout_prices(
    input: &IcmaQuotationInputs<'_>,
    context: &WorkoutContext,
    decimal_yield: f64,
    settings: IcmaWorkoutSettings,
) -> WorkflowResult<IcmaWorkoutPrices> {
    let prepared = prepare(input, context, settings, 1)?;
    let mut quotes = Vec::with_capacity(prepared.paths.len());
    let mut cash_discount_evaluations = 0_u64;
    for path in &prepared.paths {
        let quote = path.quote_price(decimal_yield)?;
        cash_discount_evaluations = cash_discount_evaluations
            .checked_add(path.cashflow_count()?)
            .ok_or(CashflowError::BoundExceeded(
                "ICMA workout cash evaluations",
            ))?;
        quotes.push(quote);
    }
    if cash_discount_evaluations > prepared.reserved_cash_evaluations {
        return Err(CashflowError::BoundExceeded("ICMA workout reserved cash evaluations").into());
    }
    Ok(IcmaWorkoutPrices {
        context: context.clone(),
        decimal_yield,
        settings,
        price_evaluations: quotes.len(),
        quotes,
        forced_by_notice: prepared.forced_by_notice,
        examined_dates: prepared.examined_dates,
        cash_discount_evaluations,
        reserved_cash_evaluations: prepared.reserved_cash_evaluations,
        reserved_reference_evaluations: prepared.reserved_reference_evaluations,
    })
}

/// Invert EVERY original legal path and select its defined worst-call yield.
/// Puts are fully quoted but cannot lower an unnotified call/maturity minimum.
/// No failed path is skipped; no expanded bracket or option value is inferred.
pub fn quote_icma_workout_worst(
    input: &IcmaQuotationInputs<'_>,
    context: &WorkoutContext,
    target: BondPriceTarget,
    settings: IcmaWorkoutSolveSettings,
) -> WorkflowResult<IcmaWorkoutWorst> {
    let prepared = prepare(
        input,
        context,
        settings.paths,
        u64::from(settings.inverse.max_iterations) + 8,
    )?;
    let mut quotes = Vec::with_capacity(prepared.paths.len());
    let mut selected_index = None;
    let mut selected_yield = f64::INFINITY;
    let mut price_evaluations = 0_usize;
    let mut cash_discount_evaluations = 0_u64;
    for (i, path) in prepared.paths.iter().enumerate() {
        let quote = path.quote_yield(target, settings.inverse)?;
        let WorkoutValue::Yield(inverse) = &quote.quotation else {
            return Err(invalid(
                "ICMA workout quotation",
                "original yield dispatch required",
            ));
        };
        price_evaluations = price_evaluations
            .checked_add(inverse.price_evaluations)
            .ok_or(CashflowError::BoundExceeded("ICMA workout evaluations"))?;
        cash_discount_evaluations = cash_discount_evaluations
            .checked_add(
                path.cashflow_count()?
                    .checked_mul(inverse.price_evaluations as u64)
                    .ok_or(CashflowError::BoundExceeded(
                        "ICMA workout cash evaluations",
                    ))?,
            )
            .ok_or(CashflowError::BoundExceeded(
                "ICMA workout cash evaluations",
            ))?;
        if (prepared.forced_by_notice
            || quote
                .exercise
                .as_ref()
                .is_none_or(|exercise| exercise.right == ExerciseRight::Call))
            && inverse.decimal_parameter < selected_yield
        {
            selected_yield = inverse.decimal_parameter;
            selected_index = Some(i);
        }
        quotes.push(quote);
    }
    if cash_discount_evaluations > prepared.reserved_cash_evaluations {
        return Err(CashflowError::BoundExceeded("ICMA workout reserved cash evaluations").into());
    }
    Ok(IcmaWorkoutWorst {
        context: context.clone(),
        target,
        settings,
        quotes,
        selected_index: selected_index.ok_or_else(|| {
            invalid(
                "ICMA workout selection",
                "complete eligible call or maturity required",
            )
        })?,
        forced_by_notice: prepared.forced_by_notice,
        examined_dates: prepared.examined_dates,
        price_evaluations,
        cash_discount_evaluations,
        reserved_cash_evaluations: prepared.reserved_cash_evaluations,
        reserved_reference_evaluations: prepared.reserved_reference_evaluations,
    })
}
