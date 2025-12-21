//! Thread-safe global object registry for FFI handles.
//!
//! This module provides a centralized registry for storing Convex objects
//! behind opaque handles (u64). Objects can be looked up by handle or by name.
//!
//! # Design
//!
//! - All objects are stored in a global registry protected by `RwLock`
//! - Handles are simple incrementing u64 values
//! - Objects can optionally have names for easy lookup
//! - Type information is preserved for runtime validation
//!
//! # Thread Safety
//!
//! The registry uses `RwLock` for concurrent read access. Write operations
//! (create, release) acquire exclusive locks.

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use once_cell::sync::Lazy;

/// Handle type for FFI objects. Zero indicates invalid/null handle.
pub type Handle = u64;

/// Invalid handle constant.
pub const INVALID_HANDLE: Handle = 0;

/// Object types that can be stored in the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
#[allow(dead_code)] // Some variants are reserved for future use
pub enum ObjectType {
    /// Unknown or invalid type
    Unknown = 0,
    /// Yield curve (RateCurve)
    Curve = 1,
    /// Fixed rate bond
    FixedBond = 2,
    /// Zero coupon bond
    ZeroBond = 3,
    /// Floating rate note
    FloatingRateNote = 4,
    /// Callable bond
    CallableBond = 5,
    /// Cash flow schedule
    CashFlows = 6,
    /// Pricing result
    PriceResult = 7,
    /// Risk metrics result
    RiskResult = 8,
    /// Spread result
    SpreadResult = 9,
    /// YAS analysis result
    YasResult = 10,
}

impl ObjectType {
    /// Returns true if this is a curve type.
    pub fn is_curve(&self) -> bool {
        matches!(self, ObjectType::Curve)
    }

    /// Returns true if this is a bond type.
    pub fn is_bond(&self) -> bool {
        matches!(
            self,
            ObjectType::FixedBond
                | ObjectType::ZeroBond
                | ObjectType::FloatingRateNote
                | ObjectType::CallableBond
        )
    }
}

/// Metadata about a registered object.
#[derive(Debug)]
struct ObjectEntry {
    /// The type of object.
    object_type: ObjectType,
    /// Optional name for lookup.
    name: Option<String>,
    /// The actual object (type-erased).
    object: Box<dyn Any + Send + Sync>,
}

/// Global object registry.
struct Registry {
    /// Next handle to assign.
    next_handle: AtomicU64,
    /// Map from handle to object entry.
    objects: RwLock<HashMap<Handle, ObjectEntry>>,
    /// Map from name to handle for named objects.
    names: RwLock<HashMap<String, Handle>>,
}

impl Registry {
    fn new() -> Self {
        Self {
            // Start at 100 for cleaner handle IDs (#CX#100, #CX#101, ...)
            next_handle: AtomicU64::new(100),
            objects: RwLock::new(HashMap::new()),
            names: RwLock::new(HashMap::new()),
        }
    }

    /// Registers an object and returns its handle.
    /// If a named object already exists, updates it in place and returns the same handle.
    fn register<T: Any + Send + Sync>(
        &self,
        object: T,
        object_type: ObjectType,
        name: Option<String>,
    ) -> Handle {
        // If named, check if object with this name already exists
        if let Some(ref n) = name {
            let names = self.names.read().unwrap();
            if let Some(&existing_handle) = names.get(n) {
                // Update existing object in place, keeping the same handle
                drop(names); // Release read lock before acquiring write lock
                let mut objects = self.objects.write().unwrap();
                if let Some(entry) = objects.get_mut(&existing_handle) {
                    entry.object = Box::new(object);
                    entry.object_type = object_type;
                    return existing_handle;
                }
            }
        }

        // Create new handle for new or unnamed objects
        let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);

        let entry = ObjectEntry {
            object_type,
            name: name.clone(),
            object: Box::new(object),
        };

        // Insert into objects map
        {
            let mut objects = self.objects.write().unwrap();
            objects.insert(handle, entry);
        }

        // Insert into names map if named
        if let Some(ref n) = name {
            let mut names = self.names.write().unwrap();
            names.insert(n.clone(), handle);
        }

        handle
    }

    /// Gets an object by handle with a callback that receives a reference.
    fn with_object<T: Any + Send + Sync, R, F: FnOnce(&T) -> R>(
        &self,
        handle: Handle,
        f: F,
    ) -> Option<R> {
        let objects = self.objects.read().unwrap();
        objects
            .get(&handle)
            .and_then(|entry| entry.object.downcast_ref::<T>())
            .map(f)
    }

    /// Gets the type of an object.
    fn get_type(&self, handle: Handle) -> ObjectType {
        let objects = self.objects.read().unwrap();
        objects
            .get(&handle)
            .map(|e| e.object_type)
            .unwrap_or(ObjectType::Unknown)
    }

    /// Gets the name of an object.
    fn get_name(&self, handle: Handle) -> Option<String> {
        let objects = self.objects.read().unwrap();
        objects.get(&handle).and_then(|e| e.name.clone())
    }

    /// Looks up a handle by name.
    fn lookup(&self, name: &str) -> Option<Handle> {
        let names = self.names.read().unwrap();
        names.get(name).copied()
    }

    /// Releases an object by handle.
    fn release(&self, handle: Handle) -> bool {
        let mut objects = self.objects.write().unwrap();
        if let Some(entry) = objects.remove(&handle) {
            // Also remove from names map if named
            if let Some(ref name) = entry.name {
                let mut names = self.names.write().unwrap();
                names.remove(name);
            }
            true
        } else {
            false
        }
    }

    /// Clones an object, returning a new handle.
    #[allow(dead_code)]
    fn clone_object<T: Any + Send + Sync + Clone>(&self, handle: Handle) -> Option<Handle> {
        let cloned = {
            let objects = self.objects.read().unwrap();
            let entry = objects.get(&handle)?;
            let obj = entry.object.downcast_ref::<T>()?;
            (obj.clone(), entry.object_type)
        };

        Some(self.register(cloned.0, cloned.1, None))
    }

    /// Lists all objects of a given type.
    fn list_objects(
        &self,
        filter_type: Option<ObjectType>,
    ) -> Vec<(Handle, ObjectType, Option<String>)> {
        let objects = self.objects.read().unwrap();
        objects
            .iter()
            .filter(|(_, entry)| filter_type.is_none() || filter_type == Some(entry.object_type))
            .map(|(&handle, entry)| (handle, entry.object_type, entry.name.clone()))
            .collect()
    }

    /// Returns the number of objects in the registry.
    fn count(&self) -> usize {
        let objects = self.objects.read().unwrap();
        objects.len()
    }

    /// Clears all objects from the registry.
    fn clear(&self) {
        let mut objects = self.objects.write().unwrap();
        let mut names = self.names.write().unwrap();
        objects.clear();
        names.clear();
    }
}

/// Global registry instance.
static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

// ============================================================================
// Public API
// ============================================================================

/// Registers an object in the global registry.
///
/// # Arguments
///
/// * `object` - The object to register
/// * `object_type` - The type classification of the object
/// * `name` - Optional name for lookup
///
/// # Returns
///
/// A unique handle for the object.
pub fn register<T: Any + Send + Sync>(
    object: T,
    object_type: ObjectType,
    name: Option<String>,
) -> Handle {
    REGISTRY.register(object, object_type, name)
}

/// Accesses an object by handle with a callback.
///
/// This is the preferred way to access objects as it handles locking correctly.
///
/// # Arguments
///
/// * `handle` - The handle returned from `register`
/// * `f` - Callback that receives a reference to the object
///
/// # Returns
///
/// `Some(result)` if the object exists and has the correct type, `None` otherwise.
pub fn with_object<T: Any + Send + Sync, R, F: FnOnce(&T) -> R>(handle: Handle, f: F) -> Option<R> {
    REGISTRY.with_object(handle, f)
}

/// Gets the type of an object by handle.
pub fn get_type(handle: Handle) -> ObjectType {
    REGISTRY.get_type(handle)
}

/// Gets the name of an object by handle.
pub fn get_name(handle: Handle) -> Option<String> {
    REGISTRY.get_name(handle)
}

/// Looks up a handle by name.
pub fn lookup(name: &str) -> Option<Handle> {
    REGISTRY.lookup(name)
}

/// Releases an object by handle.
///
/// # Returns
///
/// `true` if the object was found and released, `false` otherwise.
pub fn release(handle: Handle) -> bool {
    REGISTRY.release(handle)
}

/// Clones an object, returning a new handle.
#[allow(dead_code)]
pub fn clone_object<T: Any + Send + Sync + Clone>(handle: Handle) -> Option<Handle> {
    REGISTRY.clone_object::<T>(handle)
}

/// Lists all objects, optionally filtered by type.
pub fn list_objects(filter_type: Option<ObjectType>) -> Vec<(Handle, ObjectType, Option<String>)> {
    REGISTRY.list_objects(filter_type)
}

/// Returns the number of objects in the registry.
pub fn object_count() -> usize {
    REGISTRY.count()
}

/// Clears all objects from the registry.
///
/// # Safety
///
/// This invalidates all existing handles. Only use for testing or cleanup.
pub fn clear_all() {
    REGISTRY.clear();
}

// ============================================================================
// FFI Functions for Object Enumeration
// ============================================================================

use libc::{c_char, c_int};
use std::ffi::CString;

/// Callback type for object enumeration.
pub type ObjectEnumCallback =
    extern "C" fn(handle: Handle, object_type: c_int, name: *const c_char);

/// Enumerates all objects in the registry, calling the callback for each.
///
/// # Arguments
///
/// * `callback` - Function to call for each object
/// * `filter_type` - Object type to filter by (0 = all, 1 = Curve, 2 = FixedBond, etc.)
///
/// # Safety
///
/// The callback must be a valid function pointer.
#[no_mangle]
pub unsafe extern "C" fn convex_enumerate_objects(
    callback: ObjectEnumCallback,
    filter_type: c_int,
) {
    let filter = if filter_type == 0 {
        None
    } else {
        Some(match filter_type {
            1 => ObjectType::Curve,
            2 => ObjectType::FixedBond,
            3 => ObjectType::ZeroBond,
            4 => ObjectType::FloatingRateNote,
            5 => ObjectType::CallableBond,
            _ => ObjectType::Unknown,
        })
    };

    let objects = list_objects(filter);

    for (handle, obj_type, name) in objects {
        let name_cstring = name
            .map(|n| CString::new(n).unwrap_or_default())
            .unwrap_or_else(|| CString::new("").unwrap());

        callback(handle, obj_type as c_int, name_cstring.as_ptr());
    }
}

/// Gets the name of an object by handle.
///
/// # Arguments
///
/// * `handle` - The object handle
/// * `buffer` - Buffer to write the name into
/// * `buffer_len` - Length of the buffer
///
/// # Returns
///
/// The length of the name written, or -1 if the handle is invalid.
///
/// # Safety
///
/// `buffer` must be a valid pointer to a buffer of at least `buffer_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn convex_get_name(
    handle: Handle,
    buffer: *mut c_char,
    buffer_len: c_int,
) -> c_int {
    if buffer.is_null() || buffer_len <= 0 {
        return -1;
    }

    let name = match get_name(handle) {
        Some(n) => n,
        None => return -1,
    };

    let bytes = name.as_bytes();
    let copy_len = std::cmp::min(bytes.len(), (buffer_len - 1) as usize);

    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer as *mut u8, copy_len);
    *buffer.add(copy_len) = 0; // Null terminator

    copy_len as c_int
}

/// Gets the type of an object by handle.
///
/// # Returns
///
/// The object type as an integer, or 0 for invalid handles.
#[no_mangle]
pub extern "C" fn convex_get_type(handle: Handle) -> c_int {
    get_type(handle) as c_int
}

/// Releases an object by handle.
///
/// # Returns
///
/// 0 on success, -1 if handle was invalid.
#[no_mangle]
pub extern "C" fn convex_release(handle: Handle) -> c_int {
    if release(handle) {
        0
    } else {
        -1
    }
}

/// Looks up a handle by name.
///
/// # Returns
///
/// The handle if found, or INVALID_HANDLE (0) if not found.
///
/// # Safety
///
/// `name` must be a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn convex_lookup(name: *const c_char) -> Handle {
    if name.is_null() {
        return INVALID_HANDLE;
    }

    let c_str = std::ffi::CStr::from_ptr(name);
    let name_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return INVALID_HANDLE,
    };

    lookup(name_str).unwrap_or(INVALID_HANDLE)
}

/// Returns the number of objects in the registry.
#[no_mangle]
pub extern "C" fn convex_object_count() -> c_int {
    object_count() as c_int
}

/// Clears all objects from the registry.
#[no_mangle]
pub extern "C" fn convex_clear_all() {
    clear_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestObject {
        value: i32,
    }

    #[test]
    fn test_register_and_get() {
        let obj = TestObject { value: 42 };
        let handle = register(obj, ObjectType::Curve, Some("test.curve".to_string()));

        assert_ne!(handle, INVALID_HANDLE);
        assert_eq!(get_type(handle), ObjectType::Curve);
        assert_eq!(get_name(handle), Some("test.curve".to_string()));

        let value = with_object::<TestObject, _, _>(handle, |o| o.value);
        assert_eq!(value, Some(42));

        release(handle);
    }

    #[test]
    fn test_lookup_by_name() {
        let obj = TestObject { value: 100 };
        let handle = register(obj, ObjectType::FixedBond, Some("LOOKUP.TEST".to_string()));

        let found = lookup("LOOKUP.TEST");
        assert_eq!(found, Some(handle));

        release(handle);
        assert_eq!(lookup("LOOKUP.TEST"), None);
    }

    #[test]
    fn test_clone_object() {
        let obj = TestObject { value: 999 };
        let handle1 = register(obj, ObjectType::Curve, None);

        let handle2 = clone_object::<TestObject>(handle1).unwrap();
        assert_ne!(handle1, handle2);

        let value1 = with_object::<TestObject, _, _>(handle1, |o| o.value);
        let value2 = with_object::<TestObject, _, _>(handle2, |o| o.value);
        assert_eq!(value1, value2);

        release(handle1);
        release(handle2);
    }

    #[test]
    fn test_list_objects() {
        // Get baseline count (other parallel tests may have added objects)
        let initial_count = object_count();

        let h1 = register(TestObject { value: 1 }, ObjectType::Curve, None);
        let h2 = register(TestObject { value: 2 }, ObjectType::FixedBond, None);
        let h3 = register(TestObject { value: 3 }, ObjectType::Curve, None);

        let all = list_objects(None);
        // Check we have at least the 3 we added
        assert!(all.len() >= initial_count + 3);

        // Check our specific handles are in the list
        let handles: Vec<_> = all.iter().map(|(h, _, _)| *h).collect();
        assert!(handles.contains(&h1));
        assert!(handles.contains(&h2));
        assert!(handles.contains(&h3));

        release(h1);
        release(h2);
        release(h3);
    }

    #[test]
    fn test_invalid_handle() {
        let value = with_object::<TestObject, _, _>(INVALID_HANDLE, |o| o.value);
        assert_eq!(value, None);

        assert!(!release(INVALID_HANDLE));
        assert_eq!(get_type(INVALID_HANDLE), ObjectType::Unknown);
    }
}
