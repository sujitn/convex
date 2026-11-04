//! Complete curve and selected-side coupon parameter columns with native proofs.
use super::*;
use crate::{floating_coupon as c, quote_risk as q, risk::hedging::constrained as h};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// One exact intended parameter and its explicit native and signed precision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CouponDirectionalHedgePolicy {
    /// Complete canonical original factor, occurring exactly once in the input.
    pub key: q::QuoteRiskKey,
    /// Selected side and both native and signed derivative policies.
    pub settings: q::CouponDirectionalRiskSettings,
}

/// Full pricing proof, without relabeling a directional result as central.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "result", rename_all = "snake_case")]
pub enum TermCouponDirectionalFactor {
    /// Five complete graph valuations and their original central risk record.
    Graph(Box<q::TermCouponGraphSensitivity>),
    /// Six complete original-side valuations and both derivative orders.
    Directional(Box<q::TermCouponDirectionalParameterSensitivity>),
}

/// Full native column construction for the existing constrained optimizer.
#[derive(Debug, Serialize)]
pub struct TermCouponDirectionalHedgeRisk {
    /// Original immutable identities, quantities, factors and work policy.
    pub input: GraphHedgeRiskInput,
    /// Every supplied policy, in its original order.
    pub parameter_policies: Vec<CouponDirectionalHedgePolicy>,
    /// Original positive face, without current/pending quantity scaling.
    pub native_position: q::RiskPosition,
    /// Native currency cash PV basis.
    pub basis: ValuationBasis,
    /// Complete baseline model, economics, cash and publication/history proof.
    pub valuation: c::TermCouponOptionPrice,
    /// Five graph and six directional price calls reserved before pricing.
    pub reserved_factor_evaluations: usize,
    /// Checked numerical records with explicit computation methods.
    pub vector: h::RiskVector<q::HedgeSensitivityEvidence>,
    /// All complete native calculations in exactly the intended factor order.
    pub factors: Vec<TermCouponDirectionalFactor>,
}

/// Calculate all intended graph and directional factors, including genuine zeros.
pub fn term_coupon_directional_hedge_risk(
    prepared: &c::PreparedTermCouponModel<'_>,
    graph: &q::PreparedQuoteRiskGraph<'_>,
    input: &GraphHedgeRiskInput,
    policies: &[CouponDirectionalHedgePolicy],
) -> WorkflowResult<TermCouponDirectionalHedgeRisk> {
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
            .native
            .derivative
            .validate()
            .map_err(q::QuoteRiskError::from)?;
        policy.settings.first.validate()?;
        policy.settings.second.validate()?;
        let point = q::coupon_parameter_coordinate(&policy.key)?;
        let expected = q::QuoteRiskSettings {
            bump: policy.settings.native.derivative.bump,
            stability: policy.settings.first,
            policy_version: policy.settings.policy_version,
        };
        if !points.insert(point)
            || input
                .factors
                .iter()
                .filter(|f| f.key == policy.key && f.settings == expected)
                .count()
                != 1
        {
            return Err(invalid(
                "parameter policies",
                "duplicate, unused or mismatched exact original point/policy",
            ));
        }
    }
    let mut intended = HashSet::new();
    let mut reserved = 0;
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
                    "duplicate alias or missing exact directional policy",
                ));
            }
            reserved += 6;
        } else {
            reserved += 5;
        }
    }
    if reserved > input.max_factor_evaluations {
        return Err(invalid(
            "factor work",
            "explicit five graph/six directional price reservation exhausted",
        ));
    }
    let curves = &graph.baseline().curves;
    prepared
        .admit_baseline(curves)
        .map_err(q::QuoteRiskError::from)?;
    let mut factors = Vec::with_capacity(input.factors.len());
    let mut sensitivities = Vec::with_capacity(input.factors.len());
    for factor in &input.factors {
        let (proof, record, base_pv) = if factor
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
            let result = q::term_coupon_directional_parameter_sensitivity(
                prepared,
                curves,
                factor.key.clone(),
                input.reference_position.clone(),
                input.snapshot_id.clone(),
                policy.settings,
            )?;
            let record = result.hedge_record()?;
            let baseline = record.base_pv()?;
            (
                TermCouponDirectionalFactor::Directional(Box::new(result)),
                q::HedgeSensitivityEvidence::Directional(record),
                baseline,
            )
        } else {
            let result = q::term_coupon_graph_sensitivity(
                prepared,
                graph,
                factor.key.clone(),
                input.reference_position.clone(),
                input.snapshot_id.clone(),
                factor.settings.clone(),
            )?;
            let baseline = result.sensitivity.base_pv;
            let record = result.sensitivity.clone();
            (
                TermCouponDirectionalFactor::Graph(Box::new(result)),
                q::HedgeSensitivityEvidence::Central(record),
                baseline,
            )
        };
        if base_pv != prepared.baseline().present_value {
            return Err(invalid(
                "coupon baseline",
                "factor differs from complete original baseline PV",
            ));
        }
        factors.push(proof);
        sensitivities.push(record);
    }
    let identity = &input.identity;
    Ok(TermCouponDirectionalHedgeRisk {
        input: input.clone(),
        parameter_policies: policies.to_vec(),
        native_position: input.reference_position.clone(),
        basis: ValuationBasis::CurrencyPresentValue,
        valuation: prepared.price(curves).map_err(q::QuoteRiskError::from)?,
        reserved_factor_evaluations: reserved,
        vector: h::RiskVector {
            id: identity.id.clone(),
            product: h::HedgeProduct::CashBond,
            instrument: identity.instrument.clone(),
            economics: identity.economics.clone(),
            calculation: identity.calculation.clone(),
            entity: identity.entity.clone(),
            settlement: prepared.settlement(),
            credit_scope: Vec::new(),
            reference_position: input.reference_position.clone(),
            sensitivities,
            fx: None,
        },
        factors,
    })
}
