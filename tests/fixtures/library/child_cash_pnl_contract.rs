//! Preserve every full original financial and native conservation checker.
#[allow(dead_code)]
#[path = "dated_cash_pnl_contract.rs"]
mod original_dated;
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text(
        "tests/fixtures/library/child-cash-pnl-admitted.json",
    ))
    .unwrap()["cases"]
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
    let dated = &input["dated_entitlements"][0];
    let transfer = &input["claim_transfers"][0];
    let trades = answer["result"]["data"]["report"]["trades"]
        .as_array()
        .unwrap();
    let root = trades.iter().find(|t| t["id"] == dated["origin"]).unwrap();
    let n = input["book"]["steps"].as_array().unwrap().len();
    let first = dated["applied_points"].as_array().unwrap()[0]
        .as_u64()
        .unwrap() as usize;
    let move_at = transfer["applied_points"].as_array().unwrap()[0]
        .as_u64()
        .unwrap() as usize;
    let cash = |point: usize| {
        root["marks"][point]["claims"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["obligation"]["key"] == dated["key"])
            .unwrap()
    };
    assert_eq!(cash(first - 1)["entitled"], false);
    assert_eq!(
        cash(first)["residual_ownership"],
        cash(n + 1)["residual_ownership"]
    );
    assert_eq!(
        cash(move_at)["transfer_ownership"],
        cash(n + 2)["transfer_ownership"]
    );
    assert_eq!(
        cash(n + 2)["residual_ownership"]["dated_entitlement"],
        *dated
    );
    assert!(trades.iter().any(|t| {
        t["marks"][first]["physical_delivery"]["outstanding_face"]
            .as_str()
            .is_some_and(|s| s.parse::<f64>().unwrap().abs() > 0.0)
    }));
    for t in trades {
        assert_eq!(
            t["marks"][move_at]["transferred_claims"],
            t["marks"][n + 2]["transferred_claims"]
        );
    }
}
pub fn run(mut execute: impl FnMut(&str) -> String) -> Vec<Value> {
    cases().into_iter().map(|case| {
        let request=case["request"].to_string();let answer=execute(&request);check(&case,&answer);
        json!({"id":case["id"],"request":case["request"],"request_json":request,"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&answer).unwrap()})
    }).collect()
}
