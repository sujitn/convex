//! Shared transport checks plus separately frozen independent financial controls.
#![allow(dead_code)]
#[path = "contract.rs"]
mod original;
use serde_json::Value;
pub fn check(case: &Value, actual: &str) {
    original::check(case, actual);
}
pub fn native(input: &str) -> Value {
    original::native(input)
}
pub fn request(case: &Value) -> String {
    original::request(case)
}
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("government-inverse-protocol.json")).unwrap()
        ["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn references() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("government-inverse-admitted-reference.json"))
        .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn number(value: &Value) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| value.as_str().unwrap().parse().unwrap())
}
pub fn close(actual: &Value, expected: &Value, bound: f64) {
    let a = number(actual);
    let b = number(expected);
    assert!(
        (a - b).abs() <= bound,
        "{a} versus {b}, absolute bound {bound}"
    );
}
