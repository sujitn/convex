//! Bounded native-zero fitting; product quotes retain their own calibration owners.

use super::{
    build::point_tenor,
    build_curve_set,
    structured::{observation_times, ParametricModel},
    BuiltCurve, CalibrationMode, CurveContractError as Error, CurveFitError, CurveRecipe, CurveSet,
    CurveSource, FitNumericalControls,
};
use convex_math::optimization::{
    bounded_least_squares, FitDiagnostics, FitParameter, FitResidual, LeastSquaresSettings,
    MAX_FIT_DIMENSION,
};
use serde::{Deserialize, Serialize};

/// Original product-price objective. Numerical difference/rank controls come
/// from the enclosing complete graph, with no second ignored control policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProductParametricSettings {
    /// Original family and all starting parameters, including decays in years.
    pub initial: ParametricModel,
    /// Closed lower bounds in the model's exact parameter order.
    pub lower: Vec<f64>,
    /// Closed upper bounds in the same order.
    pub upper: Vec<f64>,
    /// Positive native parameter scales. The solver uses native/scale coordinates;
    /// graph Jacobian steps and measured conditioning refer to those coordinates.
    pub parameter_scales: Vec<f64>,
    /// Positive native-unit scales, one per full original product in source order.
    pub quote_scales: Vec<f64>,
    /// Nonnegative weights; zero never waives the original residual postcondition.
    pub weights: Vec<f64>,
    /// Bounded LM attempts, 1..256, within the original cash-fit ceiling.
    pub max_iterations: usize,
    /// Positive weighted scaled RMS target, at most one.
    pub objective_tolerance: f64,
}
impl ProductParametricSettings {
    /// Validate complete starts/bounds and original observation dimensions before
    /// product trial valuation. Undamped identification remains solver-owned.
    pub fn validate(&self, quotes: usize) -> super::CurveContractResult<()> {
        let starts = self.initial.parameters();
        if quotes < starts.len()
            || quotes > MAX_FIT_DIMENSION
            || self.lower.len() != starts.len()
            || self.upper.len() != starts.len()
            || self.parameter_scales.len() != starts.len()
            || self
                .parameter_scales
                .iter()
                .any(|s| !s.is_finite() || *s <= 0.0)
            || self.quote_scales.len() != quotes
            || self.weights.len() != quotes
            || self.max_iterations == 0
            || self.max_iterations > 256
            || !self.objective_tolerance.is_finite()
            || self.objective_tolerance <= 0.0
            || self.objective_tolerance > 1.0
            || self
                .quote_scales
                .iter()
                .any(|s| !s.is_finite() || *s <= 0.0)
            || self.weights.iter().any(|w| !w.is_finite() || *w < 0.0)
            || self.weights.iter().filter(|w| **w > 0.0).count() < starts.len()
        {
            return Err(Error::invalid("product parametric settings", "exact four/six bounded parameters, enough positive-weight full quotes, native scales and bounded solver settings required"));
        }
        self.initial.validate()?;
        self.initial.with_parameters(&self.lower)?.validate()?;
        self.initial.with_parameters(&self.upper)?.validate()?;
        for (i, ((initial, lower), upper)) in
            starts.iter().zip(&self.lower).zip(&self.upper).enumerate()
        {
            let scale = self.parameter_scales[i];
            if lower >= upper
                || !(lower..=upper).contains(&initial)
                || !(upper - lower).is_finite()
                || !(lower / scale).is_finite()
                || !(upper / scale).is_finite()
                || !(upper / scale - lower / scale).is_finite()
                || lower / scale >= upper / scale
            {
                return Err(Error::invalid(
                    "product parameter bounds",
                    "finite ordered closed intervals containing every start required",
                ));
            }
        }
        Ok(())
    }
    /// Checked numerical starts and closed native bounds; no zero-node alias.
    pub fn fit_parameters(&self, quotes: usize) -> super::CurveContractResult<Vec<FitParameter>> {
        self.validate(quotes)?;
        Ok(self
            .initial
            .parameters()
            .into_iter()
            .zip(&self.lower)
            .zip(&self.upper)
            .zip(&self.parameter_scales)
            .map(|(((initial, lower), upper), scale)| FitParameter {
                initial: initial / scale,
                lower: lower / scale,
                upper: upper / scale,
            })
            .collect())
    }
    /// Restore actual loadings/decay years; enforce original closed bounds again
    /// after multiplication to remove only rounding at a numerical endpoint.
    pub fn native_parameters(&self, values: &[f64]) -> super::CurveContractResult<Vec<f64>> {
        self.validate(self.quote_scales.len())?;
        if values.len() != self.parameter_scales.len() {
            return Err(Error::invalid(
                "scaled parameters",
                "exact native model dimension required",
            ));
        }
        values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let native = v * self.parameter_scales[i];
                if !native.is_finite()
                    || *v < self.lower[i] / self.parameter_scales[i]
                    || *v > self.upper[i] / self.parameter_scales[i]
                {
                    return Err(Error::invalid(
                        "scaled parameters",
                        "finite values inside original closed bounds required",
                    ));
                }
                Ok(native.clamp(self.lower[i], self.upper[i]))
            })
            .collect()
    }
}

/// Complete native scaled objective and original parameter bounds. Parameter
/// order is `ParametricModel::parameters`; quote vectors retain source order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ParametricFitSettings {
    /// One closed lower bound per original parameter.
    pub lower: Vec<f64>,
    /// One closed upper bound per original parameter.
    pub upper: Vec<f64>,
    /// One positive native-unit residual scale per original observation.
    pub quote_scales: Vec<f64>,
    /// One nonnegative objective weight per observation; zero does not waive
    /// any native residual postcondition or undamped identification.
    pub weights: Vec<f64>,
    /// Original bounded solver attempt budget, without adaptive retry.
    pub max_iterations: usize,
    /// Original weighted scaled RMS stopping bound.
    pub objective_tolerance: f64,
    /// Explicit original finite-difference and undamped SVD identification policy.
    pub controls: FitNumericalControls,
}
/// Complete input, actual solved curve and measured original solver outcome.
#[derive(Debug, Clone)]
pub struct ParametricFitOutput {
    /// Original full recipe and initial parameters; never overwritten by a fit.
    pub input_recipe: CurveRecipe,
    /// Original closed bounds/scales/weights/settings, in original source order.
    pub settings: ParametricFitSettings,
    /// Validated actual solved parameters and every original native quote residual.
    pub curve: BuiltCurve,
    /// Measured attempts/evaluations/objective/native residuals/rank/condition/bounds.
    pub solver: FitDiagnostics,
}

/// Fit original continuous-zero observations in their native units using the
/// shared bounded solver. All original dated observations must pass, including
/// zero-weight inputs. Direct supplied-parameter construction remains separate.
/// Underidentification/convergence/domain failures retain solver diagnostics and
/// return no curve; no grid, product-quote, objective or method substitution.
pub fn fit_parametric_curve(
    recipe: &CurveRecipe,
    settings: &ParametricFitSettings,
) -> std::result::Result<ParametricFitOutput, CurveFitError> {
    let CurveSource::Parametric {
        parameters,
        horizon,
        observations,
    } = &recipe.source
    else {
        return Err(Error::unsupported(
            "parametric fit",
            "complete original Parametric source required",
        )
        .into());
    };
    if recipe.calibration != CalibrationMode::Direct {
        return Err(Error::invalid("parametric fit mode", "explicit function fits supplied parameters; grid Global/Piecewise settings cannot impersonate a parametric objective").into());
    }
    let starts = parameters.parameters();
    if settings.lower.len() != starts.len()
        || settings.upper.len() != starts.len()
        || observations.is_empty()
        || observations.len() > MAX_FIT_DIMENSION
        || settings.quote_scales.len() != observations.len()
        || settings.weights.len() != observations.len()
    {
        return Err(Error::invalid("parametric fit dimensions", "exact original four/six parameters and 1..128 matching observations/scales/weights required").into());
    }
    parameters.with_parameters(&settings.lower)?.validate()?;
    parameters.with_parameters(&settings.upper)?.validate()?;
    let end = point_tenor(recipe.reference_date, recipe.time_basis, *horizon)?;
    let times = observation_times(recipe, observations, end)?;
    // Original generic role/key/date/time/source/method/representation guards
    // run before the solver. Initial parameters need not reprice target quotes.
    let mut checked = recipe.clone();
    if let CurveSource::Parametric { observations, .. } = &mut checked.source {
        observations.clear();
    }
    build_curve_set(std::slice::from_ref(&checked), &CurveSet::new())?;
    let contracts: Vec<_> = observations
        .iter()
        .zip(&settings.quote_scales)
        .zip(&settings.weights)
        .map(|((input, scale), weight)| FitResidual {
            scale: *scale,
            weight: *weight,
            tolerance: input.tolerance,
        })
        .collect();
    let bounds: Vec<_> = starts
        .iter()
        .zip(&settings.lower)
        .zip(&settings.upper)
        .map(|((initial, lower), upper)| FitParameter {
            initial: *initial,
            lower: *lower,
            upper: *upper,
        })
        .collect();
    let numerical = LeastSquaresSettings {
        max_iterations: settings.max_iterations,
        objective_tolerance: settings.objective_tolerance,
        jacobian_step: settings.controls.jacobian_step,
        minimum_singular_ratio: settings.controls.minimum_singular_ratio,
        svd_iterations: settings.controls.svd_iterations,
    };
    let fit = bounded_least_squares(&bounds, &contracts, numerical, |values| {
        let model = parameters.with_parameters(values)?;
        model.validate()?;
        observations
            .iter()
            .zip(&times)
            .map(|(input, t)| {
                Ok(input.quote.model_rate_quote(model.zero(*t)?)?.value - input.quote.value)
            })
            .collect::<super::CurveContractResult<Vec<f64>>>()
    })?;
    let solved: ParametricModel = parameters.with_parameters(&fit.parameters)?;
    let mut actual = recipe.clone();
    if let CurveSource::Parametric { parameters, .. } = &mut actual.source {
        *parameters = solved;
    }
    // Original final native-unit postconditions run again on the complete actual
    // source. Failed final pricing contains no accepted partial curve.
    let output = build_curve_set(std::slice::from_ref(&actual), &CurveSet::new())?;
    let mut curve = output.curves.get(&recipe.key)?.as_ref().clone();
    curve.diagnostics.iterations = fit.diagnostics.iterations;
    curve.diagnostics.method = "bounded_scaled_native_parametric_zero_fit".into();
    Ok(ParametricFitOutput {
        input_recipe: recipe.clone(),
        settings: settings.clone(),
        curve,
        solver: fit.diagnostics,
    })
}
