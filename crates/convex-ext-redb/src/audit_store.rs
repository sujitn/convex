//! Immutable audit identities and correctly filtered local read snapshots.
use crate::database::*;
use async_trait::async_trait;
use convex_ports::error::TraitError;
use convex_ports::storage::{
    AuditEntry, AuditFilter, AuditStore, Page, Pagination, MAX_AUDIT_BATCH,
};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::sync::Arc;

const AUDIT: TableDefinition<u64, &[u8]> = TableDefinition::new("audit");

/// Append-only local audit store. This is not a transaction spanning other stores.
pub struct RedbAuditStore {
    db: Arc<Database>,
    /// The most entries the log holds: `MAX_HISTORY`, lower only in this crate's tests.
    capacity: u64,
}
impl RedbAuditStore {
    /// Construct a local audit store on the shared database handle.
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            capacity: MAX_HISTORY,
        }
    }

    /// Append each admitted entry in one write transaction, all or nothing.
    fn write(&self, entries: &[AuditEntry]) -> Result<(), TraitError> {
        if entries.len() > MAX_AUDIT_BATCH {
            return Err(TraitError::InvalidInput("audit batch exceeds 64 entries"));
        }
        if entries.is_empty() {
            return Ok(());
        }
        let encoded = entries
            .iter()
            .map(|entry| admit(entry).map(|bytes| (entry, bytes)))
            .collect::<Result<Vec<_>, _>>()?;
        let write = begin_write(&self.db)?;
        let mut inserted = false;
        {
            let mut table = write.open_table(AUDIT).map_err(database_error)?;
            for (entry, bytes) in &encoded {
                // A return before `commit` drops the transaction, which aborts it: an
                // entry that conflicts or does not fit leaves the whole batch unwritten.
                if let Some(previous) = table.get(entry.id).map_err(database_error)? {
                    if decode::<AuditEntry>(previous.value())? == **entry {
                        continue;
                    }
                    return Err(TraitError::Conflict("audit ID has different content"));
                }
                if table.len().map_err(database_error)? >= self.capacity {
                    return Err(TraitError::CapacityExceeded);
                }
                table
                    .insert(entry.id, bytes.as_slice())
                    .map_err(database_error)?;
                inserted = true;
            }
        }
        if inserted {
            commit(write)
        } else {
            Ok(())
        }
    }
}

/// Validate one entry and encode it.
fn admit(entry: &AuditEntry) -> Result<Vec<u8>, TraitError> {
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
    encode(entry)
}

#[async_trait]
impl AuditStore for RedbAuditStore {
    async fn append(&self, entry: &AuditEntry) -> Result<(), TraitError> {
        self.write(std::slice::from_ref(entry))
    }

    /// One write transaction for the whole batch: every entry is written, or none is.
    async fn append_all(&self, entries: &[AuditEntry]) -> Result<(), TraitError> {
        self.write(entries)
    }

    async fn entry_count(&self) -> Result<u64, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        match read.open_table(AUDIT) {
            Ok(table) => table.len().map_err(database_error),
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(0),
            Err(e) => Err(database_error(e)),
        }
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
            // Entries are keyed by ID, so this scan is in ID order.
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

    async fn last_id(&self) -> Result<Option<u64>, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(AUDIT) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(database_error(e)),
        };
        let last = table.last().map_err(database_error)?;
        Ok(last.map(|(id, _)| id.value()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64) -> AuditEntry {
        AuditEntry {
            id,
            timestamp: 100,
            event_type: "limit_status".into(),
            entity_type: "book".into(),
            entity_id: "BOOK-A".into(),
            user: None,
            action: "breach".into(),
            details: None,
        }
    }

    /// A batch that would pass the store's capacity is refused whole, and so is one entry
    /// past it; the entries before the cap stay as they were.
    #[tokio::test]
    async fn a_batch_past_the_capacity_is_refused_whole() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::create(directory.path().join("audit.redb")).unwrap());
        let store = RedbAuditStore { db, capacity: 4 };
        store.append_all(&[entry(1), entry(2)]).await.unwrap();
        assert!(matches!(
            store.append_all(&[entry(3), entry(4), entry(5)]).await,
            Err(TraitError::CapacityExceeded)
        ));
        assert_eq!(store.entry_count().await.unwrap(), 2);
        // A retry of what is stored writes nothing, so it fits at the cap.
        store.append_all(&[entry(3), entry(4)]).await.unwrap();
        store.append_all(&[entry(1), entry(4)]).await.unwrap();
        assert!(matches!(
            store.append(&entry(5)).await,
            Err(TraitError::CapacityExceeded)
        ));
        assert_eq!(store.entry_count().await.unwrap(), 4);
        assert_eq!(store.last_id().await.unwrap(), Some(4));
    }
}
