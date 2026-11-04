//! Full guarded optional columns use the same original constrained hedge vector.
use super::*;
use crate::{optional_bond as o, quote_risk as q, risk::hedging::constrained as h};
use convex_core::contracts::{FactorTarget, QuoteType};
use serde::Serialize;

/// Full native vector and every mandatory model-specific risk certificate.
#[derive(Debug, Serialize)]
pub struct OptionalGraphHedgeRisk {
    /// Original full reference/input/vector/valuation contract.
    #[serde(flatten)]
    pub risk: GraphHedgeRisk,
    /// Complete original h/h2 model prices and independent native risk refinements.
    pub optional_factors: Vec<q::OptionalFactorSensitivity>,
    /// Six native price evaluations per factor (each h/h2 risk has its own base),
    /// plus the returned baseline. Grid/tree work retains its original bounds.
    pub native_price_evaluations: usize,
}
/// OAS is scoped to the exact original supplied security revision.
pub fn validate_optional_oas_key(
    key: &q::QuoteRiskKey,
    instrument: &convex_credit::ReferencePin,
) -> WorkflowResult<()> {
    if key.definition().factor.definition().quote_type == QuoteType::OasSpread
        && (!matches!(&key.definition().factor.definition().target,
            FactorTarget::Instrument { instrument_id } if instrument_id.as_str()==instrument.id.as_str())
            || !matches!(&key.definition().coordinate,
                q::RiskCoordinate::Raw { input_id, version } if input_id==instrument.id.as_str()
                    && *version==instrument.version))
    {
        return Err(invalid(
            "optional OAS",
            "exact original security identity/revision required",
        ));
    }
    Ok(())
}
/// Original face units are retained; no bullet, deterministic projected optional
/// coupon or generic price-only bump can replace native risk admission.
pub fn optional_hedge_risk(
    prepared: &o::PreparedOptionalBond,
    oas_decimal: f64,
    graph: &q::PreparedQuoteRiskGraph<'_>,
    input: &GraphHedgeRiskInput,
    policies: q::OptionalRiskPolicies,
) -> WorkflowResult<OptionalGraphHedgeRisk> {
    super::hedge::validate_graph_hedge_input(input)?;
    q::admit_optional_graph(prepared, graph)?;
    let position = &input.reference_position;
    if position.quantity_unit != q::RiskQuantityUnit::Notional
        || position.reference_quantity.decimal() != prepared.face_value()
        || position.currency != prepared.currency()
    {
        return Err(invalid(
            "optional reference",
            "exact original face and contract currency required",
        ));
    }
    let mut optional_factors = Vec::with_capacity(input.factors.len());
    for factor in &input.factors {
        validate_optional_oas_key(&factor.key, &input.identity.instrument)?;
        let result = q::optional_factor_sensitivity(
            prepared,
            factor.key.clone(),
            position.clone(),
            input.snapshot_id.clone(),
            factor.settings.clone(),
            oas_decimal,
            policies,
        )?;
        if optional_factors
            .first()
            .is_some_and(|r: &q::OptionalFactorSensitivity| {
                r.sensitivity.base_pv != result.sensitivity.base_pv
            })
        {
            return Err(invalid(
                "optional baseline",
                "complete factor columns disagree on original PV",
            ));
        }
        optional_factors.push(result);
    }
    let identity = &input.identity;
    let risk = GraphHedgeRisk {
        input: input.clone(),
        native_position: position.clone(),
        basis: ValuationBasis::CurrencyPresentValue,
        valuation: Valuation::Optional(Box::new(prepared.price(oas_decimal)?)),
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
            reference_position: position.clone(),
            sensitivities: optional_factors
                .iter()
                .map(|r| r.sensitivity.clone())
                .collect(),
            fx: None,
        },
    };
    Ok(OptionalGraphHedgeRisk {
        risk,
        native_price_evaluations: 6 * input.factors.len() + 1,
        optional_factors,
    })
}
