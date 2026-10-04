//! Published fixing revisions; neither observation date nor receipt time implies publication.
#![forbid(unsafe_code)]

use async_trait::async_trait;
use convex_core::contracts::{BusinessDate, DecimalRate, IndexId, SourceId, UtcTimestamp, Version};
use serde::{Deserialize, Serialize};

use crate::{quote_event::validate_text, TraitError};

/// One explicitly selected source/index/date. Supplier precedence belongs to configuration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixingKey {
    /// Provider-neutral index identity.
    pub index_id: IndexId,
    /// Economic observation date.
    pub observation_date: BusinessDate,
    /// Explicit selected provider-neutral source.
    pub source: SourceId,
}

/// Immutable published rate fixing revision, schema 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedFixing {
    /// Must be 1.
    pub schema_version: u32,
    /// Observation and selected source.
    pub key: FixingKey,
    /// Revision within this key, independent of other sources.
    pub version: Version,
    /// Actual decimal rate; negative rates are permitted.
    pub rate: DecimalRate,
    /// Actual publication time of this revision.
    pub published_at: UtcTimestamp,
    /// Actual recording time of this revision.
    pub recorded_at: UtcTimestamp,
    /// Preliminary revisions are usable for labelled research, never execution data.
    pub is_preliminary: bool,
    /// Supplying adapter's consumption entitlement.
    pub entitled: bool,
    /// Permitted evidence hash/reference, never the raw provider payload.
    pub evidence_reference: String,
}

impl PublishedFixing {
    /// Validate the supplied chronology and bounded evidence, without a clock.
    pub fn validate(&self) -> Result<(), TraitError> {
        validate_text(&self.evidence_reference, 512)?;
        if self.schema_version != 1
            || self.recorded_at < self.published_at
            || self.key.observation_date.date().as_naive_date()
                > self.published_at.datetime().date_naive()
        {
            return Err(TraitError::InvalidInput("invalid fixing schema/chronology"));
        }
        Ok(())
    }
}

/// Select the latest eligible immutable revision from a bounded supplied history.
/// Both publication and recording must be known. No forecast or previous-date substitution.
pub fn select_fixing_at(
    history: &[PublishedFixing],
    key: &FixingKey,
    known_at: UtcTimestamp,
) -> Result<Option<PublishedFixing>, TraitError> {
    if history.len() > 100_000 {
        return Err(TraitError::CapacityExceeded);
    }
    let mut selected: Option<&PublishedFixing> = None;
    let mut revisions = std::collections::BTreeSet::new();
    for fixing in history.iter().filter(|fixing| &fixing.key == key) {
        fixing.validate()?;
        if !revisions.insert(fixing.version) {
            return Err(TraitError::Conflict("duplicate fixing revision"));
        }
        if fixing.published_at <= known_at
            && fixing.recorded_at <= known_at
            && selected.is_none_or(|prior| prior.version < fixing.version)
        {
            selected = Some(fixing);
        }
    }
    Ok(selected.cloned())
}

/// Immutable historical fixing lookup; a current-only source cannot implement this by guessing.
#[async_trait]
pub trait FixingRepository: Send + Sync {
    /// Resolve exactly this key using both publication/recording cutoffs.
    async fn get_at(
        &self,
        key: &FixingKey,
        known_at: UtcTimestamp,
    ) -> Result<Option<PublishedFixing>, TraitError>;
}
