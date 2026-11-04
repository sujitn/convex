#[path = "../../../tests/fixtures/library/reinvestment_contract.rs"]
mod contract;
#[test]
fn actual_c_abi_reinvestment_match_full_independent_native_controls() {
    for case in contract::cases() {
        let request = std::ffi::CString::new(contract::request(&case)).unwrap();
        let pointer = unsafe { convex_ffi::convex_library(request.as_ptr()) };
        assert!(!pointer.is_null());
        let text = unsafe { std::ffi::CStr::from_ptr(pointer) }
            .to_str()
            .unwrap()
            .to_owned();
        contract::check(&case, &text);
        unsafe { convex_ffi::convex_string_free(pointer) };
    }
}

#[test]
fn actual_c_schema_retires_the_inferred_yas_money_market_output() {
    let name = std::ffi::CString::new("YasResponse").unwrap();
    let pointer = unsafe { convex_ffi::convex_schema(name.as_ptr()) };
    assert!(!pointer.is_null());
    let schema: serde_json::Value = serde_json::from_str(
        unsafe { std::ffi::CStr::from_ptr(pointer) }
            .to_str()
            .unwrap(),
    )
    .unwrap();
    unsafe { convex_ffi::convex_string_free(pointer) };
    assert_eq!(schema["ok"], "true");
    let properties = schema["result"]["properties"].as_object().unwrap();
    assert!(properties.contains_key("ytm_pct"));
    assert!(!properties.contains_key("money_market_yield_pct"));
}
