//! Original optional risk certificates, bound to exact graph/model coordinates.
use super::*;
use crate::optional_bond as o;
use convex_core::contracts::{FactorTarget, QuoteType, SnapshotId};
use convex_curves::contracts::{CurveRepresentation, CurveRole};
use serde::{Deserialize, Serialize};

/// Explicit separate rate and model-parameter refinement policies.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OptionalRiskPolicies {
    /// Original independently refined duration/convexity and state-resolution guard.
    pub rates: o::RiskSettings,
    /// Parameter first/second derivative refinements, in their own units.
    pub model: o::ModelRiskSettings,
}
/// Full native certificate; a model derivative is not duration or PV01.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum OptionalRiskCertificate {
    /// Original native rate/OAS sensitivity owner and all actual grids.
    Rate(Box<o::OptionalBondRisk>),
    /// Native original-tree parameter scenarios and derivative refinements.
    Model(Box<o::OptionalModelRisk>),
}
impl OptionalRiskCertificate {
    fn prices(&self) -> [&o::OptionalBondPrice; 3] {
        match self {
            Self::Rate(v) => [&v.base, &v.up, &v.down],
            Self::Model(v) => [&v.base, &v.up, &v.down],
        }
    }
}
/// Canonical five-PV result plus independently checked native h/h2 certificates.
#[derive(Debug, Clone, Serialize)]
pub struct OptionalFactorSensitivity {
    /// Same canonical monetary result used by original aggregation and hedging.
    pub sensitivity: QuoteSensitivity,
    /// Complete original native h risk and price diagnostics.
    pub native: OptionalRiskCertificate,
    /// Independently required original native h/2 diagnostics.
    pub half: OptionalRiskCertificate,
}
/// Exact pair from the canonical target; both identities/revisions affect netting.
pub fn joint_curve_keys(
    key: &QuoteRiskKey,
) -> QuoteRiskResult<(
    convex_curves::contracts::CurveKey,
    convex_curves::contracts::CurveKey,
)> {
    let FactorTarget::CurvePair {
        discount_curve_id,
        discount_curve_version,
        projection_curve_id,
        projection_curve_version,
    } = &key.definition().factor.definition().target
    else {
        return Err(invalid(
            "joint target",
            "two exact original curve identities/revisions required",
        ));
    };
    Ok((
        convex_curves::contracts::CurveKey {
            id: discount_curve_id.as_str().into(),
            version: discount_curve_version.clone(),
        },
        convex_curves::contracts::CurveKey {
            id: projection_curve_id.as_str().into(),
            version: projection_curve_version.clone(),
        },
    ))
}
/// Exact baseline identity and native parallel-shift semantics. No ACT/360,
/// shaped node, quote-rebuild or implicit model-recalibration substitution.
pub fn admit_optional_graph(
    prepared: &o::PreparedOptionalBond,
    graph: &PreparedQuoteRiskGraph<'_>,
) -> QuoteRiskResult<()> {
    admit_optional_curve_set(prepared, &graph.baseline().curves)
}
/// Same original optional graph representation/domain admission for a supplied
/// immutable scenario baseline; no derivative or model recalibration is inferred.
pub fn admit_optional_curve_set(
    prepared: &o::PreparedOptionalBond,
    curves: &convex_curves::contracts::CurveSet,
) -> QuoteRiskResult<()> {
    let (discount, projection) = prepared.curve_recipes();
    for recipe in std::iter::once(discount).chain(projection) {
        let actual = curves.get(&recipe.key)?;
        if actual.recipe() != recipe {
            return Err(invalid(
                "optional graph baseline",
                "complete original curve recipe differs",
            ));
        }
        let supported = recipe.time_basis
            == convex_core::daycounts::DayCountConvention::Act365Fixed
            && match recipe.representation {
                CurveRepresentation::ZeroRates {
                    compounding: convex_core::types::Compounding::Continuous,
                } => recipe.interpolation == convex_curves::InterpolationMethod::Linear,
                CurveRepresentation::DiscountFactors => {
                    recipe.interpolation == convex_curves::InterpolationMethod::LogLinear
                }
                _ => false,
            };
        if !supported {
            return Err(invalid("optional graph shift", "native parallel ACT/365F risk requires original linear continuous-zero or log-linear DF inputs"));
        }
        if !matches!(
            recipe.source,
            convex_curves::contracts::CurveSource::Nodes(_)
                | convex_curves::contracts::CurveSource::MoneyMarket(_)
                | convex_curves::contracts::CurveSource::Helpers(_)
        ) {
            return Err(invalid("optional graph source", "original native interpolation grid required; derived/parametric sources need their own parallel-shift integration"));
        }
        // A constant zero move commutes with log-linear DF interpolation only
        // inside the original grid. Raw Flat/Linear DF tails are different
        // financial policies and cannot be replaced by native exp(-delta*t).
        if recipe.representation == CurveRepresentation::DiscountFactors {
            let (times, _) = actual.interpolation_grid()?;
            let start = recipe.reference_date.days_between(&prepared.settlement()) as f64 / 365.;
            let end = start + prepared.curve_query_horizon()?;
            if times.first().is_none_or(|t| start < *t) || times.last().is_none_or(|t| end > *t) {
                return Err(invalid("optional graph DF range", "all original full model queries must remain inside the supplied log-linear grid"));
            }
        }
    }
    Ok(())
}
/// Full original native price/risk composition. Only actual consumed parallel
/// discount/projection grids, their explicit joint move, raw OAS and named HW1F
/// parameters are admitted. Both h and h/2 pass their own native guards FIRST.
#[allow(clippy::too_many_arguments)] // One explicit full immutable calculation boundary.
pub fn optional_factor_sensitivity(
    prepared: &o::PreparedOptionalBond,
    key: QuoteRiskKey,
    position: RiskPosition,
    snapshot_id: SnapshotId,
    settings: QuoteRiskSettings,
    oas_decimal: f64,
    policies: OptionalRiskPolicies,
) -> QuoteRiskResult<OptionalFactorSensitivity> {
    let d = key.definition();
    let f = d.factor.definition();
    if position.currency != prepared.currency()
        || f.currency != prepared.currency()
        || d.response != RiskResponse::FrozenCurves
        || d.native_unit != convex_curves::contracts::QuoteUnit::DecimalRate
    {
        return Err(invalid(
            "optional factor",
            "exact native currency, frozen response and decimal units required",
        ));
    }
    position.scale()?;
    let reference = position.reference_quantity.decimal();
    if !matches!(
        position.quantity_unit,
        RiskQuantityUnit::Notional | RiskQuantityUnit::TradeCopies
    ) || (position.quantity_unit == RiskQuantityUnit::Notional
        && reference != prepared.face_value())
        || (position.quantity_unit == RiskQuantityUnit::TradeCopies
            && reference != rust_decimal::Decimal::ONE)
    {
        return Err(invalid(
            "optional position",
            "original face or exact full trade copies required",
        ));
    }
    settings.validate_for(&key)?;
    policies.rates.validate()?;
    policies.model.validate()?;
    let (discount, projection) = prepared.curve_recipes();
    let rate_axis = match &d.coordinate {
        RiskCoordinate::Zero { curve, node: None } if *curve == discount.key => Some(o::RiskAxis::Discount),
        RiskCoordinate::Zero { curve, node: None } if projection.is_some_and(|p| p.key == *curve) => Some(o::RiskAxis::Projection),
        RiskCoordinate::Raw { .. } if f.quote_type == QuoteType::JointZeroRate
            && joint_curve_keys(&key)?.0 == discount.key
            && projection.is_some_and(|p| joint_curve_keys(&key).is_ok_and(|(_, pk)| p.key==pk))
            => Some(o::RiskAxis::DiscountAndProjection),
        RiskCoordinate::Raw { .. } if f.quote_type == QuoteType::OasSpread => Some(o::RiskAxis::Oas),
        RiskCoordinate::Raw { .. } if matches!(&f.target,
            FactorTarget::Model { model_id, model_version } if model_id.as_str()==prepared.model().key().id
                && *model_version==prepared.model().key().version) => None,
        _ => return Err(invalid("optional coordinate", "complete actual consumed parallel grids, joint roles, OAS or exact model revision required")),
    };
    if let Some(p) = projection {
        if !matches!(p.role, CurveRole::Projection { .. }) || p.currency != discount.currency {
            return Err(invalid(
                "optional projection",
                "original independent same-currency index role required",
            ));
        }
    }
    let calculate = |bump| -> QuoteRiskResult<OptionalRiskCertificate> {
        Ok(if let Some(axis) = rate_axis {
            OptionalRiskCertificate::Rate(Box::new(prepared.risk(
                oas_decimal,
                axis,
                bump,
                policies.rates,
            )?))
        } else {
            let parameter = match f.quote_type {
                QuoteType::Hw1fMeanReversion => o::ModelParameter::MeanReversion,
                QuoteType::Hw1fVolatility => o::ModelParameter::Volatility,
                _ => {
                    return Err(invalid(
                        "model coordinate",
                        "named original HW1F parameter required",
                    ))
                }
            };
            OptionalRiskCertificate::Model(Box::new(prepared.model_risk(
                oas_decimal,
                parameter,
                bump,
                policies.model,
            )?))
        })
    };
    let native = calculate(settings.bump)?;
    let half = calculate(settings.bump / 2.0)?;
    let n = native.prices();
    let h = half.prices();
    if n[0].present_value != h[0].present_value {
        return Err(invalid("optional baseline", "native h/h2 baseline differs"));
    }
    let moves = [
        0.0,
        settings.bump,
        -settings.bump,
        settings.bump / 2.0,
        -settings.bump / 2.0,
    ];
    let prices = [n[0], n[1], n[2], h[1], h[2]];
    let sensitivity =
        super::finite::reference(key, position.clone(), snapshot_id, settings, |delta| {
            let index = moves.iter().position(|d| *d == delta).ok_or_else(|| {
                invalid("optional move", "unexpected private five-price coordinate")
            })?;
            risk_reference_pv(prices[index].present_value, position.currency)
        })?;
    Ok(OptionalFactorSensitivity {
        sensitivity,
        native,
        half,
    })
}
