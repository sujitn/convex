//! Full original financial/custody/dated-cash checkers remain mandatory.
#[allow(dead_code)]
#[path = "dated_cash_pnl_contract.rs"]
mod original_dated;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("pending-cash-pnl-bound.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn check(case: &Value, output: &str) {
    original_dated::check(case, output);
    if !case["expected_error"].is_null() || case["id"].as_str().unwrap().contains("regression") {
        return;
    }
    let answer: Value = serde_json::from_str(output).unwrap();
    let input = &case["request"]["operation"]["input"];
    let fact = &input["dated_entitlements"][0];
    let root = answer["result"]["data"]["report"]["trades"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == fact["origin"])
        .unwrap();
    let marks = root["marks"].as_array().unwrap();
    let independent = input["book"]["steps"].as_array().unwrap().len() + 1;
    let cash = |point: usize| {
        marks[point]["claims"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["obligation"]["key"] == fact["key"])
            .unwrap()
    };
    assert_eq!(cash(0)["entitled"], false);
    assert_eq!(cash(1)["entitled"], true);
    assert_eq!(cash(independent)["entitled"], true);
    assert_eq!(
        cash(1)["residual_ownership"],
        cash(independent)["residual_ownership"]
    );
    let pending: f64 = marks[1]["physical_delivery"]["outstanding_face"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        pending.abs() > 0.0,
        "cash acquisition must occur with undelivered face"
    );
    assert_eq!(cash(1)["residual_ownership"]["dated_entitlement"], *fact);
}
pub fn run(mut execute: impl FnMut(&str) -> String) -> Vec<Value> {
    cases().into_iter().map(|case| {
        let request=case["request"].to_string();let answer=execute(&request);check(&case,&answer);
        json!({"id":case["id"],"request":case["request"],"request_json":request,"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&answer).unwrap()})
    }).collect()
}
