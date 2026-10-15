//! Canonical identity and signed linear scaling of genuine directional native proof.
use super::*;
use crate::floating_coupon::{
    CouponDirectionalDerivativeSettings, CouponParameterDirectionalDerivatives,
    PreparedTermCouponModel,
};
use convex_core::contracts::SnapshotId;
use convex_curves::contracts::CurveSet;
use serde::{Deserialize, Serialize};

/// Native reference and independent signed first/second currency precision.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CouponDirectionalRiskSettings {
    /// Original explicit side, native bump and native first/second admission.
    pub native: CouponDirectionalDerivativeSettings,
    /// Full/half first-derivative allowance in the actual signed position units.
    pub first: RiskTolerance,
    /// Full/half curvature allowance in signed currency per squared parameter.
    pub second: RiskTolerance,
    /// Exact positive version covering both numerical policies.
    pub policy_version: convex_core::contracts::Version,
}

/// Dedicated directional risk; every full original native valuation survives.
/// This result is not an existing Central QuoteSensitivity or hedge vector.
#[derive(Debug, Serialize)]
pub struct TermCouponDirectionalParameterSensitivity {
    /// Original complete canonical factor and native parameter units.
    pub key: QuoteRiskKey,
    /// Actual signed quantity, positive reference face and currency.
    pub position: RiskPosition,
    /// Actual caller information set; no market publication is generated.
    pub snapshot_id: SnapshotId,
    /// Original native and signed numerical policies, including selected side.
    pub settings: CouponDirectionalRiskSettings,
    /// Single native-to-position multiplier used for all reported quantities.
    pub position_scale: f64,
    /// Actual native PVs times the same signed position scale, in original order.
    pub scaled_prices: Vec<f64>,
    /// Signed currency derivative per increasing native parameter unit.
    pub first: f64,
    /// Same derivative from the accepted native half-step stencil.
    pub half_first: f64,
    /// Signed currency curvature per squared native parameter unit.
    pub second: f64,
    /// Same curvature from the accepted native half-step stencil.
    pub half_second: f64,
    /// Explicit second-order monetary unit; the key reports the first-order unit.
    pub second_unit: &'static str,
    /// Scaled native arithmetic guard plus multiplication/scale allowance.
    pub first_resolution: f64,
    /// Same arithmetic guard for the squared-parameter curvature unit.
    pub second_resolution: f64,
    /// All six complete prices, actual points/side and native precision proof.
    pub native: CouponParameterDirectionalDerivatives,
}

fn scaled(value: f64, scale: f64) -> QuoteRiskResult<f64> {
    let result = finite(value * scale, "directional position scaling")?;
    if value != 0.0 && scale != 0.0 && result == 0.0 {
        return Err(invalid(
            "directional scaling",
            "nonzero native value/guard underflow",
        ));
    }
    Ok(result)
}
fn admit(
    order: u8,
    full: f64,
    half: f64,
    native_resolution: f64,
    scale: f64,
    tolerance: RiskTolerance,
) -> QuoteRiskResult<f64> {
    let bound = tolerance.bound(full, half)?;
    if finite(full - half, "directional refinement")?.abs() > bound {
        return Err(QuoteRiskError::DirectionalStability {
            order,
            full,
            half,
            bound,
        });
    }
    let resolution = finite(
        scaled(native_resolution, scale.abs())? + 4.0 * f64::EPSILON * full.abs().max(half.abs()),
        "directional arithmetic resolution",
    )?;
    if resolution > bound {
        return Err(QuoteRiskError::DirectionalResolution {
            order,
            resolution,
            bound,
        });
    }
    Ok(resolution)
}

/// Price/validate the complete native product before applying signed scaling.
/// Zero holdings still require both original native refinement/resolution guards.
pub fn term_coupon_directional_parameter_sensitivity(
    prepared: &PreparedTermCouponModel<'_>,
    curves: &CurveSet,
    key: QuoteRiskKey,
    position: RiskPosition,
    snapshot_id: SnapshotId,
    settings: CouponDirectionalRiskSettings,
) -> QuoteRiskResult<TermCouponDirectionalParameterSensitivity> {
    validate_term_coupon_position(prepared, &position)?;
    settings.first.validate()?;
    settings.second.validate()?;
    settings.native.derivative.validate()?;
    let point = coupon_parameter_coordinate(&key)?;
    if key.definition().factor.definition().currency != prepared.currency()
        || key.definition().native_unit != coupon_parameter_units(point.parameter).1
        || key.definition().unit != QuoteRiskUnit::CurrencyPerModelParameter
        || key.definition().response != RiskResponse::FrozenCurves
    {
        return Err(invalid(
            "directional parameter",
            "exact original currency/native parameter units and frozen response required",
        ));
    }
    let native = prepared.parameter_directional_derivatives(curves, &point, settings.native)?;
    if native.prices.len() != 6 || native.price_evaluations != 6 {
        return Err(invalid(
            "directional native proof",
            "all six actual complete prices required",
        ));
    }
    let scale = position.scale()?;
    let first = scaled(native.first, scale)?;
    let half_first = scaled(native.half_first, scale)?;
    let second = scaled(native.second, scale)?;
    let half_second = scaled(native.half_second, scale)?;
    let first_resolution = admit(
        1,
        first,
        half_first,
        native.first_resolution,
        scale,
        settings.first,
    )?;
    let second_resolution = admit(
        2,
        second,
        half_second,
        native.second_resolution,
        scale,
        settings.second,
    )?;
    let scaled_prices = native
        .prices
        .iter()
        .map(|p| scaled(p.valuation.present_value, scale))
        .collect::<QuoteRiskResult<Vec<_>>>()?;
    Ok(TermCouponDirectionalParameterSensitivity {
        key,
        position,
        snapshot_id,
        settings,
        position_scale: scale,
        scaled_prices,
        first,
        half_first,
        second,
        half_second,
        second_unit: "currency_per_model_parameter_squared",
        first_resolution,
        second_resolution,
        native,
    })
}
