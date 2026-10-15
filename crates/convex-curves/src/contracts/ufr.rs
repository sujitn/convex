//! Exact immutable native curve joined to its analytic forward-convergence tail.

use super::{
    build::point_tenor, BuiltCurve, CalibrationMode, CurveContractError as Error,
    CurveContractResult as Result, CurveKey, CurvePoint, CurveRecipe, CurveRepresentation,
    CurveSet, CurveSource, ForwardSide, MarketQuote, QuoteUnit, MAX_CURVE_DERIVATIONS,
};
use crate::ExtrapolationMethod;
use convex_core::types::Compounding;
use convex_math::extrapolation::UfrConvergence;
use convex_math::optimization::{FitParameter, MAX_FIT_DIMENSION};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Native exponential-tail parameters, distinct from zero-grid values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct UfrParameters {
    /// Continuous decimal ultimate forward per original curve-time year.
    pub ufr: f64,
    /// Positive convergence speed per original curve-time year.
    pub alpha: f64,
}
impl UfrParameters {
    /// Original finite decimal-rate and positive-speed ingress checks.
    pub fn validate(&self) -> Result<()> {
        MarketQuote {
            value: self.ufr,
            unit: QuoteUnit::DecimalRate,
        }
        .validate()?;
        if !self.alpha.is_finite() || self.alpha <= 0.0 {
            return Err(Error::invalid(
                "ufr.alpha",
                "finite positive speed required",
            ));
        }
        Ok(())
    }
    /// Exact native order: continuous decimal UFR, speed per curve-time year.
    pub fn parameters(&self) -> [f64; 2] {
        [self.ufr, self.alpha]
    }
    /// Restore and validate the complete two-parameter native model.
    pub fn from_parameters(values: &[f64]) -> Result<Self> {
        let [ufr, alpha] = values else {
            return Err(Error::invalid(
                "ufr.parameters",
                "exactly two parameters required",
            ));
        };
        let value = Self {
            ufr: *ufr,
            alpha: *alpha,
        };
        value.validate()?;
        Ok(value)
    }
}

/// Full-product exponential-tail objective over a fixed exact base and LLP.
/// Numerical difference/rank policy belongs to the enclosing product graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProductUfrSettings {
    /// Exact immutable analytic base; it is never fitted or sampled here.
    pub base: CurveKey,
    /// Actual fixed date/model time, consuming the original left forward.
    pub last_liquid_point: CurvePoint,
    /// Complete initial tail, matching the recipe's explicit extrapolation.
    pub initial: UfrParameters,
    /// Closed native lower bounds in the original parameter order.
    pub lower: UfrParameters,
    /// Closed native upper bounds in the original parameter order.
    pub upper: UfrParameters,
    /// Positive native scales; solver coordinates are native/scale, UFR first.
    pub parameter_scales: [f64; 2],
    /// Positive native-unit scales for every original full product quote.
    pub quote_scales: Vec<f64>,
    /// Nonnegative weights; zero does not waive original quote repricing.
    pub weights: Vec<f64>,
    /// Explicit 1..256 bounded solver attempts, with original product work caps.
    pub max_iterations: usize,
    /// Positive weighted scaled RMS objective threshold, at most one.
    pub objective_tolerance: f64,
}
impl ProductUfrSettings {
    /// Check exact dimensions and domains before any product trial valuation.
    pub fn validate(&self, quotes: usize) -> Result<()> {
        self.base.validate()?;
        self.initial.validate()?;
        self.lower.validate()?;
        self.upper.validate()?;
        if !(2..=MAX_FIT_DIMENSION).contains(&quotes)
            || self.quote_scales.len() != quotes
            || self.weights.len() != quotes
            || self.weights.iter().any(|w| !w.is_finite() || *w < 0.0)
            || self.weights.iter().filter(|w| **w > 0.0).count() < 2
            || self
                .quote_scales
                .iter()
                .any(|s| !s.is_finite() || *s <= 0.0)
            || self
                .parameter_scales
                .iter()
                .any(|s| !s.is_finite() || *s <= 0.0)
            || !(1..=256).contains(&self.max_iterations)
            || !self.objective_tolerance.is_finite()
            || self.objective_tolerance <= 0.0
            || self.objective_tolerance > 1.0
        {
            return Err(Error::invalid("product UFR settings", "two identified bounded parameters,2..128 full quotes,exact native scales/weights and bounded numerical policy required"));
        }
        for i in 0..2 {
            let (start, lower, upper, scale) = (
                self.initial.parameters()[i],
                self.lower.parameters()[i],
                self.upper.parameters()[i],
                self.parameter_scales[i],
            );
            if lower >= upper
                || start < lower
                || start > upper
                || !(upper - lower).is_finite()
                || !(lower / scale).is_finite()
                || !(upper / scale).is_finite()
                || !(upper / scale - lower / scale).is_finite()
                || lower / scale >= upper / scale
            {
                return Err(Error::invalid(
                    "ufr.bounds",
                    "finite ordered native bounds containing the start required",
                ));
            }
        }
        Ok(())
    }
    /// Explicit scaled coordinates for the original shared bounded solver.
    pub fn fit_parameters(&self, quotes: usize) -> Result<Vec<FitParameter>> {
        self.validate(quotes)?;
        Ok((0..2)
            .map(|i| FitParameter {
                initial: self.initial.parameters()[i] / self.parameter_scales[i],
                lower: self.lower.parameters()[i] / self.parameter_scales[i],
                upper: self.upper.parameters()[i] / self.parameter_scales[i],
            })
            .collect())
    }
    /// Restore native units and preserve exact closed bounds after scaling.
    pub fn native_parameters(&self, values: &[f64]) -> Result<Vec<f64>> {
        self.validate(self.quote_scales.len())?;
        if values.len() != 2 {
            return Err(Error::invalid(
                "ufr.parameters",
                "exactly two scaled values required",
            ));
        }
        let mut native = Vec::with_capacity(2);
        for (i, value) in values.iter().enumerate() {
            let (lower, upper, scale) = (
                self.lower.parameters()[i],
                self.upper.parameters()[i],
                self.parameter_scales[i],
            );
            let n = value * scale;
            if !n.is_finite() || *value < lower / scale || *value > upper / scale {
                return Err(Error::invalid(
                    "ufr.parameters",
                    "finite scaled values inside original closed bounds required",
                ));
            }
            native.push(n.clamp(lower, upper));
        }
        Ok(native)
    }
    /// Admit the exact product recipe and initial tail before trial pricing.
    pub fn validate_recipe(&self, recipe: &CurveRecipe) -> Result<()> {
        let CurveSource::Helpers(ids) = &recipe.source else {
            return Err(Error::invalid(
                "ufr.source",
                "complete original product helper recipe required",
            ));
        };
        self.validate(ids.len())?;
        if !recipe.role.is_rate()
            || self.base.id == recipe.key.id
            || recipe
                .dependencies
                .iter()
                .filter(|key| **key == self.base)
                .count()
                != 1
            || recipe.extrapolation
                != (ExtrapolationMethod::UfrConvergence {
                    ufr: self.initial.ufr,
                    alpha: self.initial.alpha,
                })
        {
            return Err(Error::invalid("ufr.recipe", "rate role,exact external base dependency and complete matching initial UFR policy required"));
        }
        let llp = point_tenor(
            recipe.reference_date,
            recipe.time_basis,
            self.last_liquid_point,
        )?;
        UfrConvergence::try_new(self.initial.ufr, self.initial.alpha, llp)
            .map_err(|e| Error::invalid("ufr.parameters", e.to_string()))?;
        Ok(())
    }
    /// Construct an exact trial recipe; additional product dependencies stay in
    /// the surrounding supplied graph, and the UFR owner consumes only its base.
    pub fn trial_recipe(&self, recipe: &CurveRecipe, parameters: &[f64]) -> Result<CurveRecipe> {
        self.validate_recipe(recipe)?;
        let parameters = UfrParameters::from_parameters(parameters)?;
        for i in 0..2 {
            if parameters.parameters()[i] < self.lower.parameters()[i]
                || parameters.parameters()[i] > self.upper.parameters()[i]
            {
                return Err(Error::invalid(
                    "ufr.parameters",
                    "original closed native bounds required",
                ));
            }
        }
        let mut direct = recipe.clone();
        direct.calibration = CalibrationMode::Direct;
        direct.dependencies = vec![self.base.clone()];
        direct.source = CurveSource::UfrConvergence {
            base: self.base.clone(),
            last_liquid_point: self.last_liquid_point,
        };
        direct.extrapolation = ExtrapolationMethod::UfrConvergence {
            ufr: parameters.ufr,
            alpha: parameters.alpha,
        };
        Ok(direct)
    }
}

/// Actual analytic join, retaining the source version and declared LLP. These
/// are construction diagnostics, not instrument-fit residuals or certification.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct UfrCurveDiagnostics {
    /// Exact immutable source model.
    pub base: CurveKey,
    /// Original supplied date/model-time boundary.
    pub last_liquid_point: CurvePoint,
    /// Actual boundary in years of the recipe's original time basis.
    pub boundary_tenor: f64,
    /// Original source continuous annualized decimal zero at the LLP.
    pub boundary_zero: f64,
    /// Original analytic left forward; on a knot this is the ending interval.
    pub boundary_forward: f64,
    /// Supplied annualized continuous decimal long-run forward.
    pub ultimate_forward_rate: f64,
    /// Supplied positive convergence speed per curve-time year.
    pub convergence_speed: f64,
}

#[derive(Debug, Clone)]
pub(super) struct UfrCurve {
    base: Arc<BuiltCurve>,
    kernel: UfrConvergence,
    pub(super) diagnostics: UfrCurveDiagnostics,
}

pub(super) fn validate_shape(recipe: &CurveRecipe) -> Result<()> {
    if recipe.calibration != CalibrationMode::Direct {
        return Err(Error::unsupported("UFR calibration", "explicit supplied tail parameters only; no product-fit or alpha calibration substitute"));
    }
    if !recipe.role.is_rate()
        || recipe.representation
            != (CurveRepresentation::ZeroRates {
                compounding: Compounding::Continuous,
            })
    {
        return Err(Error::unsupported("UFR representation", "continuous-zero rate roles only; no DF, survival, hazard or issuer-basis reinterpretation"));
    }
    let ExtrapolationMethod::UfrConvergence { ufr, alpha } = recipe.extrapolation else {
        return Err(Error::invalid(
            "extrapolation",
            "UfrConvergence source requires its complete original UFR/alpha policy",
        ));
    };
    MarketQuote {
        value: ufr,
        unit: QuoteUnit::DecimalRate,
    }
    .validate()?;
    let CurveSource::UfrConvergence {
        last_liquid_point, ..
    } = recipe.source
    else {
        return Err(Error::invalid(
            "source",
            "complete UfrConvergence source required",
        ));
    };
    let llp = point_tenor(recipe.reference_date, recipe.time_basis, last_liquid_point)?;
    UfrConvergence::try_new(ufr, alpha, llp)
        .map_err(|e| Error::invalid("ufr.parameters", e.to_string()))?;
    Ok(())
}

impl UfrCurve {
    pub(super) fn original_base(&self) -> &Arc<BuiltCurve> {
        &self.base
    }
    pub(super) fn new(recipe: &CurveRecipe, supplied: &CurveSet) -> Result<(Self, usize)> {
        validate_shape(recipe)?;
        let CurveSource::UfrConvergence {
            base,
            last_liquid_point,
        } = &recipe.source
        else {
            return Err(Error::invalid("source", "missing UFR source"));
        };
        let ExtrapolationMethod::UfrConvergence { ufr, alpha } = recipe.extrapolation else {
            return Err(Error::invalid("extrapolation", "missing UFR policy"));
        };
        let base = Arc::clone(supplied.get(base)?);
        let original = base.recipe();
        if recipe.role != original.role
            || recipe.currency != original.currency
            || recipe.reference_date != original.reference_date
            || recipe.time_basis != original.time_basis
            || recipe.representation != original.representation
            || recipe.interpolation != original.interpolation
        {
            return Err(Error::invalid("ufr.base", "exact role/currency/reference/time basis/continuous representation/interpolation must match the supplied source"));
        }
        let llp = point_tenor(recipe.reference_date, recipe.time_basis, *last_liquid_point)?;
        let (min, max) = base.bounds();
        if llp <= min || llp > max {
            return Err(Error::invalid("last_liquid_point", "must follow the native minimum and be inside the source domain; no source extension at the join"));
        }
        let zero = base.raw(llp)?;
        let (forward, extended) = base.ufr_base_forward(llp, ForwardSide::Left)?;
        if extended {
            return Err(Error::invalid(
                "ufr.boundary",
                "source extension cannot provide the LLP forward",
            ));
        }
        if !zero.is_finite() || !forward.is_finite() {
            return Err(Error::Numerical("nonfinite original UFR join".into()));
        }
        let depth = base.depth() + 1;
        if depth > MAX_CURVE_DERIVATIONS {
            return Err(Error::BoundExceeded("derived curve depth"));
        }
        let kernel = UfrConvergence::try_new(ufr, alpha, llp)
            .map_err(|e| Error::invalid("ufr.parameters", e.to_string()))?;
        Ok((
            Self {
                diagnostics: UfrCurveDiagnostics {
                    base: original.key.clone(),
                    last_liquid_point: *last_liquid_point,
                    boundary_tenor: llp,
                    boundary_zero: zero,
                    boundary_forward: forward,
                    ultimate_forward_rate: ufr,
                    convergence_speed: alpha,
                },
                base,
                kernel,
            },
            depth,
        ))
    }

    pub(super) fn bounds(&self) -> (f64, f64) {
        (self.base.bounds().0, self.kernel.llp())
    }

    pub(super) fn value(&self, t: f64) -> Result<f64> {
        if t < self.bounds().0 {
            return Err(Error::invalid(
                "query.domain",
                "UFR supplies no short-end extension",
            ));
        }
        if t <= self.kernel.llp() {
            return self.base.raw(t);
        }
        self.kernel
            .try_zero_rate(
                t,
                self.diagnostics.boundary_zero,
                self.diagnostics.boundary_forward,
            )
            .map_err(|e| Error::Numerical(e.to_string()))
    }

    pub(super) fn forward(&self, t: f64) -> Result<f64> {
        self.kernel
            .try_forward_rate(t, self.diagnostics.boundary_forward)
            .map_err(|e| Error::invalid("ufr.forward.domain", e.to_string()))
    }

    pub(super) fn joined_forward(&self, t: f64, side: ForwardSide) -> Result<(f64, bool)> {
        if t < self.bounds().0 || (t == self.bounds().0 && side == ForwardSide::Left) {
            return Err(Error::invalid(
                "ufr.forward.domain",
                "no source extension or left-origin forward is available",
            ));
        }
        let llp = self.kernel.llp();
        if t < llp || (t == llp && side == ForwardSide::Left) {
            let value = self.base.ufr_base_forward(t, side)?;
            if value.1 {
                return Err(Error::invalid(
                    "ufr.forward.domain",
                    "source extension cannot supply a joined forward",
                ));
            }
            Ok(value)
        } else {
            Ok((self.forward(t)?, true))
        }
    }

    pub(super) fn extension_used(&self, t: f64) -> bool {
        t > self.kernel.llp()
    }
}
