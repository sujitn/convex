//! Actual owned C ABI response and full fresh graph column to hedge transport.
#[path = "../../../tests/fixtures/library/recovery_hedge_contract.rs"]
mod controls;
use std::ffi::{CStr, CString};
fn execute(case: &serde_json::Value) -> String {
    let input = CString::new(controls::request(case)).unwrap();
    // SAFETY: the immutable NUL-terminated input lives through the call.
    let p = unsafe { convex_ffi::convex_library(input.as_ptr()) };
    assert!(!p.is_null());
    // SAFETY: a nonnull response is an owned UTF-8 NUL-terminated Rust allocation.
    let output = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_owned();
    // SAFETY: free the returned allocation exactly once after copying it.
    unsafe { convex_ffi::convex_string_free(p) };
    output
}
#[test]
fn c_recovery_hedge_and_complete_hedge_columns_preserve_original_controls() {
    let mut outputs = Vec::new();
    for case in controls::cases() {
        let output = execute(&case);
        controls::check(&case, &output);
        outputs.push(serde_json::json!({"id":case["id"],"expected_envelope":serde_json::from_str::<serde_json::Value>(&output).unwrap()}));
        for owner in controls::owner_requests(&case, &output) {
            controls::check_owner(&owner, &execute(&owner));
        }
        for scenario in controls::scenarios(&case) {
            controls::check_scenario(&scenario, &execute(&scenario));
        }
    }
    for case in controls::mixed_requests(&outputs) {
        controls::check_mixed(&case, &execute(&case));
    }
}
