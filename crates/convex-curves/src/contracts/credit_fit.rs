//! Validated storage of a product-layer CDS fit. No CDS formulas live here.
use super::{
    build::{build_direct, validate_recipe},
    *,
};
use crate::{ExtrapolationMethod, InterpolationMethod};
use convex_core::{daycounts::DayCountConvention, Date};
use convex_math::portable;
use std::collections::BTreeSet;

/// Integrate actual piecewise constant ACT/365F intensities into survival nodes.
/// No spread/recovery approximation, probability floor or tail extrapolation.
pub fn credit_survival_nodes(
    reference: Date,
    pillars: &[Date],
    hazards: &[f64],
) -> CurveContractResult<Vec<CurveNode>> {
    inputs::validate_date(reference)?;
    if pillars.is_empty() || pillars.len() != hazards.len() || pillars.len() >= MAX_CURVE_INPUTS {
        return Err(CurveContractError::invalid(
            "credit.dimensions",
            "bounded matching nonempty pillar/intensity arrays required",
        ));
    }
    let mut previous = reference;
    let mut cumulative = 0.0;
    let mut nodes = vec![CurveNode {
        pillar: CurvePoint::Date(reference),
        quote: MarketQuote {
            value: 1.0,
            unit: QuoteUnit::SurvivalProbability,
        },
    }];
    for (&date, &hazard) in pillars.iter().zip(hazards) {
        inputs::validate_date(date)?;
        if date <= previous || !hazard.is_finite() || !(0.0..=5.0).contains(&hazard) {
            return Err(CurveContractError::invalid(
                "credit.parameters",
                "increasing dated pillars and finite intensities in [0,5] required",
            ));
        }
        let time = reference.days_between(&date) as f64 / 365.0;
        if time > MAX_CURVE_TENOR {
            return Err(CurveContractError::BoundExceeded("credit tenor"));
        }
        cumulative += hazard * (previous.days_between(&date) as f64 / 365.0);
        let q = portable::exp(-cumulative);
        if !q.is_finite() || q <= 0.0 {
            return Err(CurveContractError::Numerical(
                "survival underflow/nonfinite intensity integral".into(),
            ));
        }
        nodes.push(CurveNode {
            pillar: CurvePoint::Date(date),
            quote: MarketQuote {
                value: q,
                unit: QuoteUnit::SurvivalProbability,
            },
        });
        previous = date;
    }
    Ok(nodes)
}

/// Store an actually performed CDS fit with original native quotes and lineage.
/// This verifies supplied postconditions; it does not run or attest a CDS solver.
pub fn build_fitted_credit_curve(
    recipe: &CurveRecipe,
    pillars: &[Date],
    hazards: &[f64],
    quotes: Vec<QuoteRepricing>,
    iterations: usize,
    supplied: &CurveSet,
) -> CurveContractResult<BuiltCurve> {
    let CurveSource::Helpers(ids) = &recipe.source else {
        return Err(CurveContractError::invalid(
            "credit.source",
            "ordered full CDS helper identities required",
        ));
    };
    let method = match &recipe.calibration {
        CalibrationMode::Piecewise if ids.len() == pillars.len() => "cds_piecewise_brent",
        CalibrationMode::CreditGlobal(settings) => {
            if settings.initial_hazards.len() != pillars.len()
                || settings.quote_scales.len() != quotes.len()
                || settings.weights.len() != quotes.len()
                || settings.max_iterations == 0
                || settings.max_iterations > convex_math::bounds::MAX_SOLVER_ITERATIONS as usize
                || !settings.objective_tolerance.is_finite()
                || settings.objective_tolerance <= 0.0
                || !settings.lower_hazard.is_finite()
                || !settings.upper_hazard.is_finite()
                || settings.lower_hazard < 0.0
                || settings.upper_hazard > 5.0
                || settings.lower_hazard >= settings.upper_hazard
                || settings.initial_hazards.iter().chain(hazards).any(|h| {
                    !h.is_finite() || *h < settings.lower_hazard || *h > settings.upper_hazard
                })
                || settings
                    .quote_scales
                    .iter()
                    .any(|s| !s.is_finite() || *s <= 0.0)
                || settings.weights.iter().any(|w| !w.is_finite() || *w < 0.0)
                || settings.weights.iter().filter(|w| **w > 0.0).count() < pillars.len()
                || quotes.len() < pillars.len()
            {
                return Err(CurveContractError::invalid("credit.global","matching bounded intensities, starts, positive scales and identifying weights required"));
            }
            "cds_global_bounded_least_squares"
        }
        _ => {
            return Err(CurveContractError::invalid(
                "credit.calibration",
                "matching Piecewise or CreditGlobal mode required",
            ))
        }
    };
    if !matches!(recipe.role, CurveRole::Credit { .. })
        || recipe.representation != CurveRepresentation::SurvivalProbabilities
        || recipe.time_basis != DayCountConvention::Act365Fixed
        || recipe.interpolation != InterpolationMethod::LogLinear
        || recipe.extrapolation != ExtrapolationMethod::None
        || ids.is_empty()
        || ids.len() > MAX_CURVE_INPUTS
        || ids.len() != quotes.len()
        || iterations > ids.len() * convex_math::bounds::MAX_SOLVER_ITERATIONS as usize
    {
        return Err(CurveContractError::invalid(
            "credit.fit",
            "bounded Credit ACT/365F log-linear survival fit without extrapolation required",
        ));
    }
    validate_credit_repricing(recipe, ids, &quotes)?;
    let mut dependencies = BTreeSet::new();
    if recipe.dependencies.is_empty() || recipe.dependencies.len() > MAX_CURVES {
        return Err(CurveContractError::invalid(
            "credit.dependencies",
            "bounded exact discount dependencies required",
        ));
    }
    for dependency in &recipe.dependencies {
        if dependency.id == recipe.key.id || !dependencies.insert(&dependency.id) {
            return Err(CurveContractError::invalid(
                "credit.dependencies",
                "unique external dependencies required",
            ));
        }
        supplied.get(dependency)?;
    }
    let nodes = credit_survival_nodes(recipe.reference_date, pillars, hazards)?;
    let mut direct = recipe.clone();
    direct.source = CurveSource::Nodes(nodes.clone());
    direct.calibration = CalibrationMode::Direct;
    direct.dependencies.clear();
    validate_recipe(&direct)?;
    let mut curve = build_direct(&direct, &nodes, method, quotes)?;
    curve.recipe = recipe.clone();
    curve
        .diagnostics
        .dependencies
        .clone_from(&recipe.dependencies);
    curve.diagnostics.iterations = iterations;
    Ok(curve)
}

fn validate_credit_repricing(
    recipe: &CurveRecipe,
    ids: &[String],
    quotes: &[QuoteRepricing],
) -> CurveContractResult<()> {
    let mut seen = BTreeSet::new();
    for (id, quote) in ids.iter().zip(quotes) {
        inputs::validate_label(id, "helper.id")?;
        inputs::validate_date(quote.pillar)?;
        inputs::validate_tolerance(quote.tolerance)?;
        quote.input.validate()?;
        quote.model.validate()?;
        if !seen.insert(id)
            || id != &quote.helper_id
            || quote.pillar <= recipe.reference_date
            || quote.input.unit != quote.model.unit
            || !matches!(
                quote.input.unit,
                QuoteUnit::DecimalRate
                    | QuoteUnit::PercentRate
                    | QuoteUnit::BasisPoints
                    | QuoteUnit::UpfrontFraction
            )
            || !quote.residual.is_finite()
            || quote.residual != quote.model.value - quote.input.value
        {
            return Err(CurveContractError::invalid("credit.repricing","unique ordered native CDS quote records and exact model-minus-input residuals required"));
        }
        if quote.residual.abs() > quote.tolerance {
            return Err(CurveContractError::RepricingFailure {
                helper_id: id.clone(),
                residual: quote.residual,
                tolerance: quote.tolerance,
            });
        }
    }
    Ok(())
}
