//! Complete checked model/segment/delegation contracts over original native owners.

use super::{
    build::{point_tenor, positive},
    inputs::{validate_label, validate_tolerance},
    BuiltCurve, CalibrationMode, CurveContractError as Error, CurveContractResult as Result,
    CurveDomain, CurveKey, CurvePoint, CurveRecipe, CurveRepresentation, CurveSet, CurveSource,
    MarketQuote, TenorRounding, MAX_CURVE_DERIVATIONS, MAX_CURVE_INPUTS, MAX_CURVE_SEGMENTS,
    MIN_PILLAR_SEPARATION,
};
use crate::{
    curves::{CurveSegment, DelegatedCurve, DelegationFallback, SegmentSource, SegmentedCurve},
    ExtrapolationMethod, InterpolationMethod, TermStructure, ValueType,
};
use convex_core::types::{Compounding, Date};
use convex_math::interpolation::{Interpolator, NelsonSiegel, Svensson};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc};

/// Annualized continuous-zero model. Decays are years of the recipe's time axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParametricModel {
    /// Original four-parameter Nelson–Siegel mathematics.
    NelsonSiegel {
        /// Long-run decimal zero level.
        beta0: f64,
        /// Decimal short-end loading.
        beta1: f64,
        /// Decimal hump loading.
        beta2: f64,
        /// Positive decay in curve-time years.
        tau: f64,
    },
    /// Original six-parameter Svensson mathematics.
    Svensson {
        /// Long-run decimal zero level.
        beta0: f64,
        /// Decimal short-end loading.
        beta1: f64,
        /// First decimal hump loading.
        beta2: f64,
        /// Second decimal hump loading.
        beta3: f64,
        /// First positive decay in curve-time years.
        tau1: f64,
        /// Second positive decay in curve-time years.
        tau2: f64,
    },
}
impl ParametricModel {
    /// Original parameter order: beta0,beta1,beta2,tau or
    /// beta0,beta1,beta2,beta3,tau1,tau2. No normalization or fitted alias.
    pub fn parameters(&self) -> Vec<f64> {
        match *self {
            Self::NelsonSiegel {
                beta0,
                beta1,
                beta2,
                tau,
            } => vec![beta0, beta1, beta2, tau],
            Self::Svensson {
                beta0,
                beta1,
                beta2,
                beta3,
                tau1,
                tau2,
            } => vec![beta0, beta1, beta2, beta3, tau1, tau2],
        }
    }
    pub(super) fn with_parameters(&self, values: &[f64]) -> Result<Self> {
        match (self, values) {
            (Self::NelsonSiegel { .. }, [beta0, beta1, beta2, tau]) => Ok(Self::NelsonSiegel {
                beta0: *beta0,
                beta1: *beta1,
                beta2: *beta2,
                tau: *tau,
            }),
            (Self::Svensson { .. }, [beta0, beta1, beta2, beta3, tau1, tau2]) => {
                Ok(Self::Svensson {
                    beta0: *beta0,
                    beta1: *beta1,
                    beta2: *beta2,
                    beta3: *beta3,
                    tau1: *tau1,
                    tau2: *tau2,
                })
            }
            _ => Err(Error::invalid(
                "parameters",
                "exact original model dimension required",
            )),
        }
    }
    pub(super) fn validate(&self) -> Result<()> {
        let result = match *self {
            Self::NelsonSiegel {
                beta0,
                beta1,
                beta2,
                tau,
            } => NelsonSiegel::new(beta0, beta1, beta2, tau).map(|_| ()),
            Self::Svensson {
                beta0,
                beta1,
                beta2,
                beta3,
                tau1,
                tau2,
            } => Svensson::new(beta0, beta1, beta2, beta3, tau1, tau2).map(|_| ()),
        };
        result.map_err(|e| Error::invalid("parameters", e.to_string()))
    }
    pub(super) fn zero(&self, t: f64) -> Result<f64> {
        let value = match *self {
            Self::NelsonSiegel {
                beta0,
                beta1,
                beta2,
                tau,
            } => NelsonSiegel::new(beta0, beta1, beta2, tau).and_then(|m| m.interpolate(t)),
            Self::Svensson {
                beta0,
                beta1,
                beta2,
                beta3,
                tau1,
                tau2,
            } => {
                Svensson::new(beta0, beta1, beta2, beta3, tau1, tau2).and_then(|m| m.interpolate(t))
            }
        };
        value.map_err(|e| Error::Numerical(e.to_string()))
    }
    pub(super) fn forward(&self, t: f64) -> Result<f64> {
        let value = match *self {
            Self::NelsonSiegel {
                beta0,
                beta1,
                beta2,
                tau,
            } => NelsonSiegel::new(beta0, beta1, beta2, tau).and_then(|m| m.try_forward_rate(t)),
            Self::Svensson {
                beta0,
                beta1,
                beta2,
                beta3,
                tau1,
                tau2,
            } => Svensson::new(beta0, beta1, beta2, beta3, tau1, tau2)
                .and_then(|m| m.try_forward_rate(t)),
        };
        value.map_err(|e| Error::Numerical(e.to_string()))
    }
    fn method(&self) -> InterpolationMethod {
        if matches!(self, Self::NelsonSiegel { .. }) {
            InterpolationMethod::NelsonSiegel
        } else {
            InterpolationMethod::Svensson
        }
    }
}

/// Actual continuous-zero observation. Revisions never come from a provider lookup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ParametricObservation {
    /// Unique original observation identity.
    pub id: String,
    /// Exact original quote revision, independent of output curve version.
    pub version: String,
    /// Actual observation date, strictly after reference within model horizon.
    pub pillar: Date,
    /// Decimal/percent/basis-point annualized continuous-zero quote.
    pub quote: MarketQuote,
    /// Absolute model-minus-input bound in the original native unit.
    pub tolerance: f64,
}
/// Explicit finite raw interval for native delegation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CurveRange {
    /// Inclusive first model point.
    pub start: CurvePoint,
    /// Inclusive last model point, strictly after start.
    pub end: CurvePoint,
}
/// Complete native delegated segment; no anonymous or sampled dependency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CurveSegmentInput {
    /// Inclusive start; adjacent intervals must meet exactly in model time.
    pub start: CurvePoint,
    /// Exclusive internal end; the final endpoint returns the final source value.
    pub end: CurvePoint,
    /// Exact supplied dependency version with full original source/method terms.
    pub base: CurveKey,
    /// Original fallback against the dependency's own raw domain.
    pub fallback: DelegationFallback,
}
/// Every original observation, actual model time, native model quote and residual.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ParametricObservationValue {
    /// Full original dated observation and revision.
    pub input: ParametricObservation,
    /// Actual time in the declared basis.
    pub tenor: f64,
    /// Model quote in the original input unit.
    pub model: MarketQuote,
    /// Signed model minus input, without objective scaling.
    pub residual: f64,
}
/// Checked C0 join; derivatives and instantaneous forwards need not be continuous.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SegmentJoinValue {
    /// Actual shared boundary time; the right segment owns it.
    pub tenor: f64,
    /// Exact left dependency version.
    pub left: CurveKey,
    /// Exact right dependency version.
    pub right: CurveKey,
    /// Original raw left value at boundary.
    pub left_value: f64,
    /// Original raw right value at boundary.
    pub right_value: f64,
    /// Signed right minus left in the raw representation unit.
    pub residual: f64,
    /// Supplied absolute C0 bound in that same unit.
    pub tolerance: f64,
}
/// Original dependency's own domain, distinct from a delegated override.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CurveSourceDomain {
    /// Exact dependency version.
    pub curve: CurveKey,
    /// Actual inherited raw coverage and explicit tail policy.
    pub domain: CurveDomain,
}
/// Complete construction evidence; supplied parameters never assert fitted rank.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StructuredCurveDiagnostics {
    /// Actual original family.
    pub family: String,
    /// Complete actual model parameters; absent for composition.
    pub parameters: Option<ParametricModel>,
    /// Every original observation's native residual, in source order.
    pub observations: Vec<ParametricObservationValue>,
    /// Every internal raw join, in segment order.
    pub joins: Vec<SegmentJoinValue>,
    /// Every distinct original dependency's actual domain and policy.
    pub source_domains: Vec<CurveSourceDomain>,
}

#[derive(Debug, Clone)]
pub(super) struct CheckedView {
    base: Arc<BuiltCurve>,
    max_date: Date,
}
impl CheckedView {
    fn new(base: Arc<BuiltCurve>) -> Result<Self> {
        let max_date =
            base.tenor_to_date(base.domain().maximum_tenor, TenorRounding::AtOrBefore)?;
        Ok(Self { base, max_date })
    }
}
impl TermStructure for CheckedView {
    fn reference_date(&self) -> Date {
        self.base.recipe().reference_date
    }
    fn value_at(&self, t: f64) -> f64 {
        self.base.raw(t).unwrap_or(f64::NAN)
    }
    fn tenor_bounds(&self) -> (f64, f64) {
        self.base.bounds()
    }
    fn value_type(&self) -> ValueType {
        value_type(self.base.recipe())
    }
    fn max_date(&self) -> Date {
        self.max_date
    }
}
fn value_type(recipe: &CurveRecipe) -> ValueType {
    match recipe.representation {
        CurveRepresentation::ZeroRates { compounding } => ValueType::ZeroRate {
            compounding,
            day_count: recipe.time_basis,
        },
        _ => ValueType::DiscountFactor,
    }
}
#[derive(Debug, Clone)]
pub(super) enum StructuredValues {
    Parametric {
        model: ParametricModel,
        horizon: f64,
    },
    Delegated {
        curve: DelegatedCurve<CheckedView>,
        base: Arc<BuiltCurve>,
        range: (f64, f64),
        fallback: DelegationFallback,
    },
    Segmented {
        curve: SegmentedCurve,
        bases: Vec<(Arc<BuiltCurve>, DelegationFallback)>,
    },
}
#[derive(Debug, Clone)]
pub(super) struct StructuredCurve {
    pub(super) values: StructuredValues,
    pub(super) diagnostics: StructuredCurveDiagnostics,
}
impl StructuredCurve {
    pub(super) fn bounds(&self) -> (f64, f64) {
        match &self.values {
            StructuredValues::Parametric { horizon, .. } => (0.0, *horizon),
            StructuredValues::Delegated { range, .. } => *range,
            StructuredValues::Segmented { curve, .. } => curve.tenor_bounds(),
        }
    }
    pub(super) fn value(&self, t: f64, extrapolation: ExtrapolationMethod) -> Result<f64> {
        let (min, max) = self.bounds();
        let effective = if extrapolation == ExtrapolationMethod::Flat {
            t.clamp(min, max)
        } else {
            t
        };
        match &self.values {
            StructuredValues::Parametric { model, .. } => model.zero(effective),
            StructuredValues::Delegated { curve, .. } => curve
                .try_value_at(t)
                .map_err(|e| Error::Numerical(e.to_string())),
            StructuredValues::Segmented { curve, .. } => Ok(curve.value_at(effective)),
        }
    }
    pub(super) fn extension_used(&self, t: f64, extrapolation: ExtrapolationMethod) -> bool {
        let (min, max) = self.bounds();
        let outer = t < min || t > max;
        let effective = if extrapolation == ExtrapolationMethod::Flat {
            t.clamp(min, max)
        } else {
            t
        };
        outer
            || match &self.values {
                StructuredValues::Parametric { .. } => false,
                StructuredValues::Delegated {
                    base,
                    range,
                    fallback,
                    ..
                } => base.extension_used(delegated_point(t, *range, *fallback)),
                StructuredValues::Segmented { curve, bases } => {
                    let i = curve
                        .segments()
                        .partition_point(|s| s.start <= effective)
                        .saturating_sub(1);
                    let (base, fallback) = &bases[i];
                    let (low, high) = base.bounds();
                    (effective > 0.0 && (effective < low || effective > high))
                        || base.extension_used(delegated_point(effective, (low, high), *fallback))
                }
            }
    }
    pub(super) fn forward(&self, t: f64) -> Result<f64> {
        match &self.values {
            StructuredValues::Parametric {model,horizon} if t<=*horizon=>model.forward(t),
            _=>Err(Error::unsupported("instantaneous forward", "only the original parametric model inside its declared horizon; no finite-difference or boundary proxy")),
        }
    }
}
fn delegated_point(t: f64, range: (f64, f64), fallback: DelegationFallback) -> f64 {
    if matches!(
        fallback,
        DelegationFallback::Clamp | DelegationFallback::FlatExtrapolation
    ) {
        t.clamp(range.0, range.1)
    } else {
        t
    }
}
pub(super) fn interval(
    recipe: &CurveRecipe,
    start: CurvePoint,
    end: CurvePoint,
) -> Result<(f64, f64)> {
    let (a, b) = (
        point_tenor(recipe.reference_date, recipe.time_basis, start)?,
        point_tenor(recipe.reference_date, recipe.time_basis, end)?,
    );
    if b - a < MIN_PILLAR_SEPARATION {
        return Err(Error::invalid(
            "range",
            "finite ordered distinct model-time endpoints required",
        ));
    }
    Ok((a, b))
}
pub(super) fn validate_shape(recipe: &CurveRecipe) -> Result<()> {
    if recipe.calibration != CalibrationMode::Direct {
        return Err(Error::unsupported("structured calibration", "use explicit fit_parametric_curve for zero-observation calibration; no grid or global-helper alias"));
    }
    if !recipe.role.is_rate()
        || !matches!(
            recipe.representation,
            CurveRepresentation::DiscountFactors | CurveRepresentation::ZeroRates { .. }
        )
    {
        return Err(Error::unsupported("structured representation", "same-role rate DF/zero curves; survival/basis compositions need separate exact hazard/integration contracts"));
    }
    match &recipe.source {
        CurveSource::Parametric {
            parameters,
            horizon,
            observations,
        } => {
            parameters.validate()?;
            if recipe.interpolation != parameters.method()
                || recipe.representation
                    != (CurveRepresentation::ZeroRates {
                        compounding: Compounding::Continuous,
                    })
            {
                return Err(Error::invalid(
                    "parametric method",
                    "original model family and continuous-zero representation required",
                ));
            }
            let end = point_tenor(recipe.reference_date, recipe.time_basis, *horizon)?;
            if end < MIN_PILLAR_SEPARATION {
                return Err(Error::invalid(
                    "horizon",
                    "positive finite model horizon required",
                ));
            }
            observation_times(recipe, observations, end)?;
        }
        CurveSource::Delegated {
            range, fallback, ..
        } => {
            if recipe.interpolation != InterpolationMethod::Delegated {
                return Err(Error::invalid(
                    "delegated method",
                    "Delegated construction tag required; source method remains exact",
                ));
            }
            interval(recipe, range.start, range.end)?;
            let valid = match fallback {
                DelegationFallback::Strict => recipe.extrapolation == ExtrapolationMethod::None,
                DelegationFallback::Clamp | DelegationFallback::FlatExtrapolation => {
                    recipe.extrapolation == ExtrapolationMethod::Flat
                }
                DelegationFallback::Trust => true,
            };
            if !valid {
                return Err(Error::invalid("delegation fallback", "Strict requires None; Clamp/FlatExtrapolation require raw Flat; Trust inherits exact base policy"));
            }
            return Ok(());
        }
        CurveSource::Segmented {
            segments,
            join_tolerance,
        } => {
            if recipe.interpolation != InterpolationMethod::Segmented {
                return Err(Error::invalid(
                    "segmented method",
                    "Segmented construction tag required",
                ));
            }
            if segments.is_empty() || segments.len() > MAX_CURVE_SEGMENTS {
                return Err(Error::BoundExceeded("curve segments"));
            }
            validate_tolerance(*join_tolerance)?;
            let mut previous = 0.0;
            for segment in segments {
                segment.base.validate()?;
                let (start, end) = interval(recipe, segment.start, segment.end)?;
                if start != previous {
                    return Err(Error::invalid("segments", "original ordered contiguous coverage from zero required; no gap, overlap, sorting or inferred unbounded tail"));
                }
                previous = end;
            }
        }
        _ => {
            return Err(Error::invalid(
                "source",
                "complete structured source required",
            ))
        }
    }
    if !matches!(
        recipe.extrapolation,
        ExtrapolationMethod::None | ExtrapolationMethod::Flat
    ) {
        return Err(Error::unsupported("structured extrapolation", "None or raw-value Flat only; no implicit model, linear, flat-forward or UFR substitute"));
    }
    Ok(())
}
pub(super) fn observation_times(
    recipe: &CurveRecipe,
    observations: &[ParametricObservation],
    horizon: f64,
) -> Result<Vec<f64>> {
    if observations.len() > MAX_CURVE_INPUTS {
        return Err(Error::BoundExceeded("parametric observations"));
    }
    let mut ids = BTreeSet::new();
    let mut dates = BTreeSet::new();
    let mut times = Vec::with_capacity(observations.len());
    for observation in observations {
        validate_label(&observation.id, "observation.id")?;
        validate_label(&observation.version, "observation.version")?;
        observation.quote.decimal_rate()?;
        validate_tolerance(observation.tolerance)?;
        let t = point_tenor(
            recipe.reference_date,
            recipe.time_basis,
            CurvePoint::Date(observation.pillar),
        )?;
        if t <= 0.0
            || t > horizon
            || !ids.insert(&observation.id)
            || !dates.insert(observation.pillar)
        {
            return Err(Error::invalid(
                "observations",
                "unique actual positive dates/identities within the model horizon required",
            ));
        }
        times.push(t);
    }
    Ok(times)
}
pub(super) fn observation_values(
    recipe: &CurveRecipe,
    model: &ParametricModel,
    horizon: f64,
) -> Result<Vec<ParametricObservationValue>> {
    let CurveSource::Parametric { observations, .. } = &recipe.source else {
        return Err(Error::invalid("source", "Parametric required"));
    };
    observations
        .iter()
        .zip(observation_times(recipe, observations, horizon)?)
        .map(|(input, tenor)| {
            let quote = input.quote.model_rate_quote(model.zero(tenor)?)?;
            let residual = quote.value - input.quote.value;
            if residual.abs() > input.tolerance {
                return Err(Error::RepricingFailure {
                    helper_id: input.id.clone(),
                    residual,
                    tolerance: input.tolerance,
                });
            }
            Ok(ParametricObservationValue {
                input: input.clone(),
                tenor,
                model: quote,
                residual,
            })
        })
        .collect()
}
fn matching_base(
    recipe: &CurveRecipe,
    key: &CurveKey,
    supplied: &CurveSet,
) -> Result<Arc<BuiltCurve>> {
    let base = Arc::clone(supplied.get(key)?);
    let old = base.recipe();
    if old.role != recipe.role
        || old.currency != recipe.currency
        || old.reference_date != recipe.reference_date
        || old.time_basis != recipe.time_basis
        || old.representation != recipe.representation
    {
        return Err(Error::invalid("structured dependency", "exact matching role/currency/reference/time basis/representation required; use an explicit equivalent view before composition"));
    }
    if base.depth() >= MAX_CURVE_DERIVATIONS {
        return Err(Error::BoundExceeded("derived curve nesting"));
    }
    Ok(base)
}
fn check_available(
    base: &BuiltCurve,
    range: (f64, f64),
    fallback: DelegationFallback,
) -> Result<()> {
    let bounds = base.bounds();
    if fallback == DelegationFallback::Strict && (range.0 < bounds.0 || range.1 > bounds.1) {
        return Err(Error::invalid(
            "dependency horizon",
            "Strict intervals must be wholly covered by the actual source domain",
        ));
    }
    base.raw(delegated_point(range.0, bounds, fallback))?;
    base.raw(delegated_point(range.1, bounds, fallback))?;
    Ok(())
}
/// Build original model/segment/delegation values after generic immutable ingress.
pub(super) fn build(
    recipe: &CurveRecipe,
    supplied: &CurveSet,
) -> Result<(StructuredCurve, usize, Vec<f64>)> {
    validate_shape(recipe)?;
    let mut diagnostics = StructuredCurveDiagnostics {
        family: String::new(),
        parameters: None,
        observations: vec![],
        joins: vec![],
        source_domains: vec![],
    };
    let (values, depth, pillars) = match &recipe.source {
        CurveSource::Parametric {
            parameters,
            horizon,
            ..
        } => {
            let horizon = point_tenor(recipe.reference_date, recipe.time_basis, *horizon)?;
            diagnostics.family = "supplied_parametric_continuous_zero".into();
            diagnostics.parameters = Some(parameters.clone());
            diagnostics.observations = observation_values(recipe, parameters, horizon)?;
            positive(
                super::discount_from_rate(
                    parameters.zero(horizon)?,
                    horizon,
                    Compounding::Continuous,
                )?,
                "model horizon discount",
            )?;
            (
                StructuredValues::Parametric {
                    model: parameters.clone(),
                    horizon,
                },
                0,
                vec![0.0, horizon],
            )
        }
        CurveSource::Delegated {
            base,
            range,
            fallback,
        } => {
            let base = matching_base(recipe, base, supplied)?;
            let range = interval(recipe, range.start, range.end)?;
            if *fallback == DelegationFallback::Trust && range != base.bounds() {
                return Err(Error::invalid("delegation Trust bounds", "Trust must retain exact source domain; a narrower range requires Strict or explicit Clamp/FlatExtrapolation"));
            }
            if *fallback == DelegationFallback::Trust
                && recipe.extrapolation != base.recipe().extrapolation
            {
                return Err(Error::invalid(
                    "delegation Trust",
                    "exact original source extrapolation required",
                ));
            }
            // Bounds overrides cannot admit a missing portion of the source.
            check_available(&base, range, *fallback)?;
            diagnostics.family = "exact_native_delegation".into();
            diagnostics.source_domains.push(CurveSourceDomain {
                curve: base.recipe().key.clone(),
                domain: base.domain(),
            });
            let curve =
                DelegatedCurve::new(Arc::new(CheckedView::new(Arc::clone(&base))?), *fallback)
                    .with_bounds(range.0, range.1);
            let depth = base.depth() + 1;
            (
                StructuredValues::Delegated {
                    curve,
                    base,
                    range,
                    fallback: *fallback,
                },
                depth,
                vec![range.0, range.1],
            )
        }
        CurveSource::Segmented {
            segments,
            join_tolerance,
        } => build_segmented(
            recipe,
            supplied,
            segments,
            *join_tolerance,
            &mut diagnostics,
        )?,
        _ => return Err(Error::invalid("source", "structured source required")),
    };
    Ok((
        StructuredCurve {
            values,
            diagnostics,
        },
        depth,
        pillars,
    ))
}

fn build_segmented(
    recipe: &CurveRecipe,
    supplied: &CurveSet,
    segments: &[CurveSegmentInput],
    join_tolerance: f64,
    diagnostics: &mut StructuredCurveDiagnostics,
) -> Result<(StructuredValues, usize, Vec<f64>)> {
    let mut native = Vec::with_capacity(segments.len());
    let mut bases = Vec::with_capacity(segments.len());
    let mut depth = 0;
    let mut pillars = vec![0.0];
    let mut domains = BTreeSet::new();
    for segment in segments {
        let base = matching_base(recipe, &segment.base, supplied)?;
        let range = interval(recipe, segment.start, segment.end)?;
        check_available(&base, range, segment.fallback)?;
        if let Some((left, left_fallback)) = bases.last() {
            let left: &Arc<BuiltCurve> = left;
            let left_value = left.raw(delegated_point(range.0, left.bounds(), *left_fallback))?;
            let right_value =
                base.raw(delegated_point(range.0, base.bounds(), segment.fallback))?;
            let residual = right_value - left_value;
            if !residual.is_finite() || residual.abs() > join_tolerance {
                return Err(Error::RepricingFailure {
                    helper_id: format!("join.{}", bases.len()),
                    residual,
                    tolerance: join_tolerance,
                });
            }
            diagnostics.joins.push(SegmentJoinValue {
                tenor: range.0,
                left: left.recipe().key.clone(),
                right: base.recipe().key.clone(),
                left_value,
                right_value,
                residual,
                tolerance: join_tolerance,
            });
        }
        if domains.insert(base.recipe().key.clone()) {
            diagnostics.source_domains.push(CurveSourceDomain {
                curve: base.recipe().key.clone(),
                domain: base.domain(),
            });
        }
        depth = depth.max(base.depth() + 1);
        pillars.push(range.1);
        let view = Arc::new(CheckedView::new(Arc::clone(&base))?);
        let delegated = Arc::new(DelegatedCurve::new(view, segment.fallback));
        native.push(CurveSegment::new(
            range.0,
            Some(range.1),
            SegmentSource::Delegated {
                curve: delegated.clone(),
                fallback: segment.fallback,
            },
            delegated,
        ));
        bases.push((base, segment.fallback));
    }
    diagnostics.family = "exact_native_segmented".into();
    let curve = SegmentedCurve::new(recipe.reference_date, native, value_type(recipe))
        .map_err(|e| Error::Numerical(e.to_string()))?;
    if recipe.representation == CurveRepresentation::DiscountFactors && curve.value_at(0.0) != 1.0 {
        return Err(Error::invalid(
            "origin",
            "segmented DF must retain exact P(0)=1",
        ));
    }
    Ok((StructuredValues::Segmented { curve, bases }, depth, pillars))
}
