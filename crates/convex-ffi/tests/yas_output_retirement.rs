//! Actual C-ABI convenience outputs must not advertise missing swap/option models.
use serde_json::{json, Value};
use std::ffi::{CStr, CString};

unsafe fn owned(f: unsafe extern "C" fn(*const i8) -> *mut i8, text: &str) -> Value {
    let input = CString::new(text).unwrap();
    let pointer = f(input.as_ptr());
    assert!(!pointer.is_null());
    let raw = CStr::from_ptr(pointer).to_bytes().to_vec();
    convex_ffi::convex_string_free(pointer);
    serde_json::from_slice(&raw).unwrap()
}

#[test]
fn actual_yas_response_has_no_unbound_asw_or_unused_oas_output() {
    unsafe {
        let bond = CString::new(
            json!({
                "type":"fixed_rate", "name":"CVX063.YAS.RETIRE.BOND", "coupon_rate":0.05,
                "frequency":"SemiAnnual", "maturity":"2035-01-15", "issue":"2025-01-15",
                "day_count":"Thirty360US", "currency":"USD", "face_value":100
            })
            .to_string(),
        )
        .unwrap();
        let bh = convex_ffi::convex_bond_from_json(bond.as_ptr());
        assert_ne!(bh, convex_ffi::INVALID_HANDLE);
        let curve = CString::new(
            json!({
                "type":"discrete", "name":"CVX063.YAS.RETIRE.CURVE", "ref_date":"2025-01-15",
                "tenors":[0.25,0.5,1.0,2.0,5.0,10.0,30.0],
                "values":[0.04,0.04,0.041,0.042,0.043,0.044,0.045],
                "value_kind":"zero_rate", "interpolation":"linear",
                "day_count":"Act365Fixed", "compounding":"Continuous"
            })
            .to_string(),
        )
        .unwrap();
        let ch = convex_ffi::convex_curve_from_json(curve.as_ptr());
        assert_ne!(ch, convex_ffi::INVALID_HANDLE);
        let response = owned(
            convex_ffi::convex_yas,
            &json!({
                "bond":bh,"curve":ch,"settlement":"2025-04-15","mark":"99.5C"
            })
            .to_string(),
        );
        convex_ffi::convex_release(bh);
        convex_ffi::convex_release(ch);
        assert_eq!(response["ok"], "true", "{response}");
        let output = &response["result"];
        assert!((output["clean_price"].as_f64().unwrap() - 99.5).abs() < 1e-9);
        assert!(output["modified_duration"].as_f64().unwrap() > 0.0);
        for field in [
            "asw_spread_bps",
            "oas_bps",
            "g_spread_bps",
            "benchmark_spread_bps",
            "benchmark_tenor",
        ] {
            assert!(
                output.get(field).is_none(),
                "retired output emitted: {field}"
            );
        }
    }
}

#[test]
fn actual_c_schema_does_not_advertise_retired_outputs() {
    unsafe {
        let envelope = owned(convex_ffi::convex_schema, "YasResponse");
        assert_eq!(envelope["ok"], "true");
        let schema = &envelope["result"];
        assert_eq!(schema["title"], "YasResponse");
        for field in ["asw_spread_bps", "oas_bps"] {
            assert!(
                schema["properties"].get(field).is_none(),
                "retired schema field: {field}"
            );
        }
        assert!(schema["properties"].get("z_spread_bps").is_some());
        assert!(schema["properties"].get("modified_duration").is_some());
    }
}
