//! Immutable audit identities and correctly filtered local read snapshots.
//!
//! Beside the log, keyed by ID, an index holds one key per entry, (entity type, entity
//! ID, entry ID), written in the same transaction as the entry, so one entity's entries
//! are read in ID order without reading the others' (P15b, D93). A mark records the ID
//! through which every entry has its key. Opening the store indexes the entries after
//! it, which a store written before the index, or by an older binary, holds.
//!
//! A prune (P18b, D105) removes old entries and their index keys in one transaction. It
//! never removes the log's last entry, so the mark, which is never above it, stays valid.
use crate::database::*;
use async_trait::async_trait;
use convex_ports::error::TraitError;
use convex_ports::storage::{
    AuditEntry, AuditFilter, AuditPrune, AuditStore, EntityPage, Page, Pagination, Pruned,
    MAX_AUDIT_BATCH, MAX_ENTITY_PAGE, MAX_PRUNE_READ,
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

/// Local audit store: entries are immutable, and only a prune removes them. This is not a
/// transaction spanning other stores.
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

/// An entry the walk of a prune removes: its ID and its index key.
struct Removal {
    id: u64,
    entity_type: String,
    entity_id: String,
}

impl RedbAuditStore {
    /// One prune call: read from `after + 1`, collect what to remove, then remove each
    /// entry and its index key, in one write transaction. A call that removes nothing
    /// commits nothing.
    fn remove_old(&self, prune: &AuditPrune<'_>) -> Result<Pruned, TraitError> {
        prune.validate()?;
        let mut pruned = Pruned {
            removed: 0,
            next_after: prune.after,
            more: false,
        };
        let Some(start) = prune.after.checked_add(1) else {
            return Ok(pruned);
        };
        // A return before `commit` drops the transaction, which aborts it.
        let write = begin_write(&self.db)?;
        let mut removals = Vec::new();
        {
            let mut audit = write.open_table(AUDIT).map_err(database_error)?;
            let Some(last) = audit
                .last()
                .map_err(database_error)?
                .map(|(id, _)| id.value())
            else {
                return Ok(pruned);
            };
            if start >= last {
                return Ok(pruned);
            }
            for (read, item) in audit
                .range(start..last)
                .map_err(database_error)?
                .enumerate()
            {
                if removals.len() == prune.max || read == MAX_PRUNE_READ {
                    pruned.more = true;
                    break;
                }
                let (id, data) = item.map_err(database_error)?;
                let entry: AuditEntry = decode(data.value())?;
                if entry.timestamp >= prune.before {
                    break;
                }
                pruned.next_after = id.value();
                if entry.event_type == prune.event_type && !prune.keep.contains(&id.value()) {
                    removals.push(Removal {
                        id: id.value(),
                        entity_type: entry.entity_type,
                        entity_id: entry.entity_id,
                    });
                }
            }
            let mut index = write.open_table(AUDIT_BY_ENTITY).map_err(database_error)?;
            for removal in &removals {
                audit.remove(removal.id).map_err(database_error)?;
                index
                    .remove((
                        removal.entity_type.as_str(),
                        removal.entity_id.as_str(),
                        removal.id,
                    ))
                    .map_err(database_error)?;
            }
        }
        if removals.is_empty() {
            return Ok(pruned);
        }
        pruned.removed = removals.len();
        commit(write)?;
        Ok(pruned)
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

    /// Up to `max` removals a transaction, each entry with its index key.
    async fn prune(&self, prune: &AuditPrune<'_>) -> Result<Pruned, TraitError> {
        self.remove_old(prune)
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

    fn old(id: u64, event: &str) -> AuditEntry {
        AuditEntry {
            timestamp: 1,
            event_type: event.into(),
            ..entry(id)
        }
    }

    fn pruning(before: i64, after: u64, keep: &std::collections::BTreeSet<u64>) -> AuditPrune<'_> {
        AuditPrune {
            event_type: "limit_status",
            before,
            after,
            keep,
            max: convex_ports::storage::MAX_PRUNE_BATCH,
        }
    }

    /// P18b (D105): a store at its capacity refuses every append until a prune frees room;
    /// a reopen after a prune finds nothing to index, and IDs continue from the last.
    #[tokio::test]
    async fn a_full_store_appends_again_after_a_prune() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("audit.redb");
        let db = Arc::new(Database::create(&path).unwrap());
        let store = RedbAuditStore { db, capacity: 4 };
        store
            .append_all(&[
                old(1, "limit_status"),
                old(2, "limit_status"),
                old(3, "limit_status"),
                old(4, "limit_status"),
            ])
            .await
            .unwrap();
        assert!(matches!(
            store.append(&entry(5)).await,
            Err(TraitError::CapacityExceeded)
        ));
        let keep = std::collections::BTreeSet::from([2]);
        let pruned = store.prune(&pruning(100, 0, &keep)).await.unwrap();
        assert_eq!(
            (pruned.removed, pruned.more),
            (2, false),
            "1 and 3; 2 kept, 4 last"
        );
        store.append_all(&[entry(5), entry(6)]).await.unwrap();
        assert!(matches!(
            store.append(&entry(7)).await,
            Err(TraitError::CapacityExceeded)
        ));
        drop(store);
        let store = RedbAuditStore::new(Arc::new(Database::create(&path).unwrap())).unwrap();
        assert_eq!(store.index_existing(INDEX_CHUNK).unwrap(), 0);
        assert_eq!(ids(&store, "BOOK-A").await, [2, 4, 5, 6]);
        assert_eq!(store.last_id().await.unwrap(), Some(6));
    }

    /// A call reads at most `MAX_PRUNE_READ` entries, so one passing over many entries of
    /// another type stops with `more` and a cursor past them, and the next call goes on.
    #[tokio::test]
    async fn a_prune_call_reads_a_bounded_number_of_entries() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::create(directory.path().join("audit.redb")).unwrap());
        let read = MAX_PRUNE_READ as u64;
        let mut entries: Vec<AuditEntry> = (1..=read).map(|id| old(id, "other")).collect();
        entries.push(old(read + 1, "limit_status"));
        entries.push(entry(read + 2));
        write_unindexed(&db, &entries);
        let store = RedbAuditStore::new(db).unwrap();
        let none = std::collections::BTreeSet::new();
        let first = store.prune(&pruning(100, 0, &none)).await.unwrap();
        assert_eq!(
            first,
            Pruned {
                removed: 0,
                next_after: read,
                more: true
            }
        );
        let second = store
            .prune(&pruning(100, first.next_after, &none))
            .await
            .unwrap();
        assert_eq!(
            second,
            Pruned {
                removed: 1,
                next_after: read + 1,
                more: false
            }
        );
        assert_eq!(store.entry_count().await.unwrap(), read + 1);
    }

    /// P18b's measurement (D105): a full store (1,000,000 entries of about 600 bytes of
    /// details, as the monitor writes them) pruned in calls of 4,096 while another task
    /// appends one entry every 2 ms, as the monitor would; then the file's size, and its
    /// size once refilled.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "measurement; run with --ignored --nocapture under the ci profile"]
    async fn pruning_a_full_store() {
        use std::time::{Duration, Instant};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("audit.redb");
        let db = Arc::new(Database::create(&path).unwrap());
        let details = "x".repeat(600);
        let fill = |from: u64, to: u64, timestamp: i64| {
            for chunk in (from..=to).collect::<Vec<u64>>().chunks(INDEX_CHUNK) {
                let entries: Vec<AuditEntry> = chunk
                    .iter()
                    .map(|&id| AuditEntry {
                        timestamp,
                        details: Some(details.clone()),
                        ..of(id, &format!("BOOK-{:02}", (id / 64) % 64))
                    })
                    .collect();
                write_unindexed(&db, &entries);
            }
        };
        let mib =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().len() as f64 / 1048576.0;
        fill(1, MAX_HISTORY, 1);
        let store = Arc::new(RedbAuditStore::new(db.clone()).unwrap());
        let full = mib(&path);
        println!("full store: {MAX_HISTORY} entries, {full:.0} MiB");

        let appending = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let appender = {
            let store = store.clone();
            let appending = appending.clone();
            tokio::spawn(async move {
                let mut waits = Vec::new();
                let mut id = MAX_HISTORY + 1;
                while appending.load(std::sync::atomic::Ordering::SeqCst) {
                    let started = Instant::now();
                    let entry = AuditEntry {
                        timestamp: 1_000_000,
                        details: Some("x".repeat(600)),
                        ..of(id, "BOOK-NEW")
                    };
                    if store.append(&entry).await.is_ok() {
                        id += 1;
                    }
                    waits.push(started.elapsed());
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                (waits, id - MAX_HISTORY - 1)
            })
        };
        let keep = std::collections::BTreeSet::new();
        let mut after = 0;
        let mut calls = Vec::new();
        let mut removed = 0;
        let started = Instant::now();
        loop {
            let call = Instant::now();
            let pruned = store.prune(&pruning(1_000, after, &keep)).await.unwrap();
            calls.push(call.elapsed());
            removed += pruned.removed;
            after = pruned.next_after;
            if !pruned.more {
                break;
            }
        }
        let total = started.elapsed();
        appending.store(false, std::sync::atomic::Ordering::SeqCst);
        let (mut waits, appended) = appender.await.unwrap();
        calls.sort();
        waits.sort();
        let ms = |d: Duration| d.as_secs_f64() * 1e3;
        let at = |v: &[Duration], q: f64| ms(v[((v.len() - 1) as f64 * q).round() as usize]);
        println!(
            "pruned {removed} in {} calls, {:.2} s: {:.0} entries a second",
            calls.len(),
            total.as_secs_f64(),
            removed as f64 / total.as_secs_f64()
        );
        println!(
            "a call (one transaction): median {:.1} ms, p90 {:.1} ms, max {:.1} ms",
            at(&calls, 0.5),
            at(&calls, 0.9),
            at(&calls, 1.0)
        );
        println!(
            "an append meanwhile ({} tried, {appended} written): median {:.2} ms, p90 {:.1} ms, max {:.1} ms",
            waits.len(),
            at(&waits, 0.5),
            at(&waits, 0.9),
            at(&waits, 1.0)
        );
        let count = store.entry_count().await.unwrap();
        // The last old entry goes too once an append follows it.
        assert_eq!(count, appended.max(1));
        let pruned_size = mib(&path);
        drop(store);
        let refill_from = MAX_HISTORY + 1 + appended;
        fill(
            refill_from,
            refill_from + MAX_HISTORY - count - 1,
            2_000_000,
        );
        let store = RedbAuditStore::new(db.clone()).unwrap();
        assert_eq!(store.entry_count().await.unwrap(), MAX_HISTORY);
        println!(
            "file: {full:.0} MiB full, {pruned_size:.0} MiB pruned, {:.0} MiB full again",
            mib(&path)
        );
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
