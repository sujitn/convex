//! Complete original financial checks and independently sourced dated cash.
#[allow(dead_code)]
#[path = "custody_lineage_pnl_contract.rs"]
mod original_custody;
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;
fn exact(v: &Value) -> Decimal {
    Decimal::from_str(v.as_str().unwrap()).unwrap()
}
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text(
        "tests/fixtures/library/dated-cash-pnl-cash.json",
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn check(case: &Value, output: &str) {
    original_custody::check(case, output);
    if !case["expected_error"].is_null() {
        return;
    }
    let answer: Value = serde_json::from_str(output).unwrap();
    let trades = answer["result"]["data"]["report"]["trades"]
        .as_array()
        .unwrap();
    for (trade, e) in trades
        .iter()
        .zip(case["expected"]["trades"].as_array().unwrap())
    {
        for (mark, expected) in trade["marks"]
            .as_array()
            .unwrap()
            .iter()
            .zip(e["marks"].as_array().unwrap())
        {
            for cash in mark["claims"].as_array().unwrap() {
                let reference = expected["claims"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["key"] == cash["obligation"]["key"])
                    .unwrap();
                assert_eq!(
                    cash["residual_ownership"]["dated_entitlement"],
                    reference["residual"]["dated_entitlement"]
                );
            }
        }
    }
    let spec = &case["request"]["operation"]["input"];
    for dated in spec["dated_entitlements"].as_array().unwrap() {
        let root = trades.iter().find(|t| t["id"] == dated["origin"]).unwrap();
        for (i, mark) in root["marks"].as_array().unwrap().iter().enumerate() {
            let Some(cash) = mark["claims"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["obligation"]["key"] == dated["key"])
            else {
                continue;
            };
            let active = dated["applied_points"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p.as_u64() == Some(i as u64));
            assert_eq!(cash["entitled"], active);
            if !active {
                assert!(cash["residual_ownership"].is_null());
                continue;
            }
            let residual = &cash["residual_ownership"];
            assert_eq!(residual["proposal"]["event_id"], dated["proposal_id"]);
            let mut recorded = Decimal::ZERO;
            let mut outstanding = Decimal::ZERO;
            for t in trades {
                let m = &t["marks"][i];
                for c in m["claims"].as_array().unwrap() {
                    let o = &c["transfer_ownership"]["ownership"];
                    if o["origin"] == dated["origin"] && o["obligation"]["key"] == dated["key"] {
                        recorded += exact(&o["owned_recorded"]["value"]);
                        outstanding += exact(&o["owned_outstanding"]["value"]);
                    }
                }
                for c in m["transferred_claims"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                {
                    let o = &c["ownership"];
                    if o["origin"] == dated["origin"] && o["obligation"]["key"] == dated["key"] {
                        recorded += exact(&o["owned_recorded"]["value"]);
                        outstanding += exact(&o["owned_outstanding"]["value"]);
                    }
                }
            }
            assert_eq!(recorded, exact(&residual["owned_recorded"]["value"]));
            assert_eq!(outstanding, exact(&residual["owned_outstanding"]["value"]));
        }
    }
}
pub fn run(mut execute: impl FnMut(&str) -> String) -> Vec<Value> {
    cases().into_iter().map(|case| {
        let request=case["request"].to_string();let answer=execute(&request);check(&case,&answer);
        json!({"id":case["id"],"request":case["request"],"request_json":request,"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&answer).unwrap()})
    }).collect()
}
