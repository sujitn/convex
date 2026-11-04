//! Independent financial controls are separate from fresh transport envelopes.
#![allow(dead_code)]
#[path = "contract.rs"]
mod original;
use serde_json::{json, Value};

pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("term-coupon-options.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(c: &Value) -> String {
    original::request(c)
}
pub fn check(c: &Value, out: &str) {
    original::check(c, out);
    if !c["expected_error"].is_null() {
        return;
    }
    let e: Value = serde_json::from_str(out).unwrap();
    let r = &e["result"]["data"];
    for name in [
        "present_value",
        "dirty_price",
        "clean_price",
        "accrued_model",
    ] {
        let bound = if name == "accrued_model" { 1e-12 } else { 1e-9 };
        let actual = r[name].as_f64().unwrap();
        let expected = c["expected"][name].as_f64().unwrap();
        assert!(
            actual.is_finite() && (actual - expected).abs() <= bound,
            "{} {name}: {actual} != {expected}",
            c["id"]
        );
    }
    assert_eq!(
        r["terms"],
        c["request"]["operation"]["input"]["bond"]["terms"]
    );
    let segments = r["segments"].as_array().unwrap();
    let declared = c["declared_segments"].as_array().unwrap();
    assert_eq!(segments.len(), declared.len());
    for (a, b) in segments.iter().zip(declared) {
        for field in ["start", "end", "payment_date", "principal"] {
            assert_eq!(a[field], b[field]);
        }
        assert_eq!(a["request"]["observation_date"], b["observation_date"]);
        assert!(
            (a["expected_cash"].as_f64().unwrap() - b["expected_cash"].as_f64().unwrap()).abs()
                <= 1e-9
        );
    }
}
pub fn run() -> Value {
    let mut captured = Vec::new();
    for c in cases() {
        let q = request(&c);
        let e = convex::library::execute_json(&q);
        check(&c, &e);
        captured.push(json!({"id":c["id"],"request":c["request"],"expected_error":c["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&e).unwrap()}));
    }
    json!({"schema_version":1,"status":"passed","cases":captured,"certified":false})
}
