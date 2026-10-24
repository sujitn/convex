//! Complete independent original financial levels and legal lot provenance.
#[allow(dead_code)]
#[path = "residual_pnl_contract.rs"]
mod baseline;
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use serde_json::{json, Value};

pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text(
        "tests/fixtures/library/lot-pnl-chronology.json",
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn check(case: &Value, output: &str) {
    baseline::check(case, output);
    if !case["expected_error"].is_null() {
        return;
    }
    let answer: Value = serde_json::from_str(output).unwrap();
    let spec = &case["request"]["operation"]["input"];
    let book = &spec["book"];
    let mut points = vec![&book["before"]];
    points.extend(
        book["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| &s["after"]),
    );
    points.extend(
        book["standalone"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["kind"] == "available")
            .map(|s| &s["information"]),
    );
    for (j, trade) in answer["result"]["data"]["report"]["trades"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        for (i, mark) in trade["marks"].as_array().unwrap().iter().enumerate() {
            let h = &book["holdings"][points[i]["holdings"][j].as_u64().unwrap() as usize];
            let amendments = h["lifecycle"]["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["event"]["action"]["kind"] == "bond_amendment")
                .map(|s| s["event"].clone())
                .collect::<Vec<_>>();
            if amendments.is_empty() {
                assert!(mark["state"]["amendments"].is_null());
            } else {
                assert_eq!(mark["state"]["amendments"], json!(amendments));
                assert_eq!(
                    mark["state"]["version"],
                    amendments.last().unwrap()["action"]["terms"]["version"]
                );
            }
            let splits = spec["lot_splits"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| {
                    s["applied_points"].as_array().unwrap().contains(&json!(i))
                        && (s["parent"] == trade["id"]
                            || s["children"].as_array().unwrap().contains(&trade["id"]))
                })
                .cloned()
                .collect::<Vec<_>>();
            if splits.is_empty() {
                assert!(mark["lot_splits"].is_null());
            } else {
                assert_eq!(mark["lot_splits"], json!(splits));
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
