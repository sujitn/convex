//! Memory conformance adapter and bounded full-term file import.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::RwLock;

use async_trait::async_trait;
use convex_core::contracts::{RequestId, UtcTimestamp, Version};
use convex_core::ids::InstrumentId;
use convex_ports::reference_data::BondReferenceData;
use convex_ports::reference_repository::*;
use convex_ports::storage::{BondFilter, Page, Pagination};

#[derive(Default)]
struct ReferenceState {
    current: BTreeMap<String, ReferenceRecord>,
    history: BTreeMap<(String, Version), ReferenceRecord>,
    commits: BTreeMap<RequestId, CommittedReferenceCommand>,
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
        identifier: &ReferenceIdentifier,
    ) -> Result<Option<ReferenceRecord>, ReferenceError> {
        let state = self.state.read().map_err(|_| ReferenceError::Unavailable)?;
        resolve_reference(state.current.values().cloned(), identifier)
    }

    async fn commit(
        &self,
        command: &ReferenceCommand,
        recorded_at: UtcTimestamp,
    ) -> Result<ReferenceCommit, ReferenceError> {
        command.validate()?;
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
        let result = plan_reference_commit(command, recorded_at, |id| {
            Ok(state.current.get(id.as_str()).cloned())
        })?;
        let creates = result
            .records
            .iter()
            .filter(|record| !state.current.contains_key(record.instrument_id.as_str()))
            .count();
        if state.current.len() + creates > MAX_REFERENCE_RECORDS
            || state.commits.len() >= MAX_REFERENCE_COMMITS
            || state.history.len() + result.records.len() > MAX_REFERENCE_VERSIONS
        {
            return Err(ReferenceError::CapacityExceeded);
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
}

/// Load a complete JSON array of bond terms for an explicit atomic repository import.
/// Missing fields, malformed files and absent paths fail; no dates/currency/coupon are invented.
pub fn load_bond_references(
    path: impl AsRef<Path>,
) -> Result<Vec<BondReferenceData>, ReferenceError> {
    const MAX_BYTES: u64 = 8 * 1024 * 1024;
    let file = std::fs::File::open(path).map_err(|_| ReferenceError::Unavailable)?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ReferenceError::Unavailable)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(ReferenceError::CapacityExceeded);
    }
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
