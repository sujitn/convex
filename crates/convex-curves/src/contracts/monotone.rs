//! Complete supplied nonnegative MonotoneConvex model over the native owner.

use super::{
    build::normalize_nodes, CalibrationMode, CurveContractError as Error,
    CurveContractResult as Result, CurveNode, CurveRecipe, CurveRepresentation, CurveSource,
    ForwardSide,
};
use crate::{ExtrapolationMethod as Ext, InterpolationMethod as Int};
use convex_core::types::Compounding;
use convex_math::interpolation::{Interpolator, MonotoneConvex};
use serde::{Deserialize, Serialize};

/// Actual native model convention and forwards; these are interpolation
/// diagnostics, not product calibration residuals or a market certificate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MonotoneCurveDiagnostics {
    /// Exact equation, positivity and degenerate-axis convention.
    pub model: String,
    /// Original interval averages, including [0, first supplied pillar].
    pub discrete_forwards: Vec<f64>,
    /// Analytic origin followed by every original input pillar's forward.
    pub node_forwards: Vec<f64>,
}

#[derive(Debug, Clone)]
pub(super) struct MonotoneCurve {
    native: MonotoneConvex,
    extension: Ext,
    pub(super) diagnostics: MonotoneCurveDiagnostics,
}

pub(super) fn validate_shape(recipe: &CurveRecipe) -> Result<()> {
    if !matches!(recipe.source, CurveSource::MonotoneConvex { .. })
        || recipe.interpolation != Int::MonotoneConvex
    {
        return Err(Error::invalid(
            "interpolation",
            "complete MonotoneConvex source and matching method required",
        ));
    }
    if !recipe.role.is_rate()
        || recipe.representation
            != (CurveRepresentation::ZeroRates {
                compounding: Compounding::Continuous,
            })
    {
        return Err(Error::unsupported("monotone representation", "requires a rate role and original continuous-zero quotes; DF/hazard/survival/basis inputs cannot be reinterpreted"));
    }
    if matches!(&recipe.role, super::CurveRole::Discount { collateral_currency } if *collateral_currency != recipe.currency)
    {
        return Err(Error::invalid(
            "currency",
            "same-currency discount collateral required",
        ));
    }
    if recipe.calibration != CalibrationMode::Direct {
        return Err(Error::unsupported(
            "monotone calibration",
            "supplied complete nodes are not a product/helper fit",
        ));
    }
    if !matches!(
        recipe.extrapolation,
        Ext::None | Ext::Flat | Ext::FlatForward
    ) {
        return Err(Error::unsupported(
            "monotone extrapolation",
            "requires None, explicit raw Flat zero or exact integrated FlatForward tail",
        ));
    }
    Ok(())
}

impl MonotoneCurve {
    pub(super) fn new(recipe: &CurveRecipe, nodes: &[CurveNode]) -> Result<Self> {
        validate_shape(recipe)?;
        let (times, zeros) = normalize_nodes(recipe, nodes)?;
        let native = MonotoneConvex::new(times, zeros)
            .map_err(|e| Error::invalid("monotone nodes", e.to_string()))?;
        let diagnostics = MonotoneCurveDiagnostics {
            model: "hagan_west_2006_nonnegative_exact_axis_quadratic".into(),
            discrete_forwards: native.discrete_forwards().to_vec(),
            node_forwards: native.node_forward_rates().to_vec(),
        };
        let native = if recipe.extrapolation == Ext::FlatForward {
            native.with_extrapolation()
        } else {
            native
        };
        Ok(Self {
            native,
            extension: recipe.extrapolation,
            diagnostics,
        })
    }

    pub(super) fn grid(&self) -> (&[f64], &[f64]) {
        (self.native.times(), self.native.zero_rates())
    }
    pub(super) fn bounds(&self) -> (f64, f64) {
        (0.0, self.native.max_x())
    }
    pub(super) fn value(&self, t: f64) -> Result<f64> {
        if t > self.native.max_x() && self.extension == Ext::Flat {
            return Ok(self.native.zero_rates()[self.native.zero_rates().len() - 1]);
        }
        self.native
            .interpolate(t)
            .map_err(|e| Error::Numerical(e.to_string()))
    }
    pub(super) fn forward(&self, t: f64, side: ForwardSide) -> Result<(f64, bool)> {
        if t == 0.0 && side == ForwardSide::Left {
            return Err(Error::invalid(
                "forward.side",
                "no negative-time side at origin",
            ));
        }
        let extended =
            t > self.native.max_x() || (t == self.native.max_x() && side == ForwardSide::Right);
        if extended {
            match self.extension {
                Ext::None => {
                    return Err(Error::invalid(
                        "forward.side",
                        "no declared terminal extension",
                    ))
                }
                Ext::Flat => {
                    return Ok((
                        self.native.zero_rates()[self.native.zero_rates().len() - 1],
                        true,
                    ))
                }
                Ext::FlatForward => (),
                _ => {
                    return Err(Error::unsupported(
                        "forward tail",
                        "unavailable native model extension",
                    ))
                }
            }
        }
        Ok((
            self.native
                .forward_rate(t)
                .map_err(|e| Error::Numerical(e.to_string()))?,
            extended,
        ))
    }
}
