//! Immutable audit identities and correctly filtered local read snapshots.
use crate::database::*;
use async_trait::async_trait;
use convex_ports::error::TraitError;
use convex_ports::storage::{AuditEntry, AuditFilter, AuditStore, Page, Pagination};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::sync::Arc;

const AUDIT: TableDefinition<u64, &[u8]> = TableDefinition::new("audit");

/// Append-only local audit store. This is not a transaction spanning other stores.
pub struct RedbAuditStore {
    db: Arc<Database>,
}
impl RedbAuditStore {
    /// Construct a local audit store on the shared database handle.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AuditStore for RedbAuditStore {
    async fn append(&self, entry: &AuditEntry) -> Result<(), TraitError> {
        validate_id(&entry.entity_id)?;
        if entry
            .details
            .as_ref()
            .is_some_and(|v| v.len() > MAX_RECORD_BYTES)
        {
            return Err(TraitError::CapacityExceeded);
        }
        if [&entry.event_type, &entry.entity_type, &entry.action]
            .into_iter()
            .any(|v| v.is_empty() || v.len() > 1024 || v.chars().any(char::is_control))
            || entry
                .user
                .as_ref()
                .is_some_and(|v| v.len() > 1024 || v.chars().any(char::is_control))
        {
            return Err(TraitError::InvalidInput("invalid audit entry"));
        }
        let bytes = encode(entry)?;
        let write = begin_write(&self.db)?;
        {
            let mut table = write.open_table(AUDIT).map_err(database_error)?;
            if let Some(previous) = table.get(entry.id).map_err(database_error)? {
                return if decode::<AuditEntry>(previous.value())? == *entry {
                    Ok(())
                } else {
                    Err(TraitError::Conflict("audit ID has different content"))
                };
            }
            if table.len().map_err(database_error)? >= MAX_HISTORY {
                return Err(TraitError::CapacityExceeded);
            }
            table
                .insert(entry.id, bytes.as_slice())
                .map_err(database_error)?;
        }
        commit(write)
    }

    async fn query(
        &self,
        filter: &AuditFilter,
        pagination: &Pagination,
    ) -> Result<Page<AuditEntry>, TraitError> {
        filter.validate()?;
        pagination.validate()?;
        let mut page = Page {
            items: Vec::new(),
            total: 0,
            offset: pagination.offset,
            limit: pagination.limit,
        };
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(AUDIT) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(page),
            Err(e) => return Err(database_error(e)),
        };
        if table.len().map_err(database_error)? > MAX_HISTORY {
            return Err(TraitError::CapacityExceeded);
        }
        for result in table.iter().map_err(database_error)? {
            let (_, data) = result.map_err(database_error)?;
            let entry: AuditEntry = decode(data.value())?;
            if !filter.matches(&entry) {
                continue;
            }
            if page.total >= pagination.offset as u64 && page.items.len() < pagination.limit {
                page.items.push(entry);
            }
            page.total += 1;
        }
        Ok(page)
    }
}
