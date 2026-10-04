//! Single-owner pricing recovery and publication contracts. No broker or database handles.
#![forbid(unsafe_code)]

use crate::{quote_event::QuoteEvent, TraitError};
use async_trait::async_trait;
use convex_core::contracts::{
    DataQuality, ResultEnvelope, SnapshotId, SourceId, UtcTimestamp, Version,
};
use serde::{Deserialize, Serialize};

/// Maximum retained replay events before explicit archival/compaction is required.
pub const MAX_RUNTIME_EVENTS: usize = 100_000;
/// Maximum serialized checkpoint size, 32 MiB.
pub const MAX_RUNTIME_BYTES: usize = 32 * 1024 * 1024;

/// Canonical result state published with a durable, monotonic runtime revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedValuation {
    /// Stable runtime identity; combined with revision this is the delivery deduplication key.
    pub runtime_id: SourceId,
    /// Durable monotonic publication revision, independent of process-local graph counters.
    pub revision: u64,
    /// Captured input generation.
    pub generation: Version,
    /// Complete context digest; the plan and immutable snapshot reconstruct the full context.
    pub context_key: String,
    /// Live consumption expires at this original dependency deadline; absent for historical contexts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_valid_until: Option<UtcTimestamp>,
    /// Current attempt, including explicit errors and input quality.
    pub result: ResultEnvelope,
    /// Original valid complete valuation, for display only after a failed refresh.
    pub last_good: Option<ResultEnvelope>,
    /// Stale when last_good belongs to a prior generation; never modifies its timestamp.
    pub retained_quality: Option<DataQuality>,
}

/// Bounded recovery image and outbox committed atomically for one local runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCheckpoint {
    /// Checkpoint schema independent of database file format.
    pub schema_version: u32,
    /// Stable owner identity.
    pub runtime_id: SourceId,
    /// Compare-and-set revision, zero means absent to the port.
    pub revision: u64,
    /// SHA-256 of the exact configured runtime plan.
    pub plan_digest: String,
    /// Exact accepted active selection, independent of later activations. Older checkpoints omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_activation: Option<crate::configuration::ActivationRecord>,
    /// Input generation; increments before accepted work is acknowledged.
    pub generation: Version,
    /// Accepted economic cutoff.
    pub as_of: UtcTimestamp,
    /// Accepted information cutoff.
    pub known_at: UtcTimestamp,
    /// Accepted normalized events in original delivery order, with exact decimal strings.
    pub events: Vec<QuoteEvent>,
    /// Last atomically published immutable input manifest.
    pub snapshot_id: Option<SnapshotId>,
    /// Accepted work still requires calculation and checkpointing.
    pub pending: bool,
    /// Last allocated publication revision.
    pub result_revision: u64,
    /// Current result states for at most 256 instruments.
    pub results: Vec<PublishedValuation>,
    /// Undelivered results. Publishers may see retries with the same deduplication key.
    pub outbox: Vec<PublishedValuation>,
    /// Live transport continuity lost; restart must not silently restore fresh readiness.
    pub delivery_lost: bool,
}
impl RuntimeCheckpoint {
    /// Structural storage bounds. Engine recovery additionally verifies the plan, snapshots and lineage.
    pub fn validate(&self) -> Result<(), TraitError> {
        if let Some(activation) = &self.configuration_activation {
            activation.validate()?;
            if activation.command.activated_at > self.known_at {
                return Err(TraitError::InvalidInput(
                    "runtime activation is later than accepted cutoff",
                ));
            }
        }
        if self.schema_version != 1
            || self.revision == 0
            || self.plan_digest.len() != 64
            || !self.plan_digest.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(TraitError::InvalidInput(
                "invalid runtime checkpoint schema/revision/digest",
            ));
        }
        if self.events.len() > MAX_RUNTIME_EVENTS
            || self.results.len() > 256
            || self.outbox.len() > 256
        {
            return Err(TraitError::CapacityExceeded);
        }
        let mut previous = None;
        for event in &self.events {
            event.validate()?;
            if previous.is_some_and(|v| v > event.receive_time_ms) {
                return Err(TraitError::InvalidInput("runtime delivery order regressed"));
            }
            previous = Some(event.receive_time_ms);
        }
        let mut subjects = std::collections::BTreeSet::new();
        for result in &self.results {
            if !subjects.insert(
                serde_json::to_string(&result.result.provenance().subject)
                    .map_err(|_| TraitError::InvalidInput("invalid runtime subject"))?,
            ) {
                return Err(TraitError::InvalidInput("duplicate runtime result subject"));
            }
        }
        for result in self.results.iter().chain(&self.outbox) {
            if result.runtime_id != self.runtime_id
                || result.revision == 0
                || result.revision > self.result_revision
                || result.generation > self.generation
                || result.context_key.len() != 64
                || !result.context_key.bytes().all(|c| c.is_ascii_hexdigit())
                || (!self.pending
                    && (result.generation != self.generation
                        || self.snapshot_id.as_ref()
                            != Some(&result.result.provenance().snapshot_id)))
            {
                return Err(TraitError::InvalidInput("invalid runtime result lineage"));
            }
            match (&result.last_good, result.retained_quality) {
                (None, None) => {}
                (Some(good), Some(quality))
                    if good.status() == convex_core::contracts::ResultStatus::Complete
                        && good.quality() == DataQuality::Valid
                        && good.provenance().subject == result.result.provenance().subject
                        && good.provenance().build_id == result.result.provenance().build_id
                        && good.provenance().known_at <= result.result.provenance().known_at
                        && quality
                            == if good == &result.result {
                                DataQuality::Valid
                            } else {
                                DataQuality::Stale
                            } => {}
                _ => {
                    return Err(TraitError::InvalidInput(
                        "invalid runtime last-good retention",
                    ))
                }
            }
        }
        for record in &self.outbox {
            if !self.results.contains(record) {
                return Err(TraitError::InvalidInput(
                    "outbox differs from checkpointed results",
                ));
            }
        }
        Ok(())
    }
}

/// Atomic single-runtime recovery image and outbox. Managed leases are a separate contract.
#[async_trait]
pub trait RuntimeStore: Send + Sync {
    /// Exact durable image or absent; corrupt/unsupported images fail rather than resetting.
    async fn load(&self, id: &SourceId) -> Result<Option<RuntimeCheckpoint>, TraitError>;
    /// Commit next revision iff expected matches. Identical already-committed retries succeed.
    /// Conflict must not mutate state. UnknownOutcome requires load/reconciliation before retry.
    async fn checkpoint(
        &self,
        expected_revision: u64,
        image: &RuntimeCheckpoint,
    ) -> Result<(), TraitError>;
}

/// Publish explicit complete/partial/failed states. At-least-once delivery requires consumer deduplication.
#[async_trait]
pub trait ResultPublisher: Send + Sync {
    /// Success declares the adapter's own acknowledgment boundary; it is not execution authorization.
    async fn publish(&self, result: &PublishedValuation) -> Result<(), TraitError>;
}
