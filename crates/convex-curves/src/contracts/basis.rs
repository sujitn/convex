//! Signed residual spread grids, deliberately unable to supply discount factors.

use super::*;
use convex_math::interpolation::{Interpolator, LinearInterpolator};

#[derive(Debug, Clone)]
pub(super) struct BasisGrid {
    pub tenors: Vec<f64>,
    pub values: Vec<f64>,
    linear: LinearInterpolator,
}
impl BasisGrid {
    pub fn new(tenors: Vec<f64>, values: Vec<f64>) -> CurveContractResult<Self> {
        let linear = LinearInterpolator::new(tenors.clone(), values.clone())
            .map_err(|e| CurveContractError::Numerical(e.to_string()))?;
        Ok(Self {
            tenors,
            values,
            linear,
        })
    }
    pub fn value(&self, t: f64, extension: crate::ExtrapolationMethod) -> CurveContractResult<f64> {
        let last = self.tenors.len() - 1; // construction requires >=2 checked nodes
        let v = if t < self.tenors[0] || t > self.tenors[last] {
            match extension {
                crate::ExtrapolationMethod::Flat => {
                    if t < self.tenors[0] {
                        self.values[0]
                    } else {
                        self.values[last]
                    }
                }
                crate::ExtrapolationMethod::Linear => {
                    let i = if t < self.tenors[0] { 0 } else { last - 1 };
                    self.values[i]
                        + (t - self.tenors[i]) * (self.values[i + 1] - self.values[i])
                            / (self.tenors[i + 1] - self.tenors[i])
                }
                _ => {
                    return Err(CurveContractError::invalid(
                        "basis.domain",
                        "extrapolation disabled",
                    ))
                }
            }
        } else {
            self.linear
                .interpolate(t)
                .map_err(|e| CurveContractError::Numerical(e.to_string()))?
        };
        if !v.is_finite() || v.abs() > 10.0 {
            return Err(CurveContractError::invalid(
                "basis.value",
                "finite annualized residual spread within [-10,10] required",
            ));
        }
        Ok(v)
    }
}

impl BuiltCurve {
    /// Signed continuous-zero residual basis in decimal annualized units.
    /// This query never supplies a discount factor, hazard or CDS spread. The
    /// analytics caller must supply the exact separately declared pricing model.
    pub fn issuer_spread(&self, point: CurvePoint) -> CurveContractResult<f64> {
        if !matches!(self.recipe().role, CurveRole::IssuerSpread { .. })
            || self.recipe().representation != CurveRepresentation::IssuerSpreads
        {
            return Err(CurveContractError::invalid(
                "basis.role",
                "IssuerSpread/IssuerSpreads required",
            ));
        }
        Ok(self.query(&CurveQuery::Raw { point })?.value)
    }
}
