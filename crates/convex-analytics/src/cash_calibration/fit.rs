//! Shared bounded roots/objective with full original native quote postconditions.

use super::{build::Prepared, *};
use convex_math::{
    optimization::{bounded_least_squares, FitParameter, FitResidual, LeastSquaresSettings},
    solvers::{brent, SolverConfig},
};
use std::cell::RefCell;

fn nodes(reference: Date, pillars: &[Date], values: &[f64]) -> Vec<CurveNode> {
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
fn trial(
    r: &CashCurveRecipe,
    pillars: &[Date],
    parameters: &[f64],
    supplied: &CurveSet,
) -> CashCalibrationResult<BuiltCurve> {
    let mut c = r.curve.clone();
    c.calibration = CalibrationMode::Direct;
    c.dependencies.clear();
    c.source = if let CalibrationMode::Parametric(s) = &r.curve.calibration {
        CurveSource::Parametric {
            parameters: s.initial.with_parameters(parameters)?,
            horizon: CurvePoint::Date(*pillars.last().ok_or_else(|| invalid("horizon", "empty"))?),
            observations: Vec::new(),
        }
    } else {
        CurveSource::Nodes(nodes(c.reference_date, pillars, parameters))
    };
    Ok(build_curve_set(&[c], supplied)?
        .curves
        .get(&r.curve.key)?
        .as_ref()
        .clone())
}
pub(super) fn value(
    p: &Prepared<'_>,
    i: usize,
    curve: &BuiltCurve,
    supplied: &CurveSet,
    evaluations: &mut usize,
) -> CashCalibrationResult<(MarketQuote, CashHelperValuation)> {
    if *evaluations >= MAX_CASH_EVALUATIONS {
        return Err(CurveContractError::BoundExceeded("actual cash helper valuations").into());
    }
    *evaluations += 1;
    match &p.recipe.helpers[i] {
        CashCalibrationHelper::Bond { input: h } => {
            let prepared = p.bonds[i]
                .as_ref()
                .ok_or_else(|| invalid("cash resources", "unprepared original contract"))?;
            let (priced, basis) = if let Some(model) = &p.recipe.basis_model {
                let v = pricing::basis_price(prepared, h, model, curve, supplied)?;
                (v.price.clone(), Some(Box::new(v)))
            } else {
                (pricing::government_price(prepared, h, curve)?, None)
            };
            let model = pricing::model_quote(prepared, h, &priced, curve)?;
            Ok((
                model,
                CashHelperValuation::Bond {
                    observed: prepared.observed.clone(),
                    value: Box::new(priced),
                    basis,
                },
            ))
        }
        CashCalibrationHelper::Repo { input: h } => {
            let ratio = curve
                .query(&CurveQuery::DiscountRatio {
                    start: CurvePoint::Date(h.start_date),
                    end: CurvePoint::Date(h.end_date),
                })?
                .value;
            let tau = year_fraction(h.start_date, h.end_date, h.day_count)?;
            let rate = rate_from_discount(ratio, tau, convex_core::types::Compounding::Simple)?;
            let model = h.quote.model_rate_quote(rate)?;
            Ok((
                model,
                CashHelperValuation::Repo {
                    collateral_id: h.collateral_id.clone(),
                    start: h.start_date,
                    end: h.end_date,
                    accrual_fraction: tau,
                    repo_discount_ratio: ratio,
                    decimal_rate: rate,
                },
            ))
        }
    }
}
pub(super) fn fit(
    p: &Prepared<'_>,
    supplied: &CurveSet,
    controls: FitNumericalControls,
    evaluations: &mut usize,
) -> CashCalibrationResult<CalibratedCashCurve> {
    let r = p.recipe;
    let before = *evaluations;
    let (parameters, solver, iterations) = match (&r.curve.calibration, r.bootstrap) {
        (CalibrationMode::Parametric(settings), None) => {
            let parameters = settings.fit_parameters(r.helpers.len())?;
            let residuals: Vec<_> = r
                .helpers
                .iter()
                .enumerate()
                .map(|(i, h)| FitResidual {
                    scale: settings.quote_scales[i],
                    weight: settings.weights[i],
                    tolerance: h.tolerance(),
                })
                .collect();
            let fit = bounded_least_squares(
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
                    let native = settings.native_parameters(values)?;
                    let curve = trial(r, &r.pillars, &native, supplied)?;
                    (0..r.helpers.len())
                        .map(|i| {
                            value(p, i, &curve, supplied, evaluations)
                                .map(|(q, _)| q.value - r.helpers[i].quote().value)
                        })
                        .collect::<CashCalibrationResult<Vec<_>>>()
                },
            )
            .map_err(|e| CashCalibrationError::Global(Box::new(e)))?;
            let iterations = fit.diagnostics.iterations;
            let native = settings.native_parameters(&fit.parameters)?;
            let model = settings.initial.with_parameters(&native)?;
            (
                native,
                CashSolverDiagnostics::Parametric {
                    parameters: model,
                    diagnostics: fit.diagnostics,
                },
                iterations,
            )
        }
        (CalibrationMode::Piecewise, Some(settings)) => {
            let mut order: Vec<_> = (0..r.helpers.len()).collect();
            order.sort_by_key(|&i| r.helpers[i].pillar());
            let mut parameters = Vec::new();
            let mut roots = Vec::new();
            let mut ids = Vec::new();
            let mut iterations = 0;
            for (n, i) in order.into_iter().enumerate() {
                let h = &r.helpers[i];
                let state = RefCell::new((&mut *evaluations, None, Vec::new()));
                let result = brent(
                    |parameter| {
                        let mut s = state.borrow_mut();
                        if s.1.is_some() {
                            return f64::NAN;
                        }
                        let mut values = parameters.clone();
                        values.push(parameter);
                        let priced = trial(r, &r.pillars[..=n], &values, supplied).and_then(|c| {
                            value(p, i, &c, supplied, s.0).map(|(q, _)| q.value - h.quote().value)
                        });
                        match priced {
                            Ok(v) => {
                                s.2.push(v);
                                v
                            }
                            Err(e) => {
                                s.1 = Some(e);
                                f64::NAN
                            }
                        }
                    },
                    settings.lower_parameter,
                    settings.upper_parameter,
                    &SolverConfig {
                        tolerance: h.tolerance(),
                        max_iterations: settings.max_iterations,
                    },
                );
                let (_, error, residuals) = state.into_inner();
                if let Some(e) = error {
                    return Err(e);
                }
                if residuals.len() >= 2
                    && (residuals[0] - residuals[1]).abs() <= 2.0 * h.tolerance()
                {
                    return Err(invalid(
                        "piecewise identification",
                        format!(
                            "{} cannot distinguish bracket endpoints at its native quote precision",
                            h.id()
                        ),
                    ));
                }
                let root = result?;
                iterations += root.iterations as usize;
                parameters.push(root.root);
                roots.push(root);
                ids.push(h.id().into());
            }
            (
                parameters,
                CashSolverDiagnostics::Piecewise {
                    helper_ids: ids,
                    roots,
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
            let mut order: Vec<_> = (0..r.helpers.len()).collect();
            order.sort_by(|&a, &b| r.helpers[a].id().cmp(r.helpers[b].id()));
            let residuals: Vec<_> = order
                .iter()
                .map(|&i| FitResidual {
                    scale: settings.quote_scales[i],
                    weight: settings.weights[i],
                    tolerance: r.helpers[i].tolerance(),
                })
                .collect();
            let fit = bounded_least_squares(
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
                    let curve = trial(r, &r.pillars, values, supplied)?;
                    order
                        .iter()
                        .map(|&i| {
                            value(p, i, &curve, supplied, evaluations)
                                .map(|(q, _)| q.value - r.helpers[i].quote().value)
                        })
                        .collect::<CashCalibrationResult<Vec<_>>>()
                },
            )
            .map_err(|e| CashCalibrationError::Global(Box::new(e)))?;
            let iterations = fit.diagnostics.iterations;
            (
                fit.parameters,
                CashSolverDiagnostics::Global(fit.diagnostics),
                iterations,
            )
        }
        _ => return Err(invalid("mode", "invalid admitted fit")),
    };
    let c = trial(r, &r.pillars, &parameters, supplied)?;
    let mut quotes = Vec::new();
    let mut values = Vec::new();
    for (i, h) in r.helpers.iter().enumerate() {
        let (model, priced) = value(p, i, &c, supplied, evaluations)?;
        let residual = model.value - h.quote().value;
        if !residual.is_finite() || residual.abs() > h.tolerance() {
            return Err(CurveContractError::RepricingFailure {
                helper_id: h.id().into(),
                residual,
                tolerance: h.tolerance(),
            }
            .into());
        }
        quotes.push(QuoteRepricing {
            helper_id: h.id().into(),
            pillar: h.pillar(),
            input: h.quote(),
            model,
            residual,
            tolerance: h.tolerance(),
        });
        values.push(priced);
    }
    let is_parametric = matches!(r.curve.calibration, CalibrationMode::Parametric(_));
    let curve = if is_parametric {
        build_fitted_parametric_curve(
            &r.curve,
            *r.pillars
                .last()
                .ok_or_else(|| invalid("horizon", "empty"))?,
            &parameters,
            quotes,
            iterations,
            supplied,
        )?
    } else if r.basis_model.is_some() {
        let grid = nodes(r.curve.reference_date, &r.pillars, &parameters);
        build_fitted_issuer_curve(&r.curve, &grid, quotes, iterations, supplied)?
    } else {
        let grid = nodes(r.curve.reference_date, &r.pillars, &parameters);
        build_fitted_rate_curve(&r.curve, &grid, quotes, iterations, supplied)?
    };
    Ok(CalibratedCashCurve {
        recipe: r.clone(),
        curve,
        annualized_parameters: if is_parametric {
            Vec::new()
        } else {
            parameters
        },
        repriced_helpers: values,
        solver,
        price_evaluations: *evaluations - before,
    })
}
