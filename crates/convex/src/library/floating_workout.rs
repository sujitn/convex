//! Thin full-term floating conditional cash dispatch, without financial formulas.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::workflow as w;
use convex_bonds::contracts::BondProjectionContext;
use convex_rates::ReferencePin;

/// Original references and full independently versioned floating bond economics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FloatingWorkoutInput {
    /// Original full contract, calendar pins and independent quote/reference pins.
    pub security: WorkoutInput,
    /// Exact original projection context, history eligibility and accrued cutoffs.
    pub projection_context: BondProjectionContext,
    /// Actual immutable index-projection revision, distinct from discount.
    pub projection: CurveKey,
    /// Actual immutable same-currency collateral discount revision.
    pub discount: CurveKey,
    /// Exact signed original-face quantity and currency; not outstanding factor.
    pub position: convex_analytics::quote_risk::RiskPosition,
    /// Original caller-assigned policy/source revision for the conditional quote.
    pub quotation_policy: ReferencePin,
}
/// Disjoint supplied conditional price, single DM and complete path operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FloatingWorkoutOperation {
    /// Full conditional original cash at a frozen-projection annual decimal DM.
    Price {
        /// Actual legal path and explicit partial observation-weight cutoff.
        selection: w::FloatingWorkoutSelection,
        /// Annual decimal DM; no percent/basis-point or OAS substitution.
        decimal_dm: f64,
    },
    /// Original same-cash closed-domain inverse with native residual/work guards.
    DiscountMargin {
        /// Actual legal path and explicit partial observation-weight cutoff.
        selection: w::FloatingWorkoutSelection,
        /// Original observed clean/dirty original-face target.
        target: w::BondPriceTarget,
        /// Original closed parameter/price/work controls, never relaxed.
        settings: w::BondInverseSettings,
    },
    /// Every eligible actual original date and put/call; no worst-DM selection.
    Paths {
        /// Original common observed clean/dirty original-face target.
        target: w::BondPriceTarget,
        /// Original closed inverse, date and total cash-work policy.
        settings: w::WorkoutSolveSettings,
        /// Exact eligible partial shifted PayAccrued paths only; no defaults.
        shifted_prefixes: Vec<w::FloatingWorkoutSelection>,
    },
}
pub(super) fn execute(
    input: &FloatingWorkoutInput,
    context: &w::WorkoutContext,
    action: &FloatingWorkoutOperation,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let bond = &input.security.bond;
    let contract = r.bond(bond)?;
    let bound = w::FloatingWorkoutInputs {
        workout: w::WorkoutInputs {
            contract: &contract,
            context,
            exercise_calendar: (
                &contract.terms().calendars.exercise,
                r.calendar(&bond.calendars.exercise)?,
            ),
            purchase_calendar: (
                &contract.terms().calendars.payment,
                r.calendar(&bond.calendars.payment)?,
            ),
        },
        projection_context: &input.projection_context,
        market: w::SuppliedMarket {
            curves: &r.curves,
            fixings: &r.history,
            publications: &r.publications,
        },
        projection: &input.projection,
        discount: &input.discount,
        position: &input.position,
    };
    let quotation = match action {
        FloatingWorkoutOperation::Price {
            selection,
            decimal_dm,
        } => value(w::quote_floating_workout_price(
            &bound,
            selection,
            *decimal_dm,
        )?)?,
        FloatingWorkoutOperation::DiscountMargin {
            selection,
            target,
            settings,
        } => value(w::quote_floating_workout_dm(
            &bound, selection, *target, *settings,
        )?)?,
        FloatingWorkoutOperation::Paths {
            target,
            settings,
            shifted_prefixes,
        } => value(w::quote_floating_workout_paths(
            &bound,
            *target,
            *settings,
            shifted_prefixes,
        )?)?,
    };
    Ok(
        serde_json::json!({"reference":input.security.reference,"quote_revision":input.security.quote_revision,
        "quotation_policy":input.quotation_policy,"calendars":bond.calendars,"quotation":quotation}),
    )
}
