//! Removed government zero-rate proxies must fail before financial dispatch.
use std::ffi::{CStr, CString};

use serde_json::{json, Value};

type Rpc = unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_char;

fn rpc(function: Rpc, request: Value) -> Value {
    let input = CString::new(request.to_string()).unwrap();
    let pointer = unsafe { function(input.as_ptr()) };
    assert!(!pointer.is_null());
    let response = serde_json::from_str(unsafe { CStr::from_ptr(pointer) }.to_str().unwrap());
    unsafe { convex_ffi::convex_string_free(pointer) };
    response.unwrap()
}

#[test]
fn removed_spread_and_yas_fields_are_never_discarded() {
    for proxy in [json!(123), json!("obsolete.name"), Value::Null] {
        for kind in ["ZSpread", "GSpread"] {
            let result = rpc(
                convex_ffi::convex_spread,
                json!({"bond":123,"curve":456,"settlement":"2025-04-15", "mark":"99.5C",
                    "spread_type":kind,"params":{"govt_curve":proxy}}),
            );
            assert_eq!(result["ok"], "false", "{result}");
            assert_eq!(result["error"]["code"], "invalid_input", "{result}");
            assert!(result["error"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown field `govt_curve`"));
        }
        let result = rpc(
            convex_ffi::convex_yas,
            json!({"bond":123,"curve":456,"settlement":"2025-04-15", "mark":"99.5C",
                "govt_curve":proxy}),
        );
        assert_eq!(result["error"]["code"], "invalid_input", "{result}");
        assert!(result["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown field `govt_curve`"));
    }
}

#[test]
fn yas_cannot_price_a_government_mark_against_a_zero_curve() {
    let result = rpc(
        convex_ffi::convex_yas,
        json!({"bond":123,"curve":456,"settlement":"2025-04-15", "mark":"125 G@USD.TSY"}),
    );
    assert_eq!(result["ok"], "false", "{result}");
    assert_eq!(result["error"]["code"], "invalid_input", "{result}");
    assert_eq!(result["error"]["field"], "mark", "{result}");
}

#[test]
fn convenience_schemas_describe_only_admitted_government_inputs() {
    let schema = |name: &str| {
        let name = CString::new(name).unwrap();
        let pointer = unsafe { convex_ffi::convex_schema(name.as_ptr()) };
        assert!(!pointer.is_null());
        let wire: Value =
            serde_json::from_str(unsafe { CStr::from_ptr(pointer) }.to_str().unwrap()).unwrap();
        unsafe { convex_ffi::convex_string_free(pointer) };
        assert_eq!(wire["ok"], "true");
        wire["result"].clone()
    };
    let spread = schema("SpreadRequest");
    let params = &spread["properties"]["params"];
    assert_eq!(params["additionalProperties"], false);
    assert!(params["properties"].get("government").is_some());
    assert!(params["properties"].get("govt_curve").is_none());
    let yas = schema("YasRequest");
    assert_eq!(yas["additionalProperties"], false);
    assert!(yas["properties"].get("govt_curve").is_none());
    let output = schema("YasResponse");
    for field in ["g_spread_bps", "benchmark_spread_bps", "benchmark_tenor"] {
        assert!(output["properties"].get(field).is_none());
        assert!(!output["required"]
            .as_array()
            .unwrap()
            .contains(&json!(field)));
    }
}
