//! Direct original-context yield risk; differentiation and hedging stay with their owners.
use super::*;
use crate::{quote_risk as q, risk::hedging::constrained as h};
use convex_core::contracts::{EntityId, FactorTarget, QuoteType, SnapshotId, TradeId};
use convex_curves::contracts::MarketQuote;

/// Whole five-price cash-visit ceiling; this is not a latency guarantee.
pub const MAX_ICMA_RISK_CASH_EVALUATIONS: u64 = 67_108_864;

/// Original observed coordinate and explicit central/half-step work policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaYieldRiskInput {
    /// Exact own-security raw-yield key, original quote and policy revisions.
    pub key: q::QuoteRiskKey,
    /// Original observation in its explicit decimal, percent or basis-point unit.
    pub observed_quote: MarketQuote,
    /// Caller-resolved immutable information set, shared with all hedge inputs.
    pub snapshot_id: SnapshotId,
    /// Original canonical annual-decimal bump and mandatory half-step stability.
    pub settings: q::QuoteRiskSettings,
    /// Explicit total reservation for all five complete cash prices.
    pub max_cash_evaluations: u64,
}
/// One original native-unit observation and its complete signed quotation.
#[derive(Debug, Clone, Serialize)]
pub struct IcmaYieldRiskValuation {
    /// Canonical annual-decimal move; source revisions remain hypothetical and unchanged.
    pub decimal_move: f64,
    /// Scenario quote in the original observation's unit.
    pub native_quote: MarketQuote,
    /// Same complete original cash/context/source/position and dirty consideration.
    pub quotation: IcmaQuotation,
}
/// Five full quotations and the original checked monetary sensitivity.
#[derive(Debug, Clone, Serialize)]
pub struct IcmaYieldSensitivity {
    /// Exact original quote axis, snapshot and numerical/work policies.
    pub input: IcmaYieldRiskInput,
    /// Base, +h, -h, +h/2, -h/2 in the original reference owner's order.
    pub valuations: [IcmaYieldRiskValuation; 5],
    /// Original native central/half-step result; quantity is applied once.
    pub sensitivity: q::QuoteSensitivity,
    /// One-time reference-entry reservation, reused by all prices.
    pub reserved_reference_evaluations: u64,
    /// Complete five-price reservation including two boundary visits per price.
    pub reserved_cash_evaluations: u64,
}
/// Application-resolved hedge identity; these pins do not certify manifest contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaHedgeIdentity {
    /// Original holding/candidate identity.
    pub id: TradeId,
    /// Complete economics/state/market manifest revision, resolved by the caller.
    pub economics: convex_credit::ReferencePin,
    /// Full original calculation manifest revision, resolved by the caller.
    pub calculation: convex_credit::ReferencePin,
    /// Actual owning legal entity; calculation does not grant authority.
    pub entity: EntityId,
}
/// Complete source quotation plus an unchanged native hedge-factor column.
#[derive(Debug, Clone, Serialize)]
pub struct IcmaHedgeRisk {
    /// Original identity pins, with no inferred state or authority.
    pub identity: IcmaHedgeIdentity,
    /// Every original quotation and native checked five-PV result.
    pub yield_risk: IcmaYieldSensitivity,
    /// Positive original-face reference vector for existing hedge aggregation/solve.
    /// Reporting FX must be supplied explicitly to that operation when required.
    pub vector: h::HedgeRiskVector,
}
impl PreparedIcmaQuotation {
    /// Reprice the SAME complete prepared plain fixed cash five times on its own
    /// observed yield axis. Conditional workouts are not unconditional hedge risk.
    pub fn yield_sensitivity(
        &self,
        input: &IcmaYieldRiskInput,
    ) -> WorkflowResult<IcmaYieldSensitivity> {
        if !matches!(self.selection, IcmaCashSelection::Plain { .. }) {
            return Err(WorkflowError::NotApplicable(
                "direct ICMA yield risk requires unconditional plain fixed cash",
            ));
        }
        let definition = input.key.definition();
        let factor = definition.factor.definition();
        if factor.quote_type != QuoteType::Yield
            || factor.currency != self.position.currency
            || factor.convention_version != self.sources.quotation_policy.version
            || factor.model_version.as_str() != self.sources.quotation_policy.id.as_str()
            || !matches!(&factor.target, FactorTarget::Instrument { instrument_id }
                if instrument_id.as_str() == self.sources.security.id.as_str())
            || !matches!(&definition.coordinate, q::RiskCoordinate::Raw { input_id, version }
                if input_id == self.sources.quote_revision.id.as_str()
                    && *version == self.sources.quote_revision.version)
            || definition.native_unit != input.observed_quote.unit
        {
            return Err(invalid(
                "ICMA yield factor",
                "exact own-security raw yield, native unit, quote revision and quotation policy required",
            ));
        }
        input.settings.validate_for(&input.key)?;
        let rate = input.observed_quote.decimal_rate()?;
        let h = input.settings.bump;
        // Check the whole original coordinate domain before any numerical price.
        for delta in [0.0, h, -h, h / 2.0, -h / 2.0] {
            input.observed_quote.model_rate_quote(rate + delta)?;
        }
        let reserved_cash_evaluations = self
            .cashflow_count()?
            .checked_add(2)
            .and_then(|count| count.checked_mul(5))
            .ok_or(CashflowError::BoundExceeded(
                "ICMA yield risk cash evaluations",
            ))?;
        if input.max_cash_evaluations == 0
            || input.max_cash_evaluations > MAX_ICMA_RISK_CASH_EVALUATIONS
            || reserved_cash_evaluations > input.max_cash_evaluations
        {
            return Err(CashflowError::BoundExceeded("ICMA yield risk cash evaluations").into());
        }
        let mut valuations = Vec::with_capacity(5);
        let mut failure = None;
        let result = q::reference_quote_sensitivity(
            input.key.clone(),
            self.position.clone(),
            input.snapshot_id.clone(),
            input.settings.clone(),
            |delta| {
                let price = (|| {
                    let quotation = self.quote_price(rate + delta)?;
                    let WorkoutValue::Price(value) = &quotation.quotation else {
                        return Err(invalid("ICMA yield risk", "original full price required"));
                    };
                    let pv = value.present_value;
                    valuations.push(IcmaYieldRiskValuation {
                        decimal_move: delta,
                        native_quote: if delta == 0.0 {
                            input.observed_quote
                        } else {
                            input.observed_quote.model_rate_quote(rate + delta)?
                        },
                        quotation,
                    });
                    Ok(pv)
                })();
                price.map_err(|error: WorkflowError| {
                    let reason = error.to_string();
                    failure = Some(error);
                    q::QuoteRiskError::Invalid {
                        field: "original ICMA price".into(),
                        reason,
                    }
                })
            },
        );
        if let Some(error) = failure {
            return Err(error);
        }
        let sensitivity = result?;
        Ok(IcmaYieldSensitivity {
            input: input.clone(),
            valuations: valuations
                .try_into()
                .map_err(|_| invalid("ICMA yield risk", "five original prices required"))?,
            sensitivity,
            reserved_reference_evaluations: self.reserved_reference_evaluations,
            reserved_cash_evaluations,
        })
    }
    /// Compute a full positive reference-size yield column for the existing
    /// complete-factor hedge owner. Signed current/pending/trade quantities and
    /// same-snapshot reporting FX remain explicit inputs to that owner.
    pub fn hedge_yield_risk(
        &self,
        input: &IcmaYieldRiskInput,
        identity: &IcmaHedgeIdentity,
    ) -> WorkflowResult<IcmaHedgeRisk> {
        if self.position.quantity != self.position.reference_quantity {
            return Err(invalid(
                "ICMA hedge reference",
                "positive original-face reference position required before hedge quantity scaling",
            ));
        }
        let yield_risk = self.yield_sensitivity(input)?;
        let vector = h::HedgeRiskVector {
            id: identity.id.clone(),
            product: h::HedgeProduct::CashBond,
            instrument: convex_credit::ReferencePin {
                id: self.sources.security.id.clone(),
                version: self.sources.security.version,
            },
            economics: identity.economics.clone(),
            calculation: identity.calculation.clone(),
            entity: identity.entity.clone(),
            settlement: self.cash.settlement(),
            credit_scope: Vec::new(),
            reference_position: self.position.clone(),
            sensitivities: vec![yield_risk.sensitivity.clone()],
            fx: None,
        };
        Ok(IcmaHedgeRisk {
            identity: identity.clone(),
            yield_risk,
            vector,
        })
    }
}
