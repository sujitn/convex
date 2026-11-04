//! Complete original financial checks plus actual conserved custody provenance.
#[allow(dead_code)]
#[path = "claim_pnl_contract.rs"]
mod original;
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text(
        "tests/fixtures/library/physical-partition-pnl-final.json",
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn check(case: &Value, output: &str) {
    original::check(case, output);
    if !case["expected_error"].is_null() {
        return;
    }
    let answer: Value = serde_json::from_str(output).unwrap();
    for (a, e) in answer["result"]["data"]["report"]["trades"]
        .as_array()
        .unwrap()
        .iter()
        .zip(case["expected"]["trades"].as_array().unwrap())
    {
        for (a, e) in a["marks"]
            .as_array()
            .unwrap()
            .iter()
            .zip(e["marks"].as_array().unwrap())
        {
            let actual = &a["physical_delivery"];
            let expected = &e["physical"];
            if expected.is_null() {
                continue;
            }
            assert_eq!(
                actual["invoice_excluded"].as_bool().unwrap_or(false),
                expected["invoice_excluded"].as_bool().unwrap_or(false)
            );
            let partition = &actual["partition"];
            let p = &expected["partition"];
            if p.is_null() {
                assert!(partition.is_null());
                continue;
            }
            assert_eq!(partition["facts"], p["facts"]);
            for k in [
                "original_proposed_face",
                "original_received_face",
                "original_outstanding_face",
                "fraction",
            ] {
                crate_close(&partition[k], &p[k], 1e-12);
            }
            let receipts = partition["receipts"].as_array().unwrap();
            let reference = p["receipts"].as_array().unwrap();
            assert_eq!(receipts.len(), reference.len());
            for (r, e) in receipts.iter().zip(reference) {
                assert_eq!(r["event"], e["event"]);
                crate_close(&r["face"], &e["face"], 1e-12);
            }
        }
    }
}
fn crate_close(a: &Value, e: &Value, bound: f64) {
    let number = |v: &Value| {
        v.as_f64()
            .unwrap_or_else(|| v.as_str().unwrap().parse::<f64>().unwrap())
    };
    assert!((number(a) - number(e)).abs() <= bound, "{a} != {e}");
}
pub fn run(mut execute: impl FnMut(&str) -> String) -> Vec<Value> {
    cases().into_iter().map(|case| {
        let request=case["request"].to_string();let answer=execute(&request);check(&case,&answer);
        json!({"id":case["id"],"request":case["request"],"request_json":request,"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&answer).unwrap()})
    }).collect()
}
