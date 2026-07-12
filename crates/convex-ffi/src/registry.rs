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
    /// Eviction-slot name (the owning Excel cell's registry key, or the
    /// spec's own identifier when built outside a cell).
    name: Option<String>,
    /// Human identifiers (CUSIP / ISIN / free name) that resolve to this
    /// handle via [`resolve_alias`]. Kept on the entry so eviction can clean
    /// the alias map in O(aliases) instead of scanning it.
    aliases: Vec<String>,
    /// Hash of the canonicalized spec JSON that produced this object. Lets
    /// [`register`] return the existing handle when a cell re-registers an
    /// identical spec, so Excel's dependency cutoff stops recalc cascades.
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
    /// CUSIP/ISIN/free-name → handle. Last write wins on collision (two cells
    /// building the same CUSIP): the alias follows the most recent build, like
    /// a ticker resolving to the most recently defined instance.
    aliases: HashMap<String, Handle>,
}

struct Inner {
    next_handle: AtomicU64,
    /// Bumped on every mutation (register of new content, release, clear) and
    /// NOT on idempotent re-registration. Callers key caches on this: same
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

/// Registry mutation counter. See [`Inner::generation`].
pub fn generation() -> u64 {
    REGISTRY.generation.load(Ordering::SeqCst)
}

/// Register an object.
///
/// Idempotent per eviction slot: if `name` already holds an object built from
/// the same `content_hash`, the existing handle is returned unchanged — no
/// eviction, no new handle, no generation bump. A builder cell recalculating
/// with unchanged inputs therefore keeps its output value stable and Excel
/// does not cascade recalculation through its dependents.
///
/// If the slot holds *different* content, the prior object is evicted (with
/// its aliases) and a fresh handle is minted, so dependent cells see a change
/// and recompute — the behaviour real edits rely on.
pub fn register<T: Any + Send + Sync>(
    object: T,
    kind: ObjectKind,
    name: Option<String>,
    aliases: &[String],
    content_hash: u64,
) -> Handle {
    let mut t = REGISTRY.tables.write().unwrap();

    if let Some(ref n) = name {
        if let Some(&existing) = t.names.get(n) {
            match t.objects.get(&existing) {
                Some(e) if e.content_hash == content_hash && e.kind == kind => {
                    return existing; // unchanged rebuild — reuse
                }
                _ => {
                    if let Some(old) = t.objects.remove(&existing) {
                        for a in &old.aliases {
                            if t.aliases.get(a) == Some(&existing) {
                                t.aliases.remove(a);
                            }
                        }
                    }
                    t.names.remove(n);
                }
            }
        }
    }

    let handle = REGISTRY.next_handle.fetch_add(1, Ordering::SeqCst);
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
        t.aliases.insert(a.clone(), handle); // last write wins
    }
    REGISTRY.generation.fetch_add(1, Ordering::SeqCst);
    handle
}

/// Resolve a human identifier (CUSIP / ISIN / free name) — or, as a fallback,
/// an eviction-slot name — to a handle.
pub fn resolve_alias(name: &str) -> Option<Handle> {
    let t = REGISTRY.tables.read().unwrap();
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
        let t = REGISTRY.tables.read().unwrap();
        Arc::clone(&t.objects.get(&handle)?.object)
    };
    object.downcast_ref::<T>().map(f)
}

/// Returns the kind/tag of an object, or `None` if the handle is unknown.
pub fn kind_of(handle: Handle) -> Option<ObjectKind> {
    REGISTRY
        .tables
        .read()
        .unwrap()
        .objects
        .get(&handle)
        .map(|e| e.kind)
}

/// Returns the optional name attached to a handle.
pub fn name_of(handle: Handle) -> Option<String> {
    REGISTRY
        .tables
        .read()
        .unwrap()
        .objects
        .get(&handle)
        .and_then(|e| e.name.clone())
}

/// Drop an object. Returns `true` if it existed.
pub fn release(handle: Handle) -> bool {
    let mut t = REGISTRY.tables.write().unwrap();
    if let Some(entry) = t.objects.remove(&handle) {
        if let Some(name) = entry.name {
            t.names.remove(&name);
        }
        for a in &entry.aliases {
            if t.aliases.get(a) == Some(&handle) {
                t.aliases.remove(a);
            }
        }
        REGISTRY.generation.fetch_add(1, Ordering::SeqCst);
        true
    } else {
        false
    }
}

/// All objects matching an optional kind filter, as `(handle, kind, name)`.
/// The name is the human identifier (CUSIP/ISIN/free name) when the object
/// has one; the eviction-slot name (cell address) is the fallback.
pub fn list(filter: Option<ObjectKind>) -> Vec<(Handle, ObjectKind, Option<String>)> {
    REGISTRY
        .tables
        .read()
        .unwrap()
        .objects
        .iter()
        .filter(|(_, e)| filter.is_none_or(|f| f == e.kind))
        .map(|(&h, e)| {
            let display = e.aliases.first().cloned().or_else(|| e.name.clone());
            (h, e.kind, display)
        })
        .collect()
}

/// Number of registered objects.
pub fn object_count() -> usize {
    REGISTRY.tables.read().unwrap().objects.len()
}

/// Drop all objects. Used by tests and the `clear all` ribbon command.
pub fn clear_all() {
    let mut t = REGISTRY.tables.write().unwrap();
    t.objects.clear();
    t.names.clear();
    t.aliases.clear();
    REGISTRY.generation.fetch_add(1, Ordering::SeqCst);
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
        assert_eq!(generation(), gen_before, "idempotent re-register must not bump generation");
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
}
