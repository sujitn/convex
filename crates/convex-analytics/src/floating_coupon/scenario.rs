//! Exact supplied model-parameter scenarios, separate from curve/market quotes.
use super::*;

/// Native units and model families remain explicit; no HW1F parameter aliases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CouponModelParameter {
    /// Bachelier annual decimal-rate volatility per square-root year.
    NormalVolatility,
    /// Shifted Black annual relative volatility per square-root year.
    LognormalVolatility,
    /// Shifted Black decimal-rate displacement.
    LognormalDisplacement,
    /// One supplied terminal decimal-rate loading on a common unit normal factor.
    NormalFactorLoading {
        /// Exact original basis-column index; no covariance rotation is inferred.
        factor: usize,
    },
}

/// Complete actual supplied model-point identity; every field must match.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CouponParameterCoordinate {
    /// Original provider-neutral index.
    pub index_id: IndexId,
    /// Original observation date.
    pub observation_date: Date,
    /// Exact original payment measure/date.
    pub payment_date: Date,
    /// Actual original supplied model source.
    pub source_id: SourceId,
    /// Actual original positive source revision.
    pub source_version: Version,
    /// Native model parameter and unit.
    pub parameter: CouponModelParameter,
}

/// Full original and hypothetical native prices, never a market publication.
#[derive(Debug, Serialize)]
pub struct CouponModelScenario {
    /// Exact original consumed model-point coordinate.
    pub coordinate: CouponParameterCoordinate,
    /// Original native parameter value.
    pub original_parameter: f64,
    /// Finite explicitly supplied move in this parameter's native unit.
    pub delta: f64,
    /// Original value plus the supplied move, without clipping.
    pub scenario_parameter: f64,
    /// Complete original terms/history/model/curve valuation.
    pub base: TermCouponOptionPrice,
    /// Complete hypothetical model valuation with only the one value changed.
    pub scenario: TermCouponOptionPrice,
}

pub(super) fn changed_options(
    original: &TermCouponOptionInputs,
    coordinate: &CouponParameterCoordinate,
    delta: f64,
) -> Result<(TermCouponOptionInputs, f64, f64)> {
    if !delta.is_finite() {
        return Err(invalid(
            "model move",
            "finite original native-unit move required",
        ));
    }
    update_options(original, coordinate, |before| finite(before + delta))
}

pub(super) fn parameter_options(
    original: &TermCouponOptionInputs,
    coordinate: &CouponParameterCoordinate,
    value: f64,
) -> Result<(TermCouponOptionInputs, f64, f64)> {
    finite(value)?;
    update_options(original, coordinate, |_| Ok(value))
}

fn update_options(
    original: &TermCouponOptionInputs,
    coordinate: &CouponParameterCoordinate,
    update: impl FnOnce(f64) -> Result<f64>,
) -> Result<(TermCouponOptionInputs, f64, f64)> {
    let mut options = original.clone();
    let missing = || TermCouponOptionError::MissingModel {
        index_id: coordinate.index_id.clone(),
        observation_date: coordinate.observation_date,
        payment_date: coordinate.payment_date,
    };
    let value = if let CouponModelParameter::NormalFactorLoading { factor } = coordinate.parameter {
        let law = options
            .whole_coupon
            .as_mut()
            .ok_or_else(|| invalid("model parameter", "whole-coupon normal law required"))?;
        let point = law
            .factors
            .iter_mut()
            .find(|p| {
                p.index_id == coordinate.index_id
                    && p.observation_date == coordinate.observation_date
                    && p.payment_date == coordinate.payment_date
            })
            .ok_or_else(missing)?;
        if point.source_id != coordinate.source_id || point.version != coordinate.source_version {
            return Err(invalid("model revision", "original source/version differs"));
        }
        point
            .loadings
            .get_mut(factor)
            .ok_or_else(|| invalid("model factor", "original factor column absent"))?
    } else {
        if options.whole_coupon.is_some() {
            return Err(invalid(
                "model parameter",
                "segment normal/shifted-lognormal law required",
            ));
        }
        let point = options
            .models
            .iter_mut()
            .find(|p| {
                p.index_id == coordinate.index_id
                    && p.observation_date == coordinate.observation_date
                    && p.payment_date == coordinate.payment_date
            })
            .ok_or_else(missing)?;
        if point.source_id != coordinate.source_id || point.version != coordinate.source_version {
            return Err(invalid("model revision", "original source/version differs"));
        }
        match (&mut point.model, coordinate.parameter) {
            (ForwardCouponModel::Normal { volatility }, CouponModelParameter::NormalVolatility) => {
                volatility
            }
            (
                ForwardCouponModel::ShiftedLognormal { volatility, .. },
                CouponModelParameter::LognormalVolatility,
            ) => volatility,
            (
                ForwardCouponModel::ShiftedLognormal { displacement, .. },
                CouponModelParameter::LognormalDisplacement,
            ) => displacement,
            _ => {
                return Err(invalid(
                    "model parameter",
                    "parameter does not match the original model family",
                ))
            }
        }
    };
    let before = *value;
    *value = update(before)?;
    let after = *value;
    Ok((options, before, after))
}
