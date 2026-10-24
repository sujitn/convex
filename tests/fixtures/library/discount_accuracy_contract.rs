//! Full native envelopes and original independently declared discount values.
#![allow(dead_code)]
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("discount-accuracy.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
pub fn check(case: &Value, text: &str) {
    let actual: Value = serde_json::from_str(text).unwrap();
    let input = convex::library::decode_request(&request(case)).unwrap();
    let native = convex::library::execute(&input).unwrap();
    assert_eq!(
        actual,
        json!({"ok":"true","schema_version":1,"result":native}),
        "{} full native envelope",
        case["id"]
    );
    let expected = case["expected"]["independent_price"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let data = &actual["result"]["data"];
    assert_eq!(data["unit"], "input_cash");
    let price = data["present_value"].as_f64().unwrap();
    assert!(
        (price - expected).abs() <= 1e-10,
        "{}: {price:.17} vs {expected:.17}",
        case["id"]
    );
    if case["request"]["operation"]["action"]["kind"] == "dated_price" {
        let years = data["discount_cash"][0][0].as_f64().unwrap();
        let expected = case["expected"]["independent_time"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        assert!((years - expected).abs() <= 1e-14, "{} time", case["id"]);
        assert_eq!(data["discount_cash"][0][1], 100.0);
    }
}
pub fn run() -> Value {
    let rows:Vec<_>=cases().into_iter().map(|case|{
        let text=convex::library::execute_json(&request(&case));check(&case,&text);
        json!({"id":case["id"],"request":case["request"],"expected_error":null,"expected_envelope":serde_json::from_str::<Value>(&text).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
