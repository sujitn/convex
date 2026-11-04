//! Actual complete native envelopes and independently declared dated cash values.
#![allow(dead_code)]
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text("tests/fixtures/library/dated-yield.json")).unwrap()
        ["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
fn native(input: &str) -> Value {
    match convex::library::decode_request(input).and_then(|r| convex::library::execute(&r)) {
        Ok(result) => json!({"ok":"true","schema_version":1,"result":result}),
        Err(error) => json!({"ok":"false","schema_version":1,"error":error}),
    }
}
pub fn check(case: &Value, output: &str) {
    let actual: Value = serde_json::from_str(output).unwrap();
    assert_eq!(
        actual,
        native(&request(case)),
        "{} full native envelope",
        case["id"]
    );
    assert_eq!(
        actual["ok"],
        if case["expected_error"].is_null() {
            "true"
        } else {
            "false"
        },
        "{} {actual}",
        case["id"]
    );
    if !case["expected_error"].is_null() {
        assert_eq!(
            actual["error"]["code"], case["expected_error"],
            "{} {actual}",
            case["id"]
        );
        return;
    }
    let data = &actual["result"]["data"];
    let row = &case["expected"];
    let expected = |field: &str| {
        row["expected"][field]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap()
    };
    let comp = case["request"]["operation"]["action"]["kind"]
        .as_str()
        .unwrap();
    if comp == "dated_risk" {
        assert_eq!(data["unit"], "input_cash");
        assert_eq!(data["risk"].as_object().unwrap().len(), 7);
        for key in [
            "present_value",
            "first_derivative",
            "second_derivative",
            "macaulay_duration",
            "modified_duration",
            "convexity",
            "dv01_loss",
        ] {
            assert!(
                (data["risk"][key].as_f64().unwrap() - expected(key)).abs() <= 1e-9,
                "{} {key}: {data}",
                case["id"]
            );
        }
    } else {
        let cash = data["discount_cash"].as_array().unwrap();
        assert_eq!(cash.len(), row["cash"].as_array().unwrap().len());
        for (i, pair) in cash.iter().enumerate() {
            let years = row["discount_years"][i]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!(
                (pair[0].as_f64().unwrap() - years).abs() <= 1e-14,
                "{} actual discount time",
                case["id"]
            );
            assert_eq!(
                pair[1].as_f64().unwrap(),
                row["cash"][i]["amount"]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap()
            );
        }
        if comp == "dated_price" {
            assert_eq!(data["unit"], "input_cash");
            assert!(
                (data["present_value"].as_f64().unwrap() - expected("present_value")).abs()
                    <= 1e-10,
                "{} {data}",
                case["id"]
            );
        } else {
            assert_eq!(data["unit"], "annual_decimal_rate");
            assert_eq!(data["residual_unit"], "input_cash");
            let rate = row["annual_yield"]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!(
                (data["yield_value"].as_f64().unwrap() - rate).abs() <= 1e-11,
                "{} {data}",
                case["id"]
            );
            assert!(data["residual"].as_f64().unwrap().abs() < 1e-10);
            assert!(data["iterations"].as_u64().unwrap() < 100);
        }
    }
}
pub fn run() -> Value {
    let rows:Vec<_>=cases().into_iter().map(|case| {
        let output=convex::library::execute_json(&request(&case));check(&case,&output);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
