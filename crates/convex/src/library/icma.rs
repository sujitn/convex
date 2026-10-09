//! Thin full supplied-context ICMA cash dispatch; native owners retain economics.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::workflow as w;
use convex_rates::ReferencePin;

/// Complete original security/reference context, source policy and signed face.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaQuotationInput {
    /// Exact full original bond and independent security/quote/calendar pins.
    pub security: WorkoutInput,
    /// Actual independently supplied nominal reference mesh and revision.
    pub reference_context: w::IcmaReferenceContext,
    /// Independently resolved original quotation policy revision.
    pub quotation_policy: ReferencePin,
    /// Exact original-face signed quantity/currency.
    pub position: convex_analytics::quote_risk::RiskPosition,
}
/// Disjoint same-cash price and closed-domain inverse operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IcmaQuotationOperation {
    /// Original full cash price at an annual decimal yield.
    Price {
        /// Original annual decimal quotation rate, independent of coupon.
        decimal_yield: f64,
    },
    /// Original native inverse; final original-face price residual is mandatory.
    Yield {
        /// Observed clean/dirty per100 original face.
        target: w::BondPriceTarget,
        /// Original closed bounds, native residual and solver work controls.
        settings: w::BondInverseSettings,
    },
}
/// Complete legal-path operations; selected single paths use icma_quotation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IcmaWorkoutOperation {
    /// Full original maturity/call/put prices at a common annual decimal yield.
    PricePaths {
        /// Annual decimal yield under the supplied contextual convention.
        decimal_yield: f64,
        /// Explicit total legal-date, reference-preparation and cash-work bounds.
        settings: w::IcmaWorkoutSettings,
    },
    /// Every legal-path yield and the defined original call/maturity minimum.
    Worst {
        /// ORIGINAL common observed clean/dirty original-face price.
        target: w::BondPriceTarget,
        /// Original same-pricer inverse plus explicit whole-request bounds.
        settings: w::IcmaWorkoutSolveSettings,
    },
}
pub(super) fn execute(
    input: &IcmaQuotationInput,
    convention: &w::IcmaYieldConvention,
    cash: &w::IcmaCashSelection,
    action: &IcmaQuotationOperation,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    with_inputs(input, convention, r, |bound| {
        let prepared = w::PreparedIcmaQuotation::new(bound, cash)?;
        value(match action {
            IcmaQuotationOperation::Price { decimal_yield } => {
                prepared.quote_price(*decimal_yield)?
            }
            IcmaQuotationOperation::Yield { target, settings } => {
                prepared.quote_yield(*target, *settings)?
            }
        })
    })
}
pub(super) fn workout(
    input: &IcmaQuotationInput,
    convention: &w::IcmaYieldConvention,
    context: &w::WorkoutContext,
    action: &IcmaWorkoutOperation,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    with_inputs(input, convention, r, |bound| match action {
        IcmaWorkoutOperation::PricePaths {
            decimal_yield,
            settings,
        } => value(w::quote_icma_workout_prices(
            bound,
            context,
            *decimal_yield,
            *settings,
        )?),
        IcmaWorkoutOperation::Worst { target, settings } => value(w::quote_icma_workout_worst(
            bound, context, *target, *settings,
        )?),
    })
}
fn with_inputs(
    input: &IcmaQuotationInput,
    convention: &w::IcmaYieldConvention,
    r: &ResolvedInputs<'_>,
    action: impl FnOnce(&w::IcmaQuotationInputs<'_>) -> LibraryResult<Value>,
) -> LibraryResult<Value> {
    let b = &input.security.bond;
    let contract = r.bond(b)?;
    let sources = w::IcmaQuotationSources {
        security: input.security.reference.clone(),
        quote_revision: input.security.quote_revision.clone(),
        quotation_policy: input.quotation_policy.clone(),
        calendars: w::IcmaCalendarPins {
            payment: b.calendars.payment.clone(),
            reset: b.calendars.reset.clone(),
            observation: b.calendars.observation.clone(),
            exercise: b.calendars.exercise.clone(),
        },
    };
    let bound = w::IcmaQuotationInputs {
        contract: &contract,
        sources: &sources,
        reference_context: &input.reference_context,
        convention,
        position: &input.position,
        exercise_calendar: (
            &contract.terms().calendars.exercise,
            r.calendar(&b.calendars.exercise)?,
        ),
        purchase_calendar: (
            &contract.terms().calendars.payment,
            r.calendar(&b.calendars.payment)?,
        ),
    };
    action(&bound)
}
