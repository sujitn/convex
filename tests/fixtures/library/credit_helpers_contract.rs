//! Actual supplied helper transport; independent mathematics are separate.
#![allow(dead_code)]
#[path = "contract.rs"]
mod original;
pub fn check(case: &serde_json::Value, actual: &str) {
    original::check(case, actual);
}
pub fn native(input: &str) -> serde_json::Value {
    original::native(input)
}
pub fn request(case: &serde_json::Value) -> String {
    original::request(case)
}
pub fn cases() -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(include_str!("credit-helpers.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
