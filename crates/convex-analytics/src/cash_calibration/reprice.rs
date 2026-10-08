//! Complete original cash-helper diagnostics on immutable supplied curves.
use super::*;

/// Source-order diagnostics; successful calculation does not accept a fit.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CashHelperRepricing {
    /// Full original securities/loans, legal mapping, revisions and quote settings.
    pub recipe: CashCurveRecipe,
    /// Original native-unit quotes and signed model-minus-input residuals.
    pub quotes: Vec<QuoteRepricing>,
    /// Complete original cash, accrued, promised/recovery and loan valuations.
    pub valuations: Vec<CashHelperValuation>,
    /// Whether all observations meet their original native quote tolerances.
    pub within_tolerance: bool,
    /// Actual helper valuations; no solver or trial curve is evaluated.
    pub price_evaluations: usize,
}

/// Diagnose one complete original government, issuer-basis or repo recipe against
/// exact supplied curves, without fitting, replacing or publishing a curve.
/// Reuses full calibration preparation and original model/native-quote functions.
/// Every contract and basis model is checked before pricing. Any missing input,
/// admission or native failure returns no partial result. Finite off-market
/// residuals succeed with `within_tolerance:false`; accepted calibration retains
/// its separate mandatory final residual and immutable-output collision guards.
pub fn reprice_cash_helpers(
    input: &CashCurveBuildInput<'_>,
    supplied: &CurveSet,
) -> CashCalibrationResult<CashHelperRepricing> {
    let prepared = build::prepare(input)?;
    let expected = &input.recipe.curve;
    let curve = supplied.get(&expected.key)?;
    let actual = curve.recipe();
    if actual.role != expected.role
        || actual.currency != expected.currency
        || actual.reference_date != expected.reference_date
        || actual.time_basis != expected.time_basis
        || actual.representation != expected.representation
        || actual.interpolation != expected.interpolation
        || actual.extrapolation != expected.extrapolation
    {
        return Err(invalid(
            "supplied cash curve",
            "exact role, currency, origin, time basis and representation/methods required",
        ));
    }
    for key in &expected.dependencies {
        supplied.get(key)?;
    }
    if let Some(model) = &input.recipe.basis_model {
        let (reference, credit) = match model {
            BondBasisModel::ReferenceZeroSpread { reference, .. } => {
                (supplied.get(reference)?.recipe(), None)
            }
            BondBasisModel::CreditParRecovery {
                discount, survival, ..
            } => (
                supplied.get(discount)?.recipe(),
                Some(supplied.get(survival)?.recipe()),
            ),
        };
        for helper in &input.recipe.helpers {
            if let CashCalibrationHelper::Bond { input: h } = helper {
                pricing::check_basis_metadata(h, expected, model, reference, credit)?;
            }
        }
        if matches!(model, BondBasisModel::CreditParRecovery { .. }) {
            for bond in prepared.bonds.iter().flatten() {
                pricing::check_credit_cash(bond)?;
            }
        }
    }
    let mut quotes = Vec::with_capacity(input.recipe.helpers.len());
    let mut valuations = Vec::with_capacity(input.recipe.helpers.len());
    let mut evaluations = 0;
    let mut within_tolerance = true;
    for (i, h) in input.recipe.helpers.iter().enumerate() {
        let (model, valuation) = fit::value(&prepared, i, curve, supplied, &mut evaluations)?;
        let residual = model.value - h.quote().value;
        if !residual.is_finite() {
            return Err(invalid(
                "helper residual",
                "finite native-unit difference required",
            ));
        }
        within_tolerance &= residual.abs() <= h.tolerance();
        quotes.push(QuoteRepricing {
            helper_id: h.id().into(),
            pillar: h.pillar(),
            input: h.quote(),
            model,
            residual,
            tolerance: h.tolerance(),
        });
        valuations.push(valuation);
    }
    Ok(CashHelperRepricing {
        recipe: input.recipe.clone(),
        quotes,
        valuations,
        within_tolerance,
        price_evaluations: evaluations,
    })
}
