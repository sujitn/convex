//! Complete actual curve and native coupon parameter columns in original order.
use super::*;
use crate::{floating_coupon as c, quote_risk as q, risk::hedging::constrained as h};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Exact intended parameter factor and its additional native reference policies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CouponParameterHedgePolicy {
    /// Complete original factor; it must occur exactly once in the risk input.
    pub key: q::QuoteRiskKey,
    /// Native reference currency first/second derivative and resolution admission.
    pub settings: c::CouponParameterDerivativeSettings,
}

/// Every full evaluated native factor retains its own actual original proof.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "result", rename_all = "snake_case")]
pub enum TermCouponFactorSensitivity {
    /// Original full graph/calibration/scenario coupon valuations.
    Graph(Box<q::TermCouponGraphSensitivity>),
    /// Original model-parameter first/second refinement and five full prices.
    Parameter(Box<q::TermCouponParameterSensitivity>),
}
impl TermCouponFactorSensitivity {
    /// Existing canonical normalization, without recomputing any price or scale.
    pub fn sensitivity(&self) -> &q::QuoteSensitivity {
        match self {
            Self::Graph(r) => &r.sensitivity,
            Self::Parameter(r) => &r.sensitivity,
        }
    }
}

/// Actual full model/graph baseline and all complete constrained hedge columns.
#[derive(Debug, Serialize)]
pub struct TermCouponCompleteHedgeRisk {
    /// Exact original intended factor order, identity, snapshot and work policy.
    pub input: GraphHedgeRiskInput,
    /// Every explicit native parameter policy, without ignored entries.
    pub parameter_policies: Vec<CouponParameterHedgePolicy>,
    /// Original native positive face, with no second normalization.
    pub native_position: q::RiskPosition,
    /// Full original native currency cash PV basis.
    pub basis: ValuationBasis,
    /// Actual complete original baseline.
    pub valuation: c::TermCouponOptionPrice,
    /// Five full price evaluations reserved for every intended factor.
    pub reserved_factor_evaluations: usize,
    /// Existing native aggregation/constraint solver input, without order authority.
    pub vector: h::HedgeRiskVector,
    /// Full original proof in exactly the factor order.
    pub factors: Vec<TermCouponFactorSensitivity>,
}

/// Calculate every intended graph/parameter factor, including actual zero risk.
pub fn term_coupon_complete_hedge_risk(
    prepared: &c::PreparedTermCouponModel<'_>,
    graph: &q::PreparedQuoteRiskGraph<'_>,
    input: &GraphHedgeRiskInput,
    policies: &[CouponParameterHedgePolicy],
) -> WorkflowResult<TermCouponCompleteHedgeRisk> {
    super::hedge::validate_graph_hedge_input(input)?;
    q::validate_term_coupon_position(prepared, &input.reference_position)?;
    if policies.len() > 16 {
        return Err(invalid(
            "parameter policies",
            "at most16 intended factors required",
        ));
    }
    let mut points = HashSet::new();
    for policy in policies {
        policy
            .settings
            .validate()
            .map_err(q::QuoteRiskError::from)?;
        let point = q::coupon_parameter_coordinate(&policy.key)?;
        if !points.insert(point)
            || input.factors.iter().filter(|f| f.key == policy.key).count() != 1
        {
            return Err(invalid(
                "parameter policies",
                "duplicate or unused original point/policy",
            ));
        }
    }
    let mut intended = HashSet::new();
    for factor in &input.factors {
        if factor
            .key
            .definition()
            .factor
            .definition()
            .quote_type
            .is_coupon_parameter()
        {
            let point = q::coupon_parameter_coordinate(&factor.key)?;
            if !intended.insert(point)
                || policies.iter().filter(|p| p.key == factor.key).count() != 1
            {
                return Err(invalid(
                    "parameter factors",
                    "duplicate alias or missing exact native policy",
                ));
            }
        }
    }
    let curves = &graph.baseline().curves;
    prepared
        .admit_baseline(curves)
        .map_err(q::QuoteRiskError::from)?;
    let mut factors = Vec::with_capacity(input.factors.len());
    for factor in &input.factors {
        let result = if factor
            .key
            .definition()
            .factor
            .definition()
            .quote_type
            .is_coupon_parameter()
        {
            let policy = policies
                .iter()
                .find(|p| p.key == factor.key)
                .ok_or_else(|| invalid("parameter policy", "missing validated original policy"))?;
            TermCouponFactorSensitivity::Parameter(Box::new(q::term_coupon_parameter_sensitivity(
                prepared,
                curves,
                factor.key.clone(),
                input.reference_position.clone(),
                input.snapshot_id.clone(),
                factor.settings.clone(),
                policy.settings,
            )?))
        } else {
            TermCouponFactorSensitivity::Graph(Box::new(q::term_coupon_graph_sensitivity(
                prepared,
                graph,
                factor.key.clone(),
                input.reference_position.clone(),
                input.snapshot_id.clone(),
                factor.settings.clone(),
            )?))
        };
        if result.sensitivity().base_pv != prepared.baseline().present_value {
            return Err(invalid(
                "coupon baseline",
                "factor differs from complete original baseline PV",
            ));
        }
        factors.push(result);
    }
    let identity = &input.identity;
    Ok(TermCouponCompleteHedgeRisk {
        input: input.clone(),
        parameter_policies: policies.to_vec(),
        native_position: input.reference_position.clone(),
        basis: ValuationBasis::CurrencyPresentValue,
        valuation: prepared.price(curves).map_err(q::QuoteRiskError::from)?,
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
            sensitivities: factors.iter().map(|f| f.sensitivity().clone()).collect(),
            fx: None,
        },
        factors,
    })
}
