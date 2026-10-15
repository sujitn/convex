//! Exact native parameter admission before existing signed currency normalization.
use super::*;
use crate::floating_coupon::{
    CouponModelParameter, CouponParameterDerivativeSettings, CouponParameterDerivatives,
    PreparedTermCouponModel,
};
use convex_core::contracts::{QuoteType, RiskFactorKey, SnapshotId, Version};
use convex_curves::contracts::{CurveSet, QuoteUnit};
use serde::Serialize;

/// Distinct native units for the four actual supplied coupon parameter families.
pub const fn coupon_parameter_units(parameter: CouponModelParameter) -> (QuoteType, QuoteUnit) {
    match parameter {
        CouponModelParameter::NormalVolatility => (
            QuoteType::CouponNormalVolatility,
            QuoteUnit::CouponNormalVolatility,
        ),
        CouponModelParameter::LognormalVolatility => (
            QuoteType::CouponLognormalVolatility,
            QuoteUnit::CouponLognormalVolatility,
        ),
        CouponModelParameter::LognormalDisplacement => (
            QuoteType::CouponLognormalDisplacement,
            QuoteUnit::CouponLognormalDisplacement,
        ),
        CouponModelParameter::NormalFactorLoading { .. } => (
            QuoteType::CouponNormalFactorLoading,
            QuoteUnit::CouponNormalFactorLoading,
        ),
    }
}

pub(super) fn decode_coupon_parameter_coordinate(
    raw: &str,
    version: Version,
) -> QuoteRiskResult<crate::floating_coupon::CouponParameterCoordinate> {
    if raw.len() > 1024 {
        return Err(invalid(
            "coupon coordinate",
            "bounded complete point required",
        ));
    }
    let point: crate::floating_coupon::CouponParameterCoordinate =
        serde_json::from_str(raw).map_err(|e| invalid("coupon coordinate", e.to_string()))?;
    let canonical =
        serde_json::to_string(&point).map_err(|e| invalid("coupon coordinate", e.to_string()))?;
    if raw != canonical
        || version != point.source_version
        || point.observation_date > point.payment_date
    {
        return Err(invalid(
            "coupon coordinate",
            "canonical complete point and exact source revision required",
        ));
    }
    Ok(point)
}

/// Build the existing canonical key from a fully typed supplied model point.
/// Raw identity contains every field as canonical JSON, without hashing,
/// shortened labels, inferred tenors or loss of source/date/parameter information.
pub fn coupon_parameter_key(
    factor: RiskFactorKey,
    point: crate::floating_coupon::CouponParameterCoordinate,
) -> QuoteRiskResult<QuoteRiskKey> {
    let (_, native_unit) = coupon_parameter_units(point.parameter);
    let input_id =
        serde_json::to_string(&point).map_err(|e| invalid("coupon coordinate", e.to_string()))?;
    QuoteRiskKey::new(QuoteRiskDefinition {
        factor,
        coordinate: RiskCoordinate::Raw {
            input_id,
            version: point.source_version,
        },
        native_unit,
        response: RiskResponse::FrozenCurves,
        unit: QuoteRiskUnit::CurrencyPerModelParameter,
    })
}

/// Recover the exact typed point from an admitted canonical parameter key.
pub fn coupon_parameter_coordinate(
    key: &QuoteRiskKey,
) -> QuoteRiskResult<crate::floating_coupon::CouponParameterCoordinate> {
    if !key
        .definition()
        .factor
        .definition()
        .quote_type
        .is_coupon_parameter()
    {
        return Err(invalid(
            "coupon coordinate",
            "actual coupon parameter key required",
        ));
    }
    let RiskCoordinate::Raw { input_id, version } = &key.definition().coordinate else {
        return Err(invalid(
            "coupon coordinate",
            "complete original raw point required",
        ));
    };
    decode_coupon_parameter_coordinate(input_id, *version)
}

/// Actual full native derivatives and signed canonical position/snapshot binding.
#[derive(Debug, Serialize)]
pub struct TermCouponParameterSensitivity {
    /// Existing normalized signed risk with exactly the five evaluated prices.
    pub sensitivity: QuoteSensitivity,
    /// Original first/second refinement, arithmetic resolution and complete PV proof.
    pub native: CouponParameterDerivatives,
}

/// Apply the native guard before scaling the same five complete currency PVs.
/// Native policy is in reference currency; settings are in signed position units.
pub fn term_coupon_parameter_sensitivity(
    prepared: &PreparedTermCouponModel<'_>,
    curves: &CurveSet,
    key: QuoteRiskKey,
    position: RiskPosition,
    snapshot_id: SnapshotId,
    settings: QuoteRiskSettings,
    native_settings: CouponParameterDerivativeSettings,
) -> QuoteRiskResult<TermCouponParameterSensitivity> {
    validate_term_coupon_position(prepared, &position)?;
    settings.validate_for(&key)?;
    let point = coupon_parameter_coordinate(&key)?;
    if key.definition().factor.definition().currency != prepared.currency()
        || settings.bump != native_settings.bump
    {
        return Err(invalid(
            "coupon parameter",
            "original currency and identical native/canonical bump required",
        ));
    }
    let native = prepared.parameter_derivatives(curves, &point, native_settings)?;
    let moves = [
        0.0,
        settings.bump,
        -settings.bump,
        settings.bump / 2.0,
        -settings.bump / 2.0,
    ];
    let prices = [
        &native.base,
        &native.plus,
        &native.minus,
        &native.half_plus,
        &native.half_minus,
    ];
    let sensitivity =
        super::finite::reference(key, position.clone(), snapshot_id, settings, |delta| {
            let i = moves.iter().position(|d| *d == delta).ok_or_else(|| {
                invalid(
                    "coupon parameter move",
                    "unexpected private five-price coordinate",
                )
            })?;
            risk_reference_pv(prices[i].present_value, position.currency)
        })?;
    Ok(TermCouponParameterSensitivity {
        sensitivity,
        native,
    })
}
