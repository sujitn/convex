//! Complete shared dispatch with independent root and error-scope expectations.
#![allow(dead_code)]
use serde_json::{json, Value};

pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("signed-yield-selection.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
pub fn check(case: &Value, output: &str) {
    let actual: Value = serde_json::from_str(output).unwrap();
    let native = match convex::library::decode_request(&request(case))
        .and_then(|request| convex::library::execute(&request))
    {
        Ok(result) => json!({"ok":"true","schema_version":1,"result":result}),
        Err(error) => json!({"ok":"false","schema_version":1,"error":error}),
    };
    assert_eq!(actual, native, "{} complete native envelope", case["id"]);
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
        if !case["expected_root_scope"].is_null() {
            assert_eq!(
                actual["error"]["details"]["root_scope"],
                case["expected_root_scope"]
            );
        }
        return;
    }
    let data = &actual["result"]["data"];
    let selected = &data["selection"];
    let number = |key: &str| {
        case["expected"][key]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap()
    };
    assert_eq!(data["unit"], "annual_decimal_rate");
    assert_eq!(data["residual_unit"], "input_cash");
    let rate = selected["yield_value"].as_f64().unwrap();
    assert!(
        (rate - number("root")).abs() <= number("rate_bound"),
        "{}: {selected}",
        case["id"]
    );
    assert!(selected["residual"].as_f64().unwrap().abs() < number("price_bound"));
    assert!(selected["rate_error_bound"].as_f64().unwrap() <= number("rate_bound"));
    assert_eq!(
        selected["branch"],
        case["request"]["operation"]["action"]["branch"]
    );
    assert!(
        rate >= selected["branch"]["lower"].as_f64().unwrap()
            && rate <= selected["branch"]["upper"].as_f64().unwrap()
    );
    let derivatives = selected["derivative_bounds"].as_array().unwrap();
    assert!(derivatives[0].as_f64().unwrap() > 0.0 || derivatives[1].as_f64().unwrap() < 0.0);
    if !case["expected"]["discount_cash"].is_null() {
        assert_eq!(data["discount_cash"], case["expected"]["discount_cash"]);
    }
}
pub fn run() -> Value {
    let rows:Vec<_>=cases().into_iter().map(|case| {
        let output=convex::library::execute_json(&request(&case));
        check(&case,&output);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
