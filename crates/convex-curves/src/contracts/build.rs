//! Atomic immutable curve construction and explicit supported calibration.

use super::{
    inputs::*, CurveContractError as Error, CurveContractResult as Result, MAX_CURVES,
    MAX_CURVE_INPUTS, MIN_PILLAR_SEPARATION,
};
use crate::{
    DiscreteCurve, ExtrapolationMethod, InterpolationMethod, TermStructure, ValueConverter,
    ValueType,
};
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
    Spread { base: Arc<BuiltCurve>, spread: f64 },
}

/// Immutable validated curve. The recipe and input dates are retained verbatim;
/// queries use the actual recipe time basis, never an approximate nominal year.
#[derive(Debug, Clone)]
pub struct BuiltCurve {
    recipe: CurveRecipe,
    values: CurveValues,
    diagnostics: CurveDiagnostics,
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
    pub(crate) fn bounds(&self) -> (f64, f64) {
        match &self.values {
            CurveValues::Direct(curve) => curve.tenor_bounds(),
            CurveValues::Spread { base, .. } => base.bounds(),
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

fn validate_recipe(recipe: &CurveRecipe) -> Result<()> {
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
        CurveSource::ContinuousZeroSpread { base, .. } => vec![base],
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

fn validate_global_settings(settings: &GlobalFitSettings) -> Result<()> {
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
    if matches!(recipe.source, CurveSource::ContinuousZeroSpread { .. }) {
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

fn build_direct(
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

fn validate_money_market_helpers(
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
    let value = ValueConverter::zero_to_df(rate, t, compounding);
    positive(value, "zero-to-discount conversion")?;
    Ok(value)
}
