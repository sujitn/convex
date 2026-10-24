//! Authoritative, versioned reference transactions shared by API and pricing.
//!
//! A successful commit atomically retains the current records, immutable history,
//! change receipt and idempotency result. Implementations must not acknowledge a
//! partial mutation. The receipt is the reference event/audit record; transport
//! delivery and the financial ledger are separate contracts.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use async_trait::async_trait;
use convex_core::contracts::{RequestId, SchemaVersion, UtcTimestamp, Version};
use convex_core::ids::InstrumentId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::reference_data::BondReferenceData;
use crate::reference_master::*;
use crate::storage::{BondFilter, Page, Pagination};

/// Maximum changes in one atomic reference transaction.
pub const MAX_REFERENCE_CHANGES: usize = 256;
/// Maximum reference records in the initial local/test adapters, including tombstones.
pub const MAX_REFERENCE_RECORDS: usize = 100_000;
/// Maximum retained reference commits before capacity must be provisioned explicitly.
pub const MAX_REFERENCE_COMMITS: usize = 100_000;
/// Maximum retained immutable versions in the initial adapters.
pub const MAX_REFERENCE_VERSIONS: usize = 1_000_000;
/// Maximum page size for reference queries.
pub const MAX_REFERENCE_PAGE: usize = 1_000;

/// Transaction outcomes independent of database or transport error types.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReferenceError {
    /// Invalid identity, terms, command or query.
    #[error("invalid reference input: {0}")]
    InvalidInput(&'static str),
    /// Expected version, create precondition or idempotency payload conflict.
    #[error("reference conflict: {0}")]
    Conflict(&'static str),
    /// No active record for an update or deletion.
    #[error("reference not found")]
    NotFound,
    /// An external identifier resolves to more than one active instrument.
    #[error("ambiguous reference identifier")]
    AmbiguousIdentifier,
    /// A declared resource bound has been reached; nothing was committed.
    #[error("reference repository capacity exceeded")]
    CapacityExceeded,
    /// Read or transaction preparation is unavailable; nothing was acknowledged.
    #[error("reference repository unavailable")]
    Unavailable,
    /// Commit acknowledgment failed; inspect the idempotency key before retrying.
    #[error("reference commit outcome unknown; inspect the idempotency key")]
    UnknownOutcome,
    /// Required storage capability/schema is not supported.
    #[error("unsupported reference repository: {0}")]
    Unsupported(&'static str),
}

/// An immutable version. Deletion retains a tombstone and never resets identity/version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceRecord {
    /// Persisted contract version.
    pub schema_version: SchemaVersion,
    /// Internal instrument identity, independent of a provider symbol.
    pub instrument_id: InstrumentId,
    /// Monotonic positive version including deletions.
    pub version: Version,
    /// Time of the committed reference event, supplied by the application.
    pub recorded_at: UtcTimestamp,
    /// Full terms, or absent for a deletion tombstone.
    pub bond: Option<BondReferenceData>,
}

/// A single compare-and-set operation. There is no unconditional upsert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReferenceChange {
    /// Create an identity that has never been used, including a deleted identity.
    Create {
        /// Complete supplied bond terms.
        bond: Box<BondReferenceData>,
    },
    /// Replace full terms, retaining the prior version in history.
    Update {
        /// Complete replacement terms with the same internal identity.
        bond: Box<BondReferenceData>,
        /// Required current version.
        expected_version: Version,
    },
    /// Delete the active record by appending a new tombstone version.
    Delete {
        /// Internal identity.
        instrument_id: InstrumentId,
        /// Required current version.
        expected_version: Version,
    },
}

impl ReferenceChange {
    /// Identity addressed by this operation.
    pub fn instrument_id(&self) -> &InstrumentId {
        match self {
            Self::Create { bond } | Self::Update { bond, .. } => &bond.instrument_id,
            Self::Delete { instrument_id, .. } => instrument_id,
        }
    }
}

/// Bounded atomic mutation. Payload identity includes operation order and expected versions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceCommand {
    /// Repository-wide unique key; reuse with different changes must conflict.
    pub idempotency_key: RequestId,
    /// All changes commit together, or none commit.
    pub changes: Vec<ReferenceChange>,
    /// Entity, calendar, convention and mapping changes in the same transaction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub master_changes: Vec<MasterChange>,
}

impl ReferenceCommand {
    /// Compare the normalized typed wire payload, including exact decimal scale.
    /// Transport whitespace/key order and application commit time are excluded.
    pub fn identical_payload(&self, other: &Self) -> Result<bool, ReferenceError> {
        let encode = |command: &Self| {
            serde_json::to_vec(command)
                .map_err(|_| ReferenceError::InvalidInput("reference serialization failed"))
        };
        Ok(encode(self)? == encode(other)?)
    }

    /// Check the command before any external write or allocation of versions.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        let total = self.changes.len().saturating_add(self.master_changes.len());
        if total == 0 || total > MAX_REFERENCE_CHANGES {
            return Err(ReferenceError::InvalidInput("expected 1..256 changes"));
        }
        let mut ids = BTreeSet::new();
        for change in &self.changes {
            validate_reference_id(change.instrument_id())?;
            if !ids.insert(change.instrument_id().as_str()) {
                return Err(ReferenceError::InvalidInput(
                    "duplicate instrument in command",
                ));
            }
            if let ReferenceChange::Create { bond } | ReferenceChange::Update { bond, .. } = change
            {
                validate_bond_reference(bond)?;
            }
        }
        let mut keys = BTreeSet::new();
        for change in &self.master_changes {
            change.definition.validate()?;
            if !keys.insert(change.definition.key()) {
                return Err(ReferenceError::InvalidInput(
                    "duplicate master key in command",
                ));
            }
        }
        Ok(())
    }
}

/// Exact committed result, also retained as the immutable reference event/audit record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceCommit {
    /// Original command identity.
    pub idempotency_key: RequestId,
    /// Original committed application timestamp, unchanged on retries.
    pub recorded_at: UtcTimestamp,
    /// Full newly committed versions in command order.
    pub records: Vec<ReferenceRecord>,
    /// Newly committed resource definitions in command order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub master_records: Vec<MasterRecord>,
}

/// Stored deduplication record, written atomically with reference state and history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommittedReferenceCommand {
    /// Payload against which retries compare normalized typed wire content, including decimal scale.
    pub command: ReferenceCommand,
    /// Original response, retrievable after a lost acknowledgment.
    pub result: ReferenceCommit,
}

/// The single authoritative reference port. No database handle or query language escapes it.
#[async_trait]
pub trait ReferenceRepository: Send + Sync {
    /// Get current record, including a deletion tombstone. Never return a different version.
    async fn get(&self, id: &InstrumentId) -> Result<Option<ReferenceRecord>, ReferenceError>;
    /// Read one exact immutable version, including deletion versions.
    async fn get_version(
        &self,
        id: &InstrumentId,
        version: Version,
    ) -> Result<Option<ReferenceRecord>, ReferenceError>;
    /// List active records by internal ID, with filtered count from the same read snapshot.
    async fn list(
        &self,
        filter: &BondFilter,
        pagination: &Pagination,
    ) -> Result<Page<ReferenceRecord>, ReferenceError>;
    /// Resolve a current identifier mapping, refusing ambiguous results.
    async fn resolve(
        &self,
        identifier: &ExternalIdentifier,
    ) -> Result<Option<ReferenceRecord>, ReferenceError>;
    /// Commit current state, immutable versions and command/receipt together.
    /// Identical retries return the original receipt even after subsequent mutations.
    async fn commit(
        &self,
        command: &ReferenceCommand,
        recorded_at: UtcTimestamp,
    ) -> Result<ReferenceCommit, ReferenceError>;
    /// Recover a response by idempotency key after an unknown outcome or lost reply.
    async fn get_commit(&self, key: &RequestId) -> Result<Option<ReferenceCommit>, ReferenceError>;
    /// Read a bond at supplied economic time and information cutoff, never current content.
    async fn get_at(
        &self,
        id: &InstrumentId,
        lookup: ReferenceLookup,
    ) -> Result<Option<ReferenceRecord>, ReferenceError>;
    /// Read current master resource for operator inspection and compare-and-set preparation.
    async fn get_master(&self, key: &MasterKey) -> Result<Option<MasterRecord>, ReferenceError>;
    /// Read one exact immutable master resource version.
    async fn get_master_version(
        &self,
        key: &MasterKey,
        version: Version,
    ) -> Result<Option<MasterRecord>, ReferenceError>;
    /// Read resource history at supplied effective/known-at time.
    async fn get_master_at(
        &self,
        key: &MasterKey,
        lookup: ReferenceLookup,
    ) -> Result<Option<MasterRecord>, ReferenceError>;
    /// Current book records ordered by book ID, strictly after the cursor, at most `limit`.
    /// Backends without book support refuse rather than return an empty page.
    async fn list_books(
        &self,
        _after: Option<&BookId>,
        _limit: usize,
    ) -> Result<Vec<MasterRecord>, ReferenceError> {
        Err(ReferenceError::Unsupported("book listing"))
    }
    /// Current master records of one kind ordered by identity, strictly after the `after`
    /// identity, at most `limit`. Backends without master listing refuse.
    async fn list_masters(
        &self,
        _kind: MasterKind,
        _after: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<MasterRecord>, ReferenceError> {
        Err(ReferenceError::Unsupported("master listing"))
    }
    /// Resolve historical mapping assertions from one read snapshot. Ambiguity is never selected away.
    async fn resolve_at(
        &self,
        identifier: &ExternalIdentifier,
        lookup: ReferenceLookup,
    ) -> Result<Option<IdentifierResolution>, ReferenceError>;

    /// Resolve exact pinned resources and reject incomplete/unsupported service terms.
    /// All consumed versions are immutable and cutoff checked; no latest-resource substitution.
    async fn for_valuation(
        &self,
        id: &InstrumentId,
        lookup: ReferenceLookup,
    ) -> Result<Option<ResolvedBondReference>, ReferenceError> {
        let Some(record) = self.get_at(id, lookup).await? else {
            return Ok(None);
        };
        let bond = record.bond.as_ref().ok_or(ReferenceError::NotFound)?;
        let (issuer, calendar, convention) = self.resources_for_valuation(bond, lookup).await?;
        let floating = self.floating_resources_for_valuation(bond).await?;
        let resolved = ResolvedBondReference {
            record,
            issuer,
            calendar,
            convention,
            floating,
        };
        validate_resolved_bond(&resolved, lookup)?;
        Ok(Some(resolved))
    }

    /// Resolve supplied inline terms without claiming a stored instrument version.
    async fn resources_for_valuation(
        &self,
        bond: &BondReferenceData,
        lookup: ReferenceLookup,
    ) -> Result<(MasterRecord, MasterRecord, MasterRecord), ReferenceError> {
        validate_bond_reference(bond)?;
        let terms = bond
            .contractual_terms
            .as_ref()
            .ok_or(ReferenceError::InvalidInput("missing contractual terms"))?;
        let issuer = self
            .get_master_version(
                &MasterKey::Entity(terms.issuer.entity_id.clone()),
                terms.issuer.version,
            )
            .await?
            .ok_or(ReferenceError::InvalidInput("missing pinned issuer"))?;
        let calendar = self
            .get_master_version(
                &MasterKey::Calendar(terms.calendar.id.clone()),
                terms.calendar.version,
            )
            .await?
            .ok_or(ReferenceError::InvalidInput("missing pinned calendar"))?;
        let convention = self
            .get_master_version(
                &MasterKey::Convention(terms.convention.id.clone()),
                terms.convention.version,
            )
            .await?
            .ok_or(ReferenceError::InvalidInput("missing pinned convention"))?;
        validate_service_resources(bond, &issuer, &calendar, &convention, lookup)?;
        Ok((issuer, calendar, convention))
    }

    /// Resolve a floating rate note's exact pinned rate index and index calendar; None for
    /// every other bond. The caller checks them with the rest of the resolved reference.
    async fn floating_resources_for_valuation(
        &self,
        bond: &BondReferenceData,
    ) -> Result<Option<ResolvedFloating>, ReferenceError> {
        if !crate::reference_master::is_valued_floating(bond) {
            return Ok(None);
        }
        let pin = bond
            .contractual_terms
            .as_ref()
            .and_then(|t| t.floating_mechanics.as_ref())
            .map(|m| m.index.clone())
            .ok_or(ReferenceError::InvalidInput("missing floating mechanics"))?;
        let index = self
            .get_master_version(&MasterKey::RateIndex(pin.id), pin.version)
            .await?
            .ok_or(ReferenceError::InvalidInput("missing pinned rate index"))?;
        let MasterDefinition::RateIndex(definition) = &index.definition else {
            return Err(ReferenceError::InvalidInput("wrong rate index resource"));
        };
        let calendar = self
            .get_master_version(
                &MasterKey::Calendar(definition.calendar.id.clone()),
                definition.calendar.version,
            )
            .await?
            .ok_or(ReferenceError::InvalidInput(
                "missing pinned index calendar",
            ))?;
        Ok(Some(ResolvedFloating { index, calendar }))
    }
}

/// Validate an existing public identity before accepting it at a repository boundary.
pub fn validate_reference_id(id: &InstrumentId) -> Result<(), ReferenceError> {
    convex_core::contracts::InstrumentId::new(id.as_str())
        .map(|_| ())
        .map_err(|_| ReferenceError::InvalidInput("invalid instrument identity"))
}

/// Check reference ingress without inventing economic terms or certifying a pricing model.
pub fn validate_bond_reference(bond: &BondReferenceData) -> Result<(), ReferenceError> {
    validate_valuation_terms(bond)?;
    bond.canonical_boundary()
        .map_err(|_| ReferenceError::InvalidInput("invalid bond identity/dates/face"))?;
    if bond.description.is_empty() || bond.day_count.is_empty() || bond.source.is_empty() {
        return Err(ReferenceError::InvalidInput(
            "required description/convention/source is absent",
        ));
    }
    if bond.bond_type != crate::reference_data::BondType::ZeroCoupon
        && ![1, 2, 4, 12].contains(&bond.frequency)
    {
        return Err(ReferenceError::InvalidInput(
            "unsupported payment frequency",
        ));
    }
    match bond.bond_type {
        crate::reference_data::BondType::FixedBullet
        | crate::reference_data::BondType::FixedCallable
        | crate::reference_data::BondType::FixedPutable
        | crate::reference_data::BondType::Amortizing
            if bond.coupon_rate.is_none() =>
        {
            return Err(ReferenceError::InvalidInput(
                "required fixed coupon is absent",
            ));
        }
        crate::reference_data::BondType::FloatingRate if bond.floating_terms.is_none() => {
            return Err(ReferenceError::InvalidInput(
                "required floating terms are absent",
            ));
        }
        _ => {}
    }
    let text = [
        &bond.description,
        &bond.issuer_id,
        &bond.issuer_name,
        &bond.day_count,
        &bond.seniority,
        &bond.country_of_risk,
        &bond.sector,
        &bond.source,
    ];
    if text
        .iter()
        .any(|value| value.len() > 1024 || value.chars().any(char::is_control))
        || [&bond.isin, &bond.cusip, &bond.sedol, &bond.bbgid]
            .iter()
            .any(|value| {
                value.as_ref().is_some_and(|s| {
                    s.is_empty() || s.len() > 128 || s.chars().any(char::is_control)
                })
            })
        || bond.call_schedule.len() > 1024
    {
        return Err(ReferenceError::InvalidInput(
            "reference field exceeds its bound",
        ));
    }
    for entry in &bond.call_schedule {
        convex_core::contracts::BusinessDate::try_from(entry.call_date)
            .map_err(|_| ReferenceError::InvalidInput("invalid call date"))?;
    }
    if let Some(date) = bond.first_coupon_date {
        convex_core::contracts::BusinessDate::try_from(date)
            .map_err(|_| ReferenceError::InvalidInput("invalid first coupon date"))?;
    }
    Ok(())
}

/// Deterministic compare-and-set planning shared by adapters, before any mutation.
pub fn plan_reference_commit(
    command: &ReferenceCommand,
    recorded_at: UtcTimestamp,
    mut current: impl FnMut(&InstrumentId) -> Result<Option<ReferenceRecord>, ReferenceError>,
) -> Result<ReferenceCommit, ReferenceError> {
    command.validate()?;
    let mut records = Vec::with_capacity(command.changes.len());
    for change in &command.changes {
        let previous = current(change.instrument_id())?;
        if previous
            .as_ref()
            .is_some_and(|record| recorded_at < record.recorded_at)
        {
            return Err(ReferenceError::InvalidInput(
                "recording time cannot move backwards",
            ));
        }
        let (version, bond) = match change {
            ReferenceChange::Create { bond } => {
                if previous.is_some() {
                    return Err(ReferenceError::Conflict(
                        "instrument already exists or was deleted",
                    ));
                }
                (1, Some(bond.as_ref().clone()))
            }
            ReferenceChange::Update {
                bond,
                expected_version,
            } => {
                let previous = previous
                    .filter(|record| record.bond.is_some())
                    .ok_or(ReferenceError::NotFound)?;
                if previous.version != *expected_version {
                    return Err(ReferenceError::Conflict(
                        "expected version differs from current version",
                    ));
                }
                (
                    previous
                        .version
                        .get()
                        .checked_add(1)
                        .ok_or(ReferenceError::CapacityExceeded)?,
                    Some(bond.as_ref().clone()),
                )
            }
            ReferenceChange::Delete {
                expected_version, ..
            } => {
                let previous = previous
                    .filter(|record| record.bond.is_some())
                    .ok_or(ReferenceError::NotFound)?;
                if previous.version != *expected_version {
                    return Err(ReferenceError::Conflict(
                        "expected version differs from current version",
                    ));
                }
                (
                    previous
                        .version
                        .get()
                        .checked_add(1)
                        .ok_or(ReferenceError::CapacityExceeded)?,
                    None,
                )
            }
        };
        records.push(ReferenceRecord {
            schema_version: SchemaVersion::V2,
            instrument_id: change.instrument_id().clone(),
            version: Version::new(version).map_err(|_| ReferenceError::CapacityExceeded)?,
            recorded_at,
            bond,
        });
    }
    Ok(ReferenceCommit {
        idempotency_key: command.idempotency_key.clone(),
        recorded_at,
        records,
        master_records: Vec::new(),
    })
}

/// Consistent query semantics shared by adapters, with a deterministic ID order.
pub fn reference_page(
    records: impl IntoIterator<Item = ReferenceRecord>,
    filter: &BondFilter,
    pagination: &Pagination,
) -> Result<Page<ReferenceRecord>, ReferenceError> {
    if pagination.limit == 0 || pagination.limit > MAX_REFERENCE_PAGE {
        return Err(ReferenceError::InvalidInput("expected page limit 1..1000"));
    }
    filter.validate().map_err(ReferenceError::InvalidInput)?;
    let bounded: Vec<_> = records
        .into_iter()
        .take(MAX_REFERENCE_RECORDS + 1)
        .collect();
    if bounded.len() > MAX_REFERENCE_RECORDS {
        return Err(ReferenceError::CapacityExceeded);
    }
    let mut matching: Vec<_> = bounded
        .into_iter()
        .filter(|record| {
            record
                .bond
                .as_ref()
                .is_some_and(|bond| filter.matches(bond))
        })
        .collect();
    if matching.len() > MAX_REFERENCE_RECORDS {
        return Err(ReferenceError::CapacityExceeded);
    }
    matching.sort_unstable_by(|a, b| a.instrument_id.as_str().cmp(b.instrument_id.as_str()));
    let total = matching.len() as u64;
    Ok(Page {
        items: matching
            .into_iter()
            .skip(pagination.offset)
            .take(pagination.limit)
            .collect(),
        total,
        offset: pagination.offset,
        limit: pagination.limit,
    })
}

/// Resolve against one consistent read snapshot. Never select an arbitrary supplier mapping.
pub fn resolve_reference(
    records: impl IntoIterator<Item = ReferenceRecord>,
    identifier: &ExternalIdentifier,
) -> Result<Option<ReferenceRecord>, ReferenceError> {
    identifier.validate()?;
    if !matches!(
        identifier,
        ExternalIdentifier::Isin { .. } | ExternalIdentifier::Cusip { .. }
    ) {
        return Err(ReferenceError::Unsupported(
            "current bond aliases support only ISIN/CUSIP; use resolve_at for master mappings",
        ));
    }
    let mut found = None;
    for (index, record) in records
        .into_iter()
        .take(MAX_REFERENCE_RECORDS + 1)
        .enumerate()
    {
        if index >= MAX_REFERENCE_RECORDS {
            return Err(ReferenceError::CapacityExceeded);
        }
        if record.bond.as_ref().is_some_and(|bond| match identifier {
            ExternalIdentifier::Isin { value } => bond.isin.as_ref() == Some(value),
            ExternalIdentifier::Cusip { value } => bond.cusip.as_ref() == Some(value),
            _ => false,
        }) {
            if found.is_some() {
                return Err(ReferenceError::AmbiguousIdentifier);
            }
            found = Some(record);
        }
    }
    Ok(found)
}
