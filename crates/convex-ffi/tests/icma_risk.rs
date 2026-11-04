//! Actual owned C ABI response and full fresh ICMA column to hedge transport.
#[path = "../../../tests/fixtures/library/icma_risk_contract.rs"]
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
fn c_icma_risk_and_complete_hedge_columns_preserve_original_controls() {
    for case in controls::cases() {
        let output = execute(&case);
        controls::check(&case, &output);
        if case["expected_error"].is_null()
            && case["request"]["operation"]["action"]["kind"] == "hedge_vector"
        {
            for hedge in controls::hedge_requests(&case, &output) {
                controls::check_hedge(&hedge, &execute(&hedge));
            }
        }
    }
}
