//! Complete unchanged original financial models, plus exact native custody facts.
#[allow(dead_code)]
#[path = "physical_partition_pnl_contract.rs"]
mod qualified_custody;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;
fn exact(v: &Value) -> Decimal {
    Decimal::from_str(v.as_str().unwrap()).unwrap()
}
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("custody-lineage-pnl-admitted.json")).unwrap()
        ["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn check(case: &Value, output: &str) {
    qualified_custody::check(case, output);
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
            let actual = &mark["physical_delivery"];
            let e = &expected["physical"];
            if e.is_null() {
                continue;
            }
            assert_eq!(actual["lineage"], e["lineage"]);
            let actual = actual["disposals"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let expected = e["disposals"].as_array().map(Vec::as_slice).unwrap_or(&[]);
            assert_eq!(actual.len(), expected.len());
            for (a, e) in actual.iter().zip(expected) {
                let facts: convex::workflow::PhysicalDeliveryDisposal =
                    serde_json::from_value(e["facts"].clone()).unwrap();
                assert_eq!(a["facts"], serde_json::to_value(facts).unwrap());
                let fill: convex::workflow::PnlFill =
                    serde_json::from_value(e["fill"].clone()).unwrap();
                assert_eq!(a["fill"], serde_json::to_value(fill).unwrap());
                assert_eq!(exact(&a["face"]), exact(&e["face"]));
                assert_eq!(exact(&a["face"]), -exact(&a["fill"]["quantity"]));
                let receipts = a["receipts"].as_array().unwrap();
                let allocated = e["receipts"].as_array().unwrap();
                assert_eq!(receipts.len(), allocated.len());
                let mut face = Decimal::ZERO;
                for (r, e) in receipts.iter().zip(allocated) {
                    assert_eq!(r["event"], e["event"]);
                    assert_eq!(exact(&r["face"]), exact(&e["face"]));
                    face += exact(&r["face"]);
                }
                assert_eq!(face, exact(&a["face"]));
            }
        }
    }
    let input = &case["request"]["operation"]["input"];
    for link in input["physical_deliveries"].as_array().unwrap() {
        let original = input["cohorts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["trade_id"] == link["asset"])
            .unwrap();
        for point in 0..trades[0]["marks"].as_array().unwrap().len() {
            let mut proposed = Decimal::ZERO;
            let mut received = Decimal::ZERO;
            let mut outstanding = Decimal::ZERO;
            let mut disposed = Decimal::ZERO;
            let mut actual_received = Decimal::ZERO;
            let mut present = false;
            for t in trades {
                let h = &t["marks"][point]["physical_delivery"];
                if h.is_null() || h["future"] != link["future"] {
                    continue;
                }
                present = true;
                proposed += exact(&h["proposed_face"]);
                received += exact(&h["received_face"]);
                outstanding += exact(&h["outstanding_face"]);
                actual_received = h["security_receipts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| exact(&r["action"]["terms"]["face"]))
                    .sum();
                for d in h["disposals"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
                    disposed += exact(&d["face"]);
                }
                assert_eq!(
                    exact(&h["proposed_face"]),
                    exact(&h["received_face"]) + exact(&h["outstanding_face"])
                );
                if !h["partition"].is_null() {
                    let owned: Decimal = h["partition"]["receipts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| exact(&r["face"]))
                        .sum();
                    assert_eq!(owned, exact(&h["received_face"]));
                }
            }
            if present {
                assert_eq!(proposed + disposed, exact(&original["quantity"]));
                assert_eq!(received + disposed, actual_received);
                assert_eq!(received + outstanding, proposed);
            }
        }
    }
}
pub fn run(mut execute: impl FnMut(&str) -> String) -> Vec<Value> {
    cases().into_iter().map(|case| {
        let request=case["request"].to_string();let answer=execute(&request);check(&case,&answer);
        json!({"id":case["id"],"request":case["request"],"request_json":request,"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&answer).unwrap()})
    }).collect()
}
