//! Independent scalar units and exact native envelope preservation.
#![allow(dead_code)]
use serde_json::{json, Value};
use std::str::FromStr;
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("yield-scalars.json")).unwrap()["cases"]
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
        "{}: host altered native envelope",
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
    assert_eq!(data["unit"], case["unit"]);
    assert_eq!(data["representation"], case["representation"]);
    assert_eq!(data.as_object().unwrap().len(), 3);
    if case["representation"] == "decimal" {
        let parse = |v: &Value| rust_decimal::Decimal::from_str(v.as_str().unwrap()).unwrap();
        assert!(
            (parse(&data["value"]) - parse(&case["expected_decimal"])).abs()
                <= parse(&case["absolute_bound"]),
            "{}: {data}",
            case["id"]
        );
    } else {
        let expected = case["expected"].as_str().unwrap().parse::<f64>().unwrap();
        let bound = case["absolute_bound"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        assert!(
            (data["value"].as_f64().unwrap() - expected).abs() <= bound,
            "{}: {data}",
            case["id"]
        );
    }
}
pub fn run() -> Value {
    let rows: Vec<_> = cases().into_iter().map(|case| {
        let output = convex::library::execute_json(&request(&case)); check(&case, &output);
        json!({"id":case["id"], "request":case["request"], "expected_error":case["expected_error"],
            "expected_envelope":serde_json::from_str::<Value>(&output).unwrap()})
    }).collect();
    json!({"schema_version":1, "status":"passed", "cases":rows, "certified":false})
}
