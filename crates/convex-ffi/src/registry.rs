//! Type-erased object registry behind opaque handles.
//!
//! All bonds and curves live here. Handles are monotonic `u64`s starting at
//! 100 (so `#CX#100`, `#CX#101`, … are visually distinguishable from random
//! integers in spreadsheet diagnostics).

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use once_cell::sync::Lazy;

/// Opaque object handle. `0` is reserved as INVALID.
pub type Handle = u64;

/// Sentinel for "no handle".
pub const INVALID_HANDLE: Handle = 0;

/// Coarse classification of a registered object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    /// A discount/yield curve.
    Curve,
    /// A bond instrument.
    Bond(BondKind),
}

/// Distinguishes which bond struct is behind the handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BondKind {
    /// `FixedRateBond`.
    FixedRate,
    /// `CallableBond`.
    Callable,
    /// `FloatingRateNote`.
    FloatingRate,
    /// `ZeroCouponBond`.
    ZeroCoupon,
    /// `SinkingFundBond`.
    SinkingFund,
}

impl ObjectKind {
    /// Stable string tag (`"fixed_rate"`, `"callable"`, `"curve"`, …).
    pub fn tag(&self) -> &'static str {
        match self {
            ObjectKind::Curve => "curve",
            ObjectKind::Bond(b) => match b {
                BondKind::FixedRate => "fixed_rate",
                BondKind::Callable => "callable",
                BondKind::FloatingRate => "floating_rate",
                BondKind::ZeroCoupon => "zero_coupon",
                BondKind::SinkingFund => "sinking_fund",
            },
        }
    }
}

struct Entry {
    kind: ObjectKind,
    /// Eviction-slot name (owning cell's registry key, or the spec's own id).
    name: Option<String>,
    /// Human identifiers for [`resolve_alias`]; kept here so eviction can
    /// clean the alias map without scanning it.
    aliases: Vec<String>,
    /// Hash of the canonicalized spec JSON, for idempotent re-registration.
    content_hash: u64,
    /// `Arc` so [`with_object`] can clone it out and drop the read lock before
    /// running the closure — holding the lock across analytics that look up a
    /// second object nests a read lock, which `std::sync::RwLock` deadlocks on
    /// once a writer is queued.
    object: Arc<dyn Any + Send + Sync>,
}

/// Single lock over all maps so name/alias↔handle updates stay atomic.
#[derive(Default)]
struct Tables {
    objects: HashMap<Handle, Entry>,
    names: HashMap<String, Handle>,
    /// CUSIP/ISIN/free-name → handle; last write wins on collision.
    aliases: HashMap<String, Handle>,
    changes: ChangeLog,
}

/// Entries kept by the change log. A caller that falls further behind is told
/// to treat everything as changed.
pub(crate) const CHANGE_LOG_CAPACITY: usize = 4_096;

/// A reference whose resolution a mutation changed: a name (slot name or
/// alias, as [`resolve_alias`] reads them) or a handle that stopped resolving.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ChangeKey {
    Name(String),
    Handle(Handle),
}

/// Bounded record of which references each mutation changed, keyed by the
/// generation the mutation produced. It lives in [`Tables`], so it is written
/// under the same lock as the maps it describes.
#[derive(Default)]
struct ChangeLog {
    entries: std::collections::VecDeque<(u64, ChangeKey)>,
    /// A cursor below this cannot be answered completely: a later entry was
    /// evicted, or the registry was cleared.
    floor: u64,
}

impl ChangeLog {
    fn push(&mut self, generation: u64, key: ChangeKey) {
        if self.entries.len() == CHANGE_LOG_CAPACITY {
            if let Some((evicted, _)) = self.entries.pop_front() {
                self.floor = self.floor.max(evicted);
            }
        }
        self.entries.push_back((generation, key));
    }

    fn reset(&mut self, generation: u64) {
        self.entries.clear();
        self.floor = generation;
    }
}

/// What changed after a cursor. See [`changes_since`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changes {
    /// The current generation: the cursor for the next call.
    pub cursor: u64,
    /// `false` when the log cannot say what changed (the caller fell more than
    /// [`CHANGE_LOG_CAPACITY`] entries behind, the registry was cleared, or
    /// the cursor is from the future). The caller must then treat every
    /// reference as changed.
    pub complete: bool,
    /// Slot names and aliases whose resolution changed, sorted, unique.
    pub names: Vec<String>,
    /// Handles that stopped resolving, sorted, unique.
    pub handles: Vec<Handle>,
}

struct Inner {
    next_handle: AtomicU64,
    /// Bumped on every mutation, NOT on idempotent re-registration. Same
    /// generation ⇒ every handle resolves to the same object it did before.
    generation: AtomicU64,
    tables: RwLock<Tables>,
}

impl Inner {
    fn new() -> Self {
        Self {
            next_handle: AtomicU64::new(100),
            generation: AtomicU64::new(1),
            tables: RwLock::new(Tables::default()),
        }
    }
}

static REGISTRY: Lazy<Inner> = Lazy::new(Inner::new);
const MAX_OBJECTS: usize = 16_384;

fn read_tables() -> Option<std::sync::RwLockReadGuard<'static, Tables>> {
    REGISTRY
        .tables
        .read()
        .map_err(|_| crate::error::set_last_error("registry unavailable: poisoned lock"))
        .ok()
}
fn write_tables() -> Option<std::sync::RwLockWriteGuard<'static, Tables>> {
    REGISTRY
        .tables
        .write()
        .map_err(|_| crate::error::set_last_error("registry unavailable: poisoned lock"))
        .ok()
}

fn next_generation() -> Option<u64> {
    generation().checked_add(1).or_else(|| {
        crate::error::set_last_error("registry generation limit exceeded");
        None
    })
}

/// Registry mutation counter. See [`Inner::generation`].
pub fn generation() -> u64 {
    REGISTRY.generation.load(Ordering::SeqCst)
}

/// Register an object. Idempotent per eviction slot: an unchanged
/// `content_hash` returns the existing handle with no generation bump (so
/// Excel's dependency cutoff stops recalc cascades); changed content evicts
/// the prior object plus its aliases and mints a fresh handle so dependents
/// recompute.
pub fn register<T: Any + Send + Sync>(
    object: T,
    kind: ObjectKind,
    name: Option<String>,
    aliases: &[String],
    content_hash: u64,
) -> Handle {
    let Some(mut t) = write_tables() else {
        return INVALID_HANDLE;
    };
    let Some(next_generation) = next_generation() else {
        return INVALID_HANDLE;
    };
    let handle = REGISTRY.next_handle.load(Ordering::SeqCst);
    let Some(next_handle) = handle.checked_add(1) else {
        crate::error::set_last_error("registry handle limit exceeded");
        return INVALID_HANDLE;
    };
    let replacing = name.as_ref().is_some_and(|n| t.names.contains_key(n));
    if !replacing && t.objects.len() >= MAX_OBJECTS {
        crate::error::set_last_error("registry object limit exceeded");
        return INVALID_HANDLE;
    }

    if let Some(ref n) = name {
        if let Some(&existing) = t.names.get(n) {
            match t.objects.get(&existing) {
                Some(e) if e.content_hash == content_hash && e.kind == kind => {
                    return existing;
                }
                _ => {
                    if let Some(old) = t.objects.remove(&existing) {
                        for a in &old.aliases {
                            if t.aliases.get(a) == Some(&existing) {
                                t.aliases.remove(a);
                                t.changes.push(next_generation, ChangeKey::Name(a.clone()));
                            }
                        }
                    }
                    t.names.remove(n);
                    t.changes.push(next_generation, ChangeKey::Handle(existing));
                }
            }
        }
    }

    if let Some(ref n) = name {
        t.changes.push(next_generation, ChangeKey::Name(n.clone()));
    }
    for a in aliases {
        t.changes.push(next_generation, ChangeKey::Name(a.clone()));
    }
    REGISTRY.next_handle.store(next_handle, Ordering::SeqCst);
    t.objects.insert(
        handle,
        Entry {
            kind,
            name: name.clone(),
            aliases: aliases.to_vec(),
            content_hash,
            object: Arc::new(object),
        },
    );
    if let Some(n) = name {
        t.names.insert(n, handle);
    }
    for a in aliases {
        t.aliases.insert(a.clone(), handle);
    }
    REGISTRY.generation.store(next_generation, Ordering::SeqCst);
    handle
}

/// Resolve a human identifier (CUSIP / ISIN / free name) — or, as a fallback,
/// an eviction-slot name — to a handle.
pub fn resolve_alias(name: &str) -> Option<Handle> {
    let t = read_tables()?;
    t.aliases
        .get(name)
        .copied()
        .or_else(|| t.names.get(name).copied())
}

/// Run `f` against the typed object behind `handle`, returning `None` if
/// the handle is unknown or the type doesn't match.
///
/// The registry read lock is held only long enough to clone the object's
/// `Arc`; `f` runs after the lock is released. This is what makes nested
/// lookups (and concurrent multi-threaded analytics from the bindings) safe —
/// see [`Entry::object`].
pub fn with_object<T: Any + Send + Sync, R, F: FnOnce(&T) -> R>(handle: Handle, f: F) -> Option<R> {
    let object = {
        let t = read_tables()?;
        Arc::clone(&t.objects.get(&handle)?.object)
    };
    object.downcast_ref::<T>().map(f)
}

/// Returns the kind/tag of an object, or `None` if the handle is unknown.
pub fn kind_of(handle: Handle) -> Option<ObjectKind> {
    read_tables()?.objects.get(&handle).map(|e| e.kind)
}

/// Returns the optional name attached to a handle.
pub fn name_of(handle: Handle) -> Option<String> {
    read_tables()?
        .objects
        .get(&handle)
        .and_then(|e| e.name.clone())
}

/// Drop an object. Returns `true` if it existed.
pub fn release(handle: Handle) -> bool {
    let Some(mut t) = write_tables() else {
        return false;
    };
    let Some(next_generation) = next_generation() else {
        return false;
    };
    if let Some(entry) = t.objects.remove(&handle) {
        t.changes.push(next_generation, ChangeKey::Handle(handle));
        if let Some(name) = entry.name {
            t.names.remove(&name);
            t.changes.push(next_generation, ChangeKey::Name(name));
        }
        for a in &entry.aliases {
            if t.aliases.get(a) == Some(&handle) {
                t.aliases.remove(a);
                t.changes.push(next_generation, ChangeKey::Name(a.clone()));
            }
        }
        REGISTRY.generation.store(next_generation, Ordering::SeqCst);
        true
    } else {
        false
    }
}

/// All objects matching an optional kind filter, as `(handle, kind, name)`.
/// The name is the human identifier (CUSIP/ISIN/free name) when the object
/// has one; the eviction-slot name (cell address) is the fallback.
pub fn list(filter: Option<ObjectKind>) -> Option<Vec<(Handle, ObjectKind, Option<String>)>> {
    Some(
        read_tables()?
            .objects
            .iter()
            .filter(|(_, e)| filter.is_none_or(|f| f == e.kind))
            .map(|(&h, e)| {
                let display = e.aliases.first().cloned().or_else(|| e.name.clone());
                (h, e.kind, display)
            })
            .collect(),
    )
}

/// Number of objects, or unavailable after a poisoned lock.
pub fn object_count() -> Option<usize> {
    Some(read_tables()?.objects.len())
}

/// Drop all objects. Used by tests and the `clear all` ribbon command.
pub fn clear_all() {
    let Some(mut t) = write_tables() else {
        return;
    };
    let Some(next_generation) = next_generation() else {
        return;
    };
    t.objects.clear();
    t.names.clear();
    t.aliases.clear();
    t.changes.reset(next_generation);
    REGISTRY.generation.store(next_generation, Ordering::SeqCst);
}

/// References whose resolution changed after `cursor`, a generation returned
/// by an earlier call or by [`generation`]. Idempotent re-registration and
/// refused registrations record nothing. `None` only after a poisoned lock.
pub fn changes_since(cursor: u64) -> Option<Changes> {
    let t = read_tables()?;
    // Writers store the generation while holding the write lock, so it is
    // consistent with the log under this read lock.
    let current = generation();
    if cursor > current || cursor < t.changes.floor {
        return Some(Changes {
            cursor: current,
            complete: false,
            names: Vec::new(),
            handles: Vec::new(),
        });
    }
    let mut names = std::collections::BTreeSet::new();
    let mut handles = std::collections::BTreeSet::new();
    for (g, key) in t.changes.entries.iter().rev() {
        if *g <= cursor {
            break;
        }
        match key {
            ChangeKey::Name(n) => {
                names.insert(n.clone());
            }
            ChangeKey::Handle(h) => {
                handles.insert(*h);
            }
        }
    }
    Some(Changes {
        cursor: current,
        complete: true,
        names: names.into_iter().collect(),
        handles: handles.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // The registry is process-global and the test harness runs in parallel;
    // serialize these tests so generation/handle assertions don't race.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reg(name: &str, hash: u64) -> Handle {
        register(
            hash, // any Send+Sync value works as the object
            ObjectKind::Bond(BondKind::FixedRate),
            Some(name.to_string()),
            &[],
            hash,
        )
    }

    #[test]
    fn same_name_same_hash_reuses_handle_without_generation_bump() {
        let _g = TEST_LOCK.lock().unwrap();
        let h1 = reg("idem-cell", 42);
        let gen_before = generation();
        let h2 = reg("idem-cell", 42);
        assert_eq!(h1, h2);
        assert_eq!(
            generation(),
            gen_before,
            "idempotent re-register must not bump generation"
        );
        release(h1);
    }

    #[test]
    fn same_name_different_hash_evicts_and_mints_new_handle() {
        let _g = TEST_LOCK.lock().unwrap();
        let h1 = reg("edit-cell", 1);
        let gen_before = generation();
        let h2 = reg("edit-cell", 2);
        assert_ne!(h1, h2);
        assert!(generation() > gen_before);
        assert!(kind_of(h1).is_none(), "old handle must be evicted");
        assert!(kind_of(h2).is_some());
        release(h2);
    }

    #[test]
    fn aliases_resolve_and_follow_last_write() {
        let _g = TEST_LOCK.lock().unwrap();
        let h1 = register(
            1u64,
            ObjectKind::Bond(BondKind::FixedRate),
            Some("cell-a".into()),
            &["912828YK0".into()],
            10,
        );
        assert_eq!(resolve_alias("912828YK0"), Some(h1));
        // Same CUSIP built from a different cell: alias follows the new build.
        let h2 = register(
            2u64,
            ObjectKind::Bond(BondKind::FixedRate),
            Some("cell-b".into()),
            &["912828YK0".into()],
            11,
        );
        assert_eq!(resolve_alias("912828YK0"), Some(h2));
        // The first cell's own handle still works.
        assert!(kind_of(h1).is_some());
        release(h1);
        release(h2);
        assert_eq!(resolve_alias("912828YK0"), None);
    }

    #[test]
    fn resolve_alias_falls_back_to_slot_name() {
        let _g = TEST_LOCK.lock().unwrap();
        let h = reg("Sheet1!R5C2", 7);
        assert_eq!(resolve_alias("Sheet1!R5C2"), Some(h));
        release(h);
    }

    #[test]
    fn release_and_clear_bump_generation_reads_do_not() {
        let _g = TEST_LOCK.lock().unwrap();
        let h = reg("gen-cell", 3);
        let g1 = generation();
        let _ = kind_of(h);
        let _ = name_of(h);
        let _ = list(None);
        let _ = object_count();
        let _ = resolve_alias("gen-cell");
        assert_eq!(generation(), g1, "reads must not bump generation");
        release(h);
        assert!(generation() > g1);
        let g2 = generation();
        clear_all();
        assert!(generation() > g2);
    }

    #[test]
    fn release_of_stale_alias_owner_keeps_current_owner() {
        let _g = TEST_LOCK.lock().unwrap();
        let h1 = register(
            1u64,
            ObjectKind::Curve,
            Some("cell-1".into()),
            &["USD.SOFR".into()],
            20,
        );
        let h2 = register(
            2u64,
            ObjectKind::Curve,
            Some("cell-2".into()),
            &["USD.SOFR".into()],
            21,
        );
        // h1 no longer owns the alias; releasing it must not remove it.
        release(h1);
        assert_eq!(resolve_alias("USD.SOFR"), Some(h2));
        release(h2);
    }
    #[test]
    fn lookup_survives_eviction_and_does_not_hold_lock_during_callback() {
        use std::sync::mpsc;
        use std::time::Duration;
        let _g = TEST_LOCK.lock().unwrap();
        let h = reg("concurrent-cell", 17);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (updated_tx, updated_rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            with_object::<u64, _, _>(h, |value| {
                entered_tx.send(()).unwrap();
                let new_handle = updated_rx
                    .recv_timeout(Duration::from_secs(2))
                    .expect("writer blocked by callback read lock");
                assert_eq!(*value, 17, "in-flight Arc must retain evicted value");
                assert!(kind_of(h).is_none());
                assert_eq!(with_object::<u64, _, _>(new_handle, |v| *v), Some(18));
                new_handle
            })
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let writer = std::thread::spawn(move || {
            let new_handle = reg("concurrent-cell", 18);
            updated_tx.send(new_handle).unwrap();
        });
        let new_handle = reader.join().unwrap().unwrap();
        writer.join().unwrap();
        assert!(release(new_handle));
    }

    #[test]
    fn registry_capacity_rejects_without_mutating_and_allows_replacement() {
        let _g = TEST_LOCK.lock().unwrap();
        clear_all();
        for index in 0..MAX_OBJECTS {
            assert_ne!(
                reg(&format!("bounded-{index}"), index as u64),
                INVALID_HANDLE
            );
        }
        let before = generation();
        assert_eq!(reg("over-capacity", 1), INVALID_HANDLE);
        assert_eq!(generation(), before);
        let refused = changes_since(before).unwrap();
        assert!(refused.complete && refused.names.is_empty() && refused.handles.is_empty());
        assert_eq!(object_count(), Some(MAX_OBJECTS));
        let replacement = reg("bounded-0", 99);
        assert_ne!(replacement, INVALID_HANDLE);
        assert_eq!(object_count(), Some(MAX_OBJECTS));
        assert_eq!(with_object::<u64, _, _>(replacement, |v| *v), Some(99));
        clear_all();
    }

    fn names(c: &Changes) -> Vec<&str> {
        c.names.iter().map(String::as_str).collect()
    }

    #[test]
    fn change_log_records_new_slot_and_aliases_and_nothing_on_idempotent_rebuild() {
        let _g = TEST_LOCK.lock().unwrap();
        let cursor = generation();
        let h = register(
            1u64,
            ObjectKind::Curve,
            Some("log-cell".into()),
            &["LOG.SOFR".into()],
            30,
        );
        let c = changes_since(cursor).unwrap();
        assert!(c.complete);
        assert_eq!(c.cursor, generation());
        assert_eq!(names(&c), ["LOG.SOFR", "log-cell"]);
        assert!(c.handles.is_empty(), "a new handle cannot have readers yet");

        let again = register(
            1u64,
            ObjectKind::Curve,
            Some("log-cell".into()),
            &["LOG.SOFR".into()],
            30,
        );
        assert_eq!(again, h);
        let idle = changes_since(c.cursor).unwrap();
        assert_eq!(idle.cursor, c.cursor);
        assert!(idle.complete && idle.names.is_empty() && idle.handles.is_empty());
        release(h);
    }

    #[test]
    fn change_log_rebuild_reports_old_handle_and_every_name_that_moved() {
        let _g = TEST_LOCK.lock().unwrap();
        let h1 = register(
            1u64,
            ObjectKind::Curve,
            Some("rebuild-cell".into()),
            &["OLD.ALIAS".into(), "KEPT.ALIAS".into()],
            40,
        );
        let cursor = generation();
        let h2 = register(
            2u64,
            ObjectKind::Curve,
            Some("rebuild-cell".into()),
            &["KEPT.ALIAS".into(), "NEW.ALIAS".into()],
            41,
        );
        let c = changes_since(cursor).unwrap();
        assert!(c.complete);
        assert_eq!(
            names(&c),
            ["KEPT.ALIAS", "NEW.ALIAS", "OLD.ALIAS", "rebuild-cell"]
        );
        assert_eq!(c.handles, [h1]);
        release(h2);
    }

    #[test]
    fn change_log_release_reports_only_aliases_the_handle_owned() {
        let _g = TEST_LOCK.lock().unwrap();
        let h1 = register(
            1u64,
            ObjectKind::Curve,
            Some("owner-1".into()),
            &["SHARED.ALIAS".into()],
            50,
        );
        let h2 = register(
            2u64,
            ObjectKind::Curve,
            Some("owner-2".into()),
            &["SHARED.ALIAS".into()],
            51,
        );
        let cursor = generation();
        release(h1);
        let c = changes_since(cursor).unwrap();
        assert!(c.complete);
        assert_eq!(names(&c), ["owner-1"], "SHARED.ALIAS still resolves to h2");
        assert_eq!(c.handles, [h1]);
        let cursor = c.cursor;
        assert!(!release(h1), "second release is a no-op");
        assert!(changes_since(cursor).unwrap().names.is_empty());
        release(h2);
    }

    #[test]
    fn change_log_is_incomplete_after_clear_overflow_or_a_future_cursor() {
        let _g = TEST_LOCK.lock().unwrap();
        let before_clear = generation();
        clear_all();
        let c = changes_since(before_clear).unwrap();
        assert!(!c.complete, "clear must force a full recompute");
        assert!(changes_since(c.cursor).unwrap().complete);
        assert!(!changes_since(c.cursor + 1).unwrap().complete);

        let cursor = c.cursor;
        let h = reg("overflow-cell", 0);
        for i in 1..=CHANGE_LOG_CAPACITY as u64 {
            assert_ne!(reg("overflow-cell", i), INVALID_HANDLE);
        }
        assert!(
            !changes_since(cursor).unwrap().complete,
            "a cursor older than the ring must be told to recompute everything"
        );
        let recent = generation() - 1;
        let tail = changes_since(recent).unwrap();
        assert!(tail.complete);
        assert_eq!(names(&tail), ["overflow-cell"]);
        assert_eq!(tail.handles.len(), 1);
        assert!(kind_of(h).is_none());
        clear_all();
    }
}
