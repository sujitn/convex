//! Independent currency references and complete native proof precede host parity.
#![allow(dead_code)]
#[path = "contract.rs"]
mod original;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("coupon-parameter-hedges.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    original::request(case)
}
fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}
pub fn check(case: &Value, output: &str) {
    original::check(case, output);
    let envelope: Value = serde_json::from_str(output).unwrap();
    if !case["expected_error"].is_null() {
        if let Some(order) = case["expected_resolution_order"].as_u64() {
            assert!(envelope["error"]["details"]["native_kind"]
                .as_str()
                .unwrap()
                .contains(&format!("DerivativeResolution {{ order: {order},")));
        }
        return;
    }
    let data = &envelope["result"]["data"];
    let operation = &case["request"]["operation"];
    if operation["kind"] == "term_coupon_parameter_risk" {
        let sensitivity = &data["sensitivity"];
        for (field, reference) in [
            ("base_pv", "base"),
            ("plus_pv", "plus"),
            ("minus_pv", "minus"),
            ("half_plus_pv", "half_plus"),
            ("half_minus_pv", "half_minus"),
            ("value", "first"),
            ("half_value", "half_first"),
        ] {
            let bound = if field == "value" || field == "half_value" {
                "absolute_first_bound"
            } else {
                "absolute_pv_bound"
            };
            assert!(
                (sensitivity[field].as_f64().unwrap() - number(&case["expected"][reference])).abs()
                    <= number(&case["bounds"][bound]),
                "{} {field}",
                case["id"]
            );
        }
        for field in ["key", "position", "snapshot_id", "settings"] {
            assert_eq!(sensitivity[field], operation[field]);
        }
        let mut native_request = case["request"].clone();
        let point: Value =
            serde_json::from_str(operation["key"]["coordinate"]["input_id"].as_str().unwrap())
                .unwrap();
        native_request["operation"] = json!({"kind":"term_coupon_parameter_derivatives","input":operation["input"],"coordinate":point,"settings":operation["native_settings"]});
        let native = original::native(&native_request.to_string());
        assert_eq!(native["ok"], "true");
        assert_eq!(data["native"], native["result"]["data"]);
        assert_eq!(data["native"]["coordinate"], point);
        for name in ["base", "plus", "minus", "half_plus", "half_minus"] {
            assert!(
                (data["native"][name]["present_value"].as_f64().unwrap()
                    - number(&case["reference_expected"][name]))
                .abs()
                    <= 1e-8
            );
            assert_eq!(
                data["native"][name]["terms"],
                operation["input"]["bond"]["terms"]
            );
            assert_eq!(
                data["native"][name]["context"],
                operation["input"]["context"]
            );
        }
    } else {
        let typed: convex::workflow::GraphHedgeRiskInput =
            serde_json::from_value(operation["risk"].clone()).unwrap();
        assert_eq!(data["input"], serde_json::to_value(typed).unwrap());
        assert_eq!(data["parameter_policies"], operation["parameter_policies"]);
        assert_eq!(data["reserved_factor_evaluations"], 25);
        assert_eq!(data["vector"]["sensitivities"].as_array().unwrap().len(), 5);
        assert_eq!(data["factors"].as_array().unwrap().len(), 5);
        for (i, expected) in case["expected_hedge"]["factors"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let actual = &data["vector"]["sensitivities"][i];
            assert_eq!(actual, &data["factors"][i]["result"]["sensitivity"]);
            for (field, name) in [
                ("value", "expected_value"),
                ("half_value", "expected_half_value"),
            ] {
                assert!(
                    (actual[field].as_f64().unwrap() - number(&expected[name])).abs()
                        <= number(&expected["absolute_bound"])
                );
            }
            assert_eq!(actual["key"], operation["risk"]["factors"][i]["key"]);
            if i < 3 {
                assert_eq!(data["factors"][i]["kind"], "graph");
            } else {
                assert_eq!(data["factors"][i]["kind"], "parameter");
            }
        }
        assert_eq!(
            data["valuation"]["terms"],
            operation["input"]["bond"]["terms"]
        );
        assert!(
            (data["valuation"]["present_value"].as_f64().unwrap() - 5003.367373323093).abs()
                <= 1e-8
        );
    }
}
pub fn run() -> Value {
    let rows:Vec<_>=cases().into_iter().map(|case| {
        let output=convex::library::execute_json(&request(&case));check(&case,&output);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
