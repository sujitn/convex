//! Full original cash/issuer/repo diagnostics, strict native failures and bounds.
#[path = "../../../tests/fixtures/library/cash_helpers_contract.rs"]
mod contract;
use convex::library::*;
use serde_json::Value;

fn output(id: &str) -> Value {
    let case = contract::cases()
        .into_iter()
        .find(|c| c["id"] == id)
        .unwrap();
    let out = contract::native(&contract::request(&case));
    assert_eq!(out["ok"], "true", "{id}: {out}");
    out["result"]["data"].clone()
}

#[test]
fn every_original_cash_helper_and_typed_failure_roundtrips() {
    for case in contract::cases() {
        let text = contract::request(&case);
        assert_eq!(
            serde_json::to_value(decode_request(&text).unwrap()).unwrap(),
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
fn all_original_units_cash_models_revisions_and_residuals_are_retained() {
    for family in ["government", "reference_basis", "credit_basis", "repo"] {
        for suffix in ["positive", "reversed"] {
            let out = output(&format!("cash_helpers.{family}.{suffix}"));
            assert_eq!(out["within_tolerance"], true);
            assert_eq!(
                out["price_evaluations"].as_u64().unwrap() as usize,
                out["quotes"].as_array().unwrap().len()
            );
            for (h, q) in out["recipe"]["helpers"]
                .as_array()
                .unwrap()
                .iter()
                .zip(out["quotes"].as_array().unwrap())
            {
                assert_eq!(q["helper_id"], h["input"]["id"]);
                assert_eq!(q["input"], h["input"]["quote"]);
                assert_eq!(q["model"]["unit"], h["input"]["quote"]["unit"]);
                assert_eq!(q["tolerance"], h["input"]["tolerance"]);
                assert!(h["input"]["quote_version"].as_u64().unwrap() > 0);
            }
        }
        let out = output(&format!("cash_helpers.{family}.off_market"));
        assert_eq!(out["within_tolerance"], false);
        assert!(out["quotes"][0]["residual"].as_f64().unwrap() < 0.0);
    }
    let out = output("cash_helpers.credit_basis.positive");
    for v in out["valuations"].as_array().unwrap() {
        assert!(v["Bond"]["basis"]["recovery_pv"].as_f64().unwrap() > 0.0);
        assert_eq!(v["Bond"]["basis"]["model"]["kind"], "CreditParRecovery");
    }
    let repo = output("cash_helpers.repo.positive");
    for (q, unit) in
        repo["quotes"]
            .as_array()
            .unwrap()
            .iter()
            .zip(["DecimalRate", "PercentRate", "BasisPoints"])
    {
        assert_eq!(q["model"]["unit"], unit);
    }
}

#[test]
fn original_steps_amortization_and_adjusted_cash_pillars_survive() {
    let out = output("cash_helpers.government.positive");
    let original = &out["recipe"]["helpers"][1]["input"];
    assert_eq!(original["settlement"], "2026-10-13");
    assert_eq!(
        original["terms"]["profiles"]["coupon"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let p = &out["valuations"][1]["Bond"]["value"]["projection"];
    assert_eq!(p["principal"].as_array().unwrap().len(), 2);
    assert_eq!(out["recipe"]["helpers"][2]["input"]["pillar"], "2028-10-09");
    assert_eq!(
        out["recipe"]["helpers"][2]["input"]["terms"]["schedule"]["end_date"],
        "2028-10-07"
    );
    assert_ne!(out["quotes"][2]["model"]["value"], 420.0);
}

#[test]
fn maximum_cash_and_repo_helpers_preserve_every_observation_and_work() {
    for family in ["government", "repo"] {
        let out = output(&format!("cash_helpers.{family}.max_helpers"));
        assert_eq!(out["within_tolerance"], true);
        assert_eq!(out["price_evaluations"], 128);
        assert_eq!(out["valuations"].as_array().unwrap().len(), 128);
        for (i, h) in out["recipe"]["helpers"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            assert_eq!(h["input"]["id"], format!("cash-original-{i:03}"));
            assert_eq!(h["input"]["quote_version"], 1001 + i);
            assert_eq!(out["quotes"][i]["helper_id"], h["input"]["id"]);
        }
    }
}
