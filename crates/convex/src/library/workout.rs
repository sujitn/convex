//! Full native workout quotation composition; no adapter financial formulas.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::workflow as w;
use convex_rates::ReferencePin;

/// Independent exact reference and observed-quote revisions with full bond terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WorkoutInput {
    /// Actual supplied security reference identity and revision.
    pub reference: ReferencePin,
    /// Independent source revision for the observed price or quoted yield.
    pub quote_revision: ReferencePin,
    /// Complete original profiles, exercise/notice and calendar revisions.
    pub bond: BondInput,
}
/// Original same-cash quotation and inverse operations with disjoint controls.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkoutOperation {
    /// Complete original workout price at an explicit annualized dated yield.
    Price {
        /// Original maturity or exact contractual exercise.
        selection: w::WorkoutSelection,
        /// Annualized decimal yield, not rounded percent or model OAS.
        decimal_yield: f64,
    },
    /// Complete original single-workout cash yield with native postcondition.
    Yield {
        /// Original maturity or exact contractual exercise.
        selection: w::WorkoutSelection,
        /// Original clean/dirty price per100 ORIGINAL face.
        target: w::BondPriceTarget,
        /// Unchanged closed domain, native residual and solver work controls.
        settings: w::BondInverseSettings,
    },
    /// All contractual dates; issuer-call/maturity minimum, puts reported separately.
    Worst {
        /// One observed clean/dirty original-face price for every candidate.
        target: w::BondPriceTarget,
        /// Native inverse and explicit total enumeration/cash work bounds.
        settings: w::WorkoutSolveSettings,
    },
}
pub(super) fn execute(
    input: &WorkoutInput,
    context: &w::WorkoutContext,
    convention: w::DatedYieldConvention,
    action: &WorkoutOperation,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let contract = r.bond(&input.bond)?;
    let calendar = r.calendar(&input.bond.calendars.exercise)?;
    let purchase = r.calendar(&input.bond.calendars.payment)?;
    let bound = w::WorkoutInputs {
        contract: &contract,
        context,
        exercise_calendar: (&contract.terms().calendars.exercise, calendar),
        purchase_calendar: (&contract.terms().calendars.payment, purchase),
    };
    let quotation = match action {
        WorkoutOperation::Price {
            selection,
            decimal_yield,
        } => value(w::quote_workout_price(
            &bound,
            selection,
            convention,
            *decimal_yield,
        )?)?,
        WorkoutOperation::Yield {
            selection,
            target,
            settings,
        } => value(w::quote_workout_yield(
            &bound, selection, convention, *target, *settings,
        )?)?,
        WorkoutOperation::Worst { target, settings } => value(w::quote_workout_worst(
            &bound, convention, *target, *settings,
        )?)?,
    };
    Ok(
        serde_json::json!({"reference":input.reference,"quote_revision":input.quote_revision,"calendars":input.bond.calendars,"quotation":quotation}),
    )
}
