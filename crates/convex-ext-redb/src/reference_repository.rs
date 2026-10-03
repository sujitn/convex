//! Transactional local reference repository. redb is single-process storage.

use std::sync::Arc;

use async_trait::async_trait;
use convex_core::contracts::{RequestId, UtcTimestamp, Version};
use convex_core::ids::InstrumentId;
use convex_ports::reference_repository::*;
use convex_ports::storage::{BondFilter, Page, Pagination};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};

const CURRENT: TableDefinition<&str, &[u8]> = TableDefinition::new("reference_current_v2");
const HISTORY: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("reference_history_v2");
const COMMITS: TableDefinition<&str, &[u8]> = TableDefinition::new("reference_commits_v2");

/// redb adapter for atomic reference versions and retry recovery.
pub struct RedbReferenceRepository {
    db: Arc<Database>,
}

impl RedbReferenceRepository {
    /// Open the versioned schema, refusing to silently discard pre-versioned bond records.
    pub fn new(db: Arc<Database>) -> Result<Self, ReferenceError> {
        let read = db.begin_read().map_err(|_| ReferenceError::Unavailable)?;
        match read.open_table(TableDefinition::<&str, &[u8]>::new("bonds")) {
            Ok(table) if table.len().map_err(|_| ReferenceError::Unavailable)? > 0 => {
                return Err(ReferenceError::Unsupported(
                    "unversioned bonds require explicit export and reference import",
                ));
            }
            Ok(_) | Err(redb::TableError::TableDoesNotExist(_)) => {}
            Err(_) => return Err(ReferenceError::Unavailable),
        }
        drop(read);
        let mut write = db.begin_write().map_err(|_| ReferenceError::Unavailable)?;
        write
            .set_durability(redb::Durability::Immediate)
            .map_err(|_| ReferenceError::Unavailable)?;
        write
            .open_table(CURRENT)
            .map_err(|_| ReferenceError::Unavailable)?;
        write
            .open_table(HISTORY)
            .map_err(|_| ReferenceError::Unavailable)?;
        write
            .open_table(COMMITS)
            .map_err(|_| ReferenceError::Unavailable)?;
        write.commit().map_err(|_| ReferenceError::UnknownOutcome)?;
        Ok(Self { db })
    }

    fn current_records(&self) -> Result<Vec<ReferenceRecord>, ReferenceError> {
        let read = self
            .db
            .begin_read()
            .map_err(|_| ReferenceError::Unavailable)?;
        let table = read
            .open_table(CURRENT)
            .map_err(|_| ReferenceError::Unavailable)?;
        if table.len().map_err(|_| ReferenceError::Unavailable)? > MAX_REFERENCE_RECORDS as u64 {
            return Err(ReferenceError::CapacityExceeded);
        }
        table
            .iter()
            .map_err(|_| ReferenceError::Unavailable)?
            .map(|entry| {
                let (_, value) = entry.map_err(|_| ReferenceError::Unavailable)?;
                decode(value.value())
            })
            .collect()
    }
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ReferenceError> {
    serde_json::from_slice(bytes)
        .map_err(|_| ReferenceError::Unsupported("invalid persisted reference schema"))
}

#[async_trait]
impl ReferenceRepository for RedbReferenceRepository {
    async fn get(&self, id: &InstrumentId) -> Result<Option<ReferenceRecord>, ReferenceError> {
        validate_reference_id(id)?;
        let read = self
            .db
            .begin_read()
            .map_err(|_| ReferenceError::Unavailable)?;
        let table = read
            .open_table(CURRENT)
            .map_err(|_| ReferenceError::Unavailable)?;
        table
            .get(id.as_str())
            .map_err(|_| ReferenceError::Unavailable)?
            .map(|value| decode(value.value()))
            .transpose()
    }

    async fn get_version(
        &self,
        id: &InstrumentId,
        version: Version,
    ) -> Result<Option<ReferenceRecord>, ReferenceError> {
        validate_reference_id(id)?;
        let read = self
            .db
            .begin_read()
            .map_err(|_| ReferenceError::Unavailable)?;
        let table = read
            .open_table(HISTORY)
            .map_err(|_| ReferenceError::Unavailable)?;
        table
            .get((id.as_str(), version.get()))
            .map_err(|_| ReferenceError::Unavailable)?
            .map(|value| decode(value.value()))
            .transpose()
    }

    async fn list(
        &self,
        filter: &BondFilter,
        pagination: &Pagination,
    ) -> Result<Page<ReferenceRecord>, ReferenceError> {
        reference_page(self.current_records()?, filter, pagination)
    }

    async fn resolve(
        &self,
        identifier: &ReferenceIdentifier,
    ) -> Result<Option<ReferenceRecord>, ReferenceError> {
        resolve_reference(self.current_records()?, identifier)
    }

    async fn commit(
        &self,
        command: &ReferenceCommand,
        recorded_at: UtcTimestamp,
    ) -> Result<ReferenceCommit, ReferenceError> {
        command.validate()?;
        let mut write = self
            .db
            .begin_write()
            .map_err(|_| ReferenceError::Unavailable)?;
        write
            .set_durability(redb::Durability::Immediate)
            .map_err(|_| ReferenceError::Unavailable)?;
        let result;
        {
            let mut current = write
                .open_table(CURRENT)
                .map_err(|_| ReferenceError::Unavailable)?;
            let mut history = write
                .open_table(HISTORY)
                .map_err(|_| ReferenceError::Unavailable)?;
            let mut commits = write
                .open_table(COMMITS)
                .map_err(|_| ReferenceError::Unavailable)?;
            if let Some(bytes) = commits
                .get(command.idempotency_key.as_str())
                .map_err(|_| ReferenceError::Unavailable)?
            {
                let previous: CommittedReferenceCommand = decode(bytes.value())?;
                return if previous.command.identical_payload(command)? {
                    Ok(previous.result)
                } else {
                    Err(ReferenceError::Conflict(
                        "idempotency key was used with a different payload",
                    ))
                };
            }
            result = plan_reference_commit(command, recorded_at, |id| {
                current
                    .get(id.as_str())
                    .map_err(|_| ReferenceError::Unavailable)?
                    .map(|value| decode(value.value()))
                    .transpose()
            })?;
            let mut creates = 0;
            for record in &result.records {
                if current
                    .get(record.instrument_id.as_str())
                    .map_err(|_| ReferenceError::Unavailable)?
                    .is_none()
                {
                    creates += 1;
                }
            }
            if current.len().map_err(|_| ReferenceError::Unavailable)? + creates
                > MAX_REFERENCE_RECORDS as u64
                || commits.len().map_err(|_| ReferenceError::Unavailable)?
                    >= MAX_REFERENCE_COMMITS as u64
                || history.len().map_err(|_| ReferenceError::Unavailable)?
                    + result.records.len() as u64
                    > MAX_REFERENCE_VERSIONS as u64
            {
                return Err(ReferenceError::CapacityExceeded);
            }
            for record in &result.records {
                let bytes = serde_json::to_vec(record)
                    .map_err(|_| ReferenceError::InvalidInput("reference serialization failed"))?;
                current
                    .insert(record.instrument_id.as_str(), bytes.as_slice())
                    .map_err(|_| ReferenceError::Unavailable)?;
                history
                    .insert(
                        (record.instrument_id.as_str(), record.version.get()),
                        bytes.as_slice(),
                    )
                    .map_err(|_| ReferenceError::Unavailable)?;
            }
            let bytes = serde_json::to_vec(&CommittedReferenceCommand {
                command: command.clone(),
                result: result.clone(),
            })
            .map_err(|_| ReferenceError::InvalidInput("reference serialization failed"))?;
            commits
                .insert(command.idempotency_key.as_str(), bytes.as_slice())
                .map_err(|_| ReferenceError::Unavailable)?;
        }
        // Any preparation error drops/aborts the write transaction. A failed commit
        // acknowledgment is not evidence that no write happened.
        write.commit().map_err(|_| ReferenceError::UnknownOutcome)?;
        Ok(result)
    }

    async fn get_commit(&self, key: &RequestId) -> Result<Option<ReferenceCommit>, ReferenceError> {
        let read = self
            .db
            .begin_read()
            .map_err(|_| ReferenceError::Unavailable)?;
        let table = read
            .open_table(COMMITS)
            .map_err(|_| ReferenceError::Unavailable)?;
        let stored: Option<CommittedReferenceCommand> = table
            .get(key.as_str())
            .map_err(|_| ReferenceError::Unavailable)?
            .map(|value| decode(value.value()))
            .transpose()?;
        Ok(stored.map(|entry| entry.result))
    }
}
