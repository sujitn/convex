//! # Convex FFI — JSON-RPC boundary
//!
//! Twelve C functions cover the entire surface. Five are stateful (build,
//! describe, release, count, clear). Five are stateless analytics RPCs that
//! consume one JSON request and return one JSON response. Two are utilities
//! (schema introspection, mark text parser).
//!
//! Adding a new bond shape, a new spread family, or a new analytic does not
//! add a new C symbol. It is a serde enum variant in `convex_analytics::dto`
//! plus a dispatch arm in `dispatch`.
//!
//! ## Memory model
//!
//! All functions returning `*const c_char` return a heap-allocated, null-
//! terminated UTF-8 string owned by Rust. The caller MUST free it with
//! [`convex_string_free`]. Inputs are borrowed; the FFI never takes ownership
//! of caller-allocated buffers.
//!
//! ## Error model
//!
//! Stateful constructors return `0` (`INVALID_HANDLE`) on failure; the caller
//! reads [`convex_last_error`] for diagnostics. Stateless RPCs always return
//! a JSON envelope `{"ok": "true", "result": …}` or `{"ok": "false", "error":
//! {"code","message","field?"}}` — there is no out-of-band error path.

#![allow(clippy::missing_safety_doc)]

mod build;
mod dispatch;
mod error;
mod registry;
mod schemas;

use std::ffi::{CStr, CString};

use libc::c_char;

pub use registry::{Handle, INVALID_HANDLE};

// ============================================================================
// Boundary helpers
// ============================================================================

/// Free a string returned by any FFI function in this crate.
#[no_mangle]
pub unsafe extern "C" fn convex_string_free(s: *mut c_char) {
    if !s.is_null() {
        let _ = CString::from_raw(s);
    }
}

/// Get the last error message set by a stateful constructor.
///
/// The returned pointer is valid until the next call into this library on
/// the same thread; do not free it.
#[no_mangle]
pub extern "C" fn convex_last_error() -> *const c_char {
    error::last_error_message()
}

/// Clear the thread-local last-error slot.
#[no_mangle]
pub extern "C" fn convex_clear_error() {
    error::clear_error()
}

/// Library version (`CARGO_PKG_VERSION`). Static; do not free.
#[no_mangle]
pub extern "C" fn convex_version() -> *const c_char {
    static VERSION: once_cell::sync::Lazy<CString> =
        once_cell::sync::Lazy::new(|| CString::new(env!("CARGO_PKG_VERSION")).unwrap());
    VERSION.as_ptr()
}

// ============================================================================
// Construction (handles)
// ============================================================================

/// Build a bond from a `BondSpec` JSON. Returns `0` on failure.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_from_json(spec_json: *const c_char) -> Handle {
    with_str(spec_json, |s| build::bond_from_json(s))
}

/// Build a curve from a `CurveSpec` JSON. Returns `0` on failure.
#[no_mangle]
pub unsafe extern "C" fn convex_curve_from_json(spec_json: *const c_char) -> Handle {
    with_str(spec_json, |s| build::curve_from_json(s))
}

/// Returns a JSON description of the registered object.
///
/// Free the returned pointer with [`convex_string_free`].
#[no_mangle]
pub extern "C" fn convex_describe(handle: Handle) -> *mut c_char {
    to_owned_c(dispatch::describe(handle))
}

/// Releases an object by handle. No-op on invalid handle.
#[no_mangle]
pub extern "C" fn convex_release(handle: Handle) {
    registry::release(handle);
}

/// Number of registered objects.
#[no_mangle]
pub extern "C" fn convex_object_count() -> i32 {
    registry::object_count() as i32
}

/// Returns a JSON array of `{handle,kind,name?}` entries for every registered
/// object. Caller frees with [`convex_string_free`].
#[no_mangle]
pub extern "C" fn convex_list_objects() -> *mut c_char {
    let entries: Vec<_> = registry::list(None)
        .into_iter()
        .map(|(h, kind, name)| {
            serde_json::json!({
                "handle": h,
                "kind": kind.tag(),
                "name": name,
            })
        })
        .collect();
    let body = serde_json::json!({"ok": "true", "result": entries});
    to_owned_c(body.to_string())
}

/// Clears all registered objects.
#[no_mangle]
pub extern "C" fn convex_clear_all() {
    registry::clear_all()
}

// ============================================================================
// Stateless analytics RPCs (JSON in, JSON out)
// ============================================================================

/// Price a bond. Request: `PricingRequest`. Response: `PricingResponse`.
#[no_mangle]
pub unsafe extern "C" fn convex_price(request_json: *const c_char) -> *mut c_char {
    rpc(request_json, dispatch::price)
}

/// Risk metrics. Request: `RiskRequest`. Response: `RiskResponse`.
#[no_mangle]
pub unsafe extern "C" fn convex_risk(request_json: *const c_char) -> *mut c_char {
    rpc(request_json, dispatch::risk)
}

/// Spread (Z/G/I/ASW/OAS/DM dispatch). Request: `SpreadRequest`. Response:
/// `SpreadResponse`.
#[no_mangle]
pub unsafe extern "C" fn convex_spread(request_json: *const c_char) -> *mut c_char {
    rpc(request_json, dispatch::spread)
}

/// Bond cashflow schedule. Request: `CashflowRequest`. Response: `CashflowResponse`.
#[no_mangle]
pub unsafe extern "C" fn convex_cashflows(request_json: *const c_char) -> *mut c_char {
    rpc(request_json, dispatch::cashflows)
}

/// Curve point query. Request: `CurveQueryRequest`. Response: `CurveQueryResponse`.
#[no_mangle]
pub unsafe extern "C" fn convex_curve_query(request_json: *const c_char) -> *mut c_char {
    rpc(request_json, dispatch::curve_query)
}

// ============================================================================
// Introspection
// ============================================================================

/// Returns the JSON schema for a named DTO type, or an error envelope.
///
/// Type names: `Mark`, `BondSpec`, `CurveSpec`, `PricingRequest`,
/// `PricingResponse`, `RiskRequest`, `RiskResponse`, `SpreadRequest`,
/// `SpreadResponse`, `CashflowRequest`, `CashflowResponse`,
/// `CurveQueryRequest`, `CurveQueryResponse`.
#[no_mangle]
pub unsafe extern "C" fn convex_schema(type_name: *const c_char) -> *mut c_char {
    let result = with_str_owned(type_name, |s| schemas::lookup(s));
    to_owned_c(match result {
        Ok(json) => format!(r#"{{"ok":"true","result":{json}}}"#),
        Err(msg) => err_envelope("schema", &msg),
    })
}

/// Parse a textual mark (e.g. `"99.5C"`, `"4.65%"`, `"+125bps@USD.SOFR"`).
///
/// Returns a JSON envelope; on success, `result` is the canonical `Mark` JSON.
#[no_mangle]
pub unsafe extern "C" fn convex_mark_parse(text: *const c_char) -> *mut c_char {
    let payload = with_str_owned(text, |s| {
        s.parse::<convex_core::types::Mark>()
            .map_err(|e| e.to_string())
    });
    to_owned_c(match payload {
        Ok(mark) => match serde_json::to_string(&mark) {
            Ok(json) => format!(r#"{{"ok":"true","result":{json}}}"#),
            Err(e) => err_envelope("serialize", &e.to_string()),
        },
        Err(e) => err_envelope("invalid_input", &e),
    })
}

// ============================================================================
// Internal helpers
// ============================================================================

unsafe fn with_str<F, R>(ptr: *const c_char, f: F) -> R
where
    F: FnOnce(&str) -> R,
    R: Default,
{
    if ptr.is_null() {
        error::set_last_error("null pointer");
        return R::default();
    }
    match CStr::from_ptr(ptr).to_str() {
        Ok(s) => f(s),
        Err(e) => {
            error::set_last_error(format!("invalid UTF-8: {e}"));
            R::default()
        }
    }
}

unsafe fn with_str_owned<F, T>(ptr: *const c_char, f: F) -> Result<T, String>
where
    F: FnOnce(&str) -> Result<T, String>,
{
    if ptr.is_null() {
        return Err("null pointer".to_string());
    }
    let s = CStr::from_ptr(ptr)
        .to_str()
        .map_err(|e| format!("invalid UTF-8: {e}"))?;
    f(s)
}

unsafe fn rpc(request_json: *const c_char, handler: fn(&str) -> String) -> *mut c_char {
    let response = match with_str_owned(request_json, |s| Ok::<String, String>(handler(s))) {
        Ok(r) => r,
        Err(e) => err_envelope("invalid_input", &e),
    };
    to_owned_c(response)
}

fn to_owned_c(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

pub(crate) fn err_envelope(code: &str, message: &str) -> String {
    let body = serde_json::json!({
        "ok": "false",
        "error": { "code": code, "message": message }
    });
    body.to_string()
}

pub(crate) fn err_envelope_field(code: &str, message: &str, field: &str) -> String {
    let body = serde_json::json!({
        "ok": "false",
        "error": { "code": code, "message": message, "field": field }
    });
    body.to_string()
}

pub(crate) fn ok_envelope<T: serde::Serialize>(result: &T) -> String {
    match serde_json::to_value(result) {
        Ok(v) => serde_json::json!({"ok":"true","result": v}).to_string(),
        Err(e) => err_envelope("serialize", &e.to_string()),
    }
}
