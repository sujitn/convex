//! Independent supplied records and actual complete native coupon columns.
#![allow(dead_code)]
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("directional-hedges.json")).unwrap()["cases"]
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
fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}
fn near(actual: &Value, expected: &Value, bound: &Value) {
    assert!(
        (actual.as_f64().unwrap() - number(expected)).abs() <= number(bound),
        "actual={actual} reference={expected} bound={bound}"
    );
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
        if let Some(text) = case["expected_message_contains"].as_str() {
            assert!(
                actual["error"]["message"].as_str().unwrap().contains(text),
                "{}: {actual}",
                case["id"]
            );
        }
        return;
    }
    let data = &actual["result"]["data"];
    let op = &case["request"]["operation"];
    if op["kind"] == "term_coupon_directional_hedge_risk" {
        assert_eq!(data["reserved_factor_evaluations"], 27);
        assert_eq!(data["factors"].as_array().unwrap().len(), 5);
        assert_eq!(data["vector"]["sensitivities"].as_array().unwrap().len(), 5);
        assert_eq!(data["valuation"]["terms"], op["input"]["bond"]["terms"]);
        for (i, reference) in case["expected_hedge"]["factors"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let record = &data["vector"]["sensitivities"][i];
            let s = &record["result"];
            assert_eq!(s["key"], op["risk"]["factors"][i]["key"]);
            assert_eq!(s["position"], op["risk"]["reference_position"]);
            assert_eq!(s["snapshot_id"], op["risk"]["snapshot_id"]);
            if i < 3 {
                assert_eq!(record["kind"], "central");
                near(
                    &s["value"],
                    &reference["expected_value"],
                    &reference["absolute_bound"],
                );
                near(
                    &s["half_value"],
                    &reference["expected_half_value"],
                    &reference["absolute_bound"],
                );
                assert_eq!(*s, data["factors"][i]["result"]["sensitivity"]);
            } else {
                assert_eq!(record["kind"], "directional");
                let typed: convex::quote_risk::CouponDirectionalSensitivity =
                    serde_json::from_value(s.clone()).unwrap();
                typed.validate().unwrap();
                near(
                    &s["first"],
                    &reference["expected_value"],
                    &reference["absolute_bound"],
                );
                near(
                    &s["half_first"],
                    &reference["expected_half_value"],
                    &reference["absolute_bound"],
                );
                near(
                    &s["second"],
                    &reference["expected_second"],
                    &reference["absolute_second_bound"],
                );
                near(
                    &s["half_second"],
                    &reference["expected_half_second"],
                    &reference["absolute_second_bound"],
                );
                let proof = &data["factors"][i]["result"];
                let mut direct = case["request"].clone();
                direct["operation"] = json!({"kind":"term_coupon_directional_parameter_risk", "input":op["input"],
                    "key":s["key"], "position":s["position"], "snapshot_id":s["snapshot_id"], "settings":s["settings"]});
                assert_eq!(*proof, native(&direct.to_string())["result"]["data"]);
                for (sample, value) in proof["native"]["prices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(s["native_prices"].as_array().unwrap())
                {
                    assert_eq!(sample["valuation"]["present_value"], *value);
                    assert_eq!(sample["valuation"]["terms"], op["input"]["bond"]["terms"]);
                }
            }
        }
        // Full native columns feed the same shared aggregate/solver without a
        // supplied-record reconstruction or financial expected-value generation.
        let mut follow = cases()[1]["request"].clone();
        let p = &mut follow["operation"]["problem"];
        p["entity"] = data["vector"]["entity"].clone();
        p["snapshot_id"] = op["risk"]["snapshot_id"].clone();
        p["settlement"] = data["vector"]["settlement"].clone();
        p["expected_holdings"] = json!([data["vector"]["id"]]);
        p["holdings"][0]["risk"] = data["vector"].clone();
        p["candidates"][0]["risk"] = data["vector"].clone();
        p["factors"] = Value::Array(
            op["risk"]["factors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| {
                    json!({
            "key":f["key"], "risk_settings":f["settings"], "credit":null, "scale":1,
            "weight":1, "target":0, "lower":-1e8, "upper":1e8})
                })
                .collect(),
        );
        let report = native(&follow.to_string());
        assert_eq!(report["ok"], "true", "{report}");
        let result = &report["result"]["data"];
        assert_eq!(result["status"], "feasible");
        assert_eq!(result["proposal"]["quantities"], json!(["-7500"]));
        for (i, r) in case["expected_hedge"]["factors"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            near(
                &result["exposure"]["before"][i],
                &r["expected_current_pending_value"],
                &r["absolute_aggregate_bound"],
            );
        }
    } else if case["compare_original_central"] == true {
        let mut original = case["request"].clone();
        original["operation"]["kind"] = json!("hedge");
        let p = &mut original["operation"]["problem"];
        for key in ["holdings", "candidates"] {
            for col in p[key].as_array_mut().unwrap() {
                for s in col["risk"]["sensitivities"].as_array_mut().unwrap() {
                    *s = s["result"].clone();
                }
            }
        }
        let old = native(&original.to_string());
        assert_eq!(old["ok"], "true");
        for key in [
            "status",
            "authority",
            "proposal",
            "continuous",
            "rank",
            "work",
            "message",
        ] {
            assert_eq!(data[key], old["result"]["data"][key]);
        }
        for key in ["current", "pending", "before"] {
            assert_eq!(
                data["exposure"][key],
                old["result"]["data"]["exposure"][key]
            );
        }
    } else {
        let solve = op["action"]["kind"] == "solve";
        let exposure = if solve { &data["exposure"] } else { data };
        near(
            &exposure["before"][0],
            &case["expected_before"],
            &case["absolute_bound"],
        );
        if solve {
            assert_eq!(
                data["status"], case["expected_status"],
                "{}: {data}",
                case["id"]
            );
            if case["expected_quantity"].is_null() {
                assert!(data["proposal"].is_null());
            } else {
                assert_eq!(
                    data["proposal"]["quantities"],
                    json!([case["expected_quantity"]])
                );
            }
        }
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
