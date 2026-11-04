//! Complete Treasury transport checks; independent equation answers are separate.
#![allow(dead_code)]
#[path = "contract.rs"]
mod original;
use serde_json::Value;

pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("treasury-bill-admitted.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn native(input: &str) -> Value {
    original::native(input)
}
pub fn request(case: &Value) -> String {
    original::request(case)
}
pub fn check(case: &Value, actual: &str) {
    original::check(case, actual);
}
