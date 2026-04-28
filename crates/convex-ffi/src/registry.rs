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

struct Inner {
    next_handle: AtomicU64,
    objects: RwLock<HashMap<Handle, Entry>>,
    names: RwLock<HashMap<String, Handle>>,
}

impl Inner {
    fn new() -> Self {
        Self {
            next_handle: AtomicU64::new(100),
            objects: RwLock::new(HashMap::new()),
            names: RwLock::new(HashMap::new()),
        }
    }
}

static REGISTRY: Lazy<Inner> = Lazy::new(Inner::new);

/// Register an object. If `name` is provided and already exists, the prior
/// handle is released first (so dependent cells see a fresh handle).
pub fn register<T: Any + Send + Sync>(object: T, kind: ObjectKind, name: Option<String>) -> Handle {
    if let Some(ref n) = name {
        let prior = REGISTRY.names.read().unwrap().get(n).copied();
        if let Some(old) = prior {
            REGISTRY.objects.write().unwrap().remove(&old);
        }
    }
    let handle = REGISTRY.next_handle.fetch_add(1, Ordering::SeqCst);
    REGISTRY.objects.write().unwrap().insert(
        handle,
        Entry {
            kind,
            name: name.clone(),
            object: Box::new(object),
        },
    );
    if let Some(n) = name {
        REGISTRY.names.write().unwrap().insert(n, handle);
    }
    handle
}

/// Run `f` against the typed object behind `handle`, returning `None` if
/// the handle is unknown or the type doesn't match.
pub fn with_object<T: Any + Send + Sync, R, F: FnOnce(&T) -> R>(handle: Handle, f: F) -> Option<R> {
    let objects = REGISTRY.objects.read().unwrap();
    objects
        .get(&handle)
        .and_then(|e| e.object.downcast_ref::<T>())
        .map(f)
}

/// Returns the kind/tag of an object, or `None` if the handle is unknown.
pub fn kind_of(handle: Handle) -> Option<ObjectKind> {
    REGISTRY
        .objects
        .read()
        .unwrap()
        .get(&handle)
        .map(|e| e.kind)
}

/// Returns the optional name attached to a handle.
pub fn name_of(handle: Handle) -> Option<String> {
    REGISTRY
        .objects
        .read()
        .unwrap()
        .get(&handle)
        .and_then(|e| e.name.clone())
}

/// Look up a handle by name.
#[allow(dead_code)]
pub fn lookup(name: &str) -> Option<Handle> {
    REGISTRY.names.read().unwrap().get(name).copied()
}

/// Drop an object. Returns `true` if it existed.
pub fn release(handle: Handle) -> bool {
    let mut objects = REGISTRY.objects.write().unwrap();
    if let Some(entry) = objects.remove(&handle) {
        if let Some(name) = entry.name {
            REGISTRY.names.write().unwrap().remove(&name);
        }
        true
    } else {
        false
    }
}

/// All objects matching an optional kind filter, as `(handle, kind, name)`.
pub fn list(filter: Option<ObjectKind>) -> Vec<(Handle, ObjectKind, Option<String>)> {
    REGISTRY
        .objects
        .read()
        .unwrap()
        .iter()
        .filter(|(_, e)| filter.map_or(true, |f| f == e.kind))
        .map(|(&h, e)| (h, e.kind, e.name.clone()))
        .collect()
}

/// Number of registered objects.
pub fn object_count() -> usize {
    REGISTRY.objects.read().unwrap().len()
}

/// Drop all objects. Used by tests and the `clear all` ribbon command.
pub fn clear_all() {
    REGISTRY.objects.write().unwrap().clear();
    REGISTRY.names.write().unwrap().clear();
}
