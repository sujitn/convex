//! Original helper diagnostics on supplied curves, without a calibration solve.
use super::{build, helpers, *};
use convex_core::fixings::FixingSeries;
use convex_curves::contracts::*;
use serde::Serialize;

/// Full source-order repricing; a successful calculation is not a fitted curve.
#[derive(Debug, Clone, Serialize)]
pub struct RateHelperRepricing {
    /// Original complete economics, mappings and unchanged native tolerances.
    pub recipe: RateCurveRecipe,
    /// Original-unit model observations and finite signed model-minus-input residuals.
    pub quotes: Vec<QuoteRepricing>,
    /// Complete cash, fixing, curve-query and futures-source evidence.
    pub valuations: Vec<RateHelperValuation>,
    /// Whether every residual passes its original helper tolerance.
    pub within_tolerance: bool,
    /// Actual publication decisions frozen once per index/date.
    pub publications: Vec<FixingRequirement>,
    /// Actual helper valuations; no trial curves or solver work is performed.
    pub price_evaluations: usize,
}

fn check_binding(expected: &CurveRecipe, supplied: &CurveSet) -> Result<(), RateCalibrationError> {
    let actual = supplied.get(&expected.key)?.recipe();
    if actual.role != expected.role
        || actual.currency != expected.currency
        || actual.reference_date != expected.reference_date
        || actual.time_basis != expected.time_basis
        || actual.representation != expected.representation
        || actual.interpolation != expected.interpolation
        || actual.extrapolation != expected.extrapolation
    {
        return Err(invalid(
            "supplied curve",
            "exact helper key, currency, role, origin, time basis and representation/methods required",
        ));
    }
    Ok(())
}

/// Reprice one complete original recipe against immutable supplied curves.
/// Uses the same recipe admission, product preparation, complete due-history
/// preflight, implied quotes and actual query-horizon guards as calibration.
/// The declared output key must already exist; no curve is built or replaced.
/// A declared overnight zero-basis view must be the actual named discount view.
/// Off-market finite residuals are diagnostic results with `within_tolerance`
/// false; the calibration builder still rejects residual failures independently.
/// Any missing/invalid resource or native valuation failure returns no partial
/// result. Publication callbacks must terminate and must not query a live clock.
pub fn reprice_rate_helpers<P>(
    input: &RateCurveBuildInput<'_>,
    supplied: &CurveSet,
    history: &FixingSeries,
    context: RateCalibrationContext,
    mut administrator_publication: P,
) -> Result<RateHelperRepricing, RateCalibrationError>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    build::validate(input.recipe, context)?;
    if input.resources.len() != input.recipe.helpers.len()
        || Date::from(context.as_of.datetime().date_naive())
            != context.cashflows.fixings.valuation_date
    {
        return Err(invalid(
            "repricing resources",
            "one resource set per original helper and consistent economic timestamp/date required",
        ));
    }
    check_binding(&input.recipe.curve, supplied)?;
    for key in &input.recipe.curve.dependencies {
        supplied.get(key)?;
    }
    if let Some(alias) = build::alias(input.recipe) {
        check_binding(&alias, supplied)?;
        let actual = supplied.get(&alias.key)?.recipe();
        if actual.source != alias.source || actual.dependencies != alias.dependencies {
            return Err(invalid(
                "overnight projection",
                "supplied view must retain the declared exact zero-basis discount dependency",
            ));
        }
    }
    let mut publications = Publications::new();
    let mut missing = Publications::new();
    let prepared = helpers::prepare(
        input,
        history,
        context,
        &mut publications,
        &mut missing,
        &mut administrator_publication,
    )?;
    if !missing.is_empty() {
        return Err(CashflowError::MissingFixings(missing.into_values().collect()).into());
    }
    let mut quotes = Vec::with_capacity(prepared.helpers.len());
    let mut valuations = Vec::with_capacity(prepared.helpers.len());
    let mut evaluations = 0;
    let mut within_tolerance = true;
    for h in &prepared.helpers {
        let (model, valuation) = helpers::value(
            h,
            supplied,
            history,
            context,
            &publications,
            &mut evaluations,
        )?;
        helpers::horizon(&valuation, h.helper, input.recipe)?;
        let residual = model.value - h.helper.quote.value;
        if !residual.is_finite() {
            return Err(invalid(
                "helper residual",
                "finite native-unit difference required",
            ));
        }
        within_tolerance &= residual.abs() <= h.helper.tolerance;
        quotes.push(QuoteRepricing {
            helper_id: h.helper.id.clone(),
            pillar: h.helper.pillar,
            input: h.helper.quote,
            model,
            residual,
            tolerance: h.helper.tolerance,
        });
        valuations.push(valuation);
    }
    Ok(RateHelperRepricing {
        recipe: input.recipe.clone(),
        quotes,
        valuations,
        within_tolerance,
        publications: publications.into_values().collect(),
        price_evaluations: evaluations,
    })
}
