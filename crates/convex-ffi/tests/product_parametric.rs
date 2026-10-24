#[path = "../../../tests/fixtures/library/product_parametric.rs"]
mod fixture;
use std::ffi::{CStr, CString};
#[test]
fn c_owned_full_product_calibration_results_and_failures_match_native() {
    for case in fixture::transport_cases() {
        let input =
            CString::new(fixture::request(&case, serde_json::json!({"kind":"build"}))).unwrap();
        // SAFETY: immutable valid NUL-terminated allocation survives this call.
        let pointer = unsafe { convex_ffi::convex_library(input.as_ptr()) };
        assert!(!pointer.is_null());
        // SAFETY: successful nonnull owned result is valid UTF-8/NUL terminated.
        let output = unsafe { CStr::from_ptr(pointer) }
            .to_str()
            .unwrap()
            .to_owned();
        // SAFETY: this Rust-owned result is freed exactly once after copying.
        unsafe { convex_ffi::convex_string_free(pointer) };
        fixture::check_transport(&case, &output);
    }
}
