//! Explicit legal issuer coverage and actual default notices; no parent contagion.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One actual issuer notice, scoped independently by each instrument/trade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IssuerDefaultNotice {
    /// Stable source event identity, retained across all affected trades.
    pub id: ReferenceDataId,
    /// Actual economic date; no inferred cross-default date.
    pub effective_date: BusinessDate,
    /// Actual source knowledge, independent of repository receipt time.
    pub known_at: UtcTimestamp,
    /// Immutable external evidence identity.
    pub reference_id: ReferenceDataId,
    /// Actual evidence revision.
    pub reference_version: Version,
}

/// Exact legal entity attributes and its retained actual default, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IssuerCreditState {
    /// Legal issuer/reference entity, never inferred from a parent or ticker.
    pub entity_id: EntityId,
    /// Exact reference entity attributes version.
    pub version: Version,
    /// None explicitly declares no actual default notice in this revision.
    pub default: Option<IssuerDefaultNotice>,
}

/// Exact native security revision and its legal issuer/reference entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IssuerInstrument {
    /// Same internal security identity as native risk, independent of trade IDs.
    pub id: ReferenceDataId,
    /// Exact supplied security/reference revision.
    pub version: Version,
    /// Actual legal issuer/reference entity.
    pub entity_id: EntityId,
}

/// Bounded complete credit-security directory; mappings survive reference revisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IssuerDirectory {
    /// One to sixteen explicit legal entities.
    pub issuers: Vec<IssuerCreditState>,
    /// One to 256 unique security identities, including the supplied hedge universe.
    pub instruments: Vec<IssuerInstrument>,
}
impl IssuerDirectory {
    /// Validate coverage identities and bounds without source lookup or a clock.
    pub fn validate(&self) -> Result<(), ContractError> {
        if !(1..=16).contains(&self.issuers.len()) || !(1..=256).contains(&self.instruments.len()) {
            return Err(ContractError::new(
                "issuer directory",
                "1..16 issuers and1..256 securities required",
            ));
        }
        let entities: BTreeSet<_> = self.issuers.iter().map(|s| &s.entity_id).collect();
        let securities: BTreeSet<_> = self.instruments.iter().map(|s| &s.id).collect();
        if entities.len() != self.issuers.len()
            || securities.len() != self.instruments.len()
            || self
                .instruments
                .iter()
                .any(|s| !entities.contains(&s.entity_id))
        {
            return Err(ContractError::new(
                "issuer directory",
                "unique explicit entities/security owners required",
            ));
        }
        Ok(())
    }
    /// No removed/reassigned security or cleared/rewritten actual notice on update.
    /// Explicit reference-version advances and new mappings remain possible.
    pub fn preserves(&self, previous: &Self) -> Result<(), ContractError> {
        self.validate()?;
        if previous.instruments.iter().any(|old| {
            !self.instruments.iter().any(|new| {
                new.id == old.id && new.entity_id == old.entity_id && new.version >= old.version
            })
        }) || previous.issuers.iter().any(|old| {
            !self.issuers.iter().any(|new| {
                new.entity_id == old.entity_id
                    && new.version >= old.version
                    && (old.default.is_none() || new.default == old.default)
            })
        }) {
            return Err(ContractError::new(
                "issuer directory",
                "prior mappings and actual default facts must remain",
            ));
        }
        Ok(())
    }
}
