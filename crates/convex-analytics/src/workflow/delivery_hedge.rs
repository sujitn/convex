//! Complete all-member delivery-futures columns into the original hedge owner.
use super::*;
use crate::{futures as f, quote_risk as q, risk::hedging::constrained as h};
use convex_core::contracts::ExactDecimal;
use rust_decimal::Decimal;

/// Full basket/factor provenance beside the complete original monetary vector.
#[derive(Debug, serde::Serialize)]
pub struct DeliveryFuturesHedgeRisk {
    /// Complete immutable original hedge identity, quantities, keys and settings.
    pub input: GraphHedgeRiskInput,
    /// Complete original native base carry/invoice/CTD alternatives.
    pub valuation: f::BondFuturesValuation,
    /// Explicit unrounded model quote-level measurement, never discounted PV.
    pub basis: ValuationBasis,
    /// Five actual full basket valuations per factor, including unconsumed factors.
    pub reserved_factor_evaluations: usize,
    /// Original explicit all-member policy bound, independent from factor calls.
    pub policy: f::BondFuturesGraphPolicy,
    /// Twelve member visits per factor, reserved before any risk calculation.
    pub reserved_member_evaluations: usize,
    /// Full original five scenarios, actual cash/carry, ties and switches per key.
    pub factors: Vec<f::BondFuturesGraphRisk>,
    /// Positive one-contract native vector; reporting FX is explicitly attached later.
    pub vector: h::HedgeRiskVector,
}
/// Calculate every actual full-basket factor and bind one original contract to
/// its native lot unit. No baseline-CTD/duration proxy or caller-created column.
pub fn delivery_futures_hedge_risk(
    inputs: &f::BondFuturesInputs<'_>,
    discount: f::FuturesCurve<'_>,
    graph: &q::PreparedQuoteRiskGraph<'_>,
    input: &GraphHedgeRiskInput,
    policy: f::BondFuturesGraphPolicy,
) -> WorkflowResult<DeliveryFuturesHedgeRisk> {
    super::hedge::validate_graph_hedge_input(input)?;
    let identity = &input.identity;
    let position = &input.reference_position;
    if identity.instrument.id.as_str() != inputs.contract.instrument_id.as_str()
        || identity.instrument.version != inputs.contract.stamp.version
        || position.quantity_unit != q::RiskQuantityUnit::Contracts
        || position.reference_quantity != ExactDecimal::new(Decimal::ONE)
        || position.currency != inputs.contract.currency
    {
        return Err(invalid("delivery-futures reference", "exact original contract revision, currency and positive one-contract reference required"));
    }
    let reserved = policy.reserve(inputs.deliverables.len(), input.factors.len())?;
    let mut factors = Vec::with_capacity(input.factors.len());
    for factor in &input.factors {
        factors.push(f::bond_futures_graph_risk(
            inputs,
            discount,
            graph,
            &f::BondFuturesGraphRequest {
                key: factor.key.clone(),
                position: position.clone(),
                snapshot_id: input.snapshot_id.clone(),
                settings: factor.settings.clone(),
                policy,
            },
        )?);
    }
    let valuation = factors
        .first()
        .ok_or_else(|| invalid("factors", "complete original factors missing"))?
        .evaluations[0]
        .valuation
        .clone();
    if factors
        .iter()
        .any(|f| f.evaluations[0].valuation != valuation)
    {
        return Err(invalid(
            "baseline",
            "full original basket valuations differ across factors",
        ));
    }
    Ok(DeliveryFuturesHedgeRisk {
        input: input.clone(),
        valuation,
        basis: ValuationBasis::FuturesQuoteLevel,
        reserved_factor_evaluations: 5 * input.factors.len(),
        policy,
        reserved_member_evaluations: reserved,
        vector: h::HedgeRiskVector {
            id: identity.id.clone(),
            product: h::HedgeProduct::Futures,
            instrument: identity.instrument.clone(),
            economics: identity.economics.clone(),
            calculation: identity.calculation.clone(),
            entity: identity.entity.clone(),
            settlement: Date::from(inputs.context.as_of.datetime().date_naive()),
            credit_scope: Vec::new(),
            reference_position: position.clone(),
            sensitivities: factors.iter().map(|f| f.sensitivity.clone()).collect(),
            fx: None,
        },
        factors,
    })
}
