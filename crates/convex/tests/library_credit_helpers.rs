//! Complete original CDS terms, native-unit residuals and actual cash-state evidence.
#[path = "../../../tests/fixtures/library/credit_helpers_contract.rs"]
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
fn every_original_cds_helper_term_and_native_failure_roundtrips() {
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
fn original_mixed_quotes_revisions_units_and_source_order_are_retained() {
    for id in ["mixed", "global", "reversed"] {
        let out = output(&format!("credit_helpers.{id}"));
        assert_eq!(out["within_tolerance"], true);
        assert_eq!(out["price_evaluations"], 6);
        for (h, q) in out["recipe"]["helpers"]
            .as_array()
            .unwrap()
            .iter()
            .zip(out["quotes"].as_array().unwrap())
        {
            assert_eq!(q["helper_id"], h["id"]);
            assert_eq!(q["input"], h["quote"]);
            assert_eq!(q["model"]["unit"], h["quote"]["unit"]);
            assert_eq!(q["tolerance"], h["tolerance"]);
            assert!(h["quote_version"].as_u64().unwrap() >= 100);
        }
    }
    let off = output("credit_helpers.off_market");
    assert_eq!(off["within_tolerance"], false);
    assert!(off["quotes"][0]["residual"].as_f64().unwrap() < -0.9);
    let seller = output("credit_helpers.seller");
    let base = output("credit_helpers.mixed");
    assert_eq!(seller["quotes"], base["quotes"]);
    for (a, b) in seller["valuations"]
        .as_array()
        .unwrap()
        .iter()
        .zip(base["valuations"].as_array().unwrap())
    {
        assert_eq!(
            a["net_pv"].as_f64().unwrap(),
            -b["net_pv"].as_f64().unwrap()
        );
    }
}

#[test]
fn clean_dirty_and_supplied_pending_settled_cash_are_distinct() {
    let pending = output("credit_helpers.pending_cash");
    let settled = output("credit_helpers.settled_cash");
    assert_eq!(
        pending["recipe"]["helpers"][0]["terms"]["cash_settlement_date"],
        pending["recipe"]["helpers"][0]["context"]["valuation_date"]
    );
    assert_eq!(
        pending["recipe"]["helpers"][0]["terms"]["settlement_lag_business_days"],
        0
    );
    assert_eq!(
        pending["recipe"]["helpers"][0]["context"]["initial_payment_state"],
        "Pending"
    );
    assert_eq!(
        settled["recipe"]["helpers"][0]["context"]["initial_payment_state"],
        "Settled"
    );
    assert_eq!(pending["quotes"], settled["quotes"]);
    let a = &pending["valuations"][0];
    let b = &settled["valuations"][0];
    assert_ne!(a["net_pv"], b["net_pv"]);
    assert_ne!(a["clean_upfront_fraction"], a["dirty_upfront_fraction"]);
    assert_eq!(a["clean_upfront_fraction"], b["clean_upfront_fraction"]);
    assert_eq!(
        a["discount_curve"],
        pending["recipe"]["helpers"][0]["terms"]["discount_curve"]
    );
}

#[test]
fn maximum_helper_boundary_keeps_all_observations_and_work() {
    let out = output("credit_helpers.max_helpers");
    assert_eq!(out["price_evaluations"], 128);
    assert_eq!(out["quotes"].as_array().unwrap().len(), 128);
    assert_eq!(out["valuations"].as_array().unwrap().len(), 128);
    for (i, h) in out["recipe"]["helpers"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(h["id"], format!("original-cds-{i:03}"));
        assert_eq!(h["quote_version"], 1001 + i);
        assert_eq!(out["quotes"][i]["helper_id"], h["id"]);
    }
}
