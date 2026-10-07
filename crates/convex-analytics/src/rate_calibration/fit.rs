//! One shared bounded solver path; every final original native quote must pass.

use super::{
    build,
    helpers::{self, Prepared},
    *,
};
use convex_core::fixings::FixingSeries;
use convex_curves::contracts::*;
use convex_math::{
    optimization::{bounded_least_squares, FitParameter, FitResidual, LeastSquaresSettings},
    solvers::{brent, SolverConfig},
};
use std::cell::RefCell;

fn nodes(reference: Date, pillars: &[Date], zeros: &[f64]) -> Vec<CurveNode> {
    std::iter::once((reference, zeros[0]))
        .chain(pillars.iter().copied().zip(zeros.iter().copied()))
        .map(|(date, value)| CurveNode {
            pillar: CurvePoint::Date(date),
            quote: MarketQuote {
                value,
                unit: QuoteUnit::DecimalRate,
            },
        })
        .collect()
}
fn trial(
    recipe: &RateCurveRecipe,
    pillars: &[Date],
    zeros: &[f64],
    supplied: &CurveSet,
) -> Result<CurveSet, RateCalibrationError> {
    let mut direct = recipe.curve.clone();
    direct.source = CurveSource::Nodes(nodes(direct.reference_date, pillars, zeros));
    direct.dependencies.clear();
    direct.calibration = CalibrationMode::Direct;
    let mut curves = build_curve_set(&[direct], supplied)?.curves;
    if let Some(alias) = build::alias(recipe) {
        curves = build_curve_set(&[alias], &curves)?.curves;
    }
    Ok(curves)
}

pub(super) fn fit(
    prepared: &Prepared<'_>,
    supplied: &CurveSet,
    history: &FixingSeries,
    context: RateCalibrationContext,
    publications: &Publications,
    controls: FitNumericalControls,
    evaluations: &mut usize,
) -> Result<CalibratedRateCurve, RateCalibrationError> {
    let input = prepared.recipe;
    let before = *evaluations;
    let (zeros, solver, iterations) = match (&input.curve.calibration, input.bootstrap) {
        (CalibrationMode::Piecewise, Some(settings)) => {
            let mut ordered: Vec<_> = prepared.helpers.iter().collect();
            ordered.sort_by_key(|p| p.helper.pillar);
            let mut zeros = Vec::new();
            let mut roots = Vec::new();
            let mut iterations = 0;
            for (i, h) in ordered.into_iter().enumerate() {
                let state = RefCell::new((&mut *evaluations, None, Vec::new()));
                let result = brent(
                    |zero| {
                        let mut s = state.borrow_mut();
                        if s.1.is_some() {
                            return f64::NAN;
                        }
                        let mut values = zeros.clone();
                        values.push(zero);
                        let priced = trial(input, &input.pillars[..=i], &values, supplied)
                            .and_then(|curves| {
                                let (model, value) = helpers::value(
                                    h,
                                    &curves,
                                    history,
                                    context,
                                    publications,
                                    s.0,
                                )?;
                                helpers::horizon(&value, h.helper, input)?;
                                Ok(model.value - h.helper.quote.value)
                            });
                        match priced {
                            Ok(r) => {
                                s.2.push(r);
                                r
                            }
                            Err(e) => {
                                s.1 = Some(e);
                                f64::NAN
                            }
                        }
                    },
                    settings.lower_zero,
                    settings.upper_zero,
                    &SolverConfig {
                        tolerance: h.helper.tolerance,
                        max_iterations: settings.max_iterations,
                    },
                );
                let (_, error, residuals) = state.into_inner();
                if let Some(error) = error {
                    return Err(error);
                }
                if residuals.len() >= 2
                    && (residuals[0] - residuals[1]).abs() <= 2.0 * h.helper.tolerance
                {
                    return Err(invalid("piecewise identification",format!("{} cannot distinguish the supplied parameter bracket at its native threshold",h.helper.id)));
                }
                let root = result?;
                zeros.push(root.root);
                iterations += root.iterations as usize;
                roots.push(root);
            }
            (
                zeros,
                RateSolverDiagnostics::Piecewise {
                    roots,
                    evaluations: 0,
                },
                iterations,
            )
        }
        (CalibrationMode::Global(settings), None) => {
            let parameters: Vec<_> = settings
                .initial_zeros
                .iter()
                .map(|&initial| FitParameter {
                    initial,
                    lower: settings.lower_zero,
                    upper: settings.upper_zero,
                })
                .collect();
            // Pair scales/weights with originals before deterministic identity sort.
            let mut ordered: Vec<_> = prepared.helpers.iter().enumerate().collect();
            ordered.sort_by(|a, b| a.1.helper.id.cmp(&b.1.helper.id));
            let residuals: Vec<_> = ordered
                .iter()
                .map(|(i, h)| FitResidual {
                    scale: settings.quote_scales[*i],
                    weight: settings.weights[*i],
                    tolerance: h.helper.tolerance,
                })
                .collect();
            let fitted = bounded_least_squares(
                &parameters,
                &residuals,
                LeastSquaresSettings {
                    max_iterations: settings.max_iterations,
                    objective_tolerance: settings.objective_tolerance,
                    jacobian_step: controls.jacobian_step,
                    minimum_singular_ratio: controls.minimum_singular_ratio,
                    svd_iterations: controls.svd_iterations,
                },
                |values| {
                    let curves = trial(input, &input.pillars, values, supplied)?;
                    ordered
                        .iter()
                        .map(|(_, h)| {
                            let (model, value) = helpers::value(
                                h,
                                &curves,
                                history,
                                context,
                                publications,
                                evaluations,
                            )?;
                            helpers::horizon(&value, h.helper, input)?;
                            Ok(model.value - h.helper.quote.value)
                        })
                        .collect::<Result<Vec<_>, RateCalibrationError>>()
                },
            )
            .map_err(|e| RateCalibrationError::Global(Box::new(e)))?;
            let iterations = fitted.diagnostics.iterations;
            (
                fitted.parameters,
                RateSolverDiagnostics::Global(fitted.diagnostics),
                iterations,
            )
        }
        _ => return Err(invalid("calibration", "invalid admitted mode")),
    };
    let curves = trial(input, &input.pillars, &zeros, supplied)?;
    let mut repriced_helpers = Vec::new();
    let mut quotes = Vec::new();
    for h in &prepared.helpers {
        let (model, value) =
            helpers::value(h, &curves, history, context, publications, evaluations)?;
        helpers::horizon(&value, h.helper, input)?;
        let residual = model.value - h.helper.quote.value;
        if !residual.is_finite() || residual.abs() > h.helper.tolerance {
            return Err(CurveContractError::RepricingFailure {
                helper_id: h.helper.id.clone(),
                residual,
                tolerance: h.helper.tolerance,
            }
            .into());
        }
        quotes.push(QuoteRepricing {
            helper_id: h.helper.id.clone(),
            pillar: h.helper.pillar,
            input: h.helper.quote,
            model,
            residual,
            tolerance: h.helper.tolerance,
        });
        repriced_helpers.push(value);
    }
    let curve = build_fitted_rate_curve(
        &input.curve,
        &nodes(input.curve.reference_date, &input.pillars, &zeros),
        quotes,
        iterations,
        supplied,
    )?;
    let solver = match solver {
        RateSolverDiagnostics::Piecewise { roots, .. } => RateSolverDiagnostics::Piecewise {
            roots,
            evaluations: *evaluations - before,
        },
        other => other,
    };
    Ok(CalibratedRateCurve {
        recipe: input.clone(),
        curve,
        repriced_helpers,
        solver,
    })
}
