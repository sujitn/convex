//! Shared complete transport contract; independent financial values stay separate.
#![allow(dead_code)]
#[path = "contract.rs"]
mod original;
use serde_json::Value;
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("constrained-hedge-protocol.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn references() -> Vec<Value> {
    let mut values =
        serde_json::from_str::<Value>(include_str!("constrained-hedge-reference.json")).unwrap()
            ["cases"]
            .as_array()
            .unwrap()
            .clone();
    values.extend(
        serde_json::from_str::<Value>(include_str!("constrained-hedge-lots-reference.json"))
            .unwrap()["cases"]
            .as_array()
            .unwrap()
            .clone(),
    );
    values
}
pub fn request(case: &Value) -> String {
    original::request(case)
}
pub fn native(input: &str) -> Value {
    original::native(input)
}
pub fn check(case: &Value, actual: &str) {
    original::check(case, actual);
    let value: Value = serde_json::from_str(actual).unwrap();
    if !case["expected_status"].is_null() {
        let data = &value["result"]["data"];
        assert_eq!(
            data["status"], case["expected_status"],
            "{}: {data}",
            case["id"]
        );
        assert_eq!(data["authority"], "proposal_only");
        assert_eq!(
            data["proposal"].is_null(),
            case["expected_status"] != "feasible"
        );
    }
}
pub fn number(value: &Value) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| value.as_str().unwrap().parse().unwrap())
}
pub fn close(a: &Value, b: &Value, bound: f64) {
    let a = number(a);
    let b = number(b);
    assert!((a - b).abs() <= bound, "{a} vs {b}, original bound {bound}");
}
