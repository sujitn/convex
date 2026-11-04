//! Exact helper reachability, original revisions and complete native evidence.
#[path = "../../../tests/fixtures/library/rate_helpers_contract.rs"]
mod contract;
use convex::library::*;
use serde_json::Value;
fn output(id: &str) -> Value {
    let case = contract::cases()
        .into_iter()
        .find(|c| c["id"] == id)
        .unwrap();
    let value = contract::native(&contract::request(&case));
    assert_eq!(value["ok"], "true", "{id}: {value}");
    value["result"]["data"].clone()
}
#[test]
fn every_helper_term_revision_and_native_failure_roundtrips() {
    for case in contract::cases() {
        let text = contract::request(&case);
        let typed = decode_request(&text).unwrap();
        assert_eq!(
            serde_json::to_value(&typed).unwrap(),
            case["request"],
            "{}",
            case["id"]
        );
        contract::check(&case, &execute_json(&text));
        if !case["expected_error"].is_null() {
            assert!(contract::native(&text).get("result").is_none());
        }
    }
}
#[test]
fn computed_diagnostics_preserve_original_quotes_and_source_order() {
    for kind in ["deposit", "fra", "irs", "ois", "rate_future", "basis"] {
        let out = output(&format!("rate_helpers.{kind}.positive"));
        let priced = &out["repricing"];
        let original = &priced["recipe"]["helpers"][0];
        let quote = &priced["quotes"][0];
        assert_eq!(quote["helper_id"], original["id"]);
        assert_eq!(quote["input"], original["quote"]);
        assert_eq!(quote["tolerance"], original["tolerance"]);
        assert_eq!(quote["input"]["unit"], quote["model"]["unit"]);
        assert_eq!(priced["price_evaluations"], 1);
        assert_eq!(out["quote_versions"].as_array().unwrap().len(), 1);
        assert!(out["quote_versions"][0].as_u64().unwrap() > 1);
        assert_eq!(priced["within_tolerance"], true);
    }
    let off = output("rate_helpers.deposit.off_market");
    assert_eq!(off["repricing"]["within_tolerance"], false);
    assert!(
        off["repricing"]["quotes"][0]["residual"]
            .as_f64()
            .unwrap()
            .abs()
            > 0.9
    );
}
#[test]
fn actual_known_history_cash_and_future_weights_survive_dispatch() {
    let known = output("rate_helpers.fra.known");
    let v = &known["repricing"]["valuations"][0]["valuation"];
    assert!(v["query"].is_null());
    assert_eq!(v["origin"]["Published"]["revision"], 3);
    assert!(
        (known["repricing"]["quotes"][0]["model"]["value"]
            .as_f64()
            .unwrap()
            - 421.25)
            .abs()
            < 1e-10
    );
    let future = output("rate_helpers.rate_future.positive");
    let v = &future["repricing"]["valuations"][0]["valuation"];
    assert_eq!(v["known_days"], 21);
    assert_eq!(v["projected_days"], 70);
    assert!(!v["observations"].as_array().unwrap().is_empty());
    for kind in ["irs", "ois", "basis"] {
        let out = output(&format!("rate_helpers.{kind}.positive"));
        let v = &out["repricing"]["valuations"][0]["valuation"];
        assert!(v.get("cashflows").is_some() || v.get("pay").is_some());
    }
}
#[test]
fn explicit_maximum_helper_set_keeps_every_observation_and_original_order() {
    let out = output("rate_helpers.deposit.max_helpers");
    let priced = &out["repricing"];
    assert_eq!(priced["price_evaluations"], 128);
    assert_eq!(priced["quotes"].as_array().unwrap().len(), 128);
    assert_eq!(priced["valuations"].as_array().unwrap().len(), 128);
    assert_eq!(out["quote_versions"][127], 1128);
    for (index, quote) in priced["quotes"].as_array().unwrap().iter().enumerate() {
        assert_eq!(quote["helper_id"], format!("explicit-deposit-{index:03}"));
    }
}
