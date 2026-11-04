//! Full original financial checks plus residual ownership/partial receipt facts.
#[allow(dead_code)]
#[path = "cohort_pnl_contract.rs"]
mod original;
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use serde_json::{json, Value};

pub fn cases() -> Vec<Value> {
    let mut cases = serde_json::from_str::<Value>(packed::text(
        "tests/fixtures/library/residual-pnl-canonical.json",
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone();
    if let Ok(id) = std::env::var("CVX_RESIDUAL_CASE") {
        cases.retain(|c| c["id"] == id);
    }
    cases
}
fn close(actual: &Value, expected: &Value) {
    let value = |v: &Value| {
        v.as_f64()
            .unwrap_or_else(|| v.as_str().unwrap().parse::<f64>().unwrap())
    };
    assert!(
        (value(actual) - value(expected)).abs() <= 1e-10,
        "{actual} != {expected}"
    );
}
pub fn check(case: &Value, output: &str) {
    original::check(case, output);
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
        for (mark, expected) in trade["marks"]
            .as_array()
            .unwrap()
            .iter()
            .zip(reference["marks"].as_array().unwrap())
        {
            for cash in mark["claims"].as_array().unwrap() {
                let e = expected["claims"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|e| e["key"] == cash["obligation"]["key"])
                    .unwrap();
                let owner = &cash["residual_ownership"];
                let r = &e["residual"];
                if r.is_null() {
                    assert!(owner.is_null());
                } else {
                    for k in ["entitlement", "prior_receipts", "proposal"] {
                        assert_eq!(owner[k], r[k]);
                    }
                    for k in [
                        "prior_recorded",
                        "owned_proposed",
                        "owned_recorded",
                        "owned_outstanding",
                    ] {
                        close(&owner[k]["value"], &r[k]);
                        assert_eq!(
                            owner[k]["currency"],
                            cash["obligation"]["amount"]["currency"]
                        );
                    }
                }
            }
            if !expected["physical"].is_null() {
                assert_eq!(
                    mark["physical_delivery"]["security_receipts"],
                    expected["physical"]["security_receipts"]
                );
                let p = &mark["physical_delivery"];
                let face = |k: &str| {
                    p[k].as_str()
                        .unwrap()
                        .parse::<rust_decimal::Decimal>()
                        .unwrap()
                };
                assert_eq!(
                    face("proposed_face"),
                    face("received_face") + face("outstanding_face")
                );
            }
        }
    }
}
pub fn run(mut execute: impl FnMut(&str) -> String) -> Vec<Value> {
    cases().into_iter().map(|case| {
        let request=case["request"].to_string();let answer=execute(&request);
        check(&case,&answer);
        json!({"id":case["id"],"request":case["request"],"request_json":request,
            "expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&answer).unwrap()})
    }).collect()
}
