//! # Convex FFI — JSON-RPC boundary
//!
//! A small set of C functions covers the entire surface. Some are stateful
//! (build, describe, release, count, clear). The rest are stateless analytics
//! RPCs that consume one JSON request and return one JSON response — pricing,
//! risk, spreads, cashflows, curve queries, make-whole, and the hedge advisor
//! (risk profile, propose, compare). Two are utilities (schema introspection,
//! mark text parser).
//!
//! Adding a new bond shape, a new spread family, or a new analytic on an
//! existing request shape does not add a new C symbol — it is a serde enum
//! variant in `convex_analytics::dto` plus a dispatch arm in `dispatch`. A
//! genuinely new request/response verb (like the advisor) adds one thin
//! `#[no_mangle]` symbol that delegates to a `dispatch` handler.
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

#![deny(unsafe_op_in_unsafe_fn, unused_must_use)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::missing_safety_doc,
        clippy::undocumented_unsafe_blocks
    )
)]

#[cfg(panic = "abort")]
compile_error!("convex-ffi requires panic=unwind; build host libraries with --profile excel");

mod build;
mod dispatch;
mod error;
mod ingress;
mod registry;
mod schemas;

use std::ffi::CString;

use libc::c_char;

pub use registry::{Handle, INVALID_HANDLE};

static IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const MAX_IN_FLIGHT: usize = 32;
struct Admission<'a> {
    counter: &'a std::sync::atomic::AtomicUsize,
}
impl<'a> Admission<'a> {
    fn acquire_from(counter: &'a std::sync::atomic::AtomicUsize) -> Option<Self> {
        use std::sync::atomic::Ordering;
        counter
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < MAX_IN_FLIGHT).then(|| n + 1)
            })
            .ok()
            .map(|_| Self { counter })
    }
}
impl Drop for Admission<'_> {
    fn drop(&mut self) {
        self.counter
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

// ============================================================================
// Panic guards
// ============================================================================
//
// Every export runs inside `catch_unwind` so a stray panic becomes an error
// envelope / INVALID_HANDLE instead of killing the host process (Excel/JVM).
// Only effective under panic=unwind — ship cdylibs with `[profile.excel]`
// (see the root Cargo.toml). Handlers are still written to never panic; this
// is the backstop, not the error path.

/// Run `f`, converting a panic into `INVALID_HANDLE` + last-error.
fn guard_handle(f: impl FnOnce() -> Handle) -> Handle {
    let Some(_admission) = Admission::acquire_from(&IN_FLIGHT) else {
        error::set_last_error("FFI concurrent call limit exceeded");
        return INVALID_HANDLE;
    };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(h) => h,
        Err(_) => {
            error::set_last_error("internal panic contained; operation failed");
            INVALID_HANDLE
        }
    }
}

/// Run `f`, converting a panic into a `{"ok":"false",...}` error envelope.
fn guard_envelope(f: impl FnOnce() -> *mut c_char) -> *mut c_char {
    let Some(_admission) = Admission::acquire_from(&IN_FLIGHT) else {
        return to_owned_c(err_envelope(
            "resource_limit",
            "FFI concurrent call limit exceeded",
        ));
    };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(p) => p,
        Err(_) => to_owned_c(err_envelope(
            "panic",
            "internal panic contained; operation failed",
        )),
    }
}

/// Run `f`, swallowing a panic and returning `default`.
fn guard_value<R>(default: R, f: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => {
            error::set_last_error("internal panic contained; operation failed");
            default
        }
    }
}

// ============================================================================
// Boundary helpers
// ============================================================================

/// Free a string returned by any FFI function in this crate.
///
/// # Safety
/// `s` must be null or an unchanged owned pointer returned by this crate, freed
/// exactly once. Never pass borrowed version/last-error or foreign allocations.
#[no_mangle]
pub unsafe extern "C" fn convex_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: caller returns an unchanged Rust allocation exactly once.
        drop(unsafe { CString::from_raw(s) });
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
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr().cast()
}

// ============================================================================
// Construction (handles)
// ============================================================================

/// Build a bond from a `BondSpec` JSON. Returns `0` on failure.
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_from_json(spec_json: *const c_char) -> Handle {
    guard_handle(|| {
        // SAFETY: this export's borrowed input contract is forwarded unchanged.
        unsafe { with_str(spec_json, build::bond_from_json) }
    })
}

/// Build a curve from a `CurveSpec` JSON. Returns `0` on failure.
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_curve_from_json(spec_json: *const c_char) -> Handle {
    guard_handle(|| {
        // SAFETY: this export's borrowed input contract is forwarded unchanged.
        unsafe { with_str(spec_json, build::curve_from_json) }
    })
}

/// Returns a JSON description of the registered object.
///
/// Free the returned pointer with [`convex_string_free`].
#[no_mangle]
pub extern "C" fn convex_describe(handle: Handle) -> *mut c_char {
    guard_envelope(|| to_owned_c(dispatch::describe(handle)))
}

/// Releases an object by handle. No-op on invalid handle.
#[no_mangle]
pub extern "C" fn convex_release(handle: Handle) {
    guard_value((), || {
        registry::release(handle);
    });
}

/// Number of registered objects.
#[no_mangle]
pub extern "C" fn convex_object_count() -> i32 {
    guard_value(-1, || {
        registry::object_count()
            .and_then(|n| i32::try_from(n).ok())
            .unwrap_or(-1)
    })
}

/// Returns a JSON array of `{handle,kind,name?}` entries for every registered
/// object. Caller frees with [`convex_string_free`].
#[no_mangle]
pub extern "C" fn convex_list_objects() -> *mut c_char {
    guard_envelope(convex_list_objects_inner)
}

fn convex_list_objects_inner() -> *mut c_char {
    let Some(objects) = registry::list(None) else {
        return to_owned_c(err_envelope("unavailable", "registry unavailable"));
    };
    let entries: Vec<_> = objects
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
    guard_value((), registry::clear_all)
}

/// Registry mutation counter (idempotent re-registers do not bump it).
/// Same value across two reads ⇒ every handle resolves to the same object;
/// callers key response caches on it.
#[no_mangle]
pub extern "C" fn convex_generation() -> u64 {
    guard_value(0, registry::generation)
}

// ============================================================================
// Stateless analytics RPCs (JSON in, JSON out)
// ============================================================================

/// Price a bond. Request: `PricingRequest`. Response: `PricingResponse`.
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_price(request_json: *const c_char) -> *mut c_char {
    // SAFETY: this export forwards the caller-owned string contract.
    unsafe { rpc(request_json, dispatch::price) }
}

/// Risk metrics. Request: `RiskRequest`. Response: `RiskResponse`.
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_risk(request_json: *const c_char) -> *mut c_char {
    // SAFETY: this export forwards the caller-owned string contract.
    unsafe { rpc(request_json, dispatch::risk) }
}

/// Spread (Z/G/I/ASW/OAS/DM dispatch). Request: `SpreadRequest`. Response:
/// `SpreadResponse`.
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_spread(request_json: *const c_char) -> *mut c_char {
    // SAFETY: this export forwards the caller-owned string contract.
    unsafe { rpc(request_json, dispatch::spread) }
}

/// Bond cashflow schedule. Request: `CashflowRequest`. Response: `CashflowResponse`.
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_cashflows(request_json: *const c_char) -> *mut c_char {
    // SAFETY: this export forwards the caller-owned string contract.
    unsafe { rpc(request_json, dispatch::cashflows) }
}

/// Curve point query. Request: `CurveQueryRequest`. Response: `CurveQueryResponse`.
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_curve_query(request_json: *const c_char) -> *mut c_char {
    // SAFETY: this export forwards the caller-owned string contract.
    unsafe { rpc(request_json, dispatch::curve_query) }
}

/// Make-whole call price. Request: `MakeWholeRequest`. Response: `MakeWholeResponse`.
///
/// Computes the make-whole call redemption price at a future call date for
/// a callable bond carrying a make-whole spread. Discount uses the bond's
/// own day count and frequency; treasury rate is supplied by the caller.
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_make_whole(request_json: *const c_char) -> *mut c_char {
    // SAFETY: this export forwards the caller-owned string contract.
    unsafe { rpc(request_json, dispatch::make_whole) }
}

/// One-call yield & spread analysis (Bloomberg-YAS style). Request:
/// `YasRequest`. Response: `YasResponse` — every yield convention, G/Z/
/// benchmark/ASW spreads, risk metrics, and the settlement invoice from a
/// single computation.
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_yas(request_json: *const c_char) -> *mut c_char {
    // SAFETY: this export forwards the caller-owned string contract.
    unsafe { rpc(request_json, dispatch::yas) }
}

/// Curve scenario ladder in one call. Request: `ScenarioRequest` (N named
/// scenarios of parallel/steepener/flattener/key-rate/credit bumps).
/// Response: `ScenarioResponse` — base pricing + one repriced row per
/// scenario, holding the mark-implied Z-spread fixed.
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_scenario(request_json: *const c_char) -> *mut c_char {
    // SAFETY: this export forwards the caller-owned string contract.
    unsafe { rpc(request_json, dispatch::scenario) }
}

// ============================================================================
// Hedge advisor (JSON in, JSON out)
// ============================================================================

/// Build a position risk profile. Request: `RiskProfileRequest`. Response:
/// `RiskProfile` (modified/Macaulay duration, convexity, DV01, key-rate
/// buckets, market value, provenance). The profile round-trips back into
/// [`convex_hedge`] and [`convex_compare`].
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_risk_profile(request_json: *const c_char) -> *mut c_char {
    // SAFETY: this export forwards the caller-owned string contract.
    unsafe { rpc(request_json, dispatch::risk_profile) }
}

/// Propose a hedge for a position profile. Request: `HedgeRequest` (carries a
/// `strategy` tag + the `RiskProfile` from [`convex_risk_profile`]). Response:
/// `HedgeProposal` (trades, residual risk, cost bps, tradeoff notes).
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_hedge(request_json: *const c_char) -> *mut c_char {
    // SAFETY: this export forwards the caller-owned string contract.
    unsafe { rpc(request_json, dispatch::hedge) }
}

/// Compare hedge proposals against a position. Request: `CompareRequest`.
/// Response: `CompareResponse` (`ComparisonReport` + optional narrative).
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_compare(request_json: *const c_char) -> *mut c_char {
    // SAFETY: this export forwards the caller-owned string contract.
    unsafe { rpc(request_json, dispatch::compare) }
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
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_schema(type_name: *const c_char) -> *mut c_char {
    guard_envelope(|| {
        // SAFETY: caller-owned string remains valid within the guard.
        let result = unsafe { with_str_owned(type_name, schemas::lookup) };
        to_owned_c(match result {
            Ok(json) => format!(r#"{{"ok":"true","result":{json}}}"#),
            Err(msg) => err_envelope("schema", &msg),
        })
    })
}

/// Parse a textual mark (e.g. `"99.5C"`, `"4.65%"`, `"+125bps@USD.SOFR"`).
///
/// Returns a JSON envelope; on success, `result` is the canonical `Mark` JSON.
///
/// # Safety
/// Input must be null or a readable NUL-terminated C string in one allocation,
/// unchanged for the whole call. Invalid/dangling pointers are outside the ABI
/// contract. Returned owned strings must be released once with `convex_string_free`.
#[no_mangle]
pub unsafe extern "C" fn convex_mark_parse(text: *const c_char) -> *mut c_char {
    guard_envelope(|| {
        // SAFETY: caller-owned string remains valid within the guard.
        let payload = unsafe {
            with_str_owned(text, |s| {
                s.parse::<convex_core::types::Mark>()
                    .map_err(|e| e.to_string())
            })
        };
        to_owned_c(match payload {
            Ok(mark) => match serde_json::to_string(&mark) {
                Ok(json) => format!(r#"{{"ok":"true","result":{json}}}"#),
                Err(e) => err_envelope("serialize", &e.to_string()),
            },
            Err(e) => err_envelope("invalid_input", &e),
        })
    })
}

// ============================================================================
// Internal helpers
// ============================================================================

// SAFETY: internal callers forward a valid immutable C string for the call.
unsafe fn read_input<'a>(ptr: *const c_char) -> Result<&'a str, String> {
    if ptr.is_null() {
        return Err("null pointer".into());
    }
    for length in 0..=ingress::MAX_INPUT_BYTES {
        // SAFETY: the caller guarantees a C string in one allocation. Every
        // read precedes or reads its terminator; stop at the resource ceiling.
        let byte = unsafe { ptr.add(length).read() };
        if byte == 0 {
            // SAFETY: the reads above established this borrowed range before
            // its terminator; the caller keeps the allocation unchanged.
            let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), length) };
            return std::str::from_utf8(bytes).map_err(|_| "invalid UTF-8".into());
        }
    }
    Err("input byte limit exceeded".into())
}

unsafe fn with_str(ptr: *const c_char, f: fn(&str) -> Handle) -> Handle {
    // SAFETY: borrowed input contract is forwarded from the export.
    match unsafe { read_input(ptr) }.and_then(|s| ingress::parse(s).map(|_| s)) {
        Ok(s) => f(s),
        Err(error) => {
            error::set_last_error(error);
            INVALID_HANDLE
        }
    }
}

unsafe fn with_str_owned<F, T>(ptr: *const c_char, f: F) -> Result<T, String>
where
    F: FnOnce(&str) -> Result<T, String>,
{
    // SAFETY: borrowed input contract is forwarded from the export.
    f(unsafe { read_input(ptr) }?)
}

unsafe fn rpc(request_json: *const c_char, handler: fn(&str) -> String) -> *mut c_char {
    guard_envelope(|| {
        // SAFETY: caller retains the request allocation through this call.
        let response = match unsafe { read_input(request_json) }
            .and_then(|s| ingress::parse(s).map(|_| handler(s)))
        {
            Ok(response) => response,
            Err(error) => err_envelope("invalid_input", &error),
        };
        to_owned_c(response)
    })
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

#[cfg(test)]
mod panic_guard_tests {
    use super::*;
    use std::ffi::CStr;

    // These run under the test profile (panic = "unwind", the cargo default),
    // the same strategy `[profile.excel]` uses for the shipped DLL.

    #[test]
    fn admission_limit_and_unwind_release_the_slot() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let counter = AtomicUsize::new(0);
        let mut admitted: Vec<_> = (0..MAX_IN_FLIGHT)
            .map(|_| Admission::acquire_from(&counter).unwrap())
            .collect();
        assert!(Admission::acquire_from(&counter).is_none());
        admitted.pop();
        let result = std::panic::catch_unwind(|| {
            let _slot = Admission::acquire_from(&counter).unwrap();
            panic!("test unwind");
        });
        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), MAX_IN_FLIGHT - 1);
        drop(admitted);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn admission_is_atomic_under_competing_callers() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Barrier,
        };
        let counter = AtomicUsize::new(0);
        let admitted = AtomicUsize::new(0);
        let barrier = Barrier::new(64);
        std::thread::scope(|scope| {
            for _ in 0..64 {
                scope.spawn(|| {
                    let slot = Admission::acquire_from(&counter);
                    if slot.is_some() {
                        admitted.fetch_add(1, Ordering::SeqCst);
                    }
                    barrier.wait();
                    assert_eq!(counter.load(Ordering::SeqCst), MAX_IN_FLIGHT);
                    barrier.wait();
                    drop(slot);
                });
            }
        });
        assert_eq!(admitted.load(Ordering::SeqCst), MAX_IN_FLIGHT);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn guard_handle_converts_panic_to_invalid_handle_and_last_error() {
        error::clear_error();
        let h = guard_handle(|| panic!("boom in builder"));
        assert_eq!(h, INVALID_HANDLE);
        let msg = unsafe { CStr::from_ptr(error::last_error_message()) }
            .to_str()
            .unwrap();
        assert!(msg.contains("internal panic contained"), "got: {msg}");
        assert!(!msg.contains("boom in builder"));
    }

    #[test]
    fn guard_envelope_converts_panic_to_error_envelope() {
        let ptr = guard_envelope(|| panic!("boom in rpc"));
        assert!(!ptr.is_null());
        let s = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
        unsafe { convex_string_free(ptr) };
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["ok"], "false");
        assert_eq!(v["error"]["code"], "panic");
        assert!(v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("internal panic contained"));
    }

    #[test]
    fn guard_value_returns_default_on_panic() {
        let n = guard_value(-1, || -> i32 { panic!("boom") });
        assert_eq!(n, -1);
    }

    #[test]
    fn guard_passes_through_on_success() {
        assert_eq!(guard_handle(|| 42), 42);
        assert_eq!(guard_value(-1, || 7), 7);
    }
}
