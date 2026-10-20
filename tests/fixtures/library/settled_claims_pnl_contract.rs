//! Complete original financial checks and fully settled independently sourced cash.
#[allow(dead_code)]
#[path = "dated_cash_pnl_contract.rs"]
mod original_dated;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::{collections::BTreeMap, str::FromStr};
fn exact(v: &Value) -> Decimal {
    Decimal::from_str(v.as_str().unwrap()).unwrap()
}
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("settled-claims-pnl-complete.json")).unwrap()
        ["cases"]
        .as_array()
        .unwrap()
        .clone()
}
fn allocations<'a>(mark: &'a Value, fact: &Value) -> Vec<&'a Value> {
    mark["claims"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| &c["transfer_ownership"]["ownership"])
        .chain(
            mark["transferred_claims"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| &c["ownership"]),
        )
        .filter(|o| o["origin"] == fact["origin"] && o["obligation"]["key"] == fact["key"])
        .collect()
}
pub fn check(case: &Value, output: &str) {
    original_dated::check(case, output);
    if !case["expected_error"].is_null() || case["id"].as_str().unwrap().contains("regression") {
        return;
    }
    let answer: Value = serde_json::from_str(output).unwrap();
    let spec = &case["request"]["operation"]["input"];
    let book = &spec["book"];
    let n = book["steps"].as_array().unwrap().len();
    let trades = answer["result"]["data"]["report"]["trades"]
        .as_array()
        .unwrap();
    assert_eq!(spec["dated_entitlements"].as_array().unwrap().len(), 4);
    assert_eq!(spec["claim_transfers"].as_array().unwrap().len(), 12);
    for trade in trades.iter().skip(1) {
        for (point, mark) in trade["marks"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .take(n + 1)
            .skip(4)
        {
            assert_eq!(
                mark["state"]["status"],
                if point == case["closure_point"].as_u64().unwrap() as usize {
                    "closed"
                } else {
                    "matured"
                }
            );
            assert_eq!(exact(&mark["state"]["remaining_fraction"]), Decimal::ZERO);
            assert_eq!(mark["reference_amount"]["value"].as_f64().unwrap(), 0.0);
        }
    }
    for fact in spec["dated_entitlements"].as_array().unwrap() {
        let first = fact["applied_points"][0].as_u64().unwrap() as usize;
        let mut paid = BTreeMap::new();
        for i in first..=n {
            let mut shares = Decimal::ZERO;
            let mut current = BTreeMap::new();
            for trade in trades {
                for owner in allocations(&trade["marks"][i], fact) {
                    shares += exact(&owner["fraction"]);
                    for receipt in owner["receipts"].as_array().unwrap() {
                        let id = receipt["event"]["action"]["terms"]["receipt_id"]
                            .as_str()
                            .unwrap();
                        assert!(!fact["residual"]["cutoff"]["receipt_ids"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|r| r == id));
                        current.insert(
                            (id.to_owned(), trade["id"].as_str().unwrap().to_owned()),
                            receipt.clone(),
                        );
                    }
                }
            }
            assert_eq!(shares, Decimal::ONE);
            for (key, prior) in &paid {
                assert_eq!(
                    current.get(key),
                    Some(prior),
                    "paid recipient changed for {key:?}"
                );
            }
            paid = current;
        }
        if case["id"].as_str().unwrap().ends_with("prefix") {
            let zero_owner = allocations(&trades[5]["marks"][n], fact);
            assert_eq!(zero_owner.len(), 1);
            assert_eq!(exact(&zero_owner[0]["fraction"]), Decimal::ZERO);
            assert_ne!(
                exact(&zero_owner[0]["owned_recorded"]["value"]),
                Decimal::ZERO
            );
        }
    }
    let settled = case["selected_settlement_point"].as_u64().unwrap() as usize;
    let closed = case["closure_point"].as_u64().unwrap() as usize;
    for fact in spec["dated_entitlements"].as_array().unwrap() {
        for point in [settled, closed] {
            for trade in trades {
                for owner in allocations(&trade["marks"][point], fact) {
                    assert_eq!(exact(&owner["owned_outstanding"]["value"]), Decimal::ZERO);
                    assert_eq!(owner["owned_proposed"], owner["owned_recorded"]);
                }
            }
        }
    }
    for trade in trades {
        assert_eq!(trade["marks"][closed]["state"]["status"], "closed");
        assert!(trade["marks"][closed]["state"]["obligations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["settled"] == true));
    }
    assert!(trades
        .iter()
        .skip(1)
        .any(
            |t| t["marks"][settled]["physical_delivery"]["outstanding_face"]
                .as_str()
                .is_some_and(|s| Decimal::from_str(s).unwrap() != Decimal::ZERO)
        ));
    for at in case["transfer_points"].as_array().unwrap() {
        assert_eq!(
            book["standalone"][at.as_u64().unwrap() as usize - 1]["kind"],
            "unavailable"
        );
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
