//! Atomic immutable curve construction and explicit supported calibration.

use super::{
    inputs::*, CurveContractError as Error, CurveContractResult as Result, RollPolicy, MAX_CURVES,
    MAX_CURVE_DERIVATIONS, MAX_CURVE_INPUTS, MIN_PILLAR_SEPARATION,
};
use crate::{DiscreteCurve, ExtrapolationMethod, InterpolationMethod, TermStructure, ValueType};
use convex_core::{
    daycounts::DayCountConvention,
    types::{Compounding, Date},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Debug, Clone)]
enum CurveValues {
    Direct(DiscreteCurve),
    Spread {
        base: Arc<BuiltCurve>,
        spread: f64,
    },
    Equivalent {
        base: Arc<BuiltCurve>,
    },
    Rolled {
        base: Arc<BuiltCurve>,
        offset: f64,
        anchor: f64,
        policy: RollPolicy,
    },
}

/// Immutable validated curve. The recipe and input dates are retained verbatim;
/// queries use the actual recipe time basis, never an approximate nominal year.
#[derive(Debug, Clone)]
pub struct BuiltCurve {
    pub(super) recipe: CurveRecipe,
    values: CurveValues,
    pub(super) diagnostics: CurveDiagnostics,
    depth: usize,
}
impl BuiltCurve {
    /// Complete immutable input contract, including native units and versions.
    pub fn recipe(&self) -> &CurveRecipe {
        &self.recipe
    }
    /// Construction/repricing diagnostics. Direct nodes are not instrument fits.
    pub fn diagnostics(&self) -> &CurveDiagnostics {
        &self.diagnostics
    }
    /// Actual validated interpolation coordinates/values, including product fits.
    /// Derived views have no such grid and fail instead of being resampled.
    pub fn interpolation_grid(&self) -> Result<(&[f64], &[f64])> {
        match &self.values {
            CurveValues::Direct(curve) => Ok((curve.tenors(), curve.values())),
            _ => Err(Error::unsupported(
                "interpolation grid",
                "derived views require their own exact integration contract",
            )),
        }
    }
    pub(crate) fn bounds(&self) -> (f64, f64) {
        match &self.values {
            CurveValues::Direct(curve) => curve.tenor_bounds(),
            CurveValues::Spread { base, .. } => base.bounds(),
            CurveValues::Equivalent { base } => base.bounds(),
            CurveValues::Rolled {
                base,
                offset,
                policy,
                ..
            } => {
                let (min, max) = base.bounds();
                match policy {
                    RollPolicy::StickyForward => ((min - offset).max(0.0), max - offset),
                    RollPolicy::StickyZero => (min, max),
                }
            }
        }
    }
    pub(crate) fn raw(&self, t: f64) -> Result<f64> {
        validate_tenor(t)?;
        let (min, max) = self.bounds();
        if (t < min || t > max) && self.recipe.extrapolation == ExtrapolationMethod::None {
            return Err(Error::invalid(
                "query.domain",
                format!("tenor {t} outside [{min},{max}] with extrapolation disabled"),
            ));
        }
        let value = match &self.values {
            CurveValues::Direct(curve) => curve
                .checked_value_at(t)
                .map_err(|e| Error::Numerical(e.to_string()))?,
            CurveValues::Spread { base, spread } => {
                if t == 0.0 {
                    return Err(Error::unsupported(
                        "spread raw value at origin",
                        "a finite zero rate needs positive model time",
                    ));
                }
                -base.discount(t)?.ln() / t + spread
            }
            CurveValues::Equivalent { base } => match self.recipe.representation {
                CurveRepresentation::DiscountFactors => base.discount(t)?,
                CurveRepresentation::ZeroRates { compounding } => {
                    super::rate_from_discount(base.discount(t)?, t, compounding)?
                }
                _ => {
                    return Err(Error::unsupported(
                        "equivalent view",
                        "requires discount or zero representation",
                    ))
                }
            },
            CurveValues::Rolled {
                base,
                offset,
                anchor,
                policy,
            } => match policy {
                RollPolicy::StickyForward => base.discount(offset + t)? / anchor,
                RollPolicy::StickyZero => base.discount(t)?,
            },
        };
        if !value.is_finite() {
            return Err(Error::Numerical(
                "interpolation/extrapolation produced a nonfinite value".into(),
            ));
        }
        match self.recipe.representation {
            CurveRepresentation::DiscountFactors => positive(value, "discount factor")?,
            CurveRepresentation::SurvivalProbabilities if value <= 0.0 || value > 1.0 => {
                return Err(Error::Numerical(
                    "survival probability outside (0,1]".into(),
                ))
            }
            CurveRepresentation::ZeroRates { compounding } => {
                checked_zero_to_df(value, t, compounding)?;
            }
            _ => (),
        }
        Ok(value)
    }
    pub(crate) fn discount(&self, t: f64) -> Result<f64> {
        validate_tenor(t)?;
        if !self.recipe.role.is_rate() {
            return Err(Error::unsupported(
                "discount factor",
                "a survival or spread/model curve is not a discount curve",
            ));
        }
        // P(0)=1 is the definition, not a missing-rate fallback.
        if t == 0.0 {
            return Ok(1.0);
        }
        // A representation view preserves the discount function itself. Avoid
        // a roundtrip through a short-period or extreme zero-rate conversion.
        if let CurveValues::Equivalent { base } = &self.values {
            return base.discount(t);
        }
        let result = match self.recipe.representation {
            CurveRepresentation::DiscountFactors => self.raw(t)?,
            CurveRepresentation::ZeroRates { compounding } => {
                checked_zero_to_df(self.raw(t)?, t, compounding)?
            }
            _ => {
                return Err(Error::unsupported(
                    "discount factor",
                    "representation cannot supply discount factors",
                ))
            }
        };
        positive(result, "discount factor")?;
        Ok(result)
    }
    pub(crate) fn survival(&self, t: f64) -> Result<f64> {
        if !matches!(self.recipe.role, CurveRole::Credit { .. })
            || self.recipe.representation != CurveRepresentation::SurvivalProbabilities
        {
            return Err(Error::unsupported(
                "survival probability",
                "requires a credit survival curve",
            ));
        }
        self.raw(t)
    }
    pub(crate) fn probability_grid(&self) -> Result<(&[f64], &[f64])> {
        match &self.values {
            CurveValues::Direct(curve)
                if self.recipe.representation == CurveRepresentation::SurvivalProbabilities =>
            {
                Ok((curve.tenors(), curve.values()))
            }
            _ => Err(Error::unsupported(
                "hazard",
                "requires direct survival nodes with explicit side selection",
            )),
        }
    }
}

/// Immutable validated supplied/built values. No lookup reaches outside this set.
#[derive(Debug, Clone, Default)]
pub struct CurveSet {
    curves: BTreeMap<String, Arc<BuiltCurve>>,
}
impl CurveSet {
    /// Empty set, for standalone construction without external dependencies.
    pub fn new() -> Self {
        Self::default()
    }
    /// Exact version lookup. A different version cannot substitute for a missing one.
    pub fn get(&self, key: &CurveKey) -> Result<&Arc<BuiltCurve>> {
        key.validate()?;
        self.curves
            .get(&key.id)
            .filter(|curve| curve.recipe.key == *key)
            .ok_or_else(|| Error::MissingDependency(key.clone()))
    }
    /// Number of immutable curves, including supplied dependencies.
    pub fn len(&self) -> usize {
        self.curves.len()
    }
    /// Whether the set has no curves.
    pub fn is_empty(&self) -> bool {
        self.curves.is_empty()
    }
    /// Sorted exact identities, for deterministic inspection.
    pub fn keys(&self) -> impl Iterator<Item = &CurveKey> {
        self.curves.values().map(|curve| &curve.recipe.key)
    }
    /// Add a completely validated fit to a new immutable set. Duplicate IDs,
    /// capacity violations and missing exact dependencies fail atomically.
    pub fn with_curve(&self, curve: BuiltCurve) -> Result<Self> {
        if self.len() >= MAX_CURVES {
            return Err(Error::BoundExceeded("curve set"));
        }
        if self.curves.contains_key(&curve.recipe.key.id) {
            return Err(Error::invalid(
                "curve.id",
                "duplicate immutable curve identity",
            ));
        }
        for key in &curve.recipe.dependencies {
            self.get(key)?;
        }
        let mut next = self.clone();
        next.curves
            .insert(curve.recipe.key.id.clone(), Arc::new(curve));
        Ok(next)
    }
}

/// Complete output: a curve set is returned only if every recipe builds and every
/// supported helper reprices. Input sets and previous results remain immutable.
#[derive(Debug, Clone)]
pub struct CurveBuildOutput {
    /// Supplied dependencies plus all successfully built recipes.
    pub curves: CurveSet,
    /// Deterministic dependency-first build order for new curves.
    pub build_order: Vec<CurveKey>,
}

/// Build supplied recipes atomically. Independent recipes use identity ordering;
/// dependencies resolve exact versions and cycles/missing inputs return errors.
pub fn build_curve_set(recipes: &[CurveRecipe], supplied: &CurveSet) -> Result<CurveBuildOutput> {
    if recipes.is_empty() {
        return Err(Error::invalid("recipes", "at least one recipe is required"));
    }
    if recipes.len() + supplied.len() > MAX_CURVES {
        return Err(Error::BoundExceeded("curve set"));
    }
    let mut pending = BTreeMap::new();
    for recipe in recipes {
        validate_recipe(recipe)?;
        if supplied.curves.contains_key(&recipe.key.id)
            || pending.insert(recipe.key.id.clone(), recipe).is_some()
        {
            return Err(Error::invalid(
                "curve.id",
                "duplicate identities or replacement of an immutable supplied curve",
            ));
        }
    }
    for recipe in recipes {
        for dependency in &recipe.dependencies {
            if supplied
                .curves
                .get(&dependency.id)
                .is_none_or(|c| c.recipe.key != *dependency)
                && pending
                    .get(&dependency.id)
                    .is_none_or(|r| r.key != *dependency)
            {
                return Err(Error::MissingDependency(dependency.clone()));
            }
        }
    }
    let mut curves = supplied.clone();
    let mut build_order = Vec::with_capacity(recipes.len());
    // At most MAX_CURVES passes; each pass consumes one pending recipe or fails.
    for _ in 0..recipes.len() {
        let ready = pending
            .iter()
            .find(|(_, r)| r.dependencies.iter().all(|key| curves.get(key).is_ok()))
            .map(|(id, _)| id.clone());
        let Some(id) = ready else {
            return Err(Error::DependencyCycle(
                pending.values().map(|r| r.key.clone()).collect(),
            ));
        };
        let recipe = pending
            .remove(&id)
            .ok_or_else(|| Error::invalid("recipes", "ready recipe disappeared"))?;
        let built = build_one(recipe, &curves)?;
        build_order.push(recipe.key.clone());
        curves.curves.insert(id, Arc::new(built));
    }
    Ok(CurveBuildOutput {
        curves,
        build_order,
    })
}

/// Construct the validated representation of a product fit performed in a pure
/// composition layer. This function verifies native repricing records and exact
/// dependencies; it does not resolve instruments or run a product solver.
/// The original recipe remains intact instead of being relabeled as direct nodes.
pub fn build_fitted_rate_curve(
    recipe: &CurveRecipe,
    nodes: &[CurveNode],
    quotes: Vec<QuoteRepricing>,
    iterations: usize,
    supplied: &CurveSet,
) -> Result<BuiltCurve> {
    let CurveSource::Helpers(ids) = &recipe.source else {
        return Err(Error::invalid(
            "fitted.source",
            "full product helper identities required",
        ));
    };
    let method = match &recipe.calibration {
        CalibrationMode::Piecewise => "product_piecewise_brent",
        CalibrationMode::Global(settings) => {
            validate_global_settings(settings)?;
            "product_global_bounded_least_squares"
        }
        CalibrationMode::Direct => {
            return Err(Error::invalid(
                "fitted.calibration",
                "an actual calibrated mode is required",
            ))
        }
        CalibrationMode::CreditGlobal(_) => {
            return Err(Error::invalid(
                "fitted.calibration",
                "credit intensity settings cannot fit a rate curve",
            ))
        }
    };
    if !recipe.role.is_rate()
        || ids.is_empty()
        || ids.len() > MAX_CURVE_INPUTS
        || quotes.len() != ids.len()
        || iterations > ids.len() * convex_math::bounds::MAX_SOLVER_ITERATIONS as usize
    {
        return Err(Error::invalid(
            "fitted.dimensions",
            "bounded rate-curve helper/repricing/iteration dimensions required",
        ));
    }
    validate_fitted_nodes(recipe, nodes, quotes.len(), ids.len())?;
    validate_fitted_quotes(recipe.reference_date, ids, &quotes)?;
    let mut dependencies = BTreeSet::new();
    for dependency in &recipe.dependencies {
        if !dependencies.insert(&dependency.id) || dependency.id == recipe.key.id {
            return Err(Error::invalid(
                "dependencies",
                "unique external exact dependencies required",
            ));
        }
        supplied.get(dependency)?;
    }
    if recipe.dependencies.len() > MAX_CURVES {
        return Err(Error::BoundExceeded("dependencies"));
    }
    let mut direct = recipe.clone();
    direct.source = CurveSource::Nodes(nodes.to_vec());
    direct.calibration = CalibrationMode::Direct;
    direct.dependencies.clear();
    validate_recipe(&direct)?;
    let mut built = build_direct(&direct, nodes, method, quotes)?;
    built.recipe = recipe.clone();
    built
        .diagnostics
        .dependencies
        .clone_from(&recipe.dependencies);
    built.diagnostics.iterations = iterations;
    Ok(built)
}

fn validate_fitted_nodes(
    recipe: &CurveRecipe,
    nodes: &[CurveNode],
    quote_count: usize,
    helper_count: usize,
) -> Result<()> {
    if recipe.representation
        != (CurveRepresentation::ZeroRates {
            compounding: Compounding::Continuous,
        })
        || nodes.len() < 2
        || nodes
            .first()
            .is_none_or(|node| node.pillar != CurvePoint::Date(recipe.reference_date))
        || nodes[0].quote.decimal_rate()? != nodes[1].quote.decimal_rate()?
    {
        return Err(Error::invalid(
            "fitted.nodes",
            "continuous-zero dated grid with origin tied to the first parameter required",
        ));
    }
    match &recipe.calibration {
        CalibrationMode::Piecewise if helper_count != nodes.len() - 1 => {
            return Err(Error::invalid(
                "fitted.dimensions",
                "one piecewise helper per positive grid node required",
            ));
        }
        CalibrationMode::Global(settings)
            if settings.initial_zeros.len() != nodes.len() - 1
                || settings.quote_scales.len() != quote_count
                || nodes.iter().any(|node| match node.quote.decimal_rate() {
                    Ok(zero) => zero < settings.lower_zero || zero > settings.upper_zero,
                    Err(_) => true,
                }) =>
        {
            return Err(Error::invalid("fitted.global", "matching parameter/helper dimensions and solved nodes inside the declared bounds required"));
        }
        _ => (),
    }
    Ok(())
}

fn validate_fitted_quotes(
    reference_date: Date,
    ids: &[String],
    quotes: &[QuoteRepricing],
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (id, quote) in ids.iter().zip(quotes) {
        validate_label(id, "helper.id")?;
        validate_date(quote.pillar)?;
        validate_tolerance(quote.tolerance)?;
        // Diagnostics retain original native units. The product layer already
        // priced the complete futures contract; price points are not a raw FRA
        // rate and must never be converted here by assuming a convexity model.
        if !matches!(
            quote.input.unit,
            QuoteUnit::DecimalRate
                | QuoteUnit::PercentRate
                | QuoteUnit::BasisPoints
                | QuoteUnit::FuturePrice100MinusRate
        ) {
            return Err(Error::invalid(
                "fitted.quote.unit",
                "annualized rate or fully priced rate-futures native points required",
            ));
        }
        quote.input.validate()?;
        quote.model.validate()?;
        if !seen.insert(id)
            || id != &quote.helper_id
            || quote.pillar <= reference_date
            || quote.input.unit != quote.model.unit
            || !quote.residual.is_finite()
            || quote.residual != quote.model.value - quote.input.value
        {
            return Err(Error::invalid("fitted.repricing", "ordered unique native helper records and exact model-minus-input residuals required"));
        }
        if quote.residual.abs() > quote.tolerance {
            return Err(Error::RepricingFailure {
                helper_id: id.clone(),
                residual: quote.residual,
                tolerance: quote.tolerance,
            });
        }
    }
    Ok(())
}

pub(super) fn validate_recipe(recipe: &CurveRecipe) -> Result<()> {
    recipe.key.validate()?;
    recipe.role.validate()?;
    validate_date(recipe.reference_date)?;
    validate_time_basis(recipe.time_basis)?;
    if recipe.dependencies.len() > MAX_CURVES {
        return Err(Error::BoundExceeded("dependencies"));
    }
    let mut dependencies = BTreeSet::new();
    for key in &recipe.dependencies {
        key.validate()?;
        if !dependencies.insert(key.id.clone()) {
            return Err(Error::invalid(
                "dependencies",
                "duplicate dependency identity",
            ));
        }
    }
    let expected: Vec<&CurveKey> = match &recipe.source {
        CurveSource::ContinuousZeroSpread { base, .. }
        | CurveSource::DiscountEquivalent { base }
        | CurveSource::Rolled { base, .. } => vec![base],
        CurveSource::OvernightProjection { discount, .. } => vec![discount],
        CurveSource::Helpers(ids) => {
            if ids.is_empty() || ids.len() > MAX_CURVE_INPUTS {
                return Err(Error::BoundExceeded("product helpers"));
            }
            let mut seen = BTreeSet::new();
            for id in ids {
                validate_label(id, "helper.id")?;
                if !seen.insert(id) {
                    return Err(Error::invalid("helpers", "duplicate helper ID"));
                }
            }
            return Err(Error::unsupported(
                "product helper construction",
                "resolve full terms through convex-analytics::curve_inputs",
            ));
        }
        _ => vec![],
    };
    if expected.len() != recipe.dependencies.len()
        || expected
            .iter()
            .any(|key| !recipe.dependencies.contains(key))
    {
        return Err(Error::invalid(
            "dependencies",
            "must exactly match dependencies consumed by the source",
        ));
    }
    if let CalibrationMode::Global(settings) = &recipe.calibration {
        validate_global_settings(settings)?;
        return Err(Error::unsupported("global calibration", "dated product/global calibration belongs to CVX-021/057/058/059; the existing GlobalFitter remains a separate API"));
    }
    validate_representation(recipe)?;
    validate_methods(recipe)
}

pub(super) fn validate_global_settings(settings: &GlobalFitSettings) -> Result<()> {
    if settings.max_iterations == 0
        || settings.max_iterations > convex_math::bounds::MAX_SOLVER_ITERATIONS as usize
        || settings.initial_zeros.is_empty()
        || settings.initial_zeros.len() > MAX_CURVE_INPUTS
        || settings.quote_scales.is_empty()
        || settings.quote_scales.len() > MAX_CURVE_INPUTS
        || settings.weights.len() != settings.quote_scales.len()
    {
        return Err(Error::invalid(
            "global settings",
            "bounded positive iteration count and bounded nonempty initial parameters and matching quote scale/weight dimensions required",
        ));
    }
    validate_tolerance(settings.objective_tolerance)?;
    if !settings.lower_zero.is_finite()
        || !settings.upper_zero.is_finite()
        || settings.lower_zero >= settings.upper_zero
        || settings.lower_zero < -10.0
        || settings.upper_zero > 10.0
        || settings
            .initial_zeros
            .iter()
            .any(|v| !v.is_finite() || *v < settings.lower_zero || *v > settings.upper_zero)
        || settings
            .quote_scales
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        || settings.weights.iter().any(|v| !v.is_finite() || *v < 0.0)
        || !settings.weights.iter().any(|v| *v > 0.0)
    {
        return Err(Error::invalid(
            "global settings",
            "finite bounded starts, positive scales and nonnegative nonzero weights required",
        ));
    }
    Ok(())
}

fn validate_representation(recipe: &CurveRecipe) -> Result<()> {
    match recipe.representation {
        CurveRepresentation::DiscountFactors | CurveRepresentation::ZeroRates { .. }
            if recipe.role.is_rate() =>
        {
            Ok(())
        }
        CurveRepresentation::SurvivalProbabilities
            if matches!(recipe.role, CurveRole::Credit { .. }) =>
        {
            Ok(())
        }
        CurveRepresentation::HazardRates => Err(Error::unsupported(
            "hazard node construction",
            "explicit hazard integration and CDS calibration remain CVX-021",
        )),
        CurveRepresentation::IssuerSpreads => Err(Error::unsupported(
            "issuer spread construction",
            "declared basis model remains CVX-059",
        )),
        _ => Err(Error::invalid(
            "representation",
            "incompatible with curve role",
        )),
    }
}

fn validate_methods(recipe: &CurveRecipe) -> Result<()> {
    // A continuous zero spread consumes the actual base curve values; it does
    // not log-interpolate the output zero rates. build_spread checks exact
    // method inheritance from a previously validated immutable rate curve.
    if matches!(
        recipe.source,
        CurveSource::ContinuousZeroSpread { .. }
            | CurveSource::DiscountEquivalent { .. }
            | CurveSource::OvernightProjection { .. }
            | CurveSource::Rolled { .. }
    ) {
        return Ok(());
    }
    let supported = match recipe.interpolation {
        InterpolationMethod::Linear => true,
        InterpolationMethod::LogLinear => matches!(
            recipe.representation,
            CurveRepresentation::DiscountFactors | CurveRepresentation::SurvivalProbabilities
        ),
        InterpolationMethod::CubicSpline => recipe.role.is_rate(),
        _ => false,
    };
    if !supported {
        return Err(Error::unsupported("interpolation", "checked route supports Linear, LogLinear on discount/survival nodes and CubicSpline on rates; other existing methods have separate capability/model constraints"));
    }
    if !matches!(
        recipe.extrapolation,
        ExtrapolationMethod::None | ExtrapolationMethod::Flat | ExtrapolationMethod::Linear
    ) {
        return Err(Error::unsupported("extrapolation", "supports None, raw-value Flat and raw-value Linear; no approximate FlatForward/UFR substitution"));
    }
    if recipe.representation == CurveRepresentation::SurvivalProbabilities
        && recipe.extrapolation == ExtrapolationMethod::Linear
    {
        return Err(Error::unsupported(
            "survival linear extrapolation",
            "would violate the probability domain beyond a finite tail",
        ));
    }
    Ok(())
}

fn build_one(recipe: &CurveRecipe, supplied: &CurveSet) -> Result<BuiltCurve> {
    match &recipe.source {
        CurveSource::Nodes(nodes) => {
            if recipe.calibration != CalibrationMode::Direct {
                return Err(Error::invalid("calibration", "nodes require Direct mode"));
            }
            build_direct(recipe, nodes, "supplied_nodes", vec![])
        }
        CurveSource::MoneyMarket(helpers) => build_money_market(recipe, helpers),
        CurveSource::ContinuousZeroSpread { base, spread } => {
            build_spread(recipe, supplied, base, *spread)
        }
        CurveSource::DiscountEquivalent { base } => build_view(recipe, supplied, base, None),
        CurveSource::OvernightProjection { discount, index_id } => {
            build_overnight_projection(recipe, supplied, discount, index_id)
        }
        CurveSource::Rolled { base, policy } => build_view(recipe, supplied, base, Some(*policy)),
        CurveSource::Helpers(_) => Err(Error::unsupported(
            "product helpers",
            "use the pure analytics composition layer",
        )),
    }
}

fn normalize_nodes(recipe: &CurveRecipe, nodes: &[CurveNode]) -> Result<(Vec<f64>, Vec<f64>)> {
    if nodes.len() < 2 {
        return Err(Error::invalid(
            "nodes",
            "at least two distinct nodes required",
        ));
    }
    if nodes.len() > MAX_CURVE_INPUTS {
        return Err(Error::BoundExceeded("curve nodes"));
    }
    let mut normalized = Vec::with_capacity(nodes.len());
    for node in nodes {
        let t = point_tenor(recipe.reference_date, recipe.time_basis, node.pillar)?;
        node.quote.validate()?;
        let value = match recipe.representation {
            CurveRepresentation::DiscountFactors
                if node.quote.unit == QuoteUnit::DiscountFactor =>
            {
                node.quote.value
            }
            CurveRepresentation::SurvivalProbabilities
                if node.quote.unit == QuoteUnit::SurvivalProbability =>
            {
                node.quote.value
            }
            CurveRepresentation::ZeroRates { compounding } => {
                let rate = node.quote.decimal_rate()?;
                checked_zero_to_df(rate, t, compounding)?;
                rate
            }
            _ => {
                return Err(Error::invalid(
                    "node.quote.unit",
                    "incompatible with representation",
                ))
            }
        };
        normalized.push((t, value));
    }
    normalized.sort_by(|a, b| a.0.total_cmp(&b.0));
    for pair in normalized.windows(2) {
        if pair[1].0 - pair[0].0 < MIN_PILLAR_SEPARATION {
            return Err(Error::invalid(
                "pillars",
                "duplicate or numerically inseparable model-time nodes",
            ));
        }
        if recipe.representation == CurveRepresentation::SurvivalProbabilities
            && pair[1].1 > pair[0].1
        {
            return Err(Error::invalid("survival", "must be nonincreasing"));
        }
    }
    if matches!(
        recipe.representation,
        CurveRepresentation::DiscountFactors | CurveRepresentation::SurvivalProbabilities
    ) && !normalized
        .first()
        .is_some_and(|&(t, v)| t == 0.0 && v == 1.0)
    {
        return Err(Error::invalid(
            "origin",
            "explicit P(0)=1 or Q(0)=1 node required",
        ));
    }
    Ok(normalized.into_iter().unzip())
}

pub(super) fn build_direct(
    recipe: &CurveRecipe,
    nodes: &[CurveNode],
    method: &str,
    quotes: Vec<QuoteRepricing>,
) -> Result<BuiltCurve> {
    let (tenors, values) = normalize_nodes(recipe, nodes)?;
    let value_type = match recipe.representation {
        CurveRepresentation::DiscountFactors => ValueType::DiscountFactor,
        CurveRepresentation::ZeroRates { compounding } => ValueType::ZeroRate {
            compounding,
            day_count: recipe.time_basis,
        },
        CurveRepresentation::SurvivalProbabilities => ValueType::SurvivalProbability,
        _ => {
            return Err(Error::unsupported(
                "direct representation",
                "integration/model construction pending",
            ))
        }
    };
    let curve = DiscreteCurve::with_extrapolation(
        recipe.reference_date,
        tenors.clone(),
        values,
        value_type,
        recipe.interpolation,
        recipe.extrapolation,
    )
    .map_err(|e| Error::Numerical(e.to_string()))?;
    Ok(BuiltCurve {
        recipe: recipe.clone(),
        values: CurveValues::Direct(curve),
        depth: 0,
        diagnostics: CurveDiagnostics {
            curve: recipe.key.clone(),
            method: method.into(),
            iterations: 0,
            pillars: tenors,
            dependencies: recipe.dependencies.clone(),
            quotes,
        },
    })
}

pub(super) fn validate_money_market_helpers(
    recipe: &CurveRecipe,
    helpers: &[MoneyMarketHelper],
) -> Result<()> {
    let mut ids = BTreeSet::new();
    for helper in helpers {
        helper.validate(recipe.reference_date)?;
        if let (
            CurveRole::Projection { index_id },
            MoneyMarketTerms::Fra {
                index_id: helper_index,
                ..
            },
        ) = (&recipe.role, &helper.terms)
        {
            if index_id != helper_index {
                return Err(Error::invalid(
                    "helper.index_id",
                    "FRA index must match the projection curve role",
                ));
            }
        }
        if !ids.insert(&helper.id) {
            return Err(Error::invalid("helper.id", "duplicate input identity"));
        }
    }
    Ok(())
}

fn build_money_market(recipe: &CurveRecipe, helpers: &[MoneyMarketHelper]) -> Result<BuiltCurve> {
    if recipe.calibration != CalibrationMode::Piecewise
        || recipe.representation != CurveRepresentation::DiscountFactors
    {
        return Err(Error::unsupported(
            "money market construction",
            "requires Piecewise with DiscountFactors; no quoted-rate proxy curve",
        ));
    }
    // Local interpolation preserves solved prior intervals; cubic/global coupling
    // needs repeated repricing passes and is outside this exact-chain contract.
    if !matches!(
        recipe.interpolation,
        InterpolationMethod::Linear | InterpolationMethod::LogLinear
    ) {
        return Err(Error::unsupported(
            "piecewise interpolation",
            "exact chain requires local Linear or LogLinear discount interpolation",
        ));
    }
    if helpers.is_empty() || helpers.len() >= MAX_CURVE_INPUTS {
        return Err(Error::BoundExceeded("money market helpers plus origin"));
    }
    validate_money_market_helpers(recipe, helpers)?;
    let mut sorted: Vec<_> = helpers.iter().collect();
    sorted.sort_by_key(|helper| helper.terms.period().1);
    let mut dfs = BTreeMap::from([(recipe.reference_date, 1.0)]);
    let mut nodes = vec![CurveNode {
        pillar: CurvePoint::Date(recipe.reference_date),
        quote: MarketQuote {
            value: 1.0,
            unit: QuoteUnit::DiscountFactor,
        },
    }];
    for helper in &sorted {
        let (start, end, dc) = helper.terms.period();
        if dfs.contains_key(&end) {
            return Err(Error::invalid("pillars", "duplicate helper maturity"));
        }
        let start_df = dfs.get(&start).copied().ok_or_else(|| Error::unsupported("piecewise start date", "every start must equal the origin or an already solved exact pillar; no interpolation-dependent start is silently approximated"))?;
        let tau = year_fraction(start, end, dc)?;
        let denominator = 1.0 + helper.quote.decimal_rate()? * tau;
        positive(denominator, "simple accrual factor")?;
        let df = start_df / denominator;
        positive(df, "solved discount factor")?;
        dfs.insert(end, df);
        nodes.push(CurveNode {
            pillar: CurvePoint::Date(end),
            quote: MarketQuote {
                value: df,
                unit: QuoteUnit::DiscountFactor,
            },
        });
    }
    let mut built = build_direct(recipe, &nodes, "analytic_dated_deposit_fra_chain", vec![])?;
    let mut diagnostics = Vec::with_capacity(sorted.len());
    for helper in sorted {
        let (start, end, dc) = helper.terms.period();
        let t1 = point_tenor(
            recipe.reference_date,
            recipe.time_basis,
            CurvePoint::Date(start),
        )?;
        let t2 = point_tenor(
            recipe.reference_date,
            recipe.time_basis,
            CurvePoint::Date(end),
        )?;
        let rate =
            (built.discount(t1)? / built.discount(t2)? - 1.0) / year_fraction(start, end, dc)?;
        let model = helper.quote.model_rate_quote(rate)?;
        let residual = model.value - helper.quote.value;
        if !residual.is_finite() {
            return Err(Error::Numerical(
                "native quote residual is nonfinite".into(),
            ));
        }
        if residual.abs() > helper.tolerance {
            return Err(Error::RepricingFailure {
                helper_id: helper.id.clone(),
                residual,
                tolerance: helper.tolerance,
            });
        }
        diagnostics.push(QuoteRepricing {
            helper_id: helper.id.clone(),
            pillar: end,
            input: helper.quote,
            model,
            residual,
            tolerance: helper.tolerance,
        });
    }
    built.diagnostics.quotes = diagnostics;
    Ok(built)
}

fn build_spread(
    recipe: &CurveRecipe,
    supplied: &CurveSet,
    base_key: &CurveKey,
    quote: MarketQuote,
) -> Result<BuiltCurve> {
    if recipe.calibration != CalibrationMode::Direct
        || recipe.representation
            != (CurveRepresentation::ZeroRates {
                compounding: Compounding::Continuous,
            })
    {
        return Err(Error::unsupported(
            "continuous zero spread",
            "requires Direct continuous ZeroRates output",
        ));
    }
    let base = supplied.get(base_key)?;
    if base.recipe.reference_date != recipe.reference_date
        || base.recipe.currency != recipe.currency
        || base.recipe.time_basis != recipe.time_basis
        || !base.recipe.role.is_rate()
    {
        return Err(Error::invalid(
            "base",
            "currency, reference date and time basis must match a rate curve",
        ));
    }
    if base.recipe.extrapolation != recipe.extrapolation
        || base.recipe.interpolation != recipe.interpolation
    {
        return Err(Error::invalid(
            "spread methods",
            "derived spread inherits exact base interpolation/extrapolation",
        ));
    }
    let spread = quote.decimal_rate()?;
    Ok(BuiltCurve {
        recipe: recipe.clone(),
        values: CurveValues::Spread {
            base: Arc::clone(base),
            spread,
        },
        depth: derived_depth(base)?,
        diagnostics: CurveDiagnostics {
            curve: recipe.key.clone(),
            method: "continuous_zero_spread_over_exact_base".into(),
            iterations: 0,
            pillars: base.diagnostics.pillars.clone(),
            dependencies: vec![base_key.clone()],
            quotes: vec![],
        },
    })
}

fn derived_depth(base: &BuiltCurve) -> Result<usize> {
    if base.depth >= MAX_CURVE_DERIVATIONS {
        return Err(Error::BoundExceeded("derived curve nesting"));
    }
    Ok(base.depth + 1)
}

fn build_overnight_projection(
    recipe: &CurveRecipe,
    supplied: &CurveSet,
    key: &CurveKey,
    index_id: &str,
) -> Result<BuiltCurve> {
    validate_label(index_id, "overnight.index_id")?;
    let base = supplied.get(key)?;
    let old = base.recipe();
    if recipe.calibration != CalibrationMode::Direct
        || !matches!(&recipe.role, CurveRole::Projection { index_id: actual } if actual == index_id)
        || !matches!(old.role, CurveRole::Discount { collateral_currency } if collateral_currency == recipe.currency)
        || recipe.currency != old.currency
        || recipe.reference_date != old.reference_date
        || recipe.time_basis != old.time_basis
        || recipe.interpolation != old.interpolation
        || recipe.extrapolation != old.extrapolation
    {
        return Err(Error::invalid("overnight projection", "requires an explicit matching index, Direct mode, same-currency collateral discount base, identical reference/time basis and inherited methods"));
    }
    Ok(BuiltCurve {
        recipe: recipe.clone(),
        values: CurveValues::Equivalent {
            base: Arc::clone(base),
        },
        depth: derived_depth(base)?,
        diagnostics: CurveDiagnostics {
            curve: recipe.key.clone(),
            method: "explicit_zero_basis_overnight_projection".into(),
            iterations: 0,
            pillars: base.diagnostics.pillars.clone(),
            dependencies: vec![key.clone()],
            quotes: vec![],
        },
    })
}

fn build_view(
    recipe: &CurveRecipe,
    supplied: &CurveSet,
    key: &CurveKey,
    roll: Option<RollPolicy>,
) -> Result<BuiltCurve> {
    let base = supplied.get(key)?;
    let old = base.recipe();
    if recipe.calibration != CalibrationMode::Direct
        || !recipe.role.is_rate()
        || recipe.role != old.role
        || recipe.currency != old.currency
        || recipe.time_basis != old.time_basis
        || recipe.interpolation != old.interpolation
        || recipe.extrapolation != old.extrapolation
    {
        return Err(Error::invalid("curve view", "requires Direct mode and identical rate role, currency, time basis and inherited methods"));
    }
    let (values, pillars, method) = if let Some(policy) = roll {
        if recipe.reference_date < old.reference_date
            || recipe.representation != CurveRepresentation::DiscountFactors
        {
            return Err(Error::invalid(
                "roll",
                "requires a forward reference date and DiscountFactors output",
            ));
        }
        let offset = point_tenor(
            old.reference_date,
            old.time_basis,
            CurvePoint::Date(recipe.reference_date),
        )?;
        // Rolling is restricted to the original observed domain, independently
        // of an enabled tail. Otherwise a roll could silently be a tail scenario.
        let (min, max) = base.bounds();
        if offset < min || max - offset < MIN_PILLAR_SEPARATION {
            return Err(Error::invalid(
                "roll.date",
                "must lie inside the original domain with a nonempty remaining horizon",
            ));
        }
        let anchor = base.discount(offset)?;
        let pillars = match policy {
            RollPolicy::StickyForward => std::iter::once(0.0)
                .chain(base.diagnostics.pillars.iter().filter_map(|&t| {
                    if t > offset {
                        Some(t - offset)
                    } else {
                        None
                    }
                }))
                .collect(),
            RollPolicy::StickyZero => base.diagnostics.pillars.clone(),
        };
        (
            CurveValues::Rolled {
                base: Arc::clone(base),
                offset,
                anchor,
                policy,
            },
            pillars,
            match policy {
                RollPolicy::StickyForward => "exact_sticky_forward_roll",
                RollPolicy::StickyZero => "exact_sticky_zero_roll",
            },
        )
    } else {
        if recipe.reference_date != old.reference_date {
            return Err(Error::invalid(
                "view.reference_date",
                "representation views preserve the reference date",
            ));
        }
        (
            CurveValues::Equivalent {
                base: Arc::clone(base),
            },
            base.diagnostics.pillars.clone(),
            "exact_discount_equivalent_view",
        )
    };
    Ok(BuiltCurve {
        recipe: recipe.clone(),
        values,
        depth: derived_depth(base)?,
        diagnostics: CurveDiagnostics {
            curve: recipe.key.clone(),
            method: method.into(),
            iterations: 0,
            pillars,
            dependencies: vec![key.clone()],
            quotes: vec![],
        },
    })
}

pub(crate) fn point_tenor(
    reference: Date,
    dc: DayCountConvention,
    point: CurvePoint,
) -> Result<f64> {
    let t = match point {
        CurvePoint::Tenor(t) => t,
        CurvePoint::Date(date) => {
            validate_date(date)?;
            if date < reference {
                return Err(Error::invalid("date", "precedes curve reference date"));
            }
            if date == reference {
                0.0
            } else {
                year_fraction(reference, date, dc)?
            }
        }
    };
    validate_tenor(t)?;
    Ok(t)
}

pub(crate) fn positive(value: f64, operation: &str) -> Result<()> {
    if !value.is_finite() || value <= 0.0 {
        return Err(Error::Numerical(format!(
            "{operation} must be positive and finite"
        )));
    }
    Ok(())
}
pub(crate) fn checked_zero_to_df(rate: f64, t: f64, compounding: Compounding) -> Result<f64> {
    if !rate.is_finite() {
        return Err(Error::invalid("zero rate", "must be finite"));
    }
    // Validate the base even at t=0; the mathematical origin must not conceal
    // an invalid periodic/simple rate domain.
    match compounding {
        Compounding::Continuous => (),
        Compounding::Simple => positive(1.0 + rate * t, "simple compounding base")?,
        _ => positive(
            1.0 + rate / f64::from(compounding.periods_per_year()),
            "periodic compounding base",
        )?,
    }
    super::discount_from_rate(rate, t, compounding)
}
