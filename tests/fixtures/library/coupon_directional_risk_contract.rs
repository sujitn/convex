//! Signed independent controls retain the separately checked full native proof.
#![allow(dead_code)]
#[path = "coupon_parameter_directional_contract.rs"]
mod directional;
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text(
        "tests/fixtures/library/coupon-directional-risk.json",
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
fn native(input: &str) -> Value {
    let result = convex::library::decode_request(input).and_then(|r| convex::library::execute(&r));
    match result {
        Ok(result) => json!({"ok":"true","schema_version":1,"result":result}),
        Err(error) => json!({"ok":"false","schema_version":1,"error":error}),
    }
}
fn number(v: &Value) -> f64 {
    v.as_str().unwrap().parse().unwrap()
}
pub fn check(case: &Value, output: &str) {
    let expected = native(&request(case));
    let actual: Value = serde_json::from_str(output).unwrap();
    let success = case["expected_error"].is_null();
    assert_eq!(
        actual["ok"],
        if success { "true" } else { "false" },
        "{}: {actual}",
        case["id"]
    );
    if !success {
        assert_eq!(actual["error"]["code"], case["expected_error"]);
    }
    assert_eq!(
        actual, expected,
        "{}: adapter changed native result/error",
        case["id"]
    );
    if !case["expected_error"].is_null() {
        return;
    }
    let output: Value = serde_json::from_str(output).unwrap();
    let data = &output["result"]["data"];
    let operation = &case["request"]["operation"];
    assert_eq!(data["key"], operation["key"]);
    assert_eq!(data["position"], operation["position"]);
    assert_eq!(data["snapshot_id"], operation["snapshot_id"]);
    assert_eq!(data["settings"], operation["settings"]);
    assert_eq!(data["second_unit"], "currency_per_model_parameter_squared");
    let position: convex::quote_risk::RiskPosition =
        serde_json::from_value(operation["position"].clone()).unwrap();
    assert_eq!(
        data["position_scale"].as_f64().unwrap(),
        position.scale().unwrap()
    );
    for field in ["first", "half_first", "second", "half_second"] {
        let bound = number(
            &case[if field.contains("second") {
                "absolute_scaled_second_bound"
            } else {
                "absolute_scaled_first_bound"
            }],
        );
        assert!(
            (data[field].as_f64().unwrap() - number(&case["expected"][field])).abs() <= bound,
            "{}/{field}",
            case["id"]
        );
        assert_eq!(
            data[field].as_f64().unwrap(),
            data["native"][field].as_f64().unwrap() * data["position_scale"].as_f64().unwrap()
        );
    }
    assert_eq!(data["scaled_prices"].as_array().unwrap().len(), 6);
    for (actual, expected) in data["scaled_prices"]
        .as_array()
        .unwrap()
        .iter()
        .zip(case["expected"]["scaled_prices"].as_array().unwrap())
    {
        assert!(
            (actual.as_f64().unwrap() - number(expected)).abs()
                <= number(&case["absolute_scaled_pv_bound"])
        );
    }
    let coordinate: Value =
        serde_json::from_str(operation["key"]["coordinate"]["input_id"].as_str().unwrap()).unwrap();
    let mut native_request = case["request"].clone();
    native_request["operation"] = json!({"kind":"term_coupon_directional_derivatives", "input":operation["input"], "coordinate":coordinate, "settings":operation["settings"]["native"]});
    let native_case = json!({"id":case["id"], "request":native_request, "expected_error":null,
        "expected":case["reference_expected"], "absolute_pv_bound":1e-8,
        "absolute_first_bound":2e-7, "absolute_second_bound":2e-3});
    let embedded = native(&native_request.to_string());
    assert_eq!(embedded["ok"], "true");
    assert_eq!(embedded["result"]["data"], data["native"]);
    directional::check(&native_case, &embedded.to_string());
}
pub fn run() -> Value {
    let rows: Vec<_> = cases().into_iter().map(|case| {
        let output = convex::library::execute_json(&request(&case)); check(&case, &output);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
