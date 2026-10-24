//! Full native envelopes and predeclared independent supplied-calendar accrual cash.
#![allow(dead_code)]
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("accrued-methods.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
fn native(input: &str) -> Value {
    match convex::library::decode_request(input).and_then(|r| convex::library::execute(&r)) {
        Ok(result) => json!({"ok":"true", "schema_version":1, "result":result}),
        Err(error) => json!({"ok":"false", "schema_version":1, "error":error}),
    }
}
pub fn check(case: &Value, output: &str) {
    let actual: Value = serde_json::from_str(output).unwrap();
    assert_eq!(
        actual,
        native(&request(case)),
        "{} host changed native result",
        case["id"]
    );
    assert_eq!(
        actual["ok"],
        if case["expected_error"].is_null() {
            "true"
        } else {
            "false"
        },
        "{}: {actual}",
        case["id"]
    );
    if !case["expected_error"].is_null() {
        assert_eq!(
            actual["error"]["code"], case["expected_error"],
            "{}: {actual}",
            case["id"]
        );
        return;
    }
    let data = &actual["result"]["data"];
    assert_eq!(data["unit"], "input_face_cash");
    assert_eq!(data.as_object().unwrap().len(), 2);
    let number = |value: &Value| {
        value
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap()
    };
    // The declared decimal110 bounds need more fractional places than Decimal.
    // Round the positive bound down and reserve one 1e-28 unit for reference
    // parsing. This accepts a stricter bound; it cannot widen the declaration.
    let bound_text = case["absolute_bound"].as_str().unwrap();
    let (integer, fraction) = bound_text.split_once('.').unwrap();
    let bound = rust_decimal::Decimal::from_str_exact(&format!(
        "{integer}.{}",
        &fraction[..fraction.len().min(28)]
    ))
    .unwrap();
    let reference_rounding_guard = rust_decimal::Decimal::new(1, 28);
    assert!(bound > reference_rounding_guard);
    assert!(
        (number(&data["amount"]) - number(&case["expected_decimal"])).abs()
            <= bound - reference_rounding_guard,
        "{}: {data}",
        case["id"]
    );
}
pub fn run() -> Value {
    let rows:Vec<_>=cases().into_iter().map(|case| {
        let output=convex::library::execute_json(&request(&case));check(&case,&output);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
