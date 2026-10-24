//! Issuer coverage through the existing atomic versioned reference port.
#![forbid(unsafe_code)]
use crate::reference_master::*;
use crate::reference_repository::ReferenceError;
use convex_core::contracts::{IssuerDirectory, ReferenceDataId, UtcTimestamp};
use serde::{Deserialize, Serialize};

/// Versioned provider-neutral legal issuer coverage and retained default notices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssuerScopeDefinition {
    /// Stable directory identity; repository owns its version.
    pub id: ReferenceDataId,
    /// Complete explicit security-to-entity directory.
    pub directory: IssuerDirectory,
    /// Economic applicability of this revision.
    pub effective: EffectiveInterval,
    /// Original permitted source evidence.
    pub provenance: ReferenceProvenance,
}
impl IssuerScopeDefinition {
    /// Bounded structural and source-time admission; no pricing or recovery estimate.
    pub fn validate(&self, recorded_at: Option<UtcTimestamp>) -> Result<(), ReferenceError> {
        self.directory
            .validate()
            .map_err(|_| ReferenceError::InvalidInput("invalid issuer directory"))?;
        if self
            .directory
            .issuers
            .iter()
            .filter_map(|s| s.default.as_ref())
            .any(|n| {
                n.effective_date.date().as_naive_date() > n.known_at.datetime().date_naive()
                    || recorded_at.is_some_and(|t| n.known_at > t)
            })
        {
            return Err(ReferenceError::InvalidInput(
                "issuer default source is future known/effective",
            ));
        }
        Ok(())
    }
}

/// Explicit managed-core reference selection, separate from native market inputs.
pub fn issuer_scope_pin(value: &serde_json::Value) -> Result<Option<ReferencePin>, ReferenceError> {
    value
        .get("issuer_scope")
        .map(|v| {
            serde_json::from_value(v.clone())
                .map_err(|_| ReferenceError::InvalidInput("invalid exact issuer scope pin"))
        })
        .transpose()
}
