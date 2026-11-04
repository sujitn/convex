//! Actual contractual default cash has independently declared Decimal80 prices.
use convex::constrained_hedge as h;
use convex::library::{execute, LibraryRequest};
use serde_json::{json, Value};

fn control() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/cds-events-contract.json"
    ))
    .unwrap()
}
fn request(recovery: &str) -> Value {
    let mut v = control()["request"].clone();
    v["operation"]["lifecycle"]["events"][1]["event"]["action"]["terms"]["recovery_fraction"] =
        json!(recovery);
    v
}
fn calculate(v: &Value) -> Value {
    execute(&serde_json::from_value::<LibraryRequest>(v.clone()).unwrap()).unwrap()["data"].clone()
}

#[test]
fn actual_default_claims_match_independent_five_price_cash_and_risk() {
    let control = control();
    let bound: f64 = control["absolute_bound"].as_str().unwrap().parse().unwrap();
    for fraction in ["0", "0.35", "1"] {
        let data = calculate(&request(fraction));
        let expected = &control["expected"][fraction];
        let mark = &data["lifecycle_mark"];
        assert_eq!(mark["state"]["status"], "default_cash_pending");
        assert_eq!(mark["state"]["remaining_fraction"], "0");
        assert_eq!(data["lifecycle"]["default_entity"], "ACME");
        for (kind, field) in [
            ("recovery", "protection_cash"),
            ("default_accrual", "default_accrual"),
        ] {
            let claim = mark["state"]["obligations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["key"]["kind"] == kind)
                .unwrap();
            let actual: f64 = claim["amount"]["value"].as_str().unwrap().parse().unwrap();
            let expected: f64 = expected[field].as_str().unwrap().parse().unwrap();
            assert_eq!(actual, expected);
        }
        for name in [
            "base_pv",
            "plus_pv",
            "minus_pv",
            "half_plus_pv",
            "half_minus_pv",
            "value",
            "half_value",
        ] {
            let actual = data["lifecycle"]["current"][0][name].as_f64().unwrap();
            let expected: f64 = expected["prices"][name].as_str().unwrap().parse().unwrap();
            assert!(
                (actual - expected).abs() <= bound,
                "recovery={fraction} field={name} actual={actual} expected={expected}"
            );
        }
        for row in data["lifecycle"]["pending"].as_array().unwrap() {
            assert_eq!(row["base_pv"], 0.0);
            assert_eq!(row["value"], 0.0);
        }
        for row in data["lifecycle"]["current"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1)
        {
            assert_eq!(
                row["value"], 0.0,
                "actual auction cash must not retain modeled credit or projection risk"
            );
        }
    }
}

#[test]
fn unknown_recovery_is_explicitly_unpriced_and_validates_actual_owner() {
    let mut v = request("0.35");
    v["operation"]["lifecycle"]["events"]
        .as_array_mut()
        .unwrap()
        .pop();
    let data = calculate(&v);
    assert_eq!(
        data["lifecycle_unpriced"]["state"]["status"],
        "default_pending"
    );
    assert!(data["lifecycle_unpriced"]["state"]["recovery"].is_null());
    assert_eq!(
        data["lifecycle_unpriced"]["reason"],
        "actual_cds_recovery_unknown"
    );
    assert!(data.get("lifecycle").is_none());
    assert!(data.get("lifecycle_mark").is_none());
    let mut invalid = Vec::new();
    let mut bad = v.clone();
    bad["operation"]["lifecycle"]["position"]["value"] = json!("-449999");
    invalid.push(bad);
    let mut bad = v.clone();
    let closing = bad["operation"]["history"]["causes"]
        .as_array()
        .unwrap()
        .len();
    let position = bad["operation"]["history"]["points"][closing]["positions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["identity"]["id"] == "graph-hedge.CDS")
        .unwrap();
    position["pending_quantity"] = json!("1");
    invalid.push(bad);
    let mut bad = v.clone();
    bad["operation"]["lifecycle"]["events"][0]["event"]["action"]["terms"]["entity"] =
        json!("WRONG");
    invalid.push(bad);
    let mut bad = v.clone();
    bad["operation"]["product"]["terms"]["coupon_decimal"] = json!("0.02");
    invalid.push(bad);
    let mut bad = v;
    bad["operation"]["lifecycle"]["events"][0]["event"]["known_at"] = json!("2026-10-08T00:00:00Z");
    invalid.push(bad);
    for bad in invalid {
        assert!(execute(&serde_json::from_value::<LibraryRequest>(bad).unwrap()).is_err());
    }
}

fn problem(data: &Value) -> h::HedgeProblem {
    let mut vector = data["vector"].clone();
    vector["sensitivities"].as_array_mut().unwrap().truncate(1);
    let state = &data["lifecycle"];
    serde_json::from_value(json!({
        "snapshot_id":state["snapshot_id"],"entity":vector["entity"],"settlement":state["settlement"],
        "reporting_currency":"USD","position_state":state["manifest"],"expected_holdings":[vector["id"]],
        "holdings":[{"risk":vector,"current_quantity":state["position"]["current"]["quantity"],"pending_quantity":"0"}],
        "factors":[{"key":vector["sensitivities"][0]["key"],"risk_settings":vector["sensitivities"][0]["settings"],
            "credit":null,"scale":1.0,"weight":1.0,"target":0.0,"lower":-1e12,"upper":1e12}],
        "candidates":[{"risk":vector,"eligibility":{"eligible":true,"decision":{"id":"synthetic.paper","version":1},"reason":"synthetic test only"},
            "trade_lower":"-1000000","trade_upper":"1000000","holding_lower":"-10000000","holding_upper":"10000000",
            "buy_available":"1000000","sell_available":"1000000","lot":null,
            "costs":{"currency":"USD","buy_per_reference":"0","sell_per_reference":"0","observation":{"id":"synthetic.zero-cost","version":1}}}],
        "max_cost":0.0,"settings":{"policy_version":1,"rank_relative_tolerance":1e-10,"max_condition":1e4,"solver_tolerance":1e-10,
            "factor_tolerance":1e-8,"cost_tolerance":1e-10,"cost_scale":1.0,"cost_weight":0.0,"max_work":1000,"svd_iterations":256}
    })).unwrap()
}

#[test]
fn defaulted_entity_cannot_be_a_fresh_hedge_candidate() {
    let data = calculate(&request("0.35"));
    let p = problem(&data);
    let mut state: h::LifecycleHoldingRisk =
        serde_json::from_value(data["lifecycle"].clone()).unwrap();
    state.current.truncate(1);
    state.pending.truncate(1);
    let error = h::solve_lifecycle_hedge_proposal(&p, &[state])
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("no fresh defaulted-entity candidates"),
        "{error}"
    );
}

#[test]
fn recovery_reference_dates_and_legal_scope_cannot_be_substituted() {
    let original = request("0.35");
    let mut invalid = Vec::new();
    let mut v = original.clone();
    v["operation"]["lifecycle"]["events"][1]["event"]["action"]["terms"]["default_event"] =
        json!("other");
    invalid.push(v);
    let mut v = original.clone();
    v["operation"]["lifecycle"]["events"][1]["event"]["action"]["terms"]["cash_date"] =
        json!("2026-10-06");
    invalid.push(v);
    let mut v = original.clone();
    v["operation"]["lifecycle"]["events"][1]["event"]["action"]["terms"]["recovery_fraction"] =
        json!("1.01");
    invalid.push(v);
    let mut v = original;
    v["operation"]["lifecycle"]["events"][0]["event"]["action"]["terms"]["document"]["version"] =
        json!(2);
    invalid.push(v);
    for v in invalid {
        assert!(execute(&serde_json::from_value::<LibraryRequest>(v).unwrap()).is_err());
    }
    #[cfg(feature = "schemars")]
    assert!(
        serde_json::to_string(&schemars::schema_for!(LibraryRequest))
            .unwrap()
            .contains("default_entity")
    );
}
