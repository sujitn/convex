//! Bounded explicit composition of the three existing full native calibration families.
use super::invalid;
use super::*;
use crate::{cash_calibration::*, credit_calibration::*, rate_calibration::*};
use convex_core::{
    cashflows::CashflowError,
    contracts::{FactorTarget, FiniteF64, IndexId, QuoteType, UtcTimestamp, Version},
    fixings::FixingSeries,
    types::Compounding,
    Date,
};
use convex_curves::contracts::*;
use convex_math::portable;
use std::collections::{BTreeMap, BTreeSet};

/// Full mixed-rate input plus original immutable helper observation revisions.
/// The rate recipe predates quote-version fields; these pins must come from the
/// same caller snapshot, in exact original source order.
pub struct RateRiskCurveInput<'a> {
    /// Original complete recipe and actual calendars/fixing resources.
    pub build: RateCurveBuildInput<'a>,
    /// Exactly one original revision per full helper, no fabricated default.
    pub quote_versions: &'a [Version],
}
/// Actual full immutable supplied calibration graph, independent of infrastructure.
pub struct QuoteRiskGraphInput<'a> {
    /// Existing full mixed rate recipes.
    pub rates: &'a [RateRiskCurveInput<'a>],
    /// Existing full CDS recipes, calendars and original quote revisions.
    pub credit: &'a [CdsCurveBuildInput<'a>],
    /// Existing full government/issuer/repo recipes and actual resources.
    pub cash: &'a [CashCurveBuildInput<'a>],
    /// Actual independently supplied curves, excluding all recipe outputs.
    pub supplied: &'a CurveSet,
    /// Same immutable eligible history for every rate rebuild.
    pub history: &'a FixingSeries,
    /// Required when rate recipes exist; no valuation date/clock default.
    pub rate_context: Option<RateCalibrationContext>,
    /// Existing full fit numerical controls, never loosened by risk calculation.
    pub controls: FitNumericalControls,
}
/// Actual full hypothetical graph and its original native-helper diagnostics.
#[derive(Debug, Clone)]
pub struct QuoteGraphScenario {
    /// Complete actual hypothetical inputs. Original keys are retained inside a
    /// scenario; these are not published market revisions or reusable live snapshots.
    pub curves: CurveSet,
    /// Deterministic dependency-first recipe build order.
    pub build_order: Vec<CurveKey>,
    /// Every actual original helper repriced in original units, including shocked quotes.
    pub repricing: Vec<QuoteRepricing>,
    /// Actual helper evaluations across all family builders in this rebuild.
    pub price_evaluations: usize,
    /// Actual frozen rate publication decisions consumed in the baseline.
    pub publications: Vec<convex_core::fixings::FixingRequirement>,
    /// Original full family fits, including source recipes, product valuations
    /// and measured solver diagnostics. Frozen-node scenarios have no new fit.
    pub calibrations: Vec<QuoteGraphCalibration>,
}
/// Lossless original family output; no serialization or fitted-node alias.
#[derive(Debug, Clone)]
pub enum QuoteGraphCalibration {
    /// Complete mixed-rate fit.
    Rate(Box<CalibratedRateCurve>),
    /// Complete single-name credit fit.
    Credit(Box<CalibratedCdsCurve>),
    /// Complete government, issuer-basis or repo fit.
    Cash(Box<CalibratedCashCurve>),
}
/// Privately prepared actual graph. Baseline data and publication decisions cannot
/// be replaced with fabricated accepted fits.
pub struct PreparedQuoteRiskGraph<'a> {
    pub(super) input: QuoteRiskGraphInput<'a>,
    pub(super) order: Vec<(u8, usize)>,
    pub(super) baseline: QuoteGraphScenario,
    pub(super) publications: BTreeMap<(IndexId, Date), UtcTimestamp>,
}
impl PreparedQuoteRiskGraph<'_> {
    /// Original full actually built information set.
    pub fn baseline(&self) -> &QuoteGraphScenario {
        &self.baseline
    }
}
fn preflight(input: &QuoteRiskGraphInput<'_>) -> QuoteRiskResult<Vec<(u8, usize)>> {
    let total = input.rates.len() + input.credit.len() + input.cash.len();
    if total > 64
        || input.supplied.len() + total > 256
        || (!input.rates.is_empty() && input.rate_context.is_none())
    {
        return Err(invalid(
            "graph",
            "bounded graph and explicit rate context required",
        ));
    }
    let helpers = input
        .rates
        .iter()
        .map(|r| r.build.recipe.helpers.len())
        .chain(input.credit.iter().map(|r| r.recipe.helpers.len()))
        .chain(input.cash.iter().map(|r| r.recipe.helpers.len()))
        .sum::<usize>();
    if helpers > 128 {
        return Err(invalid(
            "graph",
            "at most128 total original helpers per risk graph",
        ));
    }
    let mut outputs = BTreeMap::new();
    let mut pending = BTreeMap::new();
    for (i, r) in input.rates.iter().enumerate() {
        if r.quote_versions.len() != r.build.recipe.helpers.len() {
            return Err(invalid(
                "quote revisions",
                "one exact revision per original rate helper required",
            ));
        }
        pending.insert((0, i), &r.build.recipe.curve);
        if let Some(p) = &r.build.recipe.overnight_projection {
            p.key.validate()?;
            if outputs.insert(p.key.clone(), (0, i)).is_some() {
                return Err(invalid("graph", "duplicate overnight output"));
            }
        }
    }
    for (i, r) in input.credit.iter().enumerate() {
        pending.insert((1, i), &r.recipe.curve);
    }
    for (i, r) in input.cash.iter().enumerate() {
        pending.insert((2, i), &r.recipe.curve);
    }
    for (&id, c) in &pending {
        c.key.validate()?;
        if outputs.insert(c.key.clone(), id).is_some() {
            return Err(invalid("graph", "duplicate recipe output"));
        }
    }
    if input.supplied.len() + outputs.len() > 256 {
        return Err(invalid(
            "graph",
            "at most256 total curves, including projection outputs",
        ));
    }
    let mut identities = BTreeSet::new();
    for k in input.supplied.keys().chain(outputs.keys()) {
        if !identities.insert(&k.id) {
            return Err(invalid(
                "graph",
                "duplicate identities/versions or supplied output overwrite",
            ));
        }
    }
    let mut available = input.supplied.keys().cloned().collect::<BTreeSet<_>>();
    let mut order = Vec::new();
    while !pending.is_empty() {
        let id = pending
            .iter()
            .filter(|(id, c)| {
                c.dependencies.iter().all(|k| {
                    available.contains(k)
                        || (id.0 == 0
                            && input.rates[id.1]
                                .build
                                .recipe
                                .overnight_projection
                                .as_ref()
                                .is_some_and(|p| &p.key == k))
                })
            })
            .min_by_key(|(_, c)| &c.key.id)
            .map(|(id, _)| *id)
            .ok_or_else(|| invalid("graph", "missing exact dependency/version or cycle"))?;
        let c = pending
            .remove(&id)
            .ok_or_else(|| invalid("graph", "missing pending recipe"))?;
        available.insert(c.key.clone());
        for (key, owner) in &outputs {
            if owner == &id {
                available.insert(key.clone());
            }
        }
        order.push(id);
    }
    Ok(order)
}
/// Validate topology before any solver, build every full original helper and
/// freeze actual publication/history decisions for every subsequent shock.
pub fn prepare_quote_risk_graph<P>(
    input: QuoteRiskGraphInput<'_>,
    mut publication: P,
) -> QuoteRiskResult<PreparedQuoteRiskGraph<'_>>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    let order = preflight(&input)?;
    crate::cash_calibration::preflight_icma_helpers(input.cash)?;
    let mut publications = BTreeMap::new();
    let baseline = build(&input, &order, None, |id, date| {
        if let Some(t) = publications.get(&(id.clone(), date)) {
            return Ok(*t);
        }
        if publications.len() >= 32768 {
            return Err(CashflowError::BoundExceeded("risk publications"));
        }
        let t = publication(id, date)?;
        publications.insert((id.clone(), date), t);
        Ok(t)
    })?;
    Ok(PreparedQuoteRiskGraph {
        input,
        order,
        baseline,
        publications,
    })
}
fn change(
    key: &QuoteRiskKey,
    curve: &CurveKey,
    id: &str,
    version: Version,
    native: MarketQuote,
    kind: QuoteType,
    delta: f64,
) -> QuoteRiskResult<MarketQuote> {
    let d = key.definition();
    if (!matches!(d.coordinate, RiskCoordinate::ParallelQuotes { .. })
        && d.native_unit != native.unit)
        || d.factor.definition().quote_type != kind
    {
        return Err(invalid(
            "quote",
            "key quote type/native unit differs from actual full helper",
        ));
    }
    let mut moved = native;
    match &d.coordinate {
        RiskCoordinate::Quote {
            curve: c,
            helper_id,
            quote_version,
        } if c == curve && helper_id == id => {
            if *quote_version != version {
                return Err(invalid(
                    "quote revision",
                    "original helper revision mismatch",
                ));
            }
            moved.value += key.native_move(delta)?;
        }
        RiskCoordinate::ParallelQuotes { curve: c, quotes } if c == curve => {
            if let Some((_, v, unit)) = quotes.iter().find(|(s, _, _)| s == id) {
                if *v != version || *unit != native.unit {
                    return Err(invalid(
                        "quote revision",
                        "original helper revision mismatch",
                    ));
                }
                moved = match native.unit {
                    QuoteUnit::DecimalRate | QuoteUnit::PercentRate | QuoteUnit::BasisPoints => {
                        native.model_rate_quote(native.decimal_rate()? + delta)?
                    }
                    QuoteUnit::FuturePrice100MinusRate => MarketQuote {
                        value: native.value - 100.0 * delta,
                        unit: native.unit,
                    },
                    _ => MarketQuote {
                        value: native.value + delta,
                        unit: native.unit,
                    },
                };
            }
        }
        _ => (),
    }
    moved.validate()?;
    Ok(moved)
}
fn rate_kind(h: &RateCalibrationHelper) -> QuoteType {
    match &h.terms {
        RateCalibrationTerms::MoneyMarket(t) => match &t.terms {
            MoneyMarketTerms::Deposit { .. } => QuoteType::DepositRate,
            MoneyMarketTerms::Fra { .. } => QuoteType::FraRate,
        },
        RateCalibrationTerms::RateFuture(_) => QuoteType::RateFutureRate,
        RateCalibrationTerms::BasisSwap { .. } => QuoteType::BasisSwapSpread,
        RateCalibrationTerms::Swap(t) => {
            if matches!(t.floating, convex_rates::FloatingLegTerms::Overnight(_)) {
                QuoteType::OisParRate
            } else {
                QuoteType::SwapParRate
            }
        }
    }
}
fn cash_kind(h: &CashCalibrationHelper) -> QuoteType {
    match h {
        CashCalibrationHelper::Repo { .. } => QuoteType::RepoRate,
        CashCalibrationHelper::Bond { input: h } => match &h.quote_kind {
            CashQuoteKind::CleanPrice => QuoteType::CashCleanPrice,
            CashQuoteKind::DirtyPrice => QuoteType::CashDirtyPrice,
            CashQuoteKind::CashflowYield { .. } | CashQuoteKind::ContextualIcmaYield { .. } => {
                QuoteType::Yield
            }
            CashQuoteKind::BankDiscountYield { .. } => QuoteType::BankDiscountYield,
            CashQuoteKind::ParCouponRate => QuoteType::BondParCoupon,
        },
    }
}
fn selected(key: &QuoteRiskKey, c: &CurveKey, id: &str) -> bool {
    match &key.definition().coordinate {
        RiskCoordinate::Quote {
            curve, helper_id, ..
        } => curve == c && helper_id == id,
        RiskCoordinate::ParallelQuotes { curve, quotes } => {
            curve == c && quotes.iter().any(|(s, _, _)| s == id)
        }
        _ => false,
    }
}
// Recovery is an explicit credit-role assumption, separate from survival nodes.
// Rebuilding supplied dependencies must change that role before any full helper
// is admitted. These supplied nodes remain held; full recovery cannot identify
// a fitted survival curve and is rejected by the original CDS recipe owner.
fn recovered(base: &BuiltCurve, key: &QuoteRiskKey, delta: f64) -> QuoteRiskResult<BuiltCurve> {
    let mut recipe = base.recipe().clone();
    let CurveRole::Credit {
        entity_id,
        recovery,
        ..
    } = &mut recipe.role
    else {
        return Err(invalid("recovery", "actual credit role required"));
    };
    if !matches!(&key.definition().factor.definition().target,FactorTarget::Entity{entity_id:id} if id.as_str()==entity_id)
    {
        return Err(invalid("recovery", "exact legal entity mismatch"));
    }
    *recovery += delta;
    if !(0.0..=1.0).contains(recovery) {
        return Err(invalid(
            "recovery",
            "held-survival recovery must be in[0,1]",
        ));
    }
    let (times, values) = base.interpolation_grid()?;
    if recipe.representation != CurveRepresentation::SurvivalProbabilities {
        return Err(invalid(
            "recovery",
            "actual survival probability grid required",
        ));
    }
    recipe.source = CurveSource::Nodes(
        times
            .iter()
            .zip(values)
            .map(|(&t, &value)| CurveNode {
                pillar: CurvePoint::Tenor(t),
                quote: MarketQuote {
                    value,
                    unit: QuoteUnit::SurvivalProbability,
                },
            })
            .collect(),
    );
    recipe.dependencies.clear();
    recipe.calibration = CalibrationMode::Direct;
    let curves = build_curve_set(&[recipe.clone()], &CurveSet::new())?.curves;
    Ok(curves.get(&recipe.key)?.as_ref().clone())
}
fn build<P>(
    input: &QuoteRiskGraphInput<'_>,
    order: &[(u8, usize)],
    shock: Option<(&QuoteRiskKey, f64)>,
    publication: P,
) -> QuoteRiskResult<QuoteGraphScenario>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    let moves: Vec<_> = shock.into_iter().collect();
    build_many(input, order, &moves, publication)
}
pub(super) fn build_many<P>(
    input: &QuoteRiskGraphInput<'_>,
    order: &[(u8, usize)],
    shock: &[(&QuoteRiskKey, f64)],
    mut publication: P,
) -> QuoteRiskResult<QuoteGraphScenario>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    let mut curves = input.supplied.clone();
    let mut out = QuoteGraphScenario {
        curves: CurveSet::new(),
        build_order: Vec::new(),
        repricing: Vec::new(),
        price_evaluations: 0,
        publications: Vec::new(),
        calibrations: Vec::new(),
    };
    let mut moved = vec![0; shock.len()];
    for (slot, &(key, delta)) in shock.iter().enumerate() {
        if let RiskCoordinate::Recovery { curve } = &key.definition().coordinate {
            if let Ok(base) = input.supplied.get(curve) {
                curves = replace(&curves, recovered(base, key, delta)?)?;
                moved[slot] = 1;
            }
        }
    }
    for &(family, i) in order {
        match family {
            0 => {
                let r = &input.rates[i];
                let mut recipe = r.build.recipe.clone();
                for (slot, &(key, delta)) in shock.iter().enumerate() {
                    for (h, &version) in recipe.helpers.iter_mut().zip(r.quote_versions) {
                        if selected(key, &recipe.curve.key, &h.id) {
                            let q = change(
                                key,
                                &recipe.curve.key,
                                &h.id,
                                version,
                                h.quote,
                                rate_kind(h),
                                delta,
                            )?;
                            *h = h.with_quote(q)?;
                            moved[slot] += 1;
                        }
                    }
                }
                let result = build_rate_curve_set(
                    &[RateCurveBuildInput {
                        recipe: &recipe,
                        resources: r.build.resources,
                    }],
                    &curves,
                    input.history,
                    input
                        .rate_context
                        .ok_or_else(|| invalid("context", "missing rate context"))?,
                    input.controls,
                    &mut publication,
                )?;
                out.build_order.extend(result.build_order);
                out.price_evaluations += result.price_evaluations;
                out.publications.extend(result.publications);
                out.repricing.extend(
                    result
                        .calibrations
                        .iter()
                        .flat_map(|c| c.curve.diagnostics().quotes.clone()),
                );
                out.calibrations.extend(
                    result
                        .calibrations
                        .into_iter()
                        .map(|c| QuoteGraphCalibration::Rate(Box::new(c))),
                );
                curves = result.curves;
            }
            1 => {
                let r = &input.credit[i];
                let mut recipe = r.recipe.clone();
                for (slot, &(key, delta)) in shock.iter().enumerate() {
                    for h in &mut recipe.helpers {
                        if selected(key, &recipe.curve.key, &h.id) {
                            let kind = match h.quote_kind {
                                CdsQuoteKind::RunningSpread => QuoteType::CdsSpread,
                                CdsQuoteKind::CleanUpfront => QuoteType::CdsUpfront,
                                CdsQuoteKind::DirtyUpfront => QuoteType::CdsDirtyUpfront,
                            };
                            h.quote = change(
                                key,
                                &recipe.curve.key,
                                &h.id,
                                h.quote_version,
                                h.quote,
                                kind,
                                delta,
                            )?;
                            moved[slot] += 1;
                        }
                    }
                    if let RiskCoordinate::Recovery { curve } = &key.definition().coordinate {
                        if curve == &recipe.curve.key {
                            let CurveRole::Credit {
                                entity_id,
                                recovery,
                                ..
                            } = &mut recipe.curve.role
                            else {
                                return Err(invalid("recovery", "actual credit role required"));
                            };
                            if !matches!(&key.definition().factor.definition().target,FactorTarget::Entity{entity_id:id} if id.as_str()==entity_id)
                            {
                                return Err(invalid("recovery", "exact legal entity mismatch"));
                            }
                            *recovery += delta;
                            if !(0.0..1.0).contains(recovery) {
                                return Err(invalid(
                                    "recovery",
                                    "quote-rebuilt recovery must be in[0,1)",
                                ));
                            }
                            let fraction = FiniteF64::new(*recovery)
                                .map_err(|e| invalid("recovery", e.to_string()))?;
                            for h in &mut recipe.helpers {
                                h.terms.reference.recovery_fraction = fraction;
                            }
                            moved[slot] += 1;
                        }
                    }
                }
                let result = build_credit_curve_set(
                    &[CdsCurveBuildInput {
                        recipe: &recipe,
                        calendars: r.calendars,
                    }],
                    &curves,
                    input.controls,
                )?;
                out.build_order.extend(result.build_order);
                out.price_evaluations += result.price_evaluations;
                out.repricing.extend(
                    result
                        .calibrations
                        .iter()
                        .flat_map(|c| c.curve.diagnostics().quotes.clone()),
                );
                out.calibrations.extend(
                    result
                        .calibrations
                        .into_iter()
                        .map(|c| QuoteGraphCalibration::Credit(Box::new(c))),
                );
                curves = result.curves;
            }
            2 => {
                let r = &input.cash[i];
                let mut recipe = r.recipe.clone();
                for (slot, &(key, delta)) in shock.iter().enumerate() {
                    for h in &mut recipe.helpers {
                        if selected(key, &recipe.curve.key, h.id()) {
                            if let CashCalibrationHelper::Bond { input: h } = h {
                                if let CashQuoteKind::ContextualIcmaYield { input } = &h.quote_kind
                                {
                                    if key.definition().factor.definition().convention_version
                                        != input.sources.quotation_policy.version
                                    {
                                        return Err(invalid(
                                            "ICMA risk policy",
                                            "exact original quotation policy revision required",
                                        ));
                                    }
                                }
                            }
                            let v = match h {
                                CashCalibrationHelper::Bond { input: h } => h.quote_version,
                                CashCalibrationHelper::Repo { input: h } => h.quote_version,
                            };
                            let q = change(
                                key,
                                &recipe.curve.key,
                                h.id(),
                                v,
                                h.quote(),
                                cash_kind(h),
                                delta,
                            )?;
                            match h {
                                CashCalibrationHelper::Bond { input: h } => {
                                    **h = h.with_quote(q)?
                                }
                                CashCalibrationHelper::Repo { input: h } => h.quote = q,
                            };
                            moved[slot] += 1;
                        }
                    }
                    if let RiskCoordinate::Recovery { curve } = &key.definition().coordinate {
                        if let Some(BondBasisModel::CreditParRecovery {
                            survival, recovery, ..
                        }) = &mut recipe.basis_model
                        {
                            if survival == curve {
                                let credit = curves.get(survival)?;
                                let CurveRole::Credit {
                                    recovery: actual, ..
                                } = credit.recipe().role
                                else {
                                    return Err(invalid("recovery", "actual credit role required"));
                                };
                                *recovery = actual;
                            }
                        }
                    }
                }
                let result = build_cash_curve_set(
                    &[CashCurveBuildInput {
                        recipe: &recipe,
                        resources: r.resources,
                    }],
                    &curves,
                    input.controls,
                )?;
                out.build_order.extend(result.build_order);
                out.price_evaluations += result.price_evaluations;
                out.repricing.extend(
                    result
                        .calibrations
                        .iter()
                        .flat_map(|c| c.curve.diagnostics().quotes.clone()),
                );
                out.calibrations.extend(
                    result
                        .calibrations
                        .into_iter()
                        .map(|c| QuoteGraphCalibration::Cash(Box::new(c))),
                );
                curves = result.curves;
            }
            _ => return Err(invalid("graph", "invalid private family")),
        }
    }
    for (slot, &(key, _)) in shock.iter().enumerate() {
        let expected = match &key.definition().coordinate {
            RiskCoordinate::ParallelQuotes { quotes, .. } => quotes.len(),
            _ => 1,
        };
        if moved[slot] != expected {
            return Err(invalid(
                "coordinate",
                "missing or duplicate complete original helper/recovery mapping",
            ));
        }
    }
    out.curves = curves;
    Ok(out)
}
pub(super) fn replace(curves: &CurveSet, replacement: BuiltCurve) -> QuoteRiskResult<CurveSet> {
    let key = replacement.recipe().key.clone();
    let mut remaining = curves.keys().cloned().collect::<Vec<_>>();
    let mut next = CurveSet::new();
    while !remaining.is_empty() {
        let i = remaining
            .iter()
            .position(|k| {
                let c = if *k == key {
                    &replacement
                } else {
                    match curves.get(k) {
                        Ok(c) => c.as_ref(),
                        Err(_) => return false,
                    }
                };
                c.recipe().dependencies.iter().all(|d| next.get(d).is_ok())
            })
            .ok_or_else(|| invalid("graph", "cannot reconstruct actual frozen dependency set"))?;
        let k = remaining.remove(i);
        next = next.with_curve(if k == key {
            replacement.clone()
        } else {
            curves.get(&k)?.as_ref().clone()
        })?;
    }
    Ok(next)
}
pub(super) fn shifted(
    base: &BuiltCurve,
    key: &QuoteRiskKey,
    delta: f64,
) -> QuoteRiskResult<BuiltCurve> {
    let (times, values) = base.interpolation_grid()?;
    let mut recipe = base.recipe().clone();
    let node = match &key.definition().coordinate {
        RiskCoordinate::Zero { node, .. } | RiskCoordinate::Basis { node, .. } => *node,
        RiskCoordinate::Raw { .. }
            if key.definition().factor.definition().quote_type == QuoteType::JointZeroRate =>
        {
            None
        }
        _ => return Err(invalid("node", "actual zero/basis coordinate required")),
    };
    if node.is_some_and(|n| {
        n >= times.len()
            || (times[n] == 0.0 && recipe.representation == CurveRepresentation::DiscountFactors)
    }) {
        return Err(invalid(
            "node",
            "actual informative interpolation-grid index required",
        ));
    }
    let basis = matches!(key.definition().coordinate, RiskCoordinate::Basis { .. });
    if (basis && !matches!(recipe.role, CurveRole::IssuerSpread { .. }))
        || (!basis
            && !matches!(
                recipe.role,
                CurveRole::Discount { .. }
                    | CurveRole::Projection { .. }
                    | CurveRole::Government { .. }
                    | CurveRole::Repo { .. }
            ))
    {
        return Err(invalid(
            "node",
            "factor does not match actual financial grid role",
        ));
    }
    let nodes = times
        .iter()
        .zip(values)
        .enumerate()
        .map(|(i, (&t, &v))| {
            let change = if node.is_none_or(|n| n == i) {
                delta
            } else {
                0.0
            };
            let (value, unit) = match recipe.representation {
                CurveRepresentation::ZeroRates {
                    compounding: Compounding::Continuous,
                } if !basis => (v + change, QuoteUnit::DecimalRate),
                CurveRepresentation::DiscountFactors if !basis => {
                    (v * portable::exp(-change * t), QuoteUnit::DiscountFactor)
                }
                CurveRepresentation::IssuerSpreads if basis => (v + change, QuoteUnit::DecimalRate),
                _ => {
                    return Err(invalid(
                        "node",
                        "continuous-zero/DF or explicit residual basis grid required",
                    ))
                }
            };
            Ok(CurveNode {
                pillar: CurvePoint::Tenor(t),
                quote: MarketQuote { value, unit },
            })
        })
        .collect::<QuoteRiskResult<Vec<_>>>()?;
    recipe.source = CurveSource::Nodes(nodes);
    recipe.dependencies.clear();
    recipe.calibration = CalibrationMode::Direct;
    let c = build_curve_set(&[recipe.clone()], &CurveSet::new())?.curves;
    Ok(c.get(&recipe.key)?.as_ref().clone())
}
/// Evaluate one actual curve coordinate without mutating baseline inputs. Quote
/// moves rebuild every full recipe. Node/basis risk explicitly freezes other
/// curves; dependent rebuilding of a supplied node is supported via a new supplied
/// graph. Calibrated-node rebuilding fails explicitly. Frozen recovery preserves
/// the actual survival grid and changes only its separate legal assumption.
pub fn evaluate_quote_graph(
    prepared: &PreparedQuoteRiskGraph<'_>,
    key: &QuoteRiskKey,
    delta: f64,
) -> QuoteRiskResult<QuoteGraphScenario> {
    finite(delta, "shock")?;
    let mut publication = |id: &IndexId, date: Date| {
        prepared
            .publications
            .get(&(id.clone(), date))
            .copied()
            .ok_or_else(|| CashflowError::InvalidTerms {
                field: "risk publication",
                reason: "new observation outside frozen original calibration requirements".into(),
            })
    };
    match &key.definition().coordinate {
        RiskCoordinate::Raw { .. }
            if key.definition().factor.definition().quote_type == QuoteType::JointZeroRate =>
        {
            let (discount, projection) = joint_curve_keys(key)?;
            let d = prepared.baseline.curves.get(&discount)?;
            let p = prepared.baseline.curves.get(&projection)?;
            let currency = key.definition().factor.definition().currency;
            if d.recipe().currency != currency
                || p.recipe().currency != currency
                || !matches!(d.recipe().role, CurveRole::Discount { collateral_currency } if collateral_currency == currency)
                || !matches!(p.recipe().role, CurveRole::Projection { .. })
            {
                return Err(invalid(
                    "joint zero",
                    "exact independent same-currency discount and index projection roles required",
                ));
            }
            let curves = replace(&prepared.baseline.curves, shifted(d, key, delta)?)?;
            let curves = replace(&curves, shifted(p, key, delta)?)?;
            Ok(QuoteGraphScenario {
                curves,
                build_order: Vec::new(),
                repricing: Vec::new(),
                price_evaluations: 0,
                publications: prepared.baseline.publications.clone(),
                calibrations: Vec::new(),
            })
        }
        RiskCoordinate::Quote { curve, .. }
        | RiskCoordinate::ParallelQuotes { curve, .. }
        | RiskCoordinate::Recovery { curve } => {
            let base = prepared.baseline.curves.get(curve)?;
            if base.recipe().currency != key.definition().factor.definition().currency {
                return Err(invalid("factor currency", "actual curve currency differs"));
            }
            if key.definition().response == RiskResponse::FrozenCurves {
                if !matches!(key.definition().coordinate, RiskCoordinate::Recovery { .. }) {
                    return Err(invalid("response", "market quotes require full rebuilding"));
                }
                let replacement = recovered(base, key, delta)?;
                return Ok(QuoteGraphScenario {
                    curves: replace(&prepared.baseline.curves, replacement)?,
                    build_order: Vec::new(),
                    repricing: Vec::new(),
                    price_evaluations: 0,
                    publications: prepared.baseline.publications.clone(),
                    calibrations: Vec::new(),
                });
            }
            build(
                &prepared.input,
                &prepared.order,
                Some((key, delta)),
                &mut publication,
            )
        }
        RiskCoordinate::Zero { curve, .. } | RiskCoordinate::Basis { curve, .. } => {
            let base = prepared.baseline.curves.get(curve)?;
            if base.recipe().currency != key.definition().factor.definition().currency {
                return Err(invalid("factor currency", "actual curve currency differs"));
            }
            let replacement = shifted(base, key, delta)?;
            if key.definition().response == RiskResponse::RebuildDependents {
                prepared.input.supplied.get(curve).map_err(|_|invalid("response","rebuilding calibrated node shocks requires a separate full quote graph; supplied node coordinates only"))?;
                let supplied = replace(prepared.input.supplied, replacement)?;
                let input = QuoteRiskGraphInput {
                    rates: prepared.input.rates,
                    credit: prepared.input.credit,
                    cash: prepared.input.cash,
                    supplied: &supplied,
                    history: prepared.input.history,
                    rate_context: prepared.input.rate_context,
                    controls: prepared.input.controls,
                };
                build(&input, &prepared.order, None, &mut publication)
            } else {
                Ok(QuoteGraphScenario {
                    curves: replace(&prepared.baseline.curves, replacement)?,
                    build_order: Vec::new(),
                    repricing: Vec::new(),
                    price_evaluations: 0,
                    publications: prepared.baseline.publications.clone(),
                    calibrations: Vec::new(),
                })
            }
        }
        RiskCoordinate::Hazard { curve, node } => {
            let base = prepared.baseline.curves.get(curve)?;
            let (times, values) = base.interpolation_grid()?;
            if base.recipe().currency != key.definition().factor.definition().currency
                || !matches!(base.recipe().role, CurveRole::Credit { .. })
                || base.recipe().representation != CurveRepresentation::SurvivalProbabilities
                || base.recipe().interpolation != convex_curves::InterpolationMethod::LogLinear
                || key.definition().response != RiskResponse::FrozenCurves
                || node.is_some_and(|n| n == 0 || n >= times.len())
                || times.first() != Some(&0.0)
                || values.first() != Some(&1.0)
            {
                return Err(invalid(
                    "hazard",
                    "actual frozen log-linear survival intervals/currency with Q(0)=1 required",
                ));
            }
            let mut nodes = Vec::with_capacity(times.len());
            let mut survival = 1.0;
            for (i, &t) in times.iter().enumerate() {
                if i > 0 {
                    let dt = t - times[i - 1];
                    let hazard = -portable::log(values[i] / values[i - 1]) / dt
                        + if node.is_none_or(|n| n == i) {
                            delta
                        } else {
                            0.0
                        };
                    if hazard < 0.0 || !hazard.is_finite() {
                        return Err(invalid(
                            "hazard",
                            "negative or nonfinite bumped interval hazard",
                        ));
                    }
                    survival *= portable::exp(-hazard * dt);
                }
                nodes.push(CurveNode {
                    pillar: CurvePoint::Tenor(t),
                    quote: MarketQuote {
                        value: survival,
                        unit: QuoteUnit::SurvivalProbability,
                    },
                });
            }
            let mut recipe = base.recipe().clone();
            recipe.source = CurveSource::Nodes(nodes);
            recipe.dependencies.clear();
            recipe.calibration = CalibrationMode::Direct;
            let c = build_curve_set(&[recipe.clone()], &CurveSet::new())?.curves;
            Ok(QuoteGraphScenario {
                curves: replace(
                    &prepared.baseline.curves,
                    c.get(&recipe.key)?.as_ref().clone(),
                )?,
                build_order: Vec::new(),
                repricing: Vec::new(),
                price_evaluations: 0,
                publications: prepared.baseline.publications.clone(),
                calibrations: Vec::new(),
            })
        }
        RiskCoordinate::Raw { .. } => Err(invalid(
            "coordinate",
            "raw product quotes require their complete native pricer",
        )),
    }
}
/// Signed stable graph risk. The supplied pure callback prices the SAME full
/// native trade against the actual complete scenario curves. Any trade-specific
/// recovery/model economics must bind to the declared scenario curve assumptions.
pub fn curve_quote_sensitivity<F>(
    prepared: &PreparedQuoteRiskGraph<'_>,
    key: QuoteRiskKey,
    position: RiskPosition,
    snapshot_id: convex_core::contracts::SnapshotId,
    settings: QuoteRiskSettings,
    mut price: F,
) -> QuoteRiskResult<QuoteSensitivity>
where
    F: FnMut(&QuoteGraphScenario) -> QuoteRiskResult<convex_core::contracts::ModelMoney>,
{
    reference_quote_sensitivity(key.clone(), position, snapshot_id, settings, |d| {
        if d == 0.0 {
            price(&prepared.baseline)
        } else {
            price(&evaluate_quote_graph(prepared, &key, d)?)
        }
    })
}

/// Reconcile every actual informative grid coordinate to a simultaneous full
/// node move. Missing and duplicate indices fail independently of the numeric
/// discrepancy tolerance, including zero-risk nodes. DF/hazard origins are
/// mathematical anchors and cannot masquerade as informative risk buckets.
pub fn reconcile_curve_node_buckets(
    prepared: &PreparedQuoteRiskGraph<'_>,
    buckets: &[QuoteSensitivity],
    parallel: &QuoteSensitivity,
    tolerance: RiskTolerance,
) -> QuoteRiskResult<RiskReconciliation> {
    let (curve, hazard) = match &parallel.key.definition().coordinate {
        RiskCoordinate::Zero { curve, node: None }
        | RiskCoordinate::Basis { curve, node: None } => (curve, false),
        RiskCoordinate::Hazard { curve, node: None } => (curve, true),
        _ => {
            return Err(invalid(
                "parallel",
                "complete actual parallel node scope required",
            ))
        }
    };
    let base = prepared.baseline.curves.get(curve)?;
    let (times, _) = base.interpolation_grid()?;
    let expected = (0..times.len())
        .filter(|&n| {
            !(times[n] == 0.0
                && (hazard || base.recipe().representation == CurveRepresentation::DiscountFactors))
        })
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    for b in buckets {
        let index = match &b.key.definition().coordinate {
            RiskCoordinate::Zero {
                curve: c,
                node: Some(n),
            }
            | RiskCoordinate::Basis {
                curve: c,
                node: Some(n),
            }
            | RiskCoordinate::Hazard {
                curve: c,
                node: Some(n),
            } if c == curve => *n,
            _ => {
                return Err(invalid(
                    "buckets",
                    "original single-node grid scope required",
                ))
            }
        };
        if !actual.insert(index) {
            return Err(invalid("buckets", "duplicate original node index"));
        }
    }
    if actual != expected {
        return Err(invalid(
            "buckets",
            "all original informative grid nodes required",
        ));
    }
    super::finite::reconcile(buckets, parallel, tolerance)
}
