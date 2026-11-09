//! Immutable audit identities and correctly filtered local read snapshots.
//!
//! Beside the log, keyed by ID, an index holds one key per entry, (entity type, entity
//! ID, entry ID), written in the same transaction as the entry, so one entity's entries
//! are read in ID order without reading the others' (P15b, D93). A mark records the ID
//! through which every entry has its key. Opening the store indexes the entries after
//! it, which a store written before the index, or by an older binary, holds.
use crate::database::*;
use async_trait::async_trait;
use convex_ports::error::TraitError;
use convex_ports::storage::{
    AuditEntry, AuditFilter, AuditStore, EntityPage, Page, Pagination, MAX_AUDIT_BATCH,
    MAX_ENTITY_PAGE,
};
use redb::{
    Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition,
    WriteTransaction,
};
use std::sync::Arc;

const AUDIT: TableDefinition<u64, &[u8]> = TableDefinition::new("audit");
/// One key per entry: (entity type, entity ID, entry ID).
const AUDIT_BY_ENTITY: TableDefinition<(&str, &str, u64), ()> =
    TableDefinition::new("audit_by_entity");
/// Under [`INDEXED_THROUGH`], the ID through which every entry has its index key.
const AUDIT_INDEXED: TableDefinition<&str, u64> = TableDefinition::new("audit_indexed");
const INDEXED_THROUGH: &str = "through";
/// Keys one transaction of the open-time indexing writes, about 3 MiB of index, so a
/// full store of 1,000,000 entries is indexed in 16 transactions.
const INDEX_CHUNK: usize = 65_536;

/// Append-only local audit store. This is not a transaction spanning other stores.
pub struct RedbAuditStore {
    db: Arc<Database>,
    /// The most entries the log holds: `MAX_HISTORY`, lower only in this crate's tests.
    capacity: u64,
}
impl RedbAuditStore {
    /// Construct a local audit store on the shared database handle, first indexing each
    /// entry that has no index key yet.
    pub fn new(db: Arc<Database>) -> Result<Self, TraitError> {
        let store = Self {
            db,
            capacity: MAX_HISTORY,
        };
        store.index_existing(INDEX_CHUNK)?;
        Ok(store)
    }

    /// Index the entries after the mark, `chunk` keys a transaction, in ID order; each
    /// transaction moves the mark, so an interrupted run resumes where it stopped.
    /// Returns the transactions committed.
    fn index_existing(&self, chunk: usize) -> Result<usize, TraitError> {
        let mut commits = 0;
        loop {
            // A return before `commit` aborts the transaction, which here writes nothing.
            let write = begin_write(&self.db)?;
            let mark = indexed_through(&write)?;
            let Some(start) = mark.map_or(Some(0), |mark| mark.checked_add(1)) else {
                return Ok(commits);
            };
            let mut through = None;
            {
                let audit = write.open_table(AUDIT).map_err(database_error)?;
                let mut index = write.open_table(AUDIT_BY_ENTITY).map_err(database_error)?;
                for item in audit.range(start..).map_err(database_error)?.take(chunk) {
                    let (id, data) = item.map_err(database_error)?;
                    let entry: AuditEntry = decode(data.value())?;
                    index
                        .insert(
                            (
                                entry.entity_type.as_str(),
                                entry.entity_id.as_str(),
                                id.value(),
                            ),
                            (),
                        )
                        .map_err(database_error)?;
                    through = Some(id.value());
                }
            }
            let Some(through) = through else {
                return Ok(commits);
            };
            write
                .open_table(AUDIT_INDEXED)
                .map_err(database_error)?
                .insert(INDEXED_THROUGH, through)
                .map_err(database_error)?;
            commit(write)?;
            commits += 1;
        }
    }

    /// Append each admitted entry, and its index key, in one write transaction, all or
    /// nothing.
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
        let mut through = indexed_through(&write)?;
        let mut inserted = false;
        {
            let mut table = write.open_table(AUDIT).map_err(database_error)?;
            let mut index = write.open_table(AUDIT_BY_ENTITY).map_err(database_error)?;
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
                index
                    .insert(
                        (
                            entry.entity_type.as_str(),
                            entry.entity_id.as_str(),
                            entry.id,
                        ),
                        (),
                    )
                    .map_err(database_error)?;
                // Opening the store indexed every entry before this one.
                through = through.max(Some(entry.id));
                inserted = true;
            }
        }
        if !inserted {
            return Ok(());
        }
        if let Some(through) = through {
            write
                .open_table(AUDIT_INDEXED)
                .map_err(database_error)?
                .insert(INDEXED_THROUGH, through)
                .map_err(database_error)?;
        }
        commit(write)
    }
}

/// The index mark, or `None` before any entry is indexed.
fn indexed_through(write: &WriteTransaction) -> Result<Option<u64>, TraitError> {
    let table = write.open_table(AUDIT_INDEXED).map_err(database_error)?;
    let mark = table.get(INDEXED_THROUGH).map_err(database_error)?;
    Ok(mark.map(|mark| mark.value()))
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

    /// A range of the index, then each entry by ID, in one read snapshot.
    async fn entity_page(
        &self,
        entity_type: &str,
        entity_id: &str,
        after: u64,
        limit: usize,
    ) -> Result<EntityPage, TraitError> {
        if limit > MAX_ENTITY_PAGE {
            return Err(TraitError::InvalidInput("page limit exceeds 1000"));
        }
        AuditFilter {
            entity_type: Some(entity_type.to_owned()),
            entity_id: Some(entity_id.to_owned()),
            ..AuditFilter::default()
        }
        .validate()?;
        let mut page = EntityPage {
            entries: Vec::new(),
            more: false,
        };
        let Some(start) = after.checked_add(1) else {
            return Ok(page);
        };
        let read = self.db.begin_read().map_err(database_error)?;
        let (index, audit) = match (read.open_table(AUDIT_BY_ENTITY), read.open_table(AUDIT)) {
            (Ok(index), Ok(audit)) => (index, audit),
            (Err(redb::TableError::TableDoesNotExist(_)), _)
            | (_, Err(redb::TableError::TableDoesNotExist(_))) => return Ok(page),
            (Err(e), _) | (_, Err(e)) => return Err(database_error(e)),
        };
        let keys = index
            .range((entity_type, entity_id, start)..=(entity_type, entity_id, u64::MAX))
            .map_err(database_error)?;
        for key in keys {
            let (key, _) = key.map_err(database_error)?;
            if page.entries.len() == limit {
                page.more = true;
                break;
            }
            let id = key.value().2;
            let data = audit
                .get(id)
                .map_err(database_error)?
                .ok_or_else(|| database_error("an audit index key names no entry"))?;
            page.entries.push(decode(data.value())?);
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

    fn of(id: u64, book: &str) -> AuditEntry {
        AuditEntry {
            entity_id: book.into(),
            ..entry(id)
        }
    }

    /// Write entries as a store without the index did: the log only.
    fn write_unindexed(db: &Database, entries: &[AuditEntry]) {
        let write = begin_write(db).unwrap();
        {
            let mut table = write.open_table(AUDIT).unwrap();
            for entry in entries {
                table
                    .insert(entry.id, encode(entry).unwrap().as_slice())
                    .unwrap();
            }
        }
        commit(write).unwrap();
    }

    async fn ids(store: &RedbAuditStore, book: &str) -> Vec<u64> {
        let page = store.entity_page("book", book, 0, 1000).await.unwrap();
        assert!(!page.more);
        page.entries.iter().map(|e| e.id).collect()
    }

    /// Each write indexes its entries, and a reopen finds nothing more to index.
    #[tokio::test]
    async fn the_index_is_written_with_each_entry_and_kept_across_a_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("audit.redb");
        {
            let db = Arc::new(Database::create(&path).unwrap());
            let store = RedbAuditStore::new(db).unwrap();
            store
                .append_all(&[of(1, "BOOK-A"), of(2, "BOOK-B"), of(3, "BOOK-A")])
                .await
                .unwrap();
            store.append(&of(4, "BOOK-B")).await.unwrap();
            // A retry of a stored entry writes no second key.
            store.append(&of(3, "BOOK-A")).await.unwrap();
            assert_eq!(ids(&store, "BOOK-A").await, [1, 3]);
        }
        let db = Arc::new(Database::create(&path).unwrap());
        let store = RedbAuditStore::new(db).unwrap();
        assert_eq!(store.index_existing(INDEX_CHUNK).unwrap(), 0);
        assert_eq!(ids(&store, "BOOK-A").await, [1, 3]);
        assert_eq!(ids(&store, "BOOK-B").await, [2, 4]);
    }

    /// A store written without the index is indexed when it opens, a chunk a
    /// transaction; entries an older binary adds later are indexed at the next open.
    #[tokio::test]
    async fn opening_a_store_indexes_the_entries_without_a_key() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::create(directory.path().join("audit.redb")).unwrap());
        let books = ["BOOK-A", "BOOK-B", "BOOK-C"];
        let old: Vec<AuditEntry> = (1..=10).map(|id| of(id, books[id as usize % 3])).collect();
        write_unindexed(&db, &old);
        let store = RedbAuditStore {
            db: db.clone(),
            capacity: MAX_HISTORY,
        };
        assert_eq!(ids(&store, "BOOK-A").await, Vec::<u64>::new());
        assert_eq!(
            store.index_existing(3).unwrap(),
            4,
            "10 entries, 3 a commit"
        );
        assert_eq!(ids(&store, "BOOK-A").await, [3, 6, 9]);
        assert_eq!(ids(&store, "BOOK-B").await, [1, 4, 7, 10]);
        assert_eq!(store.index_existing(3).unwrap(), 0);

        // An older binary appends without keys; the new one indexes them at open.
        write_unindexed(&db, &[of(11, "BOOK-A"), of(12, "BOOK-C")]);
        let store = RedbAuditStore::new(db).unwrap();
        assert_eq!(ids(&store, "BOOK-A").await, [3, 6, 9, 11]);
        assert_eq!(ids(&store, "BOOK-C").await, [2, 5, 8, 12]);
        store.append(&of(13, "BOOK-C")).await.unwrap();
        assert_eq!(store.index_existing(3).unwrap(), 0);
        assert_eq!(ids(&store, "BOOK-C").await, [2, 5, 8, 12, 13]);
    }

    /// P15b's measurement of the open-time indexing: a full store (1,000,000 entries of 64
    /// books, about 600 bytes of details each) written without the index, then opened.
    #[tokio::test]
    #[ignore = "measurement; run with --ignored --nocapture under the ci profile"]
    async fn indexing_a_full_store_at_open() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::create(directory.path().join("audit.redb")).unwrap());
        let details = "x".repeat(600);
        let started = std::time::Instant::now();
        for chunk in (1..=MAX_HISTORY).collect::<Vec<u64>>().chunks(INDEX_CHUNK) {
            let entries: Vec<AuditEntry> = chunk
                .iter()
                .map(|&id| AuditEntry {
                    details: Some(details.clone()),
                    ..of(id, &format!("BOOK-{:02}", (id / 64) % 64))
                })
                .collect();
            write_unindexed(&db, &entries);
        }
        println!(
            "fixture without the index: {:.1} s",
            started.elapsed().as_secs_f64()
        );
        let store = RedbAuditStore {
            db: db.clone(),
            capacity: MAX_HISTORY,
        };
        let started = std::time::Instant::now();
        let commits = store.index_existing(INDEX_CHUNK).unwrap();
        println!(
            "indexing {MAX_HISTORY} entries at open: {commits} commits, {:.2} s",
            started.elapsed().as_secs_f64()
        );
        assert_eq!(commits, 16);
        let started = std::time::Instant::now();
        RedbAuditStore::new(db).unwrap();
        println!(
            "an open with nothing to index: {:.2} ms",
            started.elapsed().as_secs_f64() * 1e3
        );
        let page = store.entity_page("book", "BOOK-17", 0, 1000).await.unwrap();
        assert_eq!((page.entries.len(), page.more), (1000, true));
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
