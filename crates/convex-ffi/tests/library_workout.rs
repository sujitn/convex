//! Additional actual C ABI route and owned-response parity.
#[path = "../../../tests/fixtures/library/workout_contract.rs"]
mod contract;
use std::ffi::{CStr, CString};
#[test]
fn c_workout_preserve_native_workout_and_errors() {
    for case in contract::cases() {
        let input = CString::new(contract::request(&case)).unwrap();
        // SAFETY: input remains an immutable NUL-terminated allocation for the call.
        let pointer = unsafe { convex_ffi::convex_library(input.as_ptr()) };
        assert!(!pointer.is_null());
        // SAFETY: a nonnull library result is an owned NUL-terminated UTF-8 string.
        let result = unsafe { CStr::from_ptr(pointer) }
            .to_str()
            .unwrap()
            .to_owned();
        // SAFETY: free this Rust-owned result once, after copying its contents.
        unsafe { convex_ffi::convex_string_free(pointer) };
        contract::check(&case, &result);
    }
}
