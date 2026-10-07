//! Atomic deterministic graph validation/preparation; no partial publication.

use convex_core::{fixings::FixingSeries, types::Compounding};
use convex_curves::{contracts::*, InterpolationMethod};
use convex_math::solvers::SolverConfig;
use std::collections::{BTreeMap, BTreeSet};

use super::{fit, helpers, *};

pub(super) fn validate(
    recipe: &RateCurveRecipe,
    context: RateCalibrationContext,
) -> Result<(), RateCalibrationError> {
    let curve = &recipe.curve;
    curve.key.validate()?;
    let CurveSource::Helpers(ids) = &curve.source else {
        return Err(invalid(
            "source",
            "complete ordered helper identities required",
        ));
    };
    if recipe.helpers.is_empty()
        || recipe.helpers.len() > MAX_RATE_FIT_HELPERS
        || recipe.pillars.is_empty()
        || recipe.pillars.len() > MAX_RATE_FIT_HELPERS
        || recipe.helpers.len() != ids.len()
        || curve.reference_date > context.cashflows.fixings.valuation_date
        || curve.representation
            != (CurveRepresentation::ZeroRates {
                compounding: Compounding::Continuous,
            })
    {
        return Err(invalid("recipe","bounded positive full helper/parameter dimensions and continuous-zero dated representation required"));
    }
    match &curve.role {
        CurveRole::Discount {
            collateral_currency,
        } if collateral_currency == &curve.currency => (),
        CurveRole::Projection { .. } if recipe.overnight_projection.is_none() => (),
        _ => {
            return Err(unsupported(
                "supports collateral discount and separate index projection only",
            ))
        }
    }
    if let Some(p) = &recipe.overnight_projection {
        p.key.validate()?;
        if p.key.id == curve.key.id
            || p.index_id.is_empty()
            || p.index_id.len() > 128
            || p.index_id.trim() != p.index_id
            || p.index_id.chars().any(char::is_control)
        {
            return Err(invalid(
                "overnight projection",
                "distinct exact projection identity and bounded explicit index required",
            ));
        }
    }
    for (i, date) in recipe.pillars.iter().enumerate() {
        if *date <= curve.reference_date || (i > 0 && *date <= recipe.pillars[i - 1]) {
            return Err(invalid(
                "pillars",
                "strictly increasing dates after curve reference required",
            ));
        }
    }
    let mut identities = BTreeSet::new();
    let mut dates = BTreeSet::new();
    for (h, id) in recipe.helpers.iter().zip(ids) {
        if &h.id != id
            || id.is_empty()
            || id.len() > 128
            || id.trim() != id
            || id.chars().any(char::is_control)
            || !identities.insert(id)
            || h.pillar <= context.cashflows.fixings.valuation_date
            || h.pillar
                > *recipe
                    .pillars
                    .last()
                    .ok_or_else(|| invalid("pillars", "empty"))?
            || !h.tolerance.is_finite()
            || h.tolerance <= 0.0
        {
            return Err(invalid("helpers","unique original ordered IDs, future bounded pillars and positive native tolerances required"));
        }
        h.quote.validate()?;
        if !dates.insert(h.pillar) && recipe.overlap_policy == RateOverlapPolicy::Reject {
            return Err(invalid(
                "overlapping pillars",
                format!("duplicate original pillar {}", h.pillar),
            ));
        }
    }
    match (&curve.calibration,recipe.bootstrap) {
        (CalibrationMode::Piecewise,Some(s)) => {
            SolverConfig { tolerance: 1e-12,max_iterations:s.max_iterations }.validate()?;
            if curve.interpolation!=InterpolationMethod::Linear || recipe.overlap_policy!=RateOverlapPolicy::Reject
                || recipe.helpers.len()!=recipe.pillars.len() || dates.into_iter().collect::<Vec<_>>()!=recipe.pillars
                || !s.lower_zero.is_finite() || !s.upper_zero.is_finite() || s.lower_zero>=s.upper_zero || s.lower_zero < -10.0 || s.upper_zero>10.0 {
                return Err(unsupported("piecewise requires Linear continuous zeros, one original per pillar and a finite closed bracket within [-10,10]"));
            }
        }
        (CalibrationMode::Global(s),None) => {
            if !matches!(curve.interpolation,InterpolationMethod::Linear|InterpolationMethod::CubicSpline)
                || s.initial_zeros.len()!=recipe.pillars.len() || s.quote_scales.len()!=recipe.helpers.len()
                || s.weights.len()!=recipe.helpers.len() || s.weights.iter().any(|w|!w.is_finite()||*w<=0.0)
                || !s.lower_zero.is_finite() || !s.upper_zero.is_finite() || s.lower_zero>=s.upper_zero || s.lower_zero < -10.0 || s.upper_zero>10.0
                || s.initial_zeros.iter().any(|v|!v.is_finite()||*v<s.lower_zero||*v>s.upper_zero)
                || s.quote_scales.iter().any(|v|!v.is_finite()||*v<=0.0) {
                return Err(invalid("global settings","Linear/CubicSpline zeros, matching starts/positive native scales/weights and bounded closed zero rates required"));
            }
        }
        _ => return Err(invalid("calibration","Piecewise requires its explicit bracket; Global requires original settings with no bootstrap override")),
    }
    Ok(())
}

pub(super) fn alias(recipe: &RateCurveRecipe) -> Option<CurveRecipe> {
    recipe.overnight_projection.as_ref().map(|p| CurveRecipe {
        key: p.key.clone(),
        role: CurveRole::Projection {
            index_id: p.index_id.clone(),
        },
        currency: recipe.curve.currency,
        reference_date: recipe.curve.reference_date,
        time_basis: recipe.curve.time_basis,
        representation: recipe.curve.representation,
        interpolation: recipe.curve.interpolation,
        extrapolation: recipe.curve.extrapolation,
        dependencies: vec![recipe.curve.key.clone()],
        calibration: CalibrationMode::Direct,
        source: CurveSource::OvernightProjection {
            discount: recipe.curve.key.clone(),
            index_id: p.index_id.clone(),
        },
    })
}

/// Build/rebuild a complete supplied mixed-product curve graph through public
/// pure pricers. Freeze publication times once per unique observation and resolve
/// the entire graph's due history before any trial index projection. Quote order
/// cannot choose a different source or duplicate-pillar winner. Each global solve
/// is per dependency-ordered curve; cyclic simultaneous equations are rejected.
/// Failure returns no partial set and never changes supplied last-good curves.
pub fn build_rate_curve_set<P>(
    inputs: &[RateCurveBuildInput<'_>],
    supplied: &CurveSet,
    history: &FixingSeries,
    context: RateCalibrationContext,
    controls: FitNumericalControls,
    mut administrator_publication: P,
) -> Result<RateCurveBuildOutput, RateCalibrationError>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    if inputs.is_empty()
        || inputs.len() > MAX_CURVES
        || inputs
            .iter()
            .try_fold(0usize, |n, i| n.checked_add(i.recipe.helpers.len()))
            .is_none_or(|n| n > MAX_RATE_FIT_HELPERS)
        || inputs
            .iter()
            .try_fold(0usize, |n, i| n.checked_add(i.recipe.pillars.len()))
            .is_none_or(|n| n > MAX_RATE_FIT_HELPERS)
        || Date::from(context.as_of.datetime().date_naive())
            != context.cashflows.fixings.valuation_date
    {
        return Err(invalid(
            "batch",
            "bounded nonempty complete graph and consistent economic timestamp/date required",
        ));
    }
    let mut output_keys = BTreeMap::new();
    for input in inputs {
        validate(input.recipe, context)?;
        if input.recipe.helpers.len() != input.resources.len() {
            return Err(invalid(
                "resources",
                "one actual resource set per original helper required",
            ));
        }
        for key in std::iter::once(&input.recipe.curve.key)
            .chain(input.recipe.overnight_projection.iter().map(|p| &p.key))
        {
            if supplied.keys().any(|k| k.id == key.id)
                || output_keys.insert(key.id.clone(), key.clone()).is_some()
            {
                return Err(invalid(
                    "curve identity",
                    "duplicate outputs or replacement of a supplied immutable curve",
                ));
            }
        }
    }
    if supplied.len() + output_keys.len() > MAX_CURVES {
        return Err(CurveContractError::BoundExceeded("rate curve set").into());
    }
    let mut publications = Publications::new();
    let mut missing = Publications::new();
    let mut pending = BTreeMap::new();
    for input in inputs {
        let p = helpers::prepare(
            input,
            history,
            context,
            &mut publications,
            &mut missing,
            &mut administrator_publication,
        )?;
        for key in &input.recipe.curve.dependencies {
            if supplied.get(key).is_err() && output_keys.get(&key.id) != Some(key) {
                return Err(CurveContractError::MissingDependency(key.clone()).into());
            }
        }
        pending.insert(input.recipe.curve.key.id.clone(), p);
    }
    if !missing.is_empty() {
        return Err(CashflowError::MissingFixings(missing.into_values().collect()).into());
    }
    // Validate the full topology before solving anything, including a late cycle.
    let mut available: BTreeSet<_> = supplied.keys().cloned().collect();
    let mut ordered = Vec::new();
    let mut todo: BTreeSet<_> = pending.keys().cloned().collect();
    for _ in 0..inputs.len() {
        let ready = todo
            .iter()
            .find(|id| {
                pending.get(*id).is_some_and(|p| {
                    p.recipe
                        .curve
                        .dependencies
                        .iter()
                        .all(|k| available.contains(k))
                })
            })
            .cloned();
        let Some(id) = ready else {
            return Err(CurveContractError::DependencyCycle(
                todo.iter()
                    .filter_map(|id| pending.get(id).map(|p| p.recipe.curve.key.clone()))
                    .collect(),
            )
            .into());
        };
        let p = pending
            .get(&id)
            .ok_or_else(|| invalid("topology", "ready recipe disappeared"))?;
        available.insert(p.recipe.curve.key.clone());
        if let Some(v) = &p.recipe.overnight_projection {
            available.insert(v.key.clone());
        }
        todo.remove(&id);
        ordered.push(id);
    }
    let mut curves = supplied.clone();
    let mut build_order = Vec::new();
    let mut calibrations = Vec::new();
    let mut evaluations = 0;
    for id in ordered {
        let p = pending
            .remove(&id)
            .ok_or_else(|| invalid("recipes", "ordered recipe disappeared"))?;
        let fitted = fit::fit(
            &p,
            &curves,
            history,
            context,
            &publications,
            controls,
            &mut evaluations,
        )?;
        curves = curves.with_curve(fitted.curve.clone())?;
        build_order.push(p.recipe.curve.key.clone());
        if let Some(view) = alias(p.recipe) {
            let built = build_curve_set(&[view], &curves)?;
            curves = built.curves;
            build_order.extend(built.build_order);
        }
        calibrations.push(fitted);
    }
    Ok(RateCurveBuildOutput {
        curves,
        build_order,
        calibrations,
        publications: publications.into_values().collect(),
        price_evaluations: evaluations,
    })
}
