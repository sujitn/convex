//! Issuer coverage admission over original full-state risk and optimizer owners.
use super::*;
use convex_core::contracts::{IssuerDirectory, SnapshotId};
use convex_credit::ReferencePin;
use serde::{Deserialize, Serialize};

/// Exact captured issuer reference, bound to the same native risk snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IssuerRiskScope {
    /// Immutable reference-directory revision.
    pub reference: ReferencePin,
    /// The complete native risk information set, not the later hedge-result key.
    pub snapshot_id: SnapshotId,
    /// Supplied exact mappings and retained actual default notices.
    pub directory: IssuerDirectory,
}

/// Admit complete explicit issuer coverage, then reuse the original lifecycle solve.
pub fn solve_issuer_lifecycle_hedge_proposal(
    problem: &HedgeProblem,
    states: &[LifecycleHoldingRisk],
    scope: &IssuerRiskScope,
) -> Result<HedgeSolveReport, HedgeError> {
    if problem.factors.is_empty()
        || problem.factors.len() > 16
        || problem.candidates.len() > 4
        || problem.holdings.len() > 256
        || problem.expected_holdings.len() > 256
        || states.len() > problem.holdings.len()
    {
        return Err(HedgeError::Bound(
            "1..16 factors, at most4 candidates and256 complete holdings/states",
        ));
    }
    scope
        .directory
        .validate()
        .map_err(|e| invalid("issuer scope", e.to_string()))?;
    if scope.snapshot_id != problem.snapshot_id
        || scope
            .directory
            .issuers
            .iter()
            .filter_map(|s| s.default.as_ref())
            .any(|n| n.effective_date.date() > problem.settlement)
    {
        return Err(invalid(
            "issuer scope",
            "same native snapshot and no future effective default required",
        ));
    }
    let owner = |risk: &RiskVector| {
        let mapping = scope
            .directory
            .instruments
            .iter()
            .find(|m| m.id == risk.instrument.id);
        if !matches!(risk.product, HedgeProduct::CashBond | HedgeProduct::Cds) {
            return if mapping.is_none() {
                Ok(None)
            } else {
                Err(invalid(
                    "issuer scope",
                    "IRS/futures cannot be assigned an issuer credit default",
                ))
            };
        }
        let mapping = mapping.ok_or_else(|| {
            invalid(
                "issuer scope",
                "complete explicit bond/CDS security mapping required",
            )
        })?;
        if mapping.version != risk.instrument.version
            || risk
                .credit_scope
                .iter()
                .any(|c| c.entity_id != mapping.entity_id)
            || risk.product == HedgeProduct::Cds && risk.credit_scope.len() != 1
        {
            return Err(invalid(
                "issuer scope",
                "exact security revision and original CDS/legal issuer required",
            ));
        }
        scope
            .directory
            .issuers
            .iter()
            .find(|i| i.entity_id == mapping.entity_id)
            .map(Some)
            .ok_or_else(|| invalid("issuer scope", "mapped entity absent"))
    };
    for holding in &problem.holdings {
        let issuer = owner(&holding.risk)?;
        let state = states
            .iter()
            .find(|s| s.position.identity.id == holding.risk.id);
        match issuer {
            Some(i) if i.default.is_some() => {
                if state.is_none_or(|s| s.default_entity.as_ref() != Some(&i.entity_id)) {
                    return Err(invalid(
                        "issuer default",
                        "every affected bond/CDS holding needs its native actual default state",
                    ));
                }
            }
            _ if state.is_some_and(|s| s.default_entity.is_some()) => {
                return Err(invalid(
                    "issuer default",
                    "native default must match the captured issuer notice",
                ));
            }
            _ => {}
        }
    }
    for candidate in &problem.candidates {
        if owner(&candidate.risk)?.is_some_and(|i| i.default.is_some()) {
            return Err(invalid(
                "issuer default",
                "no fresh defaulted-issuer bond/CDS candidates permitted",
            ));
        }
    }
    solve_lifecycle_hedge_proposal(problem, states)
}
