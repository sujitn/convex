//! Actual supplied coupon-model sensitivities in existing constrained hedge units.
use super::*;
use crate::{floating_coupon as c, quote_risk as q, risk::hedging::constrained as h};
use serde::Serialize;

/// Complete native coupon baseline and every actually calculated hedge factor.
#[derive(Debug, Serialize)]
pub struct TermCouponHedgeRisk {
    /// Original identity/reference/snapshot/factor/work controls.
    pub input: GraphHedgeRiskInput,
    /// Original positive face reference, applied once by the native risk owner.
    pub native_position: q::RiskPosition,
    /// Coupon model value is actual discounted currency PV.
    pub basis: ValuationBasis,
    /// Complete baseline terms, expected cash, histories and model diagnostics.
    pub valuation: c::TermCouponOptionPrice,
    /// Five actual native prices per factor under the original central owner.
    pub reserved_factor_evaluations: usize,
    /// Existing checked vector for aggregate/constrained hedge calculations.
    pub vector: h::HedgeRiskVector,
    /// All same-law native prices/graph diagnostics for each factor.
    pub coupon_factors: Vec<q::TermCouponGraphSensitivity>,
}

/// Price all intended factors, including true zeros. Original curve rebuilding,
/// fixing eligibility, model laws and half-step stability remain mandatory.
pub fn term_coupon_hedge_risk(
    prepared: &c::PreparedTermCouponModel<'_>,
    graph: &q::PreparedQuoteRiskGraph<'_>,
    input: &GraphHedgeRiskInput,
) -> WorkflowResult<TermCouponHedgeRisk> {
    super::hedge::validate_graph_hedge_input(input)?;
    q::validate_term_coupon_position(prepared, &input.reference_position)?;
    let mut factors = Vec::with_capacity(input.factors.len());
    for factor in &input.factors {
        let value = q::term_coupon_graph_sensitivity(
            prepared,
            graph,
            factor.key.clone(),
            input.reference_position.clone(),
            input.snapshot_id.clone(),
            factor.settings.clone(),
        )?;
        if factors
            .first()
            .is_some_and(|prior: &q::TermCouponGraphSensitivity| {
                prior.sensitivity.base_pv != value.sensitivity.base_pv
            })
        {
            return Err(invalid(
                "coupon baseline",
                "factor columns disagree on original full PV",
            ));
        }
        factors.push(value);
    }
    let identity = &input.identity;
    Ok(TermCouponHedgeRisk {
        input: input.clone(),
        native_position: input.reference_position.clone(),
        basis: ValuationBasis::CurrencyPresentValue,
        valuation: prepared
            .price(&graph.baseline().curves)
            .map_err(q::QuoteRiskError::from)?,
        reserved_factor_evaluations: 5 * input.factors.len(),
        vector: h::HedgeRiskVector {
            id: identity.id.clone(),
            product: h::HedgeProduct::CashBond,
            instrument: identity.instrument.clone(),
            economics: identity.economics.clone(),
            calculation: identity.calculation.clone(),
            entity: identity.entity.clone(),
            settlement: prepared.settlement(),
            credit_scope: Vec::new(),
            reference_position: input.reference_position.clone(),
            sensitivities: factors.iter().map(|r| r.sensitivity.clone()).collect(),
            fx: None,
        },
        coupon_factors: factors,
    })
}
