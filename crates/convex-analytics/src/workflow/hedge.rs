//! Complete original graph sensitivities into positive native hedge columns.
use super::*;
use crate::{quote_risk as q, risk::hedging::constrained as h};
use convex_core::contracts::{ExactDecimal, SnapshotId};
use convex_credit::ReferencePin;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Explicit application identity; resolving manifest pins remains application work.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GraphHedgeIdentity {
    /// Complete holding or candidate identity.
    pub id: convex_core::contracts::TradeId,
    /// Actual security revision; CDS/swaps/futures must match their full terms.
    pub instrument: ReferencePin,
    /// Complete terms/state/market manifest revision.
    pub economics: ReferencePin,
    /// Original calculation/model manifest revision.
    pub calculation: ReferencePin,
    /// Owning entity, without limit or order authority.
    pub entity: convex_core::contracts::EntityId,
}
/// One exact intended common factor, never an inferred tenor or yield alias.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GraphHedgeFactor {
    /// Complete graph coordinate and monetary normalization.
    pub key: q::QuoteRiskKey,
    /// Original factor-specific central and half-step policy.
    pub settings: q::QuoteRiskSettings,
}
/// Supplied complete-factor calculation; nullable reporting FX is attached later.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GraphHedgeRiskInput {
    /// Original application pins, retained in full.
    pub identity: GraphHedgeIdentity,
    /// Positive original face/notional or one futures contract.
    /// IRS uses the first original fixed-leg notional, explicitly supplied here.
    pub reference_position: q::RiskPosition,
    /// One immutable information set for every column and reporting FX.
    pub snapshot_id: SnapshotId,
    /// All intended factors in the exact hedge problem order, 1..16.
    pub factors: Vec<GraphHedgeFactor>,
    /// Explicit ceiling on five-price factor calls, 1..80.
    /// Original graph/calibration/product bounds also remain mandatory.
    pub max_factor_evaluations: usize,
}
/// Full original baseline, unit binding and every actually calculated factor.
#[derive(Debug, Serialize)]
pub struct GraphHedgeRisk {
    /// Complete original request; no caller-created risk columns are accepted.
    pub input: GraphHedgeRiskInput,
    /// Original full-product reference position, including swap trade-copy units.
    pub native_position: q::RiskPosition,
    /// Actual PV versus futures quote-level measurement.
    pub basis: ValuationBasis,
    /// Full original native baseline cash/leg/history/model diagnostics.
    pub valuation: Valuation,
    /// Five calls for every declared factor, including genuine zero exposures.
    pub reserved_factor_evaluations: usize,
    /// Exact original common-factor column; reporting FX is explicitly absent.
    pub vector: h::HedgeRiskVector,
}

/// Calculate every full graph factor with the SAME original product owner.
/// Swaps bind one full trade copy to its original fixed-leg reference notional;
/// no coupon/profile/side or reference price is rescaled or reconstructed.
pub fn curve_hedge_risk(
    product: &ValuationInputs<'_>,
    graph: &q::PreparedQuoteRiskGraph<'_>,
    input: &GraphHedgeRiskInput,
) -> WorkflowResult<GraphHedgeRisk> {
    if input.factors.is_empty() || input.factors.len() > 16 {
        return Err(invalid(
            "factors",
            "1..16 complete original factors required",
        ));
    }
    let reserved = 5 * input.factors.len();
    if input.max_factor_evaluations == 0
        || input.max_factor_evaluations > 80
        || reserved > input.max_factor_evaluations
    {
        return Err(invalid(
            "factor work",
            "explicit five-price reservation exhausted",
        ));
    }
    for (i, factor) in input.factors.iter().enumerate() {
        factor.settings.validate_for(&factor.key)?;
        if input.factors[..i].iter().any(|f| f.key == factor.key) {
            return Err(invalid("factors", "duplicate original factor"));
        }
    }
    let position = &input.reference_position;
    position.scale()?;
    if position.quantity != position.reference_quantity {
        return Err(invalid(
            "reference_position",
            "positive original reference quantity required",
        ));
    }
    let identity = &input.identity;
    let mut native_position = position.clone();
    let (family, settlement, instrument, credit_scope) = match product {
        ValuationInputs::Bond { cash, .. } => (
            h::HedgeProduct::CashBond, cash.settlement(), None, Vec::new(),
        ),
        ValuationInputs::FloatingBond { context, .. } => (
            h::HedgeProduct::CashBond, context.settlement, None, Vec::new(),
        ),
        ValuationInputs::Swap { contract, context, .. } => {
            let terms = contract.terms();
            let reference = terms.fixed.leg.notionals.first()
                .ok_or_else(|| invalid("reference_position", "missing original fixed-leg notional"))?.value;
            if position.quantity_unit != q::RiskQuantityUnit::Notional
                || position.reference_quantity.decimal() != reference
            {
                return Err(invalid("reference_position", "exact first original fixed-leg notional required"));
            }
            native_position.quantity = ExactDecimal::new(Decimal::ONE);
            native_position.reference_quantity = ExactDecimal::new(Decimal::ONE);
            native_position.quantity_unit = q::RiskQuantityUnit::TradeCopies;
            (h::HedgeProduct::Irs, context.fixings.valuation_date,
                Some((terms.trade_id.as_str(), terms.version)), Vec::new())
        }
        ValuationInputs::Cds { contract, context, .. } => {
            let terms = contract.terms();
            (h::HedgeProduct::Cds, context.valuation_date,
                Some((terms.trade_id.as_str(), terms.version)), vec![terms.reference.clone()])
        }
        ValuationInputs::RateFuture { inputs, .. } => (
            h::HedgeProduct::Futures,
            Date::from(inputs.context.as_of.datetime().date_naive()),
            Some((inputs.contract.instrument_id.as_str(), inputs.contract.stamp.version)), Vec::new(),
        ),
        _ => return Err(WorkflowError::Unsupported("optional bonds and delivery futures require their complete original model/all-member risk owners")),
    };
    if instrument.is_some_and(|(id, version)| {
        identity.instrument.id.as_str() != id || identity.instrument.version != version
    }) {
        return Err(invalid(
            "instrument",
            "actual full product identity/revision mismatch",
        ));
    }
    product.validate_position(&native_position)?;
    let mut sensitivities = Vec::with_capacity(input.factors.len());
    for factor in &input.factors {
        let native = curve_quote_risk(
            product,
            graph,
            factor.key.clone(),
            native_position.clone(),
            input.snapshot_id.clone(),
            factor.settings.clone(),
        )?;
        let result = if native_position == *position {
            native
        } else {
            // Both reference positions have scale exactly one. Replay the SAME
            // five evaluated PVs through the original checked normalization owner.
            // This binds units; it performs no new pricing or financial arithmetic.
            let moves = [
                0.0,
                factor.settings.bump,
                -factor.settings.bump,
                factor.settings.bump / 2.0,
                -factor.settings.bump / 2.0,
            ];
            let prices = [
                native.base_pv,
                native.plus_pv,
                native.minus_pv,
                native.half_plus_pv,
                native.half_minus_pv,
            ];
            q::reference_quote_sensitivity(
                factor.key.clone(),
                position.clone(),
                input.snapshot_id.clone(),
                factor.settings.clone(),
                |delta| {
                    let i = moves.iter().position(|v| *v == delta).ok_or_else(|| {
                        q::QuoteRiskError::Invalid {
                            field: "reference units".into(),
                            reason: "unexpected original five-price coordinate".into(),
                        }
                    })?;
                    q::risk_reference_pv(prices[i], position.currency)
                },
            )?
        };
        if sensitivities
            .first()
            .is_some_and(|s: &q::QuoteSensitivity| s.base_pv != result.base_pv)
        {
            return Err(invalid(
                "baseline",
                "factor columns disagree on original base PV",
            ));
        }
        sensitivities.push(result);
    }
    Ok(GraphHedgeRisk {
        input: input.clone(),
        native_position,
        basis: product.basis(),
        valuation: price(product)?,
        reserved_factor_evaluations: reserved,
        vector: h::HedgeRiskVector {
            id: identity.id.clone(),
            product: family,
            instrument: identity.instrument.clone(),
            economics: identity.economics.clone(),
            calculation: identity.calculation.clone(),
            entity: identity.entity.clone(),
            settlement,
            credit_scope,
            reference_position: position.clone(),
            sensitivities,
            fx: None,
        },
    })
}
