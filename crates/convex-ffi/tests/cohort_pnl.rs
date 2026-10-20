//! Actual C-owned response; complete native/independent mixed-book scenario controls.
#[path = "../../../tests/fixtures/library/cohort_pnl_contract.rs"]
mod controls;
use std::ffi::{CStr, CString};
#[test]
fn c_cohort_outstanding_cash_physical_reconciliation() {
    controls::run(|request| {
        let input = CString::new(request).unwrap();
        // SAFETY: immutable NUL-terminated input stays alive through the call.
        let p = unsafe { convex_ffi::convex_library(input.as_ptr()) };
        assert!(!p.is_null());
        // SAFETY: returned nonnull pointer owns a NUL-terminated UTF-8 allocation.
        let output = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_owned();
        // SAFETY: free this returned allocation exactly once after copying it.
        unsafe { convex_ffi::convex_string_free(p) };
        output
    });
}
