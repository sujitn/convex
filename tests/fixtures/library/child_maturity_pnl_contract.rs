//! Full original financial controls plus earned-child cash at native maturity.
#[allow(dead_code)]
#[path = "dated_cash_pnl_contract.rs"]
mod original_dated;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;
fn exact(v: &Value) -> Decimal {
    Decimal::from_str(v.as_str().unwrap()).unwrap()
}
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("child-maturity-pnl-admitted.json")).unwrap()
        ["cases"]
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
    let spec = &case["request"]["operation"]["input"];
    let book = &spec["book"];
    let first = case["cash_point"].as_u64().unwrap() as usize;
    let mature = case["maturity_point"].as_u64().unwrap() as usize;
    let moved = case["transfer_point"].as_u64().unwrap() as usize;
    let n = book["steps"].as_array().unwrap().len();
    let trades = answer["result"]["data"]["report"]["trades"]
        .as_array()
        .unwrap();
    let fact = &spec["dated_entitlements"][0];
    let origin = trades.iter().find(|t| t["id"] == fact["origin"]).unwrap();
    for at in [mature, first, moved] {
        assert_eq!(book["standalone"][at - 1]["kind"], "unavailable");
    }
    assert_eq!(book["before"]["point"]["valuation_date"], "2029-05-07");
    for trade in trades.iter().skip(1) {
        assert_eq!(trade["marks"][mature - 1]["state"]["status"], "active");
        for mark in &trade["marks"].as_array().unwrap()[mature..=n] {
            assert_eq!(mark["state"]["status"], "matured");
            assert_eq!(exact(&mark["state"]["remaining_fraction"]), Decimal::ZERO);
            assert_eq!(mark["reference_amount"]["value"].as_f64().unwrap(), 0.0);
            if !mark["physical_delivery"].is_null() {
                assert_eq!(
                    mark["physical_delivery"]["outstanding_asset_present_value"]["value"]
                        .as_f64()
                        .unwrap(),
                    0.0
                );
            }
        }
    }
    let claim = |point: usize| {
        origin["marks"][point]["claims"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["obligation"]["key"] == fact["key"])
            .unwrap()
    };
    assert_eq!(claim(first - 1)["entitled"], false);
    assert_eq!(
        claim(first)["residual_ownership"]["dated_entitlement"],
        *fact
    );
    assert_eq!(
        claim(first)["residual_ownership"]["proposal"]["event_id"],
        fact["proposal_id"]
    );
    assert!(trades
        .iter()
        .any(|t| !t["marks"][mature]["physical_delivery"].is_null()
            && exact(&t["marks"][mature]["physical_delivery"]["outstanding_face"])
                != Decimal::ZERO));
    let excluded = fact["residual"]["cutoff"]["receipt_ids"]
        .as_array()
        .unwrap();
    if fact["origin"] == spec["cohorts"][2]["trade_id"] {
        assert_eq!(excluded.len(), 1);
        assert_ne!(
            exact(&claim(first)["residual_ownership"]["prior_recorded"]["value"]),
            Decimal::ZERO
        );
    } else {
        assert!(excluded.is_empty());
        let ratio = claim(first)["transfer_ownership"]["discount_ratio"]
            .as_f64()
            .unwrap();
        assert!(ratio > 0.0 && ratio < 1.0);
    }
}
pub fn run(mut execute: impl FnMut(&str) -> String) -> Vec<Value> {
    cases().into_iter().map(|case| {
        let request = case["request"].to_string();
        let answer = execute(&request);
        check(&case, &answer);
        json!({"id":case["id"],"request":case["request"],"request_json":request,
            "expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&answer).unwrap()})
    }).collect()
}
