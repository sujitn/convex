//! Full supplied CDS helpers composed with checked curves and bounded solvers.
//! No provider, database, MQ, system clock or external model is consulted.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_core::{contracts::Version, daycounts::DayCountConvention, Date};
use convex_credit::{
    price_cds, CdsCalendars, CdsContract, CdsError, CdsPricingContext, CdsTerms, CdsValuation,
};
use convex_curves::{
    contracts::{
        build_curve_set, build_fitted_credit_curve, credit_survival_nodes, BuiltCurve,
        CalibrationMode, CurveContractError, CurveKey, CurveRecipe, CurveRepresentation, CurveRole,
        CurveSet, CurveSource, FitNumericalControls, MarketQuote, QuoteRepricing, QuoteUnit,
        MAX_CURVES,
    },
    ExtrapolationMethod, InterpolationMethod,
};
use convex_math::{
    optimization::{
        bounded_least_squares, FitDiagnostics, FitFailure, FitParameter, FitResidual,
        LeastSquaresSettings, MAX_FIT_DIMENSION,
    },
    solvers::{brent, SolverConfig, SolverResult},
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

/// Total helpers/parameters in one atomic credit batch.
pub const MAX_CREDIT_FIT_HELPERS: usize = MAX_FIT_DIMENSION;
/// Total actual single-helper valuations, including final native repricing.
pub const MAX_CREDIT_PRICE_EVALUATIONS: usize = 8192;

/// Native CDS quotation; clean and dirty amounts are never interchangeable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CdsQuoteKind {
    /// CLEAN par running spread; DecimalRate/PercentRate/BasisPoints.
    RunningSpread,
    /// CLEAN buyer-to-seller upfront fraction, at the retained actual coupon.
    CleanUpfront,
    /// CASH buyer-to-seller fraction, including accrued refund at actual coupon.
    DirtyUpfront,
}

/// Full original economic and quote input. Nothing is reduced to a tenor proxy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CdsCalibrationHelper {
    /// Stable provider-neutral helper identity, unique within the curve.
    pub id: String,
    /// Actual immutable original quote revision, selected by the caller.
    pub quote_version: Version,
    /// Actual unadjusted protection maturity, including its final protected day.
    pub pillar: Date,
    /// Complete original terms and calendar/document/curve versions.
    pub terms: CdsTerms,
    /// Explicit surviving valuation and cash state for the quoted instrument.
    pub context: CdsPricingContext,
    /// Meaning of the native quote.
    pub quote_kind: CdsQuoteKind,
    /// Original value/unit; upfront fractions never become rate basis points.
    pub quote: MarketQuote,
    /// Positive absolute tolerance in the ORIGINAL quote unit.
    pub tolerance: f64,
}

/// Closed nonnegative intensity interval for each causal piecewise root.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CdsBootstrapSettings {
    /// Lower ACT/365F forward intensity, at least zero.
    pub lower_hazard: f64,
    /// Upper ACT/365F forward intensity, at most five.
    pub upper_hazard: f64,
    /// Actual bounded Brent budget per pillar; no bracket expansion.
    pub max_iterations: u32,
}

/// Complete credit recipe over an ACT/365F log-linear survival grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CdsCurveRecipe {
    /// Exact identity/entity/seniority/recovery/currency/dependencies and method.
    pub curve: CurveRecipe,
    /// Strictly increasing intensity-interval end dates, supplied explicitly.
    pub pillars: Vec<Date>,
    /// Full terms and quote revisions in ORIGINAL input order.
    pub helpers: Vec<CdsCalibrationHelper>,
    /// Required only for Piecewise. Global uses CreditGlobal settings.
    pub bootstrap: Option<CdsBootstrapSettings>,
}

/// Actual immutable independently pinned calendars, in helper source order.
pub struct CdsCurveBuildInput<'a> {
    /// Full supplied economic/numerical recipe.
    pub recipe: &'a CdsCurveRecipe,
    /// One actual calendar set for each original helper.
    pub calendars: &'a [CdsCalendars<'a>],
}

/// Typed admission/product/solver failures. No partial accepted curve is returned.
#[derive(Debug, thiserror::Error)]
pub enum CreditCalibrationError {
    /// Original immutable curve/dependency/native repricing error.
    #[error(transparent)]
    Curve(#[from] CurveContractError),
    /// Original CDS terms/role/date/valuation failure.
    #[error(transparent)]
    Cds(#[from] CdsError),
    /// Original failed pillar with actual common solver error.
    #[error("CDS helper {helper_id}: {source}")]
    Root {
        /// Failed original helper identity.
        helper_id: String,
        /// Bracket, finite arithmetic or convergence failure.
        source: convex_math::MathError,
    },
    /// Failed global fit retains measured rank, residual and work information.
    #[error(transparent)]
    Global(Box<FitFailure<CreditCalibrationError>>),
}

/// Measured numerical work; source-order native records live on the curve.
#[derive(Debug, Clone)]
pub enum CreditSolverDiagnostics {
    /// Sequential roots in increasing pillar order, each with native residual.
    Piecewise {
        /// Actual helper IDs, not assumed source ordering.
        helper_ids: Vec<String>,
        /// Actual roots, residuals and iterations.
        roots: Vec<SolverResult>,
    },
    /// Common undamped identification/convergence diagnostics.
    Global(FitDiagnostics),
}

/// Accepted fit retaining full inputs, actual parameters and full repricing.
#[derive(Debug, Clone)]
pub struct CalibratedCdsCurve {
    /// Original source-order inputs, including exact quote/calendar/reference pins.
    pub recipe: CdsCurveRecipe,
    /// Immutable survival curve; recipe remains Helpers and actually fitted mode.
    pub curve: BuiltCurve,
    /// Solved nonnegative forward intensities, one per declared interval.
    pub hazards_per_act365f_year: Vec<f64>,
    /// Actual complete helper leg/quote valuations, in original source order.
    pub repriced_helpers: Vec<CdsValuation>,
    /// Actual selected solver outcome.
    pub solver: CreditSolverDiagnostics,
    /// Actual helper valuations, including final native repricing.
    pub price_evaluations: usize,
}

/// All-or-error immutable output; supplied and last-good inputs are untouched.
#[derive(Debug, Clone)]
pub struct CreditCurveBuildOutput {
    /// Supplied exact discount dependencies plus all accepted credit fits.
    pub curves: CurveSet,
    /// Deterministic curve-ID order; all discount dependencies are supplied.
    pub build_order: Vec<CurveKey>,
    /// Full actual fit and financial diagnostics for each credit curve.
    pub calibrations: Vec<CalibratedCdsCurve>,
    /// Actual work across the entire batch, not a per-curve ceiling reset.
    pub price_evaluations: usize,
}

pub(crate) fn invalid(field: &str, reason: &str) -> CreditCalibrationError {
    CurveContractError::InvalidInput {
        field: field.into(),
        reason: reason.into(),
    }
    .into()
}
pub(crate) fn label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

/// Recover the model quote in this helper's original unit from the full pricer.
pub fn cds_model_quote(
    helper: &CdsCalibrationHelper,
    value: &CdsValuation,
) -> Result<MarketQuote, CreditCalibrationError> {
    helper.quote.validate()?;
    if helper.quote_kind != CdsQuoteKind::RunningSpread
        && helper.quote.unit != QuoteUnit::UpfrontFraction
    {
        return Err(invalid(
            "quote.unit",
            "clean/dirty upfront requires its original fraction unit",
        ));
    }
    let quote = match helper.quote_kind {
        CdsQuoteKind::RunningSpread => helper.quote.model_rate_quote(
            value
                .par_spread_decimal
                .ok_or_else(|| invalid("par_quote", "no positive clean risky annuity"))?,
        )?,
        CdsQuoteKind::CleanUpfront => MarketQuote {
            value: value.clean_upfront_fraction,
            unit: QuoteUnit::UpfrontFraction,
        },
        CdsQuoteKind::DirtyUpfront => MarketQuote {
            value: value.dirty_upfront_fraction,
            unit: QuoteUnit::UpfrontFraction,
        },
    };
    quote.validate()?;
    Ok(quote)
}

fn validate(recipe: &CdsCurveRecipe, supplied: &CurveSet) -> Result<(), CreditCalibrationError> {
    let c = &recipe.curve;
    c.key.validate()?;
    let CurveSource::Helpers(ids) = &c.source else {
        return Err(invalid(
            "source",
            "ordered full CDS helper identities required",
        ));
    };
    let CurveRole::Credit {
        entity_id,
        seniority,
        recovery,
    } = &c.role
    else {
        return Err(invalid("role", "Credit role required"));
    };
    if !label(entity_id)
        || !label(seniority)
        || !recovery.is_finite()
        || !(0.0..1.0).contains(recovery)
        || c.representation != CurveRepresentation::SurvivalProbabilities
        || c.time_basis != DayCountConvention::Act365Fixed
        || c.interpolation != InterpolationMethod::LogLinear
        || c.extrapolation != ExtrapolationMethod::None
        || recipe.helpers.is_empty()
        || recipe.helpers.len() > MAX_CREDIT_FIT_HELPERS
        || ids.len() != recipe.helpers.len()
        || recipe.pillars.is_empty()
        || recipe.pillars.len() > MAX_FIT_DIMENSION
    {
        return Err(invalid("credit.recipe","bounded full CDS helpers; nonidentified full recovery excluded; explicit ACT/365F log-linear survival without extrapolation required"));
    }
    credit_survival_nodes(
        c.reference_date,
        &recipe.pillars,
        &vec![0.0; recipe.pillars.len()],
    )?;
    if supplied.keys().any(|key| key.id == c.key.id) {
        return Err(invalid(
            "curve.id",
            "cannot replace a supplied immutable curve",
        ));
    }
    match (&c.calibration,recipe.bootstrap) {
        (CalibrationMode::Piecewise,Some(b))=>{
            SolverConfig {tolerance:1e-12,max_iterations:b.max_iterations}.validate().map_err(|source|CreditCalibrationError::Root {helper_id:"settings".into(),source})?;
            let mut helper_dates:Vec<_>=recipe.helpers.iter().map(|h|h.pillar).collect();helper_dates.sort();
            if helper_dates!=recipe.pillars || !b.lower_hazard.is_finite() || !b.upper_hazard.is_finite()
                || b.lower_hazard<0.0 || b.upper_hazard>5.0 || b.lower_hazard>=b.upper_hazard {
                return Err(invalid("bootstrap","one unique helper per dated pillar and closed finite intensity bounds in [0,5] required"));
            }
        },
        (CalibrationMode::CreditGlobal(s),None)=>{
            if s.initial_hazards.len()!=recipe.pillars.len() || s.quote_scales.len()!=recipe.helpers.len()
                || s.weights.len()!=recipe.helpers.len() || recipe.helpers.len()<recipe.pillars.len()
                || !s.lower_hazard.is_finite() || !s.upper_hazard.is_finite() || s.lower_hazard<0.0 || s.upper_hazard>5.0 || s.lower_hazard>=s.upper_hazard
                || s.initial_hazards.iter().any(|h|!h.is_finite() || *h<s.lower_hazard || *h>s.upper_hazard)
                || s.quote_scales.iter().any(|v|!v.is_finite() || *v<=0.0)
                || s.weights.iter().any(|v|!v.is_finite() || *v<0.0) || s.weights.iter().filter(|v|**v>0.0).count()<recipe.pillars.len() {
                return Err(invalid("global","matching bounded starts, native scales and identifying weights required"));
            }
        },
        _=>return Err(invalid("calibration","Piecewise requires its bracket; CreditGlobal requires its own intensity settings and no bootstrap override")),
    }
    let mut seen = BTreeSet::new();
    let mut dependencies = BTreeSet::new();
    let first = &recipe.helpers[0].terms;
    for (h, id) in recipe.helpers.iter().zip(ids) {
        h.quote.validate()?;
        let units = match h.quote_kind {
            CdsQuoteKind::RunningSpread => h.quote.decimal_rate().is_ok() && h.quote.value >= 0.0,
            _ => h.quote.unit == QuoteUnit::UpfrontFraction,
        };
        if &h.id != id
            || !label(id)
            || !seen.insert(id)
            || !units
            || !h.tolerance.is_finite()
            || !(0.0..=1.0).contains(&h.tolerance)
            || h.tolerance == 0.0
            || h.pillar != h.terms.premium_schedule.end_date
            || h.pillar <= c.reference_date
            || h.context.valuation_date != c.reference_date
            || h.context.step_in_date > h.pillar
            || h.terms.currency != c.currency
            || h.terms.survival_curve != c.key
            || h.terms.reference != first.reference
            || h.terms.model != first.model
            || h.terms.reference.entity_id.as_str() != entity_id
            || h.terms.reference.seniority.as_str() != seniority
            || h.terms.reference.recovery_fraction.get() != *recovery
        {
            return Err(invalid("helper","unique original quotes/units/tolerances and exact surviving date/entity/seniority/document/model/recovery/currency/curve inputs required"));
        }
        dependencies.insert(h.terms.discount_curve.clone());
    }
    if c.dependencies.len() != dependencies.len()
        || c.dependencies.iter().collect::<BTreeSet<_>>() != dependencies.iter().collect()
    {
        return Err(invalid(
            "dependencies",
            "exact discount versions consumed by all helpers required",
        ));
    }
    for key in &dependencies {
        supplied.get(key)?;
    }
    if recipe
        .helpers
        .iter()
        .any(|h| h.pillar > *recipe.pillars.last().unwrap_or(&c.reference_date))
    {
        return Err(invalid(
            "pillar.horizon",
            "all helper maturities must be covered without extrapolation",
        ));
    }
    Ok(())
}

fn trial(
    recipe: &CdsCurveRecipe,
    pillars: &[Date],
    hazards: &[f64],
    supplied: &CurveSet,
) -> Result<CurveSet, CreditCalibrationError> {
    let mut curve = recipe.curve.clone();
    curve.source = CurveSource::Nodes(credit_survival_nodes(
        curve.reference_date,
        pillars,
        hazards,
    )?);
    curve.calibration = CalibrationMode::Direct;
    curve.dependencies.clear();
    Ok(build_curve_set(&[curve], supplied)?.curves)
}
fn price(
    contract: &CdsContract,
    h: &CdsCalibrationHelper,
    curves: &CurveSet,
    evaluations: &mut usize,
) -> Result<CdsValuation, CreditCalibrationError> {
    if *evaluations >= MAX_CREDIT_PRICE_EVALUATIONS {
        return Err(CurveContractError::BoundExceeded("credit helper valuations").into());
    }
    *evaluations += 1;
    Ok(price_cds(contract, &h.context, curves)?)
}

fn fit(
    input: &CdsCurveBuildInput<'_>,
    supplied: &CurveSet,
    controls: FitNumericalControls,
    evaluations: &mut usize,
) -> Result<CalibratedCdsCurve, CreditCalibrationError> {
    let recipe = input.recipe;
    let before = *evaluations;
    if input.calendars.len() != recipe.helpers.len() {
        return Err(invalid(
            "calendars",
            "one actual calendar binding per original helper required",
        ));
    }
    let contracts: Vec<_> = recipe
        .helpers
        .iter()
        .zip(input.calendars)
        .map(|(h, c)| h.terms.build(c))
        .collect::<Result<_, _>>()?;
    // Canonical objective order prevents source-order changes affecting solver work.
    let mut order: Vec<_> = (0..recipe.helpers.len()).collect();
    order.sort_by_key(|&i| (recipe.helpers[i].pillar, &recipe.helpers[i].id));
    let (hazards, solver, iterations) = match &recipe.curve.calibration {
        CalibrationMode::Piecewise => {
            let b = recipe
                .bootstrap
                .ok_or_else(|| invalid("bootstrap", "missing checked bracket"))?;
            let mut hazards = Vec::new();
            let mut roots = Vec::new();
            let mut helper_ids = Vec::new();
            for (position, &index) in order.iter().enumerate() {
                let h = &recipe.helpers[index];
                let contract = &contracts[index];
                let state = RefCell::new((&mut *evaluations, None, Vec::new()));
                let callback = |hazard: f64| {
                    let mut state = state.borrow_mut();
                    if state.1.is_some() {
                        return f64::NAN;
                    }
                    let mut values = hazards.clone();
                    values.push(hazard);
                    let result = trial(recipe, &recipe.pillars[..=position], &values, supplied)
                        .and_then(|curves| price(contract, h, &curves, state.0))
                        .and_then(|v| cds_model_quote(h, &v))
                        .map(|q| q.value - h.quote.value);
                    match result {
                        Ok(v) => {
                            state.2.push(v);
                            v
                        }
                        Err(error) => {
                            state.1 = Some(error);
                            f64::NAN
                        }
                    }
                };
                let solved = brent(
                    callback,
                    b.lower_hazard,
                    b.upper_hazard,
                    &SolverConfig {
                        tolerance: h.tolerance / 4.0,
                        max_iterations: b.max_iterations,
                    },
                );
                let (_, failure, residuals) = state.into_inner();
                if let Some(error) = failure {
                    return Err(error);
                }
                if residuals.len() >= 2 && (residuals[0] - residuals[1]).abs() <= 2.0 * h.tolerance
                {
                    return Err(invalid("credit.identification", "closed bracket endpoints do not distinguish the pillar at its original quote tolerance"));
                }
                let root = solved.map_err(|source| CreditCalibrationError::Root {
                    helper_id: h.id.clone(),
                    source,
                })?;
                if root.root < b.lower_hazard
                    || root.root > b.upper_hazard
                    || root.residual.abs() > h.tolerance
                {
                    return Err(invalid(
                        "root.postcondition",
                        "closed intensity bracket and original native residual must pass",
                    ));
                }
                hazards.push(root.root);
                helper_ids.push(h.id.clone());
                roots.push(root);
            }
            let iterations = roots.iter().map(|r| r.iterations as usize).sum();
            (
                hazards,
                CreditSolverDiagnostics::Piecewise { helper_ids, roots },
                iterations,
            )
        }
        CalibrationMode::CreditGlobal(s) => {
            let parameters: Vec<_> = s
                .initial_hazards
                .iter()
                .map(|&initial| FitParameter {
                    initial,
                    lower: s.lower_hazard,
                    upper: s.upper_hazard,
                })
                .collect();
            let residuals: Vec<_> = order
                .iter()
                .map(|&i| FitResidual {
                    scale: s.quote_scales[i],
                    weight: s.weights[i],
                    tolerance: recipe.helpers[i].tolerance,
                })
                .collect();
            let settings = LeastSquaresSettings {
                max_iterations: s.max_iterations,
                objective_tolerance: s.objective_tolerance,
                jacobian_step: controls.jacobian_step,
                minimum_singular_ratio: controls.minimum_singular_ratio,
                svd_iterations: controls.svd_iterations,
            };
            let output = bounded_least_squares(&parameters, &residuals, settings, |hazards| {
                let curves = trial(recipe, &recipe.pillars, hazards, supplied)?;
                order
                    .iter()
                    .map(|&i| {
                        let h = &recipe.helpers[i];
                        let v = price(&contracts[i], h, &curves, evaluations)?;
                        Ok(cds_model_quote(h, &v)?.value - h.quote.value)
                    })
                    .collect()
            })
            .map_err(|failure| CreditCalibrationError::Global(Box::new(failure)))?;
            let iterations = output.diagnostics.iterations;
            let mut diagnostics = output.diagnostics;
            let canonical = diagnostics.native_residuals.clone();
            for (position, &i) in order.iter().enumerate() {
                diagnostics.native_residuals[i] = canonical[position];
            }
            (
                output.parameters,
                CreditSolverDiagnostics::Global(diagnostics),
                iterations,
            )
        }
        _ => return Err(invalid("calibration", "unsupported checked fit mode")),
    };
    let curves = trial(recipe, &recipe.pillars, &hazards, supplied)?;
    let mut repriced_helpers = Vec::new();
    let mut quotes = Vec::new();
    for (h, contract) in recipe.helpers.iter().zip(&contracts) {
        let v = price(contract, h, &curves, evaluations)?;
        let model = cds_model_quote(h, &v)?;
        quotes.push(QuoteRepricing {
            helper_id: h.id.clone(),
            pillar: h.pillar,
            input: h.quote,
            model,
            residual: model.value - h.quote.value,
            tolerance: h.tolerance,
        });
        repriced_helpers.push(v);
    }
    let curve = build_fitted_credit_curve(
        &recipe.curve,
        &recipe.pillars,
        &hazards,
        quotes,
        iterations,
        supplied,
    )?;
    Ok(CalibratedCdsCurve {
        recipe: recipe.clone(),
        curve,
        hazards_per_act365f_year: hazards,
        repriced_helpers,
        solver,
        price_evaluations: *evaluations - before,
    })
}

/// Calibrate complete single-name curves atomically from supplied discount curves.
/// Helper and curve order are canonicalized internally; original source-order
/// quotes/units/versions remain in diagnostics. A failure returns no partial set.
pub fn build_credit_curve_set(
    inputs: &[CdsCurveBuildInput<'_>],
    supplied: &CurveSet,
    controls: FitNumericalControls,
) -> Result<CreditCurveBuildOutput, CreditCalibrationError> {
    if inputs.is_empty()
        || inputs.len() + supplied.len() > MAX_CURVES
        || inputs.iter().map(|i| i.recipe.helpers.len()).sum::<usize>() > MAX_CREDIT_FIT_HELPERS
    {
        return Err(invalid(
            "batch",
            "bounded nonempty curve/helper batch required",
        ));
    }
    let mut ordered = BTreeMap::new();
    for input in inputs {
        validate(input.recipe, supplied)?;
        if ordered.insert(&input.recipe.curve.key.id, input).is_some() {
            return Err(invalid("curve.id", "duplicate output identity"));
        }
    }
    let mut curves = supplied.clone();
    let mut calibrations = Vec::new();
    let mut build_order = Vec::new();
    let mut evaluations = 0;
    for input in ordered.into_values() {
        let calibrated = fit(input, supplied, controls, &mut evaluations)?;
        build_order.push(calibrated.curve.recipe().key.clone());
        curves = curves.with_curve(calibrated.curve.clone())?;
        calibrations.push(calibrated);
    }
    Ok(CreditCurveBuildOutput {
        curves,
        build_order,
        calibrations,
        price_evaluations: evaluations,
    })
}
