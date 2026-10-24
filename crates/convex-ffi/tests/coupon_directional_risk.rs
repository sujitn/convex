#[path = "../../../tests/fixtures/library/coupon_directional_risk_contract.rs"]
mod contract;
#[test]
fn actual_c_abi_coupon_directional_risk_match_full_independent_native_controls() {
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
