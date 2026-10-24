//! Financial references precede actual transport envelopes.
#![allow(dead_code)]
#[path = "contract.rs"]
mod original;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("coupon-graph-risk.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    original::request(case)
}
fn prices(actual: &Value, expected: &Value) {
    for (field, name) in [
        ("base_pv", "base"),
        ("plus_pv", "plus"),
        ("minus_pv", "minus"),
        ("half_plus_pv", "half_plus"),
        ("half_minus_pv", "half_minus"),
        ("value", "value"),
        ("half_value", "half_value"),
    ] {
        let a = actual[field].as_f64().unwrap();
        let b = expected[name].as_f64().unwrap();
        assert!(
            a.is_finite() && (a - b).abs() <= 1e-8,
            "{field}: {a} != {b}"
        );
    }
}
pub fn check(case: &Value, out: &str) {
    original::check(case, out);
    if !case["expected_error"].is_null() {
        return;
    }
    let e: Value = serde_json::from_str(out).unwrap();
    let data = &e["result"]["data"];
    if case["request"]["operation"]["kind"] == "term_coupon_graph_risk" {
        prices(&data["sensitivity"], &case["expected"]);
        assert_eq!(data["valuations"].as_array().unwrap().len(), 5);
        for v in data["valuations"].as_array().unwrap() {
            assert_eq!(
                v["valuation"]["terms"],
                case["request"]["operation"]["input"]["bond"]["terms"]
            );
        }
    } else {
        assert_eq!(data["reserved_factor_evaluations"], 20);
        let actual = data["vector"]["sensitivities"].as_array().unwrap();
        assert_eq!(actual.len(), 4);
        assert_eq!(data["coupon_factors"].as_array().unwrap().len(), 4);
        for (a, b) in actual
            .iter()
            .take(3)
            .zip(case["expected_factors"].as_array().unwrap())
        {
            prices(a, b);
        }
        assert_eq!(actual[3]["value"], 0.0);
        assert_eq!(actual[3]["half_value"], 0.0);
        assert!(
            (data["valuation"]["present_value"].as_f64().unwrap()
                - case["expected_base"].as_f64().unwrap())
            .abs()
                <= 1e-8
        );
        assert_eq!(
            data["valuation"]["terms"],
            case["request"]["operation"]["input"]["bond"]["terms"]
        );
    }
}
pub fn run() -> Value {
    let mut rows = Vec::new();
    for c in cases() {
        let output = convex::library::execute_json(&request(&c));
        check(&c, &output);
        rows.push(json!({"id":c["id"],"request":c["request"],"expected_error":c["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()}));
    }
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
