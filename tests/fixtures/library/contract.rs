//! Shared adapter parity corpus. Native financial oracles remain separate.
#![allow(dead_code)]
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("cases.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
pub fn native(input: &str) -> Value {
    let result = convex::library::decode_request(input).and_then(|r| convex::library::execute(&r));
    match result {
        Ok(result) => json!({"ok":"true","schema_version":1,"result":result}),
        Err(error) => json!({"ok":"false","schema_version":1,"error":error}),
    }
}
pub fn check(case: &Value, actual: &str) {
    let input = request(case);
    let expected = native(&input);
    let actual: Value = serde_json::from_str(actual).unwrap();
    let success = case["expected_error"].is_null();
    assert_eq!(
        actual["ok"],
        if success { "true" } else { "false" },
        "{}: {actual}",
        case["id"]
    );
    if !success {
        assert_eq!(
            actual["error"]["code"], case["expected_error"],
            "{}",
            case["id"]
        );
    }
    assert_eq!(
        actual, expected,
        "{}: adapter changed native result/error",
        case["id"]
    );
}
