//! Complete original CDS-helper diagnostics on immutable supplied curves.
use super::*;

/// Full source-order diagnostics; a successful calculation is not a fitted curve.
#[derive(Debug, Clone, Serialize)]
pub struct CdsHelperRepricing {
    /// Full original terms, legal/calendar/quote revisions and native tolerances.
    pub recipe: CdsCurveRecipe,
    /// Native model quotes and signed model-minus-input residuals, in source order.
    pub quotes: Vec<QuoteRepricing>,
    /// Complete original premium, protection, accrual, initial cash and curve evidence.
    pub valuations: Vec<CdsValuation>,
    /// Whether every residual meets its original helper tolerance.
    pub within_tolerance: bool,
    /// Actual helper valuations; no solver or trial curve is evaluated.
    pub price_evaluations: usize,
}

/// Diagnose a complete original CDS recipe against exact supplied survival and
/// discount curves, without fitting, replacing or publishing any curve.
/// Reuses calibration recipe admission, checked complete contracts, the original
/// Standard183 pricer, native quote conversion and valuation work limit.
/// Every contract is constructed before pricing; any error returns no partial
/// result. Finite off-market residuals succeed with `within_tolerance` false,
/// independently of the builder's mandatory accepted-fit residual checks.
pub fn reprice_cds_helpers(
    input: &CdsCurveBuildInput<'_>,
    supplied: &CurveSet,
) -> Result<CdsHelperRepricing, CreditCalibrationError> {
    validate_recipe(input.recipe, supplied)?;
    if input.calendars.len() != input.recipe.helpers.len() {
        return Err(invalid(
            "calendars",
            "one actual calendar binding per original helper required",
        ));
    }
    // These declared recipe settings remain bounded even though no fit runs.
    if let CalibrationMode::CreditGlobal(s) = &input.recipe.curve.calibration {
        if s.max_iterations == 0
            || s.max_iterations > convex_math::bounds::MAX_SOLVER_ITERATIONS as usize
            || !s.objective_tolerance.is_finite()
            || s.objective_tolerance <= 0.0
        {
            return Err(invalid(
                "global",
                "bounded iterations and positive finite objective tolerance required",
            ));
        }
    }
    let expected = &input.recipe.curve;
    let actual = supplied.get(&expected.key)?.recipe();
    if actual.role != expected.role
        || actual.currency != expected.currency
        || actual.reference_date != expected.reference_date
        || actual.time_basis != expected.time_basis
        || actual.representation != expected.representation
        || actual.interpolation != expected.interpolation
        || actual.extrapolation != expected.extrapolation
    {
        return Err(invalid("supplied credit curve", "exact role, recovery, currency, origin, time basis and representation/methods required"));
    }
    let contracts = input
        .recipe
        .helpers
        .iter()
        .zip(input.calendars)
        .map(|(h, c)| h.terms.build(c))
        .collect::<Result<Vec<_>, _>>()?;
    let mut quotes = Vec::with_capacity(contracts.len());
    let mut valuations = Vec::with_capacity(contracts.len());
    let mut evaluations = 0;
    let mut within_tolerance = true;
    for (h, contract) in input.recipe.helpers.iter().zip(&contracts) {
        let valuation = price(contract, h, supplied, &mut evaluations)?;
        let model = cds_model_quote(h, &valuation)?;
        let residual = model.value - h.quote.value;
        if !residual.is_finite() {
            return Err(invalid(
                "helper residual",
                "finite native-unit difference required",
            ));
        }
        within_tolerance &= residual.abs() <= h.tolerance;
        quotes.push(QuoteRepricing {
            helper_id: h.id.clone(),
            pillar: h.pillar,
            input: h.quote,
            model,
            residual,
            tolerance: h.tolerance,
        });
        valuations.push(valuation);
    }
    Ok(CdsHelperRepricing {
        recipe: input.recipe.clone(),
        quotes,
        valuations,
        within_tolerance,
        price_evaluations: evaluations,
    })
}
