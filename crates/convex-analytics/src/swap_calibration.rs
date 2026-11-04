//! Pure product composition: full supplied swaps, one shared native pricer and
//! common bounded solvers. Curves never depend on product crates or server state.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_core::{
    cashflows::{CashRounding, CashflowError, NegativeCouponPolicy},
    contracts::{IndexId, UtcTimestamp},
    fixings::{FixingRequirement, FixingSeries},
    types::Compounding,
    Date,
};
use convex_curves::{
    contracts::{
        build_curve_set, build_fitted_rate_curve, BuiltCurve, CalibrationMode, CurveContractError,
        CurveKey, CurveNode, CurvePoint, CurveRecipe, CurveRepresentation, CurveRole, CurveSet,
        CurveSource, FitNumericalControls, GlobalFitSettings, MarketQuote, QuoteRepricing,
        QuoteUnit, MAX_CURVES,
    },
    InterpolationMethod,
};
use convex_math::{
    optimization::{
        bounded_least_squares, FitDiagnostics, FitFailure, FitParameter, FitResidual,
        LeastSquaresSettings, MAX_FIT_DIMENSION,
    },
    solvers::{brent, SolverConfig, SolverResult},
};
use convex_rates::{
    pricing::{price_swap, SwapParQuotes, SwapValuation, SwapValuationInputs},
    CashflowContext, FloatingLegTerms, SwapCalendars, SwapContract, SwapError, SwapTerms,
};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

/// Total helpers in one atomic product batch. Matches the numerical fit ceiling.
pub const MAX_SWAP_FIT_HELPERS: usize = MAX_FIT_DIMENSION;
/// Maximum unique index/date publication records frozen in one batch.
pub const MAX_SWAP_FIT_OBSERVATIONS: usize = 32_768;
/// Total individual helper valuations in one batch, including final repricing.
pub const MAX_SWAP_PRICE_EVALUATIONS: usize = 8_192;

/// Complete par-fixed-rate quote, retaining every supplied economic/date profile.
/// Helpers require a uniform fixed-rate profile equal to the native input quote;
/// general stepped contracts remain fully priceable through `price_swap`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwapCalibrationHelper {
    /// Stable original helper identity, in recipe order.
    pub id: String,
    /// Explicit diagnostic pillar; includes every actually queried horizon.
    pub pillar: Date,
    /// Full checked contract inputs; never reduced to a tenor or flat notional.
    pub terms: SwapTerms,
    /// Original decimal/percentage/basis-point rate quote.
    pub quote: MarketQuote,
    /// Positive absolute tolerance in this quote's original unit.
    pub tolerance: f64,
}

/// Explicit finite bracket and iteration budget for each sequential par root.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwapBootstrapSettings {
    /// Lower continuous decimal zero-rate bracket endpoint.
    pub lower_zero: f64,
    /// Upper continuous decimal zero-rate bracket endpoint.
    pub upper_zero: f64,
    /// Maximum Brent steps per pillar, bounded by the common kernel.
    pub max_iterations: u32,
}

/// Product recipe over a dated continuous-zero grid. The exact index/collateral
/// mappings live in the full helper terms and must match `curve.dependencies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwapCurveRecipe {
    /// Original curve recipe with `Helpers` IDs and Piecewise/Global mode.
    pub curve: CurveRecipe,
    /// Strictly increasing positive parameter dates; starts use this order.
    pub pillars: Vec<Date>,
    /// Native quotes and complete contractual inputs, in original source order.
    pub helpers: Vec<SwapCalibrationHelper>,
    /// Required for discount-curve OIS fitting: exact separate projection key
    /// under the explicit zero-basis overnight model. Forbidden on index fits.
    pub overnight_projection: Option<CurveKey>,
    /// Required only for Piecewise; Global uses the original recipe settings.
    pub bootstrap: Option<SwapBootstrapSettings>,
}

/// Actual independently pinned calendars for each helper, in source order.
pub struct SwapCurveBuildInput<'a> {
    /// Full economic/numerical recipe.
    pub recipe: &'a SwapCurveRecipe,
    /// Actual calendar bindings; lengths and exact versions are checked.
    pub calendars: &'a [SwapCalendars<'a>],
}

/// Domain failures preserve missing history and exact dependency identities.
#[derive(Debug, thiserror::Error)]
pub enum SwapCalibrationError {
    /// Original immutable curve/recipe/query failure.
    #[error(transparent)]
    Curve(#[from] CurveContractError),
    /// Original swap contract/history/valuation failure.
    #[error(transparent)]
    Swap(#[from] SwapError),
    /// Global fit failure retains measured rank/residual/iteration information.
    #[error(transparent)]
    Global(Box<FitFailure<SwapCalibrationError>>),
    /// Original bracket/iteration/domain failure from the common Brent kernel.
    #[error(transparent)]
    Root(#[from] convex_math::MathError),
}

/// Actual measured numerical work, separate from financial quote diagnostics.
#[derive(Debug, Clone)]
pub enum SwapSolverDiagnostics {
    /// Sequential bounded roots, in parameter/helper order.
    Piecewise {
        /// Individual Brent roots, residuals and iteration counts.
        roots: Vec<SolverResult>,
        /// Total helper valuations, including final native repricing.
        evaluations: usize,
    },
    /// Original common least-squares identification and convergence diagnostics.
    Global(FitDiagnostics),
}

/// Successfully fitted product curve; full inputs and actual repricing survive.
#[derive(Debug, Clone)]
pub struct CalibratedSwapCurve {
    /// Original full terms, native units, starts, weights, tolerances and mappings.
    pub recipe: SwapCurveRecipe,
    /// Immutable curve with native helper repricing, not relabeled direct nodes.
    pub curve: BuiltCurve,
    /// Complete final valuation/cash/fixing/query evidence, in helper source order.
    pub repriced_helpers: Vec<SwapValuation>,
    /// Numerical outcomes from the selected algorithm.
    pub solver: SwapSolverDiagnostics,
}

/// Atomic output; failure returns no partial curve set or fitted parameters.
#[derive(Debug, Clone)]
pub struct SwapCurveBuildOutput {
    /// Supplied curves plus all successfully fitted curves and explicit OIS views.
    pub curves: CurveSet,
    /// Deterministic dependency-first order, including explicit projection views.
    pub build_order: Vec<CurveKey>,
    /// Full product and repricing evidence in fitted curve order.
    pub calibrations: Vec<CalibratedSwapCurve>,
    /// Actually frozen administrator publication instants, ordered by index/date.
    pub publications: Vec<FixingRequirement>,
    /// Total individual helper valuations; enforces an independent work ceiling.
    pub price_evaluations: usize,
}

type Publications = BTreeMap<(IndexId, Date), FixingRequirement>;
struct Prepared<'a> {
    input: &'a SwapCurveRecipe,
    contracts: Vec<SwapContract>,
}
fn invalid(field: &str, reason: &str) -> SwapCalibrationError {
    CurveContractError::InvalidInput {
        field: field.into(),
        reason: reason.into(),
    }
    .into()
}
fn unsupported(reason: &str) -> SwapCalibrationError {
    CurveContractError::Unsupported {
        capability: "swap calibration".into(),
        reason: reason.into(),
    }
    .into()
}
fn publication(
    map: &Publications,
    index: &IndexId,
    at: Date,
) -> Result<UtcTimestamp, CashflowError> {
    map.get(&(index.clone(), at))
        .map(|r| r.expected_publication)
        .ok_or_else(|| CashflowError::InvalidTerms {
            field: "publication",
            reason: "missing frozen index/date publication".into(),
        })
}

fn validate_recipe(recipe: &SwapCurveRecipe) -> Result<(), SwapCalibrationError> {
    let curve = &recipe.curve;
    curve.key.validate()?;
    let CurveSource::Helpers(ids) = &curve.source else {
        return Err(invalid(
            "source",
            "full ordered swap helper identities required",
        ));
    };
    if recipe.helpers.is_empty()
        || recipe.helpers.len() > MAX_SWAP_FIT_HELPERS
        || recipe.pillars.is_empty()
        || recipe.pillars.len() > MAX_FIT_DIMENSION
        || recipe.helpers.len() != ids.len()
        || curve.representation
            != (CurveRepresentation::ZeroRates {
                compounding: Compounding::Continuous,
            })
    {
        return Err(invalid(
            "dimensions",
            "bounded nonempty helpers/parameters and continuous-zero representation required",
        ));
    }
    for (i, &date) in recipe.pillars.iter().enumerate() {
        if date <= curve.reference_date || (i > 0 && date <= recipe.pillars[i - 1]) {
            return Err(invalid(
                "pillars",
                "strictly increasing dates after reference required",
            ));
        }
    }
    match (&curve.calibration, recipe.bootstrap) {
        (CalibrationMode::Piecewise, Some(settings)) => {
            SolverConfig { tolerance: 1e-12, max_iterations: settings.max_iterations }.validate()?;
            if !settings.lower_zero.is_finite() || !settings.upper_zero.is_finite()
                || settings.lower_zero >= settings.upper_zero || settings.lower_zero < -10.0 || settings.upper_zero > 10.0
                || curve.interpolation != InterpolationMethod::Linear || recipe.helpers.len() != recipe.pillars.len()
                || { let mut dates: Vec<_> = recipe.helpers.iter().map(|h| h.pillar).collect(); dates.sort(); dates != recipe.pillars }
            { return Err(unsupported("Piecewise requires Linear zeros, one helper per ascending pillar and a finite bounded bracket")); }
        }
        (CalibrationMode::Global(settings), None) => {
            validate_global(settings, recipe)?;
        }
        _ => return Err(invalid("calibration", "Piecewise requires its bracket; Global requires its original settings without a bootstrap override")),
    }
    let mut dependencies = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut overnight_index = None;
    for (helper, id) in recipe.helpers.iter().zip(ids) {
        if &helper.id != id
            || id.is_empty()
            || id.len() > 128
            || id.trim() != id
            || id.chars().any(char::is_control)
            || !seen.insert(id)
            || helper.pillar <= curve.reference_date
            || !helper.tolerance.is_finite()
            || helper.tolerance <= 0.0
        {
            return Err(invalid(
                "helpers",
                "unique ordered IDs, positive pillars and positive native tolerances required",
            ));
        }
        let quote = helper.quote.decimal_rate()?;
        let terms = &helper.terms;
        if terms.currency != curve.currency
            || terms
                .fixed
                .rates
                .iter()
                .any(|s| s.value.to_f64() != Some(quote))
            || terms.fixed.rates.is_empty()
            || terms.fixed.leg.rounding != CashRounding::Unrounded
            || terms.floating.leg().rounding != CashRounding::Unrounded
            || terms.fixed.leg.negative_coupons != NegativeCouponPolicy::Allow
            || terms.floating.leg().negative_coupons != NegativeCouponPolicy::Allow
        {
            return Err(unsupported("native par-rate helpers require matching currency, uniform quoted fixed rates, unrounded cash and Allow negative coupons"));
        }
        let discount = &terms.collateral.discount_curve;
        let projection = terms.floating.projection_curve();
        match &curve.role {
            CurveRole::Projection { index_id }
                if index_id == terms.floating.index_id().as_str()
                    && projection == &curve.key
                    && recipe.overnight_projection.is_none() => {}
            CurveRole::Discount {
                collateral_currency,
            } if *collateral_currency == curve.currency && discount == &curve.key => {
                if !matches!(terms.floating, FloatingLegTerms::Overnight(_))
                    || recipe.overnight_projection.as_ref() != Some(projection)
                    || overnight_index.is_some_and(|i| i != terms.floating.index_id())
                {
                    return Err(invalid("overnight_projection", "discount calibration requires explicit same-index overnight projection mapping in every helper"));
                }
                overnight_index = Some(terms.floating.index_id());
            }
            _ => {
                return Err(invalid(
                    "curve.role",
                    "output role must match every actual index or collateral dependency",
                ))
            }
        }
        for key in [discount, projection] {
            key.validate()?;
            if key != &curve.key && Some(key) != recipe.overnight_projection.as_ref() {
                dependencies.insert(key.clone());
            }
        }
    }
    if let Some(alias) = &recipe.overnight_projection {
        alias.validate()?;
        if alias.id == curve.key.id {
            return Err(invalid(
                "overnight_projection",
                "discount and projection identities must be separate",
            ));
        }
    }
    if dependencies != curve.dependencies.iter().cloned().collect()
        || dependencies.len() != curve.dependencies.len()
    {
        return Err(invalid(
            "dependencies",
            "must equal all and only exact external helper discount/projection versions",
        ));
    }
    Ok(())
}

fn validate_global(
    settings: &GlobalFitSettings,
    recipe: &SwapCurveRecipe,
) -> Result<(), SwapCalibrationError> {
    if settings.initial_zeros.len() != recipe.pillars.len()
        || settings.quote_scales.len() != recipe.helpers.len()
        || settings.weights.len() != recipe.helpers.len()
        || !settings.lower_zero.is_finite()
        || !settings.upper_zero.is_finite()
        || settings.lower_zero < -10.0
        || settings.upper_zero > 10.0
    {
        return Err(invalid(
            "global dimensions",
            "matching starts/scales/weights and finite bounded zero domain required",
        ));
    }
    // The shared solver validates all starts, bounds, scales, weights, objective
    // controls and iteration limits before its first callback.
    Ok(())
}

fn alias_recipe(recipe: &SwapCurveRecipe) -> Option<CurveRecipe> {
    recipe.overnight_projection.as_ref().map(|key| {
        let mut curve = recipe.curve.clone();
        curve.key = key.clone();
        curve.role = CurveRole::Projection {
            index_id: recipe.helpers[0].terms.floating.index_id().as_str().into(),
        };
        curve.source = CurveSource::OvernightProjection {
            discount: recipe.curve.key.clone(),
            index_id: recipe.helpers[0].terms.floating.index_id().as_str().into(),
        };
        curve.dependencies = vec![recipe.curve.key.clone()];
        curve.calibration = CalibrationMode::Direct;
        curve
    })
}
fn nodes(reference: Date, dates: &[Date], zeros: &[f64]) -> Vec<CurveNode> {
    std::iter::once((reference, zeros[0]))
        .chain(dates.iter().copied().zip(zeros.iter().copied()))
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
    recipe: &SwapCurveRecipe,
    dates: &[Date],
    zeros: &[f64],
    supplied: &CurveSet,
) -> Result<CurveSet, SwapCalibrationError> {
    let mut direct = recipe.curve.clone();
    direct.source = CurveSource::Nodes(nodes(direct.reference_date, dates, zeros));
    direct.dependencies.clear();
    direct.calibration = CalibrationMode::Direct;
    let mut values = build_curve_set(&[direct], supplied)?.curves;
    if let Some(alias) = alias_recipe(recipe) {
        values = build_curve_set(&[alias], &values)?.curves;
    }
    Ok(values)
}
fn value(
    contract: &SwapContract,
    curves: &CurveSet,
    history: &FixingSeries,
    context: CashflowContext,
    publications: &Publications,
    evaluations: &mut usize,
) -> Result<SwapValuation, SwapCalibrationError> {
    if *evaluations >= MAX_SWAP_PRICE_EVALUATIONS {
        return Err(CurveContractError::BoundExceeded("swap helper valuations").into());
    }
    *evaluations += 1;
    Ok(price_swap(
        contract,
        &SwapValuationInputs {
            curves,
            fixings: history,
        },
        context,
        |index, at| publication(publications, index, at),
    )?)
}
fn model_quote(
    helper: &SwapCalibrationHelper,
    value: &SwapValuation,
) -> Result<MarketQuote, SwapCalibrationError> {
    let SwapParQuotes::Available { fixed_rate, .. } = &value.par else {
        return Err(unsupported(
            "helper has no admitted remaining par-fixed-rate exposure",
        ));
    };
    Ok(helper.quote.model_rate_quote(fixed_rate.value().get())?)
}
fn validate_horizon(
    value: &SwapValuation,
    helper: &SwapCalibrationHelper,
    output: &CurveKey,
    alias: Option<&CurveKey>,
) -> Result<(), SwapCalibrationError> {
    let discount_horizon = value
        .fixed
        .iter()
        .chain(&value.floating)
        .any(|coupon| &coupon.query.curve == output && coupon.payment_date > helper.pillar)
        || value.valuation_discount.as_ref().is_some_and(|query| {
            &query.curve == output && value.cashflows.context.fixings.valuation_date > helper.pillar
        });
    let projection_horizon = value.cashflows.rates.iter().any(|rate| {
        (&rate.request.projection_curve == output || Some(&rate.request.projection_curve) == alias)
            && matches!(
                rate.resolved.origin,
                convex_core::fixings::RateOrigin::Projected { .. }
            )
            && rate.request.rate_end > helper.pillar
    });
    if discount_horizon || projection_horizon {
        return Err(invalid(
            "helper.pillar",
            "must include every actual output-curve projection and delayed payment query",
        ));
    }
    Ok(())
}

fn fit(
    prepared: &Prepared<'_>,
    supplied: &CurveSet,
    history: &FixingSeries,
    context: CashflowContext,
    publications: &Publications,
    controls: FitNumericalControls,
    evaluations: &mut usize,
) -> Result<CalibratedSwapCurve, SwapCalibrationError> {
    let input = prepared.input;
    let before = *evaluations;
    let (zeros, solver, iterations) = match (&input.curve.calibration, input.bootstrap) {
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
            let residuals: Vec<_> = input
                .helpers
                .iter()
                .enumerate()
                .map(|(i, h)| FitResidual {
                    scale: settings.quote_scales[i],
                    weight: settings.weights[i],
                    tolerance: h.tolerance,
                })
                .collect();
            let result = bounded_least_squares(
                &parameters,
                &residuals,
                LeastSquaresSettings {
                    max_iterations: settings.max_iterations,
                    objective_tolerance: settings.objective_tolerance,
                    jacobian_step: controls.jacobian_step,
                    minimum_singular_ratio: controls.minimum_singular_ratio,
                    svd_iterations: controls.svd_iterations,
                },
                |parameters| {
                    let curves = trial(input, &input.pillars, parameters, supplied)?;
                    prepared
                        .contracts
                        .iter()
                        .zip(&input.helpers)
                        .map(|(contract, helper)| {
                            let priced = value(
                                contract,
                                &curves,
                                history,
                                context,
                                publications,
                                evaluations,
                            )?;
                            validate_horizon(
                                &priced,
                                helper,
                                &input.curve.key,
                                input.overnight_projection.as_ref(),
                            )?;
                            Ok(model_quote(helper, &priced)?.value - helper.quote.value)
                        })
                        .collect::<Result<Vec<_>, SwapCalibrationError>>()
                },
            )
            .map_err(|e| SwapCalibrationError::Global(Box::new(e)))?;
            let iterations = result.diagnostics.iterations;
            (
                result.parameters,
                SwapSolverDiagnostics::Global(result.diagnostics),
                iterations,
            )
        }
        (CalibrationMode::Piecewise, Some(settings)) => {
            let mut zeros = Vec::new();
            let mut roots = Vec::new();
            let mut iterations = 0;
            let mut ordered: Vec<_> = prepared.contracts.iter().zip(&input.helpers).collect();
            ordered.sort_by_key(|(_, helper)| helper.pillar);
            for (i, (contract, helper)) in ordered.into_iter().enumerate() {
                let mut prefix = zeros.clone();
                prefix.push(0.0);
                // Brent accepts an infallible Fn. This narrow adapter preserves
                // the first fallible pricing failure and makes the kernel stop
                // on a nonfinite callback; no missing value is treated as zero.
                let state = RefCell::new((&mut *evaluations, None, Vec::new()));
                let root = brent(
                    |zero| {
                        let mut state = state.borrow_mut();
                        if state.1.is_some() {
                            return f64::NAN;
                        }
                        let mut values = prefix.clone();
                        values[i] = zero;
                        let result = trial(input, &input.pillars[..=i], &values, supplied)
                            .and_then(|curves| {
                                let priced = value(
                                    contract,
                                    &curves,
                                    history,
                                    context,
                                    publications,
                                    state.0,
                                )?;
                                validate_horizon(
                                    &priced,
                                    helper,
                                    &input.curve.key,
                                    input.overnight_projection.as_ref(),
                                )?;
                                Ok(model_quote(helper, &priced)?.value - helper.quote.value)
                            });
                        match result {
                            Ok(residual) => {
                                state.2.push(residual);
                                residual
                            }
                            Err(error) => {
                                state.1 = Some(error);
                                f64::NAN
                            }
                        }
                    },
                    settings.lower_zero,
                    settings.upper_zero,
                    &SolverConfig {
                        tolerance: helper.tolerance,
                        max_iterations: settings.max_iterations,
                    },
                );
                let (_, error, residuals) = state.into_inner();
                if let Some(error) = error {
                    return Err(error);
                }
                if residuals.len() >= 2
                    && (residuals[0] - residuals[1]).abs() <= 2.0 * helper.tolerance
                {
                    return Err(invalid("piecewise identification", "bracket endpoints do not distinguish a pillar at the supplied native quote tolerance"));
                }
                let root = root?;
                zeros.push(root.root);
                iterations += root.iterations as usize;
                roots.push(root);
            }
            (
                zeros,
                SwapSolverDiagnostics::Piecewise {
                    roots,
                    evaluations: 0,
                },
                iterations,
            )
        }
        _ => return Err(invalid("calibration", "invalid admitted mode")),
    };
    let curves = trial(input, &input.pillars, &zeros, supplied)?;
    let mut repriced_helpers = Vec::new();
    let mut quotes = Vec::new();
    for (contract, helper) in prepared.contracts.iter().zip(&input.helpers) {
        let priced = value(
            contract,
            &curves,
            history,
            context,
            publications,
            evaluations,
        )?;
        validate_horizon(
            &priced,
            helper,
            &input.curve.key,
            input.overnight_projection.as_ref(),
        )?;
        let model = model_quote(helper, &priced)?;
        quotes.push(QuoteRepricing {
            helper_id: helper.id.clone(),
            pillar: helper.pillar,
            input: helper.quote,
            model,
            residual: model.value - helper.quote.value,
            tolerance: helper.tolerance,
        });
        repriced_helpers.push(priced);
    }
    let curve = build_fitted_rate_curve(
        &input.curve,
        &nodes(input.curve.reference_date, &input.pillars, &zeros),
        quotes,
        iterations,
        supplied,
    )?;
    let solver = match solver {
        SwapSolverDiagnostics::Piecewise { roots, .. } => SwapSolverDiagnostics::Piecewise {
            roots,
            evaluations: *evaluations - before,
        },
        other => other,
    };
    Ok(CalibratedSwapCurve {
        recipe: input.clone(),
        curve,
        repriced_helpers,
        solver,
    })
}

/// Fit an entire supplied dependency graph deterministically and atomically.
/// Freeze administrator publication instants and validate all due history before
/// ANY trial curve projection. No provider, clock, cache or current-version lookup
/// occurs; mutable publication callbacks are called once per unique index/date.
pub fn build_swap_curve_set<P>(
    inputs: &[SwapCurveBuildInput<'_>],
    supplied: &CurveSet,
    history: &FixingSeries,
    context: CashflowContext,
    controls: FitNumericalControls,
    mut administrator_publication: P,
) -> Result<SwapCurveBuildOutput, SwapCalibrationError>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    if inputs.is_empty()
        || inputs.len() + supplied.len() > MAX_CURVES
        || inputs.iter().map(|i| i.recipe.helpers.len()).sum::<usize>() > MAX_SWAP_FIT_HELPERS
    {
        return Err(invalid(
            "batch",
            "bounded nonempty product curves and total helper count required",
        ));
    }
    let mut pending = BTreeMap::new();
    let mut output_keys = BTreeMap::new();
    let mut publications = Publications::new();
    let mut missing = Publications::new();
    for input in inputs {
        validate_recipe(input.recipe)?;
        if input.calendars.len() != input.recipe.helpers.len() {
            return Err(invalid(
                "calendars",
                "one actual pinned calendar set per helper required",
            ));
        }
        for key in
            std::iter::once(&input.recipe.curve.key).chain(input.recipe.overnight_projection.iter())
        {
            if supplied.keys().any(|old| old.id == key.id)
                || output_keys.insert(key.id.clone(), key.clone()).is_some()
            {
                return Err(invalid(
                    "curve.id",
                    "duplicate outputs or replacement of supplied immutable curves",
                ));
            }
        }
        let mut contracts = Vec::new();
        for (helper, calendars) in input.recipe.helpers.iter().zip(input.calendars) {
            let contract = helper.terms.clone().build(calendars)?;
            let requirements = contract.fixing_requirements(context, |index, at| {
                if let Some(frozen) = publications.get(&(index.clone(), at)) {
                    return Ok(frozen.expected_publication);
                }
                if publications.len() >= MAX_SWAP_FIT_OBSERVATIONS {
                    return Err(CashflowError::BoundExceeded(
                        "swap calibration publications",
                    ));
                }
                let expected_publication = administrator_publication(index, at)?;
                publications.insert(
                    (index.clone(), at),
                    FixingRequirement {
                        index_id: index.clone(),
                        observation_date: at,
                        expected_publication,
                    },
                );
                Ok(expected_publication)
            })?;
            for chunk in requirements.chunks(convex_core::fixings::MAX_FIXING_REQUIREMENTS) {
                let resolution = history
                    .resolve(chunk, context.fixings)
                    .map_err(SwapError::from)?;
                for requirement in resolution.missing() {
                    missing.insert(
                        (requirement.index_id.clone(), requirement.observation_date),
                        requirement.clone(),
                    );
                }
            }
            contracts.push(contract);
        }
        pending.insert(
            input.recipe.curve.key.id.clone(),
            Prepared {
                input: input.recipe,
                contracts,
            },
        );
    }
    if !missing.is_empty() {
        return Err(SwapError::Cashflow(CashflowError::MissingFixings(
            missing.into_values().collect(),
        ))
        .into());
    }
    if supplied.len() + output_keys.len() > MAX_CURVES {
        return Err(CurveContractError::BoundExceeded("curve set").into());
    }
    for prepared in pending.values() {
        for dependency in &prepared.input.curve.dependencies {
            if supplied.get(dependency).is_err()
                && output_keys.get(&dependency.id) != Some(dependency)
            {
                return Err(CurveContractError::MissingDependency(dependency.clone()).into());
            }
        }
    }
    let mut curves = supplied.clone();
    let mut build_order = Vec::new();
    let mut calibrations = Vec::new();
    let mut evaluations = 0;
    for _ in 0..inputs.len() {
        let ready = pending
            .iter()
            .find(|(_, p)| {
                p.input
                    .curve
                    .dependencies
                    .iter()
                    .all(|key| curves.get(key).is_ok())
            })
            .map(|(id, _)| id.clone());
        let Some(id) = ready else {
            return Err(CurveContractError::DependencyCycle(
                pending
                    .values()
                    .map(|p| p.input.curve.key.clone())
                    .collect(),
            )
            .into());
        };
        let prepared = pending
            .remove(&id)
            .ok_or_else(|| invalid("recipes", "ready curve disappeared"))?;
        let calibrated = fit(
            &prepared,
            &curves,
            history,
            context,
            &publications,
            controls,
            &mut evaluations,
        )?;
        curves = curves.with_curve(calibrated.curve.clone())?;
        build_order.push(prepared.input.curve.key.clone());
        if let Some(alias) = alias_recipe(prepared.input) {
            let output = build_curve_set(&[alias], &curves)?;
            curves = output.curves;
            build_order.extend(output.build_order);
        }
        calibrations.push(calibrated);
    }
    Ok(SwapCurveBuildOutput {
        curves,
        build_order,
        calibrations,
        publications: publications.into_values().collect(),
        price_evaluations: evaluations,
    })
}
