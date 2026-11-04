//! Memory conformance adapter and bounded full-term file import.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::RwLock;

use async_trait::async_trait;
use convex_core::contracts::{RequestId, UtcTimestamp, Version};
use convex_core::ids::InstrumentId;
use convex_ports::reference_data::BondReferenceData;
use convex_ports::reference_master::*;
use convex_ports::reference_repository::*;
use convex_ports::storage::{BondFilter, Page, Pagination};

#[derive(Default)]
struct ReferenceState {
    current: BTreeMap<String, ReferenceRecord>,
    history: BTreeMap<(String, Version), ReferenceRecord>,
    commits: BTreeMap<RequestId, CommittedReferenceCommand>,
    master_current: BTreeMap<MasterKey, MasterRecord>,
    master_history: BTreeMap<(MasterKey, Version), MasterRecord>,
}

/// Volatile reference adapter for tests and explicit embedded demonstrations.
/// A single lock makes domain transactions atomic; it provides no crash durability.
#[derive(Default)]
pub struct MemoryReferenceRepository {
    state: RwLock<ReferenceState>,
}

#[async_trait]
impl ReferenceRepository for MemoryReferenceRepository {
    async fn get(&self, id: &InstrumentId) -> Result<Option<ReferenceRecord>, ReferenceError> {
        validate_reference_id(id)?;
        Ok(self
            .state
            .read()
            .map_err(|_| ReferenceError::Unavailable)?
            .current
            .get(id.as_str())
            .cloned())
    }

    async fn get_version(
        &self,
        id: &InstrumentId,
        version: Version,
    ) -> Result<Option<ReferenceRecord>, ReferenceError> {
        validate_reference_id(id)?;
        Ok(self
            .state
            .read()
            .map_err(|_| ReferenceError::Unavailable)?
            .history
            .get(&(id.as_str().to_owned(), version))
            .cloned())
    }

    async fn list(
        &self,
        filter: &BondFilter,
        pagination: &Pagination,
    ) -> Result<Page<ReferenceRecord>, ReferenceError> {
        let state = self.state.read().map_err(|_| ReferenceError::Unavailable)?;
        reference_page(state.current.values().cloned(), filter, pagination)
    }

    async fn resolve(
        &self,
        identifier: &ExternalIdentifier,
    ) -> Result<Option<ReferenceRecord>, ReferenceError> {
        let state = self.state.read().map_err(|_| ReferenceError::Unavailable)?;
        resolve_reference(state.current.values().cloned(), identifier)
    }

    async fn commit(
        &self,
        command: &ReferenceCommand,
        recorded_at: UtcTimestamp,
    ) -> Result<ReferenceCommit, ReferenceError> {
        let mut state = self
            .state
            .write()
            .map_err(|_| ReferenceError::Unavailable)?;
        if let Some(previous) = state.commits.get(&command.idempotency_key) {
            return if previous.command.identical_payload(command)? {
                Ok(previous.result.clone())
            } else {
                Err(ReferenceError::Conflict(
                    "idempotency key was used with a different payload",
                ))
            };
        }
        let mut result = plan_reference_commit(command, recorded_at, |id| {
            Ok(state.current.get(id.as_str()).cloned())
        })?;
        result.master_records = plan_master_changes(&command.master_changes, recorded_at, |key| {
            Ok(state.master_current.get(key).cloned())
        })?;
        validate_reference_links(
            &result,
            |id| Ok(state.current.get(id.as_str()).cloned()),
            |key, version| Ok(state.master_history.get(&(key.clone(), version)).cloned()),
            |id, version| {
                Ok(state
                    .history
                    .get(&(id.as_str().to_owned(), version))
                    .cloned())
            },
        )?;
        let creates = result
            .records
            .iter()
            .filter(|record| !state.current.contains_key(record.instrument_id.as_str()))
            .count();
        let master_creates = result
            .master_records
            .iter()
            .filter(|r| !state.master_current.contains_key(&r.definition.key()))
            .count();
        if state.current.len() + state.master_current.len() + creates + master_creates
            > MAX_REFERENCE_RECORDS
            || state.commits.len() >= MAX_REFERENCE_COMMITS
            || state.history.len()
                + state.master_history.len()
                + result.records.len()
                + result.master_records.len()
                > MAX_REFERENCE_VERSIONS
        {
            return Err(ReferenceError::CapacityExceeded);
        }
        for record in &result.master_records {
            let key = record.definition.key();
            state.master_current.insert(key.clone(), record.clone());
            state
                .master_history
                .insert((key, record.version), record.clone());
        }
        // No fallible operations remain after preflight. Readers cannot observe a prefix.
        for record in &result.records {
            state
                .current
                .insert(record.instrument_id.as_str().to_owned(), record.clone());
            state.history.insert(
                (record.instrument_id.as_str().to_owned(), record.version),
                record.clone(),
            );
        }
        state.commits.insert(
            command.idempotency_key.clone(),
            CommittedReferenceCommand {
                command: command.clone(),
                result: result.clone(),
            },
        );
        Ok(result)
    }

    async fn get_commit(&self, key: &RequestId) -> Result<Option<ReferenceCommit>, ReferenceError> {
        Ok(self
            .state
            .read()
            .map_err(|_| ReferenceError::Unavailable)?
            .commits
            .get(key)
            .map(|entry| entry.result.clone()))
    }

    async fn get_at(
        &self,
        id: &InstrumentId,
        lookup: ReferenceLookup,
    ) -> Result<Option<ReferenceRecord>, ReferenceError> {
        validate_reference_id(id)?;
        let state = self.state.read().map_err(|_| ReferenceError::Unavailable)?;
        select_bond_at(
            state
                .history
                .range(
                    (
                        id.as_str().to_owned(),
                        Version::new(1).map_err(|_| ReferenceError::Unavailable)?,
                    )
                        ..=(
                            id.as_str().to_owned(),
                            Version::new(u64::MAX).map_err(|_| ReferenceError::Unavailable)?,
                        ),
                )
                .map(|(_, r)| r.clone()),
            lookup,
        )
    }

    async fn get_master(&self, key: &MasterKey) -> Result<Option<MasterRecord>, ReferenceError> {
        Ok(self
            .state
            .read()
            .map_err(|_| ReferenceError::Unavailable)?
            .master_current
            .get(key)
            .cloned())
    }

    async fn list_books(
        &self,
        after: Option<&BookId>,
        limit: usize,
    ) -> Result<Vec<MasterRecord>, ReferenceError> {
        if limit == 0 || limit > MAX_REFERENCE_PAGE {
            return Err(ReferenceError::InvalidInput("invalid book page size"));
        }
        let state = self.state.read().map_err(|_| ReferenceError::Unavailable)?;
        let books: Vec<&MasterRecord> = state
            .master_current
            .iter()
            .filter(|(key, _)| matches!(key, MasterKey::Book(id) if after.is_none_or(|a| id > a)))
            .map(|(_, record)| record)
            .collect();
        Ok(books.into_iter().take(limit).cloned().collect())
    }

    async fn list_masters(
        &self,
        kind: MasterKind,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MasterRecord>, ReferenceError> {
        if limit == 0 || limit > MAX_REFERENCE_PAGE {
            return Err(ReferenceError::InvalidInput("invalid master page size"));
        }
        let state = self.state.read().map_err(|_| ReferenceError::Unavailable)?;
        let cursor = after.map(|id| format!("{}/{id}", kind.prefix()));
        let mut records: Vec<(String, &MasterRecord)> = state
            .master_current
            .iter()
            .filter(|(key, _)| key.kind() == kind)
            .map(|(key, record)| (key.storage_key(), record))
            .filter(|(key, _)| cursor.as_ref().is_none_or(|c| key > c))
            .collect();
        records.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(records
            .into_iter()
            .take(limit)
            .map(|(_, record)| record.clone())
            .collect())
    }

    async fn get_master_version(
        &self,
        key: &MasterKey,
        version: Version,
    ) -> Result<Option<MasterRecord>, ReferenceError> {
        Ok(self
            .state
            .read()
            .map_err(|_| ReferenceError::Unavailable)?
            .master_history
            .get(&(key.clone(), version))
            .cloned())
    }

    async fn get_master_at(
        &self,
        key: &MasterKey,
        lookup: ReferenceLookup,
    ) -> Result<Option<MasterRecord>, ReferenceError> {
        let state = self.state.read().map_err(|_| ReferenceError::Unavailable)?;
        select_master_at(
            state
                .master_history
                .range(
                    (
                        key.clone(),
                        Version::new(1).map_err(|_| ReferenceError::Unavailable)?,
                    )
                        ..=(
                            key.clone(),
                            Version::new(u64::MAX).map_err(|_| ReferenceError::Unavailable)?,
                        ),
                )
                .map(|(_, r)| r.clone()),
            lookup,
        )
    }

    async fn resolve_at(
        &self,
        identifier: &ExternalIdentifier,
        lookup: ReferenceLookup,
    ) -> Result<Option<IdentifierResolution>, ReferenceError> {
        let state = self.state.read().map_err(|_| ReferenceError::Unavailable)?;
        let Some(mapping) =
            select_mapping_at(state.master_history.values().cloned(), identifier, lookup)?
        else {
            return Ok(None);
        };
        let MasterDefinition::Identifier(definition) = &mapping.definition else {
            return Err(ReferenceError::InvalidInput("not a mapping"));
        };
        let target = definition.target.clone().ok_or(ReferenceError::NotFound)?;
        let target_version = match &target {
            ReferenceTarget::Futures { instrument_id } => {
                select_master_at(
                    state
                        .master_history
                        .iter()
                        .filter(|((key, _), _)| *key == MasterKey::Futures(instrument_id.clone()))
                        .map(|(_, r)| r.clone()),
                    lookup,
                )?
                .ok_or(ReferenceError::NotFound)?
                .version
            }
            ReferenceTarget::Instrument { instrument_id } => {
                let record = select_bond_at(
                    state
                        .history
                        .iter()
                        .filter(|((id, _), _)| id == instrument_id.as_str())
                        .map(|(_, r)| r.clone()),
                    lookup,
                )?
                .ok_or(ReferenceError::NotFound)?;
                validate_bond_reference(record.bond.as_ref().ok_or(ReferenceError::NotFound)?)?;
                record.version
            }
            ReferenceTarget::Entity { entity_id } => {
                select_master_at(
                    state
                        .master_history
                        .iter()
                        .filter(|((key, _), _)| *key == MasterKey::Entity(entity_id.clone()))
                        .map(|(_, r)| r.clone()),
                    lookup,
                )?
                .ok_or(ReferenceError::NotFound)?
                .version
            }
        };
        Ok(Some(IdentifierResolution {
            mapping,
            target,
            target_version,
        }))
    }
}

/// Load a complete JSON array of bond terms for an explicit atomic repository import.
/// Missing fields, malformed files and absent paths fail; no dates/currency/coupon are invented.
pub fn load_bond_references(
    path: impl AsRef<Path>,
) -> Result<Vec<BondReferenceData>, ReferenceError> {
    let bytes = crate::reference_import::read_reference_file(path.as_ref())?;
    let bonds: Vec<BondReferenceData> = serde_json::from_slice(&bytes)
        .map_err(|_| ReferenceError::InvalidInput("expected complete bond reference JSON array"))?;
    if bonds.is_empty() || bonds.len() > MAX_REFERENCE_CHANGES {
        return Err(ReferenceError::InvalidInput(
            "expected 1..256 import records",
        ));
    }
    for bond in &bonds {
        validate_bond_reference(bond)?;
    }
    Ok(bonds)
}
