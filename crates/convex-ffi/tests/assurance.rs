//! Actual C ABI boundary tests, with valid caller-owned memory.

use convex_ffi::*;
use proptest::prelude::*;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};

const RPCS: &[unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_char] = &[
    convex_price,
    convex_risk,
    convex_spread,
    convex_cashflows,
    convex_curve_query,
    convex_make_whole,
    convex_yas,
    convex_scenario,
    convex_risk_profile,
    convex_hedge,
    convex_compare,
];

fn bond_spec() -> String {
    json!({"type":"fixed_rate","name":"CVX039-BOND", "coupon_rate":0.05,
        "frequency":"SemiAnnual", "issue":"2025-01-15", "maturity":"2035-01-15",
        "day_count":"Thirty360US", "currency":"USD", "face_value":100})
    .to_string()
}

#[test]
fn null_utf8_nested_duplicates_and_resource_limits_fail_at_the_abi() {
    for function in RPCS {
        // SAFETY: the documented ABI explicitly permits null as rejected input.
        unsafe {
            let output = function(std::ptr::null());
            assert!(!output.is_null());
            let value: Value =
                serde_json::from_str(CStr::from_ptr(output).to_str().unwrap()).unwrap();
            convex_string_free(output);
            assert_eq!(value["error"]["code"], "invalid_input");
        }
        for input in [
            vec![0xff, 0xfe],
            br#"{"params":{"volatility":0.01,"volatility":0.02}}"#.to_vec(),
            format!("{{\"scenarios\":[{}]}}", vec!["{}"; 65].join(",")).into_bytes(),
            br#"{"settlement":"+262142-12-31"}"#.to_vec(),
            br#"{"tenor":1e300}"#.to_vec(),
        ] {
            let value = call(*function, &input);
            assert_eq!(value["ok"], "false");
            assert_eq!(value["error"]["code"], "invalid_input");
        }
    }
}

#[test]
fn unsupported_zero_compounding_is_rejected_without_approximation() {
    let spec = CString::new(json!({"type":"zero_coupon","cusip":"039ZEROAA", "issue":"2025-01-15", "maturity":"2035-01-15", "currency":"USD", "compounding":"Simple"}).to_string()).unwrap();
    // SAFETY: one immutable valid C string, no retained caller pointer.
    let handle = unsafe { convex_bond_from_json(spec.as_ptr()) };
    assert_eq!(handle, INVALID_HANDLE);
    // SAFETY: borrowed error is read on this thread before another library call.
    assert!(unsafe { CStr::from_ptr(convex_last_error()) }
        .to_str()
        .unwrap()
        .contains("unsupported"));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, rng_seed: proptest::test_runner::RngSeed::Fixed(0x039), .. ProptestConfig::default() })]

    #[test]
    fn fuzz_arbitrary_c_string_bytes_remain_error_envelopes(bytes in prop::collection::vec(1u8..=255, 0..2048)) {
        for function in RPCS {
            let value = call(*function, &bytes);
            prop_assert_eq!(&value["ok"], "false");
            prop_assert_ne!(&value["error"]["code"], "panic");
        }
        let input = CString::new(bytes).unwrap();
        for function in [convex_bond_from_json, convex_curve_from_json] {
            // SAFETY: arbitrary bytes remain allocated and NUL-terminated.
            let handle = unsafe { function(input.as_ptr()) };
            prop_assert_eq!(handle, INVALID_HANDLE);
            // SAFETY: thread-local borrowed diagnostic is valid until next call.
            let error = unsafe { CStr::from_ptr(convex_last_error()) }.to_str().unwrap();
            prop_assert!(!error.contains("internal panic"));
        }
    }

    #[test]
    fn fuzz_valid_json_mutations_fail_without_constructor_panics(field in prop::sample::select(vec!["coupon_rate", "frequency", "issue", "maturity", "face_value"]), value in prop::sample::select(vec![json!(null), json!([]), json!({}), json!("NaN"), json!(1e300), json!("bad")])) {
        let mut spec: Value = serde_json::from_str(&bond_spec()).unwrap();
        spec[field] = value;
        let input = CString::new(spec.to_string()).unwrap();
        // SAFETY: input storage spans the full constructor call.
        let handle = unsafe { convex_bond_from_json(input.as_ptr()) };
        prop_assert_eq!(handle, INVALID_HANDLE);
        // SAFETY: no library call intervenes before this borrowed error read.
        let error = unsafe { CStr::from_ptr(convex_last_error()) }.to_str().unwrap();
        prop_assert!(!error.contains("internal panic"));
    }
}

fn call(
    function: unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_char,
    input: &[u8],
) -> Value {
    let input = CString::new(input).unwrap();
    // SAFETY: input remains allocated through the call; returned Rust allocation
    // is inspected once and released once by its matching allocator.
    unsafe {
        let output = function(input.as_ptr());
        assert!(!output.is_null());
        let json = CStr::from_ptr(output).to_str().unwrap().to_owned();
        convex_string_free(output);
        serde_json::from_str(&json).unwrap()
    }
}

#[test]
fn duplicate_rpc_fields_are_rejected_before_dispatch() {
    let spec = CString::new(bond_spec()).unwrap();
    // SAFETY: valid immutable C string for this call.
    let bond = unsafe { convex_bond_from_json(spec.as_ptr()) };
    assert_ne!(bond, INVALID_HANDLE);
    let request =
        format!(r#"{{"bond":{bond},"bond":{bond},"settlement":"2025-04-15","mark":"99.5C"}}"#);
    let output = call(convex_price, request.as_bytes());
    convex_release(bond);
    assert_eq!(output["ok"], "false");
    assert_eq!(output["error"]["code"], "invalid_input");
}

#[test]
fn oversized_valid_constructor_input_is_rejected_without_mutation() {
    let input = CString::new(format!("{}{}", " ".repeat(2 * 1024 * 1024), bond_spec())).unwrap();
    // SAFETY: the entire oversized string remains readable through the call.
    let handle = unsafe { convex_bond_from_json(input.as_ptr()) };
    if handle != INVALID_HANDLE {
        convex_release(handle);
    }
    assert_eq!(handle, INVALID_HANDLE);
    // SAFETY: last error is borrowed on this thread until the next library call.
    let error = unsafe { CStr::from_ptr(convex_last_error()) }
        .to_str()
        .unwrap();
    assert!(error.contains("limit"), "{error}");
}
