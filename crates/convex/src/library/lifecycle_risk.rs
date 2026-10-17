//! Thin lifecycle-risk composition over original graph, replay and P&L owners.
use super::*;
use convex_analytics::{
    lifecycle as l, quote_risk as q, risk::hedging::constrained as h, workflow as w,
};
use convex_core::contracts::{ExactDecimal, ModelMoney};
use rust_decimal::Decimal;

pub(super) fn calculate(
    graph_input: &CalibrationInput,
    product: &RiskProductInput,
    lifecycle: &LifecycleInput,
    history: &w::CausalPnlInput,
    input: &w::GraphHedgeRiskInput,
    max_state_evaluations: usize,
    resources: &inputs::ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let n = input.factors.len();
    if !(1..=16).contains(&n)
        || !(1..=80).contains(&max_state_evaluations)
        || n * 5 > max_state_evaluations
    {
        return Err(LibraryError::limit(
            "explicit five-price lifecycle reservation exhausted",
        ));
    }
    if !matches!(
        product,
        RiskProductInput::Bond { .. }
            | RiskProductInput::Swap { .. }
            | RiskProductInput::Cds { .. }
    ) {
        return Err(LibraryError::invalid(
            "lifecycle risk",
            "standard bond, IRS/OIS or CDS required; optional and physical owners remain separate",
        ));
    }
    let path = w::prepare_causal_pnl(history).map_err(reconciliation::native_error)?;
    let closing = history.causes.len();
    let point = &path.input().points[closing];
    let index = point
        .positions
        .iter()
        .position(|p| p.identity == input.identity)
        .ok_or_else(|| {
            LibraryError::invalid(
                "lifecycle identity",
                "exact closing holding identity required",
            )
        })?;
    let position = &point.positions[index];
    let mut reference = position.current.clone();
    reference.quantity = reference.reference_quantity;
    if reference != input.reference_position || lifecycle.trade_id != input.identity.id {
        return Err(LibraryError::invalid(
            "lifecycle position",
            "same native original reference and actual holding required",
        ));
    }
    reconciliation::validate_risk_point(point, product, lifecycle, resources.input(), index)?;
    let replay = events::replay_native(lifecycle, resources)?;
    let state = replay.state.view();
    let default_entity = match (product, replay.state.instrument()) {
        (RiskProductInput::Cds { terms, .. }, _) if state.default_event.is_some() => {
            Some(terms.reference.entity_id.clone())
        }
        (
            RiskProductInput::Bond { .. },
            l::LifecycleInstrument::Bond {
                credit: Some(credit),
                ..
            },
        ) if state.default_event.is_some() => Some(credit.entity.clone()),
        _ => None,
    };
    if !matches!(
        state.status,
        l::EconomicStatus::Active
            | l::EconomicStatus::Terminated
            | l::EconomicStatus::Closed
            | l::EconomicStatus::DefaultPending
            | l::EconomicStatus::DefaultCashPending
    ) || !state.exercise_notices.is_empty()
        || state.delivery_pending.is_some()
        || state.default_event.is_some() && default_entity.is_none()
    {
        return Err(LibraryError::invalid("lifecycle state", "original cash or admitted bond/CDS default owner required; exercise/physical risk remains separate"));
    }
    curves::with_graph(graph_input, resources, |graph| {
        let original = resources.with_curves(graph.baseline().curves.clone());
        risk::with_product(product, &original, |product| {
            let fresh = w::curve_hedge_risk(product, graph, input)?;
            let discount = graph.baseline().curves.get(&position.cash_discount)?;
            let native = w::CausalPnlProduct::Standard(product);
            if state.status == l::EconomicStatus::DefaultPending {
                w::validate_causal_pnl_default_position(
                    &path,
                    closing,
                    index,
                    &replay.state,
                    &native,
                    discount.as_ref(),
                )
                .map_err(reconciliation::native_error)?;
                let mut result = value(fresh)?;
                result["lifecycle_unpriced"] = serde_json::json!({
                    "position":position,"manifest":point.manifest,"settlement":point.valuation_date,
                    "snapshot_id":input.snapshot_id,"state":state,"default_entity":default_entity,
                    "reason":if matches!(product, w::ValuationInputs::Cds { .. }) {
                        "actual_cds_recovery_unknown"
                    } else { "actual_bond_recovery_unknown" }});
                return Ok(result);
            }
            let mark = w::mark_causal_pnl_position(
                &path,
                closing,
                index,
                &replay.state,
                &native,
                discount.as_ref(),
            )
            .map_err(reconciliation::native_error)?;
            let baseline = (mark.local.current, mark.local.pending);
            let unit = q::RiskPosition {
                quantity: ExactDecimal::new(Decimal::ONE),
                reference_quantity: ExactDecimal::new(Decimal::ONE),
                quantity_unit: q::RiskQuantityUnit::TradeCopies,
                currency: position.current.currency,
            };
            let mut current = Vec::new();
            let mut pending = Vec::new();
            for factor in &input.factors {
                let moves = [
                    0.0,
                    factor.settings.bump,
                    -factor.settings.bump,
                    factor.settings.bump / 2.0,
                    -factor.settings.bump / 2.0,
                ];
                let mut prices = vec![baseline];
                for delta in moves.iter().skip(1) {
                    let scenario = q::evaluate_quote_graph(graph, &factor.key, *delta)?;
                    let discount = scenario.curves.get(&position.cash_discount)?;
                    let value = w::mark_causal_pnl_scenario_position(
                        &path,
                        closing,
                        index,
                        &replay.state,
                        &native,
                        discount.as_ref(),
                        &scenario.curves,
                    )
                    .map_err(reconciliation::native_error)?;
                    prices.push((value.local.current, value.local.pending));
                }
                let sensitivity = |pending: bool| {
                    q::reference_quote_sensitivity(
                        factor.key.clone(),
                        unit.clone(),
                        input.snapshot_id.clone(),
                        factor.settings.clone(),
                        |delta| {
                            let at = moves.iter().position(|v| *v == delta).ok_or_else(|| {
                                q::QuoteRiskError::Invalid {
                                    field: "lifecycle coordinate".into(),
                                    reason: "original five-price move required".into(),
                                }
                            })?;
                            let values: &(ModelMoney, ModelMoney) = &prices[at];
                            Ok(if pending { values.1 } else { values.0 })
                        },
                    )
                };
                current.push(sensitivity(false)?);
                pending.push(sensitivity(true)?);
            }
            let actual = h::LifecycleHoldingRisk {
                default_entity: default_entity.clone(),
                position: position.clone(),
                manifest: point.manifest.clone(),
                settlement: point.valuation_date,
                snapshot_id: input.snapshot_id.clone(),
                current,
                pending,
            };
            let mut result = value(fresh)?;
            let object = result
                .as_object_mut()
                .ok_or_else(|| LibraryError::invalid("risk result", "native object required"))?;
            object.insert("lifecycle".into(), value(actual)?);
            object.insert("lifecycle_mark".into(), value(mark)?);
            object.insert("reserved_state_evaluations".into(), value(n * 5)?);
            object.insert("state_evaluations".into(), value(1 + 4 * n)?);
            Ok(result)
        })
    })
}
