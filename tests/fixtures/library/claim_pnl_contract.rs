//! Full original financial checks and independently declared legal cash shares.
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
#[allow(dead_code)]
#[path = "lot_pnl_contract.rs"]
mod qualified_lot;
use serde_json::{json, Value};

pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text("tests/fixtures/library/claim-pnl-bound.json"))
        .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
fn close(a: &Value, e: &Value, bound: f64) {
    let number = |v: &Value| {
        v.as_f64()
            .unwrap_or_else(|| v.as_str().unwrap().parse::<f64>().unwrap())
    };
    assert!(
        (number(a) - number(e)).abs() <= bound,
        "{a} != {e}; bound={bound}"
    );
}
fn ownership(a: &Value, e: &Value) {
    let v = &a["ownership"];
    assert_eq!(v["origin"], e["origin"]);
    assert_eq!(v["obligation"]["key"], e["key"]);
    for k in ["entitlement", "proposal", "transfers"] {
        assert_eq!(v[k], e[k]);
    }
    close(&v["fraction"], &e["fraction"], 1e-12);
    for k in ["owned_proposed", "owned_recorded", "owned_outstanding"] {
        close(&v[k]["value"], &e[k], 1e-10);
        assert_eq!(v[k]["currency"], v["obligation"]["amount"]["currency"]);
    }
    close(&a["discount_ratio"], &e["ratio"], 1e-12);
    close(&a["present_value"]["value"], &e["pv"], 1e-7);
    let receipts = v["receipts"].as_array().unwrap();
    let expected = e["receipts"].as_array().unwrap();
    assert_eq!(receipts.len(), expected.len());
    for (r, e) in receipts.iter().zip(expected) {
        assert_eq!(r["event"], e["event"]);
        close(&r["fraction"], &e["fraction"], 1e-12);
        close(&r["amount"]["value"], &e["amount"], 1e-10);
    }
}
pub fn check(case: &Value, output: &str) {
    qualified_lot::check(case, output);
    if !case["expected_error"].is_null() {
        return;
    }
    let answer: Value = serde_json::from_str(output).unwrap();
    let trades = answer["result"]["data"]["report"]["trades"]
        .as_array()
        .unwrap();
    for (trade, reference) in trades
        .iter()
        .zip(case["expected"]["trades"].as_array().unwrap())
    {
        for (mark, e) in trade["marks"]
            .as_array()
            .unwrap()
            .iter()
            .zip(reference["marks"].as_array().unwrap())
        {
            for claim in mark["claims"].as_array().unwrap() {
                let expected = e["claims"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["key"] == claim["obligation"]["key"])
                    .unwrap();
                if expected["transfer"].is_null() {
                    assert!(claim["transfer_ownership"].is_null());
                } else {
                    ownership(&claim["transfer_ownership"], &expected["transfer"]);
                }
            }
            let received = e["transferred"].as_array().unwrap();
            if received.is_empty() {
                assert!(mark["transferred_claims"].is_null());
            } else {
                let actual = mark["transferred_claims"].as_array().unwrap();
                assert_eq!(actual.len(), received.len());
                for (a, e) in actual.iter().zip(received) {
                    ownership(a, e);
                }
            }
        }
    }
}
pub fn run(mut execute: impl FnMut(&str) -> String) -> Vec<Value> {
    cases().into_iter().map(|case| {
        let request=case["request"].to_string();let answer=execute(&request);check(&case,&answer);
        json!({"id":case["id"],"request":case["request"],"request_json":request,
               "expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&answer).unwrap()})
    }).collect()
}
