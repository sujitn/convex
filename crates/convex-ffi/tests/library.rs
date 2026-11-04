//! Actual C entry point and ownership parity with the complete pure native request.
#[path = "../../../tests/fixtures/library/contract.rs"]
mod contract;
use std::ffi::{CStr, CString};
#[test]
fn c_library_preserves_full_native_outputs_errors_and_owned_memory() {
    for case in contract::cases() {
        let input = CString::new(contract::request(&case)).unwrap();
        let ptr = unsafe { convex_ffi::convex_library(input.as_ptr()) };
        assert!(!ptr.is_null());
        let text = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        unsafe { convex_ffi::convex_string_free(ptr) };
        contract::check(&case, &text);
    }
    let ptr = unsafe { convex_ffi::convex_library(std::ptr::null()) };
    assert!(!ptr.is_null());
    let error: serde_json::Value =
        serde_json::from_str(unsafe { CStr::from_ptr(ptr) }.to_str().unwrap()).unwrap();
    unsafe { convex_ffi::convex_string_free(ptr) };
    assert_eq!(error["ok"], "false");
}
#[test]
fn c_schema_is_the_actual_full_request_schema() {
    let name = CString::new("LibraryRequest").unwrap();
    let ptr = unsafe { convex_ffi::convex_schema(name.as_ptr()) };
    assert!(!ptr.is_null());
    let schema: serde_json::Value =
        serde_json::from_str(unsafe { CStr::from_ptr(ptr) }.to_str().unwrap()).unwrap();
    unsafe { convex_ffi::convex_string_free(ptr) };
    assert_eq!(schema["ok"], "true");
    assert!(
        schema["result"] == convex::library::request_schema(),
        "C schema differs from the actual native request schema"
    );
}

#[test]
fn existing_sinking_cashflow_route_retains_actual_principal_and_reduced_coupons() {
    let spec = CString::new(
        serde_json::json!({
            "type":"sinking_fund", "name":"library.sinking.parity",
            "coupon_rate":0.04, "frequency":"SemiAnnual",
            "issue":"2025-01-15", "maturity":"2028-01-15",
            "day_count":"Thirty360US", "currency":"USD", "face_value":100,
            "schedule":[{"date":"2026-01-15","amount":50,"price":100}]
        })
        .to_string(),
    )
    .unwrap();
    let handle = unsafe { convex_ffi::convex_bond_from_json(spec.as_ptr()) };
    assert_ne!(handle, convex_ffi::INVALID_HANDLE);
    let request =
        CString::new(serde_json::json!({"bond":handle,"settlement":"2025-02-01"}).to_string())
            .unwrap();
    let pointer = unsafe { convex_ffi::convex_cashflows(request.as_ptr()) };
    assert!(!pointer.is_null());
    let response: serde_json::Value =
        serde_json::from_str(unsafe { CStr::from_ptr(pointer) }.to_str().unwrap()).unwrap();
    unsafe {
        convex_ffi::convex_string_free(pointer);
        convex_ffi::convex_release(handle);
    }
    assert_eq!(response["ok"], "true");
    let flows = response["result"]["flows"].as_array().unwrap();
    assert!(flows
        .iter()
        .any(|f| f["date"] == "2026-01-15" && f["kind"] == "redemption" && f["amount"] == 50.0));
    assert!(flows
        .iter()
        .any(|f| f["date"] == "2026-07-15" && f["kind"] == "coupon" && f["amount"] == 1.0));
    assert!(flows
        .iter()
        .any(|f| f["date"] == "2028-01-15" && f["kind"] == "redemption" && f["amount"] == 50.0));
}
