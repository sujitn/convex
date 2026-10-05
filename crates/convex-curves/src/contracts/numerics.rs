//! Native-unit dated money-market fitting over the common numerical kernel.
//! Product-backed multi-curve calibration is owned by its product composition.

use convex_core::types::{Compounding, Date};
use convex_math::optimization::{
    bounded_least_squares, FitDiagnostics, FitFailure, FitParameter, FitResidual,
    LeastSquaresSettings, MAX_FIT_DIMENSION,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::{
    build::{build_direct, validate_global_settings, validate_money_market_helpers},
    *,
};

/// Numerical identification/difference controls, separate from recipe economics
/// and its objective/bounds/starting conditions. No hidden adaptive tolerance.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FitNumericalControls {
    /// Absolute zero-rate step; one-sided at a supplied closed bound.
    pub jacobian_step: f64,
    /// Minimum undamped Jacobian singular ratio; smaller ratios fail explicitly.
    pub minimum_singular_ratio: f64,
    /// Positive iteration budget per SVD, at most 10,000.
    pub svd_iterations: usize,
}
impl Default for FitNumericalControls {
    fn default() -> Self {
        let settings = LeastSquaresSettings::default();
        Self {
            jacobian_step: settings.jacobian_step,
            minimum_singular_ratio: settings.minimum_singular_ratio,
            svd_iterations: settings.svd_iterations,
        }
    }
}

/// Input/financial failure or a measured solver failure; no curve on either path.
#[derive(Debug, thiserror::Error)]
pub enum CurveFitError {
    /// Original domain or quote failure.
    #[error(transparent)]
    Contract(#[from] CurveContractError),
    /// Native solver failure with measured residuals/rank/iterations.
    #[error(transparent)]
    Solver(Box<FitFailure<CurveContractError>>),
}

impl From<FitFailure<CurveContractError>> for CurveFitError {
    fn from(value: FitFailure<CurveContractError>) -> Self {
        Self::Solver(Box::new(value))
    }
}

/// A complete fitted curve and identification/convergence information.
#[derive(Debug, Clone)]
pub struct CurveFitOutput {
    /// Immutable curve retaining the original global recipe and full helpers.
    pub curve: BuiltCurve,
    /// Weighted objective, native residuals, evaluation count, rank and bounds.
    pub solver: FitDiagnostics,
}

fn parameter_nodes(pillars: &[Date], values: &[f64], reference: Date) -> Vec<CurveNode> {
    std::iter::once((reference, values[0]))
        .chain(pillars.iter().copied().zip(values.iter().copied()))
        .map(|(date, value)| CurveNode {
            pillar: CurvePoint::Date(date),
            quote: MarketQuote {
                value,
                unit: QuoteUnit::DecimalRate,
            },
        })
        .collect()
}

fn model_quote(curve: &BuiltCurve, helper: &MoneyMarketHelper) -> CurveContractResult<MarketQuote> {
    let (start, end, day_count) = helper.terms.period();
    let value = curve
        .query(&CurveQuery::ForwardRate {
            start: CurvePoint::Date(start),
            end: CurvePoint::Date(end),
            compounding: Compounding::Simple,
            day_count,
        })?
        .value;
    helper.quote.model_rate_quote(value)
}

fn validate_fit<'a>(
    recipe: &'a CurveRecipe,
    pillars: &[Date],
) -> CurveContractResult<(&'a GlobalFitSettings, &'a [MoneyMarketHelper])> {
    let (CalibrationMode::Global(settings), CurveSource::MoneyMarket(helpers)) =
        (&recipe.calibration, &recipe.source)
    else {
        return Err(CurveContractError::unsupported(
            "dated global fit",
            "requires Global settings and full MoneyMarket helpers",
        ));
    };
    validate_global_settings(settings)?;
    if !recipe.role.is_rate()
        || recipe.representation
            != (CurveRepresentation::ZeroRates {
                compounding: Compounding::Continuous,
            })
        || !recipe.dependencies.is_empty()
    {
        return Err(CurveContractError::unsupported("dated global fit", "supports independent continuous-zero rate curves; product/multi-curve routes remain explicit owning work"));
    }
    if pillars.is_empty()
        || pillars.len() > MAX_FIT_DIMENSION
        || helpers.is_empty()
        || helpers.len() > MAX_FIT_DIMENSION
        || settings.initial_zeros.len() != pillars.len()
        || settings.quote_scales.len() != helpers.len()
    {
        return Err(CurveContractError::invalid(
            "global dimensions",
            "1..128 parameters/helpers with matching supplied starts/scales/weights required",
        ));
    }
    validate_money_market_helpers(recipe, helpers)?;
    let mut seen = BTreeSet::new();
    for &date in pillars {
        if date <= recipe.reference_date || !seen.insert(date) {
            return Err(CurveContractError::invalid(
                "fit.pillars",
                "positive distinct parameter dates required; associated starts sort with dates",
            ));
        }
    }
    Ok((settings, helpers))
}

/// Fit complete dated deposit/FRA quotes on explicitly selected parameter dates.
/// Starts correspond to parameter dates in supplied order; scales and weights
/// correspond to helpers in supplied order. Both pairs sort deterministically.
/// Origin zero is tied to the first parameter, not an unidentified extra variable.
/// Success requires full weighted rank, the scaled RMS target AND every original
/// native quote tolerance. Bounds are supplied; failures carry diagnostics only.
/// Existing `build_curve_set` v1 Global dispatch remains explicitly unsupported;
/// use this function, then `CurveSet::with_curve` for downstream exact dependencies.
pub fn fit_money_market_curve(
    recipe: &CurveRecipe,
    pillars: &[Date],
    controls: FitNumericalControls,
) -> Result<CurveFitOutput, CurveFitError> {
    let (settings, helpers) = validate_fit(recipe, pillars)?;
    let mut parameters: Vec<_> = pillars
        .iter()
        .copied()
        .zip(settings.initial_zeros.iter().copied())
        .collect();
    parameters.sort_by_key(|(date, _)| *date);
    let (dates, starts): (Vec<_>, Vec<_>) = parameters.into_iter().unzip();
    let nodes = parameter_nodes(&dates, &starts, recipe.reference_date);
    // Shared direct ingress validates keys, time basis, methods, rate domains and
    // pillar separation before calling the solver or evaluating any helper.
    let mut checked_recipe = recipe.clone();
    checked_recipe.calibration = CalibrationMode::Direct;
    checked_recipe.source = CurveSource::Nodes(nodes);
    let checked = build_curve_set(std::slice::from_ref(&checked_recipe), &CurveSet::new())?;
    let initial = checked.curves.get(&recipe.key)?;
    for helper in helpers {
        model_quote(initial, helper)?;
    }
    let mut observations: Vec<_> = helpers
        .iter()
        .zip(&settings.quote_scales)
        .zip(&settings.weights)
        .map(|((helper, scale), weight)| (helper, *scale, *weight))
        .collect();
    observations.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    let contracts: Vec<_> = observations
        .iter()
        .map(|(helper, scale, weight)| FitResidual {
            scale: *scale,
            weight: *weight,
            tolerance: helper.tolerance,
        })
        .collect();
    let parameters: Vec<_> = starts
        .iter()
        .map(|initial| FitParameter {
            initial: *initial,
            lower: settings.lower_zero,
            upper: settings.upper_zero,
        })
        .collect();
    let numerical = LeastSquaresSettings {
        max_iterations: settings.max_iterations,
        objective_tolerance: settings.objective_tolerance,
        jacobian_step: controls.jacobian_step,
        minimum_singular_ratio: controls.minimum_singular_ratio,
        svd_iterations: controls.svd_iterations,
    };
    let fit = bounded_least_squares(&parameters, &contracts, numerical, |values| {
        let curve = build_direct(
            &checked_recipe,
            &parameter_nodes(&dates, values, recipe.reference_date),
            "global_trial",
            vec![],
        )?;
        observations
            .iter()
            .map(|(helper, _, _)| {
                model_quote(&curve, helper).map(|quote| quote.value - helper.quote.value)
            })
            .collect::<CurveContractResult<Vec<_>>>()
    })?;
    let mut curve = build_direct(
        &checked_recipe,
        &parameter_nodes(&dates, &fit.parameters, recipe.reference_date),
        "bounded_scaled_dated_money_market_global_fit",
        vec![],
    )?;
    let mut quotes = Vec::with_capacity(helpers.len());
    for (helper, _, _) in observations {
        let model = model_quote(&curve, helper)?;
        let residual = model.value - helper.quote.value;
        if residual.abs() > helper.tolerance {
            return Err(CurveContractError::RepricingFailure {
                helper_id: helper.id.clone(),
                residual,
                tolerance: helper.tolerance,
            }
            .into());
        }
        quotes.push(QuoteRepricing {
            helper_id: helper.id.clone(),
            pillar: helper.terms.period().1,
            input: helper.quote,
            model,
            residual,
            tolerance: helper.tolerance,
        });
    }
    curve.recipe = recipe.clone();
    curve.diagnostics.iterations = fit.diagnostics.iterations;
    curve.diagnostics.quotes = quotes;
    Ok(CurveFitOutput {
        curve,
        solver: fit.diagnostics,
    })
}
