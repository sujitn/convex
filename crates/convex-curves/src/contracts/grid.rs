//! Explicit step-zero and integrated flat-forward sources over original owners.

use super::{
    build::normalize_nodes, CalibrationMode, CurveContractError as Error,
    CurveContractResult as Result, CurveNode, CurveRecipe, CurveRepresentation, CurveSource,
};
use crate::{
    DiscreteCurve, ExtrapolationMethod as Ext, InterpolationMethod as Int, TermStructure, ValueType,
};
use convex_core::types::Compounding;
use convex_math::interpolation::{FlatForward, Interpolator};
use serde::{Deserialize, Serialize};

/// The requested one-sided instantaneous forward at an exact supplied knot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ForwardSide {
    /// Interval ending at the point; no negative-time side exists at the origin.
    Left,
    /// Interval starting at the point; a final right side needs exact extension.
    Right,
}

#[derive(Debug, Clone)]
pub(super) struct MethodGrid {
    curve: DiscreteCurve,
    forward: Option<FlatForward>,
    extension: Ext,
}

pub(super) fn validate_shape(recipe: &CurveRecipe) -> Result<()> {
    let expected = match recipe.source {
        CurveSource::PiecewiseConstant { .. } => Int::PiecewiseConstant,
        CurveSource::FlatForward { .. } => Int::FlatForward,
        _ => return Err(Error::invalid("source", "explicit grid source required")),
    };
    if recipe.interpolation != expected {
        return Err(Error::invalid(
            "interpolation",
            "must match the complete source",
        ));
    }
    if !recipe.role.is_rate()
        || recipe.representation
            != (CurveRepresentation::ZeroRates {
                compounding: Compounding::Continuous,
            })
    {
        return Err(Error::unsupported("method grid representation", "requires a rate role and continuous-zero quotes; no DF, hazard, survival or issuer-basis reinterpretation"));
    }
    if matches!(&recipe.role, super::CurveRole::Discount { collateral_currency } if *collateral_currency != recipe.currency)
    {
        return Err(Error::invalid(
            "currency",
            "discount collateral currency must match the supplied rate currency",
        ));
    }
    if recipe.calibration != CalibrationMode::Direct {
        return Err(Error::unsupported(
            "method grid calibration",
            "supplied nodes are not product/helper calibration",
        ));
    }
    if !(matches!(recipe.extrapolation, Ext::None | Ext::Flat | Ext::Linear)
        || (expected == Int::FlatForward && recipe.extrapolation == Ext::FlatForward))
    {
        return Err(Error::unsupported(
            "method grid extrapolation",
            "None, raw Flat/Linear and original FlatForward extension on its own model only",
        ));
    }
    Ok(())
}

impl MethodGrid {
    pub(super) fn new(recipe: &CurveRecipe, nodes: &[CurveNode]) -> Result<Self> {
        validate_shape(recipe)?;
        let (tenors, zeros) = normalize_nodes(recipe, nodes)?;
        let is_forward = recipe.interpolation == Int::FlatForward;
        if is_forward && tenors[0] == 0.0 && zeros[0] != zeros[1] {
            return Err(Error::invalid(
                "nodes.origin",
                "flat-forward origin must equal its actual first positive interval zero",
            ));
        }
        let forward = if is_forward {
            let native = if tenors[0] == 0.0 {
                FlatForward::with_origin(tenors.clone(), zeros.clone())
            } else {
                FlatForward::new(tenors.clone(), zeros.clone())
            };
            Some(
                native
                    .map_err(|e| Error::Numerical(e.to_string()))?
                    .with_extrapolation(),
            )
        } else {
            None
        };
        let extension = recipe.extrapolation;
        let raw_extension = if extension == Ext::FlatForward {
            Ext::None
        } else {
            extension
        };
        let curve = DiscreteCurve::with_extrapolation(
            recipe.reference_date,
            tenors,
            zeros,
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: recipe.time_basis,
            },
            recipe.interpolation,
            raw_extension,
        )
        .map_err(|e| Error::Numerical(e.to_string()))?;
        Ok(Self {
            curve,
            forward,
            extension,
        })
    }

    pub(super) fn grid(&self) -> (&[f64], &[f64]) {
        (self.curve.tenors(), self.curve.values())
    }

    pub(super) fn extension_used(&self, t: f64) -> bool {
        let (min, max) = self.bounds();
        t < min || t > max
    }

    pub(super) fn bounds(&self) -> (f64, f64) {
        self.curve.tenor_bounds()
    }

    pub(super) fn value(&self, t: f64) -> Result<f64> {
        let (min, max) = self.bounds();
        if (t < min || t > max) && self.extension == Ext::FlatForward {
            return self
                .forward
                .as_ref()
                .ok_or_else(|| Error::invalid("source", "missing native forward model"))?
                .interpolate(t)
                .map_err(|e| Error::Numerical(e.to_string()));
        }
        self.curve
            .checked_value_at(t)
            .map_err(|e| Error::Numerical(e.to_string()))
    }

    pub(super) fn forward_extension_used(&self, t: f64, side: ForwardSide) -> bool {
        let (min, max) = self.bounds();
        t < min
            || t > max
            || (t == min && side == ForwardSide::Left)
            || (t == max && side == ForwardSide::Right)
    }

    pub(super) fn forward(&self, t: f64, side: ForwardSide) -> Result<f64> {
        let native = self.forward.as_ref().ok_or_else(|| {
            Error::unsupported(
                "one-sided flat forward",
                "step zeros have discount jumps; no derivative/forward proxy",
            )
        })?;
        let (min, max) = self.bounds();
        if t == 0.0 && side == ForwardSide::Left {
            return Err(Error::invalid(
                "forward.side",
                "no negative-time interval at the origin",
            ));
        }
        if t < min || (t == min && side == ForwardSide::Left) {
            if self.extension == Ext::FlatForward {
                return Ok(native.zero_rates()[0]);
            }
            return Err(Error::invalid(
                "forward.side",
                "requested side precedes the supplied native grid",
            ));
        }
        if t > max || (t == max && side == ForwardSide::Right) {
            if self.extension == Ext::FlatForward {
                return native
                    .forward_rates_vec()
                    .last()
                    .copied()
                    .ok_or_else(|| Error::invalid("source", "missing original tail forward"));
            }
            return Err(Error::unsupported("forward tail", "requires the original FlatForward extension; raw Flat/Linear are separate policies"));
        }
        if side == ForwardSide::Left {
            if let Ok(i) = native.tenors().binary_search_by(|p| p.total_cmp(&t)) {
                return native
                    .forward_rates_vec()
                    .get(i.saturating_sub(1))
                    .copied()
                    .ok_or_else(|| Error::invalid("forward.side", "missing actual left interval"));
            }
        }
        native
            .forward_rate(t)
            .map_err(|e| Error::Numerical(e.to_string()))
    }
}
