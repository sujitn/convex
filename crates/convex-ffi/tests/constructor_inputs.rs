//! Actual C constructor admission and generated schema capture.
use serde_json::Value;
use std::ffi::{CStr, CString};
use std::sync::Mutex;

static OWN_CALLS: Mutex<()> = Mutex::new(());

#[test]
fn original_raw_and_unsupported_field_requests_use_checked_native_admission() {
    let _guard = OWN_CALLS.lock().unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/constructor-admission.json")).unwrap();
    let mut observations = Vec::new();
    let initial = convex_ffi::convex_object_count();
    assert_eq!(initial, 0);
    for row in reference["cases"].as_array().unwrap() {
        convex_ffi::convex_clear_error();
        let input = CString::new(row["request_json"].as_str().unwrap()).unwrap();
        // SAFETY: the immutable CString lives across the entire constructor call.
        let handle = unsafe {
            if row.get("constructor").and_then(Value::as_str) == Some("curve") {
                convex_ffi::convex_curve_from_json(input.as_ptr())
            } else {
                convex_ffi::convex_bond_from_json(input.as_ptr())
            }
        };
        let accepted = handle != 0;
        let expected = row["expected_constructor_acceptance"].as_bool().unwrap();
        let mut description = None;
        if accepted {
            // SAFETY: this live handle belongs to this test; returned strings are
            // Rust-owned and copied before exactly one matching free and release.
            unsafe {
                let pointer = convex_ffi::convex_describe(handle);
                assert!(!pointer.is_null());
                description = Some(
                    serde_json::from_str::<Value>(CStr::from_ptr(pointer).to_str().unwrap())
                        .unwrap(),
                );
                convex_ffi::convex_string_free(pointer);
                convex_ffi::convex_release(handle);
            }
        }
        // SAFETY: borrowed error text is only read before the next native call.
        let error = unsafe {
            let pointer = convex_ffi::convex_last_error();
            (!pointer.is_null()).then(|| CStr::from_ptr(pointer).to_string_lossy().into_owned())
        };
        assert_eq!(accepted, expected, "{}: {error:?}", row["id"]);
        if !accepted {
            assert!(error.as_deref().is_some_and(|text| !text.is_empty()));
        }
        assert_eq!(convex_ffi::convex_object_count(), initial);
        observations.push(serde_json::json!({"id":row["id"],"accepted":accepted,"description":description,"error":error}));
    }
    assert_eq!(observations.len(), 39);
    println!(
        "CONSTRUCTOR_ADMISSION_OBSERVATIONS={}",
        serde_json::to_string(&observations).unwrap()
    );
}

#[test]
fn actual_c_constructor_schemas_are_generated_from_native_dtos() {
    let _guard = OWN_CALLS.lock().unwrap();
    let mut schemas = serde_json::Map::new();
    for name in ["BondSpec", "CurveSpec"] {
        let input = CString::new(name).unwrap();
        // SAFETY: input lives through the call; copy the owned output before its
        // single matching free, as required by the exported ownership contract.
        let envelope: Value = unsafe {
            let pointer = convex_ffi::convex_schema(input.as_ptr());
            assert!(!pointer.is_null());
            let value = serde_json::from_str(CStr::from_ptr(pointer).to_str().unwrap()).unwrap();
            convex_ffi::convex_string_free(pointer);
            value
        };
        assert_eq!(envelope["ok"], "true");
        assert_eq!(envelope["result"]["title"], name);
        assert!(envelope["result"]["$schema"].is_string());
        schemas.insert(name.into(), envelope["result"].clone());
    }
    println!("ACTUAL_CONSTRUCTOR_SCHEMAS={}", Value::Object(schemas));
}
