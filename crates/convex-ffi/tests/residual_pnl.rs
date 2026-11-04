#[path = "../../../tests/fixtures/library/residual_pnl_contract.rs"]
mod controls;
use std::ffi::{CStr, CString};
#[test]
fn c_full_residual_cash_partial_security_and_causal_pnl() {
    controls::run(|request| {
        let input = CString::new(request).unwrap();
        // SAFETY: immutable NUL-terminated input lives through the call.
        let pointer = unsafe { convex_ffi::convex_library(input.as_ptr()) };
        assert!(!pointer.is_null());
        // SAFETY: the C owner returned a valid NUL-terminated UTF-8 allocation.
        let output = unsafe { CStr::from_ptr(pointer) }
            .to_str()
            .unwrap()
            .to_owned();
        // SAFETY: release this allocation exactly once after copying it.
        unsafe { convex_ffi::convex_string_free(pointer) };
        output
    });
}
