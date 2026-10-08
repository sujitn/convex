//! Exact immutable native curve joined to its analytic forward-convergence tail.

use super::{
    build::point_tenor, BuiltCurve, CalibrationMode, CurveContractError as Error,
    CurveContractResult as Result, CurveKey, CurvePoint, CurveRecipe, CurveRepresentation,
    CurveSet, CurveSource, ForwardSide, MarketQuote, QuoteUnit, MAX_CURVE_DERIVATIONS,
};
use crate::ExtrapolationMethod;
use convex_core::types::Compounding;
use convex_math::extrapolation::UfrConvergence;
use serde::Serialize;
use std::sync::Arc;

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
        if !matches!(
            original.source,
            CurveSource::FlatForward { .. } | CurveSource::Parametric { .. }
        ) {
            return Err(Error::unsupported("UFR boundary model", "requires the original analytic FlatForward or NelsonSiegel/Svensson source; no sampled derivative, stepped, derived or generic Nodes proxy"));
        }
        let llp = point_tenor(recipe.reference_date, recipe.time_basis, *last_liquid_point)?;
        let (min, max) = base.bounds();
        if llp <= min || llp > max {
            return Err(Error::invalid("last_liquid_point", "must follow the native minimum and be inside the source domain; no source extension at the join"));
        }
        let zero = base.raw(llp)?;
        let forward = if matches!(original.source, CurveSource::FlatForward { .. }) {
            let (forward, extended) = base.flat_forward(llp, ForwardSide::Left)?;
            if extended {
                return Err(Error::invalid(
                    "ufr.boundary",
                    "source extension cannot provide the LLP forward",
                ));
            }
            forward
        } else {
            base.instantaneous_forward(llp)?
        };
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

    pub(super) fn extension_used(&self, t: f64) -> bool {
        t > self.kernel.llp()
    }
}
