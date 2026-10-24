//! Complete supplied coupon-model graph revaluation, with actual five prices.
use super::*;
use crate::floating_coupon::{PreparedTermCouponModel, TermCouponOptionPrice};
use convex_core::contracts::SnapshotId;
use serde::Serialize;

/// Original native price/known-fixing/model proof at each declared risk move.
#[derive(Debug, Serialize)]
pub struct TermCouponGraphValuation {
    /// Original canonical graph move, never a quoted per100 price bump.
    pub delta: f64,
    /// Complete original coupon model valuation against actual scenario curves.
    pub valuation: TermCouponOptionPrice,
    /// Actual full dependency-first graph build order.
    pub build_order: Vec<convex_curves::contracts::CurveKey>,
    /// Every original helper repriced in its actual native quote unit.
    pub repricing: Vec<convex_curves::contracts::QuoteRepricing>,
    /// Actual calibration helper evaluations in the scenario.
    pub price_evaluations: usize,
}

/// Checked common-factor sensitivity and complete same-model native prices.
#[derive(Debug, Serialize)]
pub struct TermCouponGraphSensitivity {
    /// Original signed currency normalization and mandatory h/h2 stability.
    pub sensitivity: QuoteSensitivity,
    /// Exactly five complete native revaluations in reference owner order.
    pub valuations: Vec<TermCouponGraphValuation>,
}

/// Bind reference face/currency once; signed holding quantities remain explicit.
pub fn validate_term_coupon_position(
    prepared: &PreparedTermCouponModel<'_>,
    position: &RiskPosition,
) -> QuoteRiskResult<()> {
    position.scale()?;
    if position.quantity_unit != RiskQuantityUnit::Notional
        || position.reference_quantity.decimal() != prepared.face_value()
        || position.currency != prepared.currency()
    {
        return Err(invalid(
            "coupon reference",
            "exact original face/notional and contract currency required",
        ));
    }
    Ok(())
}

/// Rebuild/query original graph coordinates and reprice actual future coupon
/// distributions. Eligible history, publication decisions and models stay fixed.
/// Original HW1F/OAS guards remain in their own owner and cannot use this route.
pub fn term_coupon_graph_sensitivity(
    prepared: &PreparedTermCouponModel<'_>,
    graph: &PreparedQuoteRiskGraph<'_>,
    key: QuoteRiskKey,
    position: RiskPosition,
    snapshot_id: SnapshotId,
    settings: QuoteRiskSettings,
) -> QuoteRiskResult<TermCouponGraphSensitivity> {
    prepared.admit_baseline(&graph.baseline().curves)?;
    validate_term_coupon_position(prepared, &position)?;
    let mut valuations = Vec::with_capacity(5);
    let sensitivity =
        reference_quote_sensitivity(key.clone(), position, snapshot_id, settings, |delta| {
            let scenario = if delta == 0.0 {
                graph.baseline().clone()
            } else {
                evaluate_quote_graph(graph, &key, delta)?
            };
            let valuation = prepared.price(&scenario.curves)?;
            let value = risk_reference_pv(valuation.present_value, prepared.currency())?;
            valuations.push(TermCouponGraphValuation {
                delta,
                valuation,
                build_order: scenario.build_order,
                repricing: scenario.repricing,
                price_evaluations: scenario.price_evaluations,
            });
            Ok(value)
        })?;
    if sensitivity.base_pv != prepared.baseline().present_value * sensitivity.position.scale()? {
        return Err(invalid(
            "coupon baseline",
            "original full native baseline PV differs",
        ));
    }
    Ok(TermCouponGraphSensitivity {
        sensitivity,
        valuations,
    })
}
