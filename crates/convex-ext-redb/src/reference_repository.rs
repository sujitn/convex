//! Transactional local reference repository. redb is single-process storage.

use std::sync::Arc;

use async_trait::async_trait;
use convex_core::contracts::{RequestId, UtcTimestamp, Version};
use convex_core::ids::InstrumentId;
use convex_ports::reference_master::*;
use convex_ports::reference_repository::*;
use convex_ports::storage::{BondFilter, Page, Pagination};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};

const CURRENT: TableDefinition<&str, &[u8]> = TableDefinition::new("reference_current_v2");
const HISTORY: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("reference_history_v2");
const COMMITS: TableDefinition<&str, &[u8]> = TableDefinition::new("reference_commits_v2");
const MASTER_CURRENT: TableDefinition<&str, &[u8]> =
    TableDefinition::new("reference_master_current_v2");
const MASTER_HISTORY: TableDefinition<(&str, u64), &[u8]> =
    TableDefinition::new("reference_master_history_v2");

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
        let presence = [
            exists(&read, CURRENT)?,
            exists(&read, HISTORY)?,
            exists(&read, COMMITS)?,
            exists(&read, MASTER_CURRENT)?,
            exists(&read, MASTER_HISTORY)?,
        ];
        let bond_tables = presence[..3].iter().filter(|present| **present).count();
        let master_tables = presence[3..].iter().filter(|present| **present).count();
        if (bond_tables != 0 && bond_tables != 3)
            || master_tables == 1
            || (bond_tables == 0 && master_tables != 0)
        {
            return Err(ReferenceError::Unsupported(
                "incomplete reference repository schema",
            ));
        }
        if presence.iter().all(|present| *present) {
            // Inspection/reopen is read-only once the complete schema exists.
            return Ok(Self { db });
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
        write
            .open_table(MASTER_CURRENT)
            .map_err(|_| ReferenceError::Unavailable)?;
        write
            .open_table(MASTER_HISTORY)
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

fn exists<K: redb::Key + 'static, V: redb::Value + 'static>(
    read: &redb::ReadTransaction,
    table: TableDefinition<K, V>,
) -> Result<bool, ReferenceError> {
    match read.open_table(table) {
        Ok(_) => Ok(true),
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(false),
        Err(_) => Err(ReferenceError::Unavailable),
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
        identifier: &ExternalIdentifier,
    ) -> Result<Option<ReferenceRecord>, ReferenceError> {
        resolve_reference(self.current_records()?, identifier)
    }

    async fn commit(
        &self,
        command: &ReferenceCommand,
        recorded_at: UtcTimestamp,
    ) -> Result<ReferenceCommit, ReferenceError> {
        // A retained retry needs no write transaction. Recheck after acquiring the
        // write transaction below, because concurrent first deliveries may race.
        let previous: Option<CommittedReferenceCommand> = {
            let read = self
                .db
                .begin_read()
                .map_err(|_| ReferenceError::Unavailable)?;
            let table = read
                .open_table(COMMITS)
                .map_err(|_| ReferenceError::Unavailable)?;
            table
                .get(command.idempotency_key.as_str())
                .map_err(|_| ReferenceError::Unavailable)?
                .map(|value| decode(value.value()))
                .transpose()?
        };
        if let Some(previous) = previous {
            return if previous.command.identical_payload(command)? {
                Ok(previous.result)
            } else {
                Err(ReferenceError::Conflict(
                    "idempotency key was used with a different payload",
                ))
            };
        }
        let mut write = self
            .db
            .begin_write()
            .map_err(|_| ReferenceError::Unavailable)?;
        write
            .set_durability(redb::Durability::Immediate)
            .map_err(|_| ReferenceError::Unavailable)?;
        let mut result;
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
            let mut master_current = write
                .open_table(MASTER_CURRENT)
                .map_err(|_| ReferenceError::Unavailable)?;
            let mut master_history = write
                .open_table(MASTER_HISTORY)
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
            result.master_records =
                plan_master_changes(&command.master_changes, recorded_at, |key| {
                    master_current
                        .get(key.storage_key().as_str())
                        .map_err(|_| ReferenceError::Unavailable)?
                        .map(|v| decode(v.value()))
                        .transpose()
                })?;
            validate_reference_links(
                &result,
                |id| {
                    current
                        .get(id.as_str())
                        .map_err(|_| ReferenceError::Unavailable)?
                        .map(|v| decode(v.value()))
                        .transpose()
                },
                |key, version| {
                    master_history
                        .get((key.storage_key().as_str(), version.get()))
                        .map_err(|_| ReferenceError::Unavailable)?
                        .map(|v| decode(v.value()))
                        .transpose()
                },
                |id, version| {
                    history
                        .get((id.as_str(), version.get()))
                        .map_err(|_| ReferenceError::Unavailable)?
                        .map(|v| decode(v.value()))
                        .transpose()
                },
            )?;
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
            let mut master_creates = 0;
            for record in &result.master_records {
                if master_current
                    .get(record.definition.key().storage_key().as_str())
                    .map_err(|_| ReferenceError::Unavailable)?
                    .is_none()
                {
                    master_creates += 1;
                }
            }
            if current.len().map_err(|_| ReferenceError::Unavailable)?
                + master_current
                    .len()
                    .map_err(|_| ReferenceError::Unavailable)?
                + creates
                + master_creates
                > MAX_REFERENCE_RECORDS as u64
                || commits.len().map_err(|_| ReferenceError::Unavailable)?
                    >= MAX_REFERENCE_COMMITS as u64
                || history.len().map_err(|_| ReferenceError::Unavailable)?
                    + master_history
                        .len()
                        .map_err(|_| ReferenceError::Unavailable)?
                    + result.records.len() as u64
                    + result.master_records.len() as u64
                    > MAX_REFERENCE_VERSIONS as u64
            {
                return Err(ReferenceError::CapacityExceeded);
            }
            for record in &result.master_records {
                let key = record.definition.key().storage_key();
                let bytes = serde_json::to_vec(record)
                    .map_err(|_| ReferenceError::InvalidInput("reference serialization failed"))?;
                master_current
                    .insert(key.as_str(), bytes.as_slice())
                    .map_err(|_| ReferenceError::Unavailable)?;
                master_history
                    .insert((key.as_str(), record.version.get()), bytes.as_slice())
                    .map_err(|_| ReferenceError::Unavailable)?;
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

    async fn get_at(
        &self,
        id: &InstrumentId,
        lookup: ReferenceLookup,
    ) -> Result<Option<ReferenceRecord>, ReferenceError> {
        validate_reference_id(id)?;
        let read = self
            .db
            .begin_read()
            .map_err(|_| ReferenceError::Unavailable)?;
        bond_at(&read, id, lookup)
    }
    async fn get_master(&self, key: &MasterKey) -> Result<Option<MasterRecord>, ReferenceError> {
        let read = self
            .db
            .begin_read()
            .map_err(|_| ReferenceError::Unavailable)?;
        read.open_table(MASTER_CURRENT)
            .map_err(|_| ReferenceError::Unavailable)?
            .get(key.storage_key().as_str())
            .map_err(|_| ReferenceError::Unavailable)?
            .map(|v| decode(v.value()))
            .transpose()
    }
    async fn get_master_version(
        &self,
        key: &MasterKey,
        version: Version,
    ) -> Result<Option<MasterRecord>, ReferenceError> {
        let read = self
            .db
            .begin_read()
            .map_err(|_| ReferenceError::Unavailable)?;
        read.open_table(MASTER_HISTORY)
            .map_err(|_| ReferenceError::Unavailable)?
            .get((key.storage_key().as_str(), version.get()))
            .map_err(|_| ReferenceError::Unavailable)?
            .map(|v| decode(v.value()))
            .transpose()
    }
    async fn get_master_at(
        &self,
        key: &MasterKey,
        lookup: ReferenceLookup,
    ) -> Result<Option<MasterRecord>, ReferenceError> {
        let read = self
            .db
            .begin_read()
            .map_err(|_| ReferenceError::Unavailable)?;
        master_at(&read, key, lookup)
    }
    async fn resolve_at(
        &self,
        identifier: &ExternalIdentifier,
        lookup: ReferenceLookup,
    ) -> Result<Option<IdentifierResolution>, ReferenceError> {
        identifier.validate()?;
        let read = self
            .db
            .begin_read()
            .map_err(|_| ReferenceError::Unavailable)?;
        let table = read
            .open_table(MASTER_HISTORY)
            .map_err(|_| ReferenceError::Unavailable)?;
        if table.len().map_err(|_| ReferenceError::Unavailable)? > MAX_REFERENCE_VERSIONS as u64 {
            return Err(ReferenceError::CapacityExceeded);
        }
        let records = table
            .range(("identifier/", 1)..("identifier0", 0))
            .map_err(|_| ReferenceError::Unavailable)?
            .map(|entry| {
                let (_, value) = entry.map_err(|_| ReferenceError::Unavailable)?;
                decode(value.value())
            })
            .collect::<Result<Vec<MasterRecord>, ReferenceError>>()?;
        let Some(mapping) = select_mapping_at(records, identifier, lookup)? else {
            return Ok(None);
        };
        let MasterDefinition::Identifier(definition) = &mapping.definition else {
            return Err(ReferenceError::InvalidInput("not a mapping"));
        };
        let target = definition.target.clone().ok_or(ReferenceError::NotFound)?;
        let target_version = match &target {
            ReferenceTarget::Futures { instrument_id } => {
                master_at(&read, &MasterKey::Futures(instrument_id.clone()), lookup)?
                    .ok_or(ReferenceError::NotFound)?
                    .version
            }
            ReferenceTarget::Instrument { instrument_id } => {
                let record = bond_at(&read, &instrument_id.clone().into(), lookup)?
                    .ok_or(ReferenceError::NotFound)?;
                validate_bond_reference(record.bond.as_ref().ok_or(ReferenceError::NotFound)?)?;
                record.version
            }
            ReferenceTarget::Entity { entity_id } => {
                master_at(&read, &MasterKey::Entity(entity_id.clone()), lookup)?
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

fn bond_at(
    read: &redb::ReadTransaction,
    id: &InstrumentId,
    lookup: ReferenceLookup,
) -> Result<Option<ReferenceRecord>, ReferenceError> {
    let table = read
        .open_table(HISTORY)
        .map_err(|_| ReferenceError::Unavailable)?;
    if table.len().map_err(|_| ReferenceError::Unavailable)? > MAX_REFERENCE_VERSIONS as u64 {
        return Err(ReferenceError::CapacityExceeded);
    }
    let records = table
        .range((id.as_str(), 1)..=(id.as_str(), u64::MAX))
        .map_err(|_| ReferenceError::Unavailable)?
        .map(|entry| {
            let (_, value) = entry.map_err(|_| ReferenceError::Unavailable)?;
            decode(value.value())
        })
        .collect::<Result<Vec<ReferenceRecord>, ReferenceError>>()?;
    select_bond_at(records, lookup)
}

fn master_at(
    read: &redb::ReadTransaction,
    key: &MasterKey,
    lookup: ReferenceLookup,
) -> Result<Option<MasterRecord>, ReferenceError> {
    let table = read
        .open_table(MASTER_HISTORY)
        .map_err(|_| ReferenceError::Unavailable)?;
    if table.len().map_err(|_| ReferenceError::Unavailable)? > MAX_REFERENCE_VERSIONS as u64 {
        return Err(ReferenceError::CapacityExceeded);
    }
    let key = key.storage_key();
    let records = table
        .range((key.as_str(), 1)..=(key.as_str(), u64::MAX))
        .map_err(|_| ReferenceError::Unavailable)?
        .map(|entry| {
            let (_, value) = entry.map_err(|_| ReferenceError::Unavailable)?;
            decode(value.value())
        })
        .collect::<Result<Vec<MasterRecord>, ReferenceError>>()?;
    select_master_at(records, lookup)
}
