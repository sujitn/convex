//! Type-erased object registry behind opaque handles.
//!
//! All bonds and curves live here. Handles are monotonic `u64`s starting at
//! 100 (so `#CX#100`, `#CX#101`, … are visually distinguishable from random
//! integers in spreadsheet diagnostics).

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

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
    name: Option<String>,
    object: Box<dyn Any + Send + Sync>,
}

/// Both maps live behind a single lock so name↔handle updates stay atomic.
/// Splitting them into two locks let a reader observe the new name binding
/// before the new handle was inserted (or vice-versa during release).
#[derive(Default)]
struct Tables {
    objects: HashMap<Handle, Entry>,
    names: HashMap<String, Handle>,
}

struct Inner {
    next_handle: AtomicU64,
    tables: RwLock<Tables>,
}

impl Inner {
    fn new() -> Self {
        Self {
            next_handle: AtomicU64::new(100),
            tables: RwLock::new(Tables::default()),
        }
    }
}

static REGISTRY: Lazy<Inner> = Lazy::new(Inner::new);

/// Register an object. If `name` is provided and already exists, the prior
/// handle is released first (so dependent cells see a fresh handle).
pub fn register<T: Any + Send + Sync>(object: T, kind: ObjectKind, name: Option<String>) -> Handle {
    let handle = REGISTRY.next_handle.fetch_add(1, Ordering::SeqCst);
    let mut t = REGISTRY.tables.write().unwrap();
    if let Some(ref n) = name {
        if let Some(old) = t.names.get(n).copied() {
            t.objects.remove(&old);
        }
    }
    t.objects.insert(
        handle,
        Entry {
            kind,
            name: name.clone(),
            object: Box::new(object),
        },
    );
    if let Some(n) = name {
        t.names.insert(n, handle);
    }
    handle
}

/// Run `f` against the typed object behind `handle`, returning `None` if
/// the handle is unknown or the type doesn't match.
pub fn with_object<T: Any + Send + Sync, R, F: FnOnce(&T) -> R>(handle: Handle, f: F) -> Option<R> {
    let t = REGISTRY.tables.read().unwrap();
    t.objects
        .get(&handle)
        .and_then(|e| e.object.downcast_ref::<T>())
        .map(f)
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

/// Look up a handle by name.
#[allow(dead_code)]
pub fn lookup(name: &str) -> Option<Handle> {
    REGISTRY.tables.read().unwrap().names.get(name).copied()
}

/// Drop an object. Returns `true` if it existed.
pub fn release(handle: Handle) -> bool {
    let mut t = REGISTRY.tables.write().unwrap();
    if let Some(entry) = t.objects.remove(&handle) {
        if let Some(name) = entry.name {
            t.names.remove(&name);
        }
        true
    } else {
        false
    }
}

/// All objects matching an optional kind filter, as `(handle, kind, name)`.
pub fn list(filter: Option<ObjectKind>) -> Vec<(Handle, ObjectKind, Option<String>)> {
    REGISTRY
        .tables
        .read()
        .unwrap()
        .objects
        .iter()
        .filter(|(_, e)| filter.map_or(true, |f| f == e.kind))
        .map(|(&h, e)| (h, e.kind, e.name.clone()))
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
}
