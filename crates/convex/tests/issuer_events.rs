//! Explicit issuer admission reuses original contract, claim, risk and solver owners.
use convex::constrained_hedge as h;
use convex::library::{execute, LibraryRequest};
use serde_json::{json, Value};

fn reference() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/issuer-events-contract.json"
    ))
    .unwrap()
}
fn request(name: &str) -> Value {
    let mut value = if name == "cds" {
        serde_json::from_str::<Value>(include_str!(
            "../../../tests/fixtures/library/cds-events-contract.json"
        ))
        .unwrap()["request"]
            .clone()
    } else {
        serde_json::from_str::<Value>(include_str!(
            "../../../tests/fixtures/library/bond-events-contract.json"
        ))
        .unwrap()["requests"]["original"]
            .clone()
    };
    let op = &mut value["operation"];
    if name != "cds" {
        op["lifecycle"]["instrument"]["credit"]["entity"] = json!("ACME");
    }
    for event in op["lifecycle"]["events"].as_array_mut().unwrap() {
        if event["event"]["action"]["kind"] == "credit_default" {
            event["event"]["action"]["terms"]["entity"] = json!("ACME");
            event["event"]["event_id"] = json!("core.issuer.default.1");
        }
        if event["event"]["action"]["kind"] == "recovery" {
            event["event"]["action"]["terms"]["default_event"] = json!("core.issuer.default.1");
            event["event"]["action"]["terms"]["recovery_fraction"] =
                json!(if name == "cds" { "0.40" } else { "0.35" });
        }
    }
    if name == "second_bond" {
        for field in ["product", "instrument"] {
            let bond = if field == "product" {
                &mut op["product"]["bond"]
            } else {
                &mut op["lifecycle"]["instrument"]["bond"]
            };
            bond["terms"]["face_value"] = json!("2400");
            bond["terms"]["rounding"] = json!({"NearestEven":2});
            bond["terms"]["profiles"]["notional"][0]["value"] = json!("2400");
            bond["terms"]["profiles"]["notional"][1]["value"] = json!("1440");
        }
        op["input"]["reference_position"]["quantity"] = json!("2400");
        op["input"]["reference_position"]["reference_quantity"] = json!("2400");
        op["input"]["identity"]["id"] = json!("issuer.bond.2");
        op["input"]["identity"]["instrument"]["id"] = json!("issuer.security.2");
        op["lifecycle"]["trade_id"] = json!("issuer.bond.2");
        op["lifecycle"]["position"]["value"] = json!("2400");
        for e in op["lifecycle"]["events"].as_array_mut().unwrap() {
            e["event"]["trade_id"] = json!("issuer.bond.2");
        }
        for point in op["history"]["points"].as_array_mut().unwrap() {
            for position in point["positions"].as_array_mut().unwrap() {
                if position["identity"]["id"] == "mixed.bond" {
                    position["identity"]["id"] = json!("issuer.bond.2");
                    position["identity"]["instrument"]["id"] = json!("issuer.security.2");
                    position["current"]["quantity"] = json!("2400");
                    position["current"]["reference_quantity"] = json!("2400");
                }
            }
        }
    }
    value
}
fn calculate(value: Value) -> Value {
    execute(&serde_json::from_value::<LibraryRequest>(value).unwrap()).unwrap()["data"].clone()
}
fn number(v: &Value) -> f64 {
    v.as_str().unwrap().parse().unwrap()
}

#[test]
fn shared_notice_retains_each_contracts_signed_cash_and_original_collateral_risk() {
    let reference = reference();
    for name in ["bond", "second_bond", "cds"] {
        let data = calculate(request(name));
        assert_eq!(data["lifecycle"]["default_entity"], "ACME");
        assert_eq!(
            data["lifecycle_mark"]["state"]["default_event"],
            "core.issuer.default.1"
        );
        let expected = &reference["expected"][name];
        for (kind, field) in [
            ("recovery", "recovery_cash"),
            (
                if name == "cds" {
                    "default_accrual"
                } else {
                    "bond_coupon"
                },
                "earned_cash",
            ),
        ] {
            let claim = data["lifecycle_mark"]["state"]["obligations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["key"]["kind"] == kind)
                .unwrap();
            assert_eq!(number(&claim["amount"]["value"]), number(&expected[field]));
        }
        for field in [
            "base_pv",
            "plus_pv",
            "minus_pv",
            "half_plus_pv",
            "half_minus_pv",
            "value",
            "half_value",
        ] {
            let actual = data["lifecycle"]["current"][0][field].as_f64().unwrap();
            assert!(
                (actual - number(&expected["prices"][field])).abs()
                    <= number(&reference["absolute_bound"]),
                "{name}/{field}: {actual}"
            );
        }
        for state in data["lifecycle"]["pending"].as_array().unwrap() {
            assert_eq!(state["value"], 0.0);
        }
    }
}

fn problem(data: &Value) -> h::HedgeProblem {
    let mut vector = data["vector"].clone();
    vector["sensitivities"].as_array_mut().unwrap().truncate(1);
    let state = &data["lifecycle"];
    let mut candidate = vector.clone();
    candidate["id"] = json!("other.candidate");
    candidate["instrument"] = json!({"id":"other.security","version":1});
    serde_json::from_value(json!({
        "snapshot_id":state["snapshot_id"],"entity":vector["entity"],"settlement":state["settlement"],"reporting_currency":"USD",
        "position_state":state["manifest"],"expected_holdings":[vector["id"]],
        "holdings":[{"risk":vector,"current_quantity":state["position"]["current"]["quantity"],"pending_quantity":"0"}],
        "factors":[{"key":vector["sensitivities"][0]["key"],"risk_settings":vector["sensitivities"][0]["settings"],"credit":null,
            "scale":1.0,"weight":1.0,"target":0.0,"lower":-1e12,"upper":1e12}],
        "candidates":[{"risk":candidate,"eligibility":{"eligible":true,"decision":{"id":"synthetic.paper","version":1},"reason":"synthetic only"},
            "trade_lower":"-1000000","trade_upper":"1000000","holding_lower":"-10000000","holding_upper":"10000000",
            "buy_available":"1000000","sell_available":"1000000","lot":null,
            "costs":{"currency":"USD","buy_per_reference":"0","sell_per_reference":"0","observation":{"id":"synthetic.zero-cost","version":1}}}],
        "max_cost":0.0,"settings":{"policy_version":1,"rank_relative_tolerance":1e-10,"max_condition":1e4,"solver_tolerance":1e-10,
            "factor_tolerance":1e-8,"cost_tolerance":1e-10,"cost_scale":1.0,"cost_weight":0.0,"max_work":1000,"svd_iterations":256}
    })).unwrap()
}
fn setup() -> (h::HedgeProblem, h::LifecycleHoldingRisk, h::IssuerRiskScope) {
    let data = calculate(request("bond"));
    let p = problem(&data);
    let mut state: h::LifecycleHoldingRisk =
        serde_json::from_value(data["lifecycle"].clone()).unwrap();
    state.current.truncate(1);
    state.pending.truncate(1);
    let notice = reference()["assumptions"].clone();
    let scope=serde_json::from_value(json!({"reference":{"id":"scope","version":2},"snapshot_id":p.snapshot_id,
        "directory":{"issuers":[{"entity_id":"ACME","version":1,"default":{"id":notice["default_event"],
            "effective_date":notice["default_date"],"known_at":"2026-10-07T12:00:00Z","reference_id":"actual.source","reference_version":1}},
            {"entity_id":"OTHER","version":1,"default":null}],
        "instruments":[{"id":p.holdings[0].risk.instrument.id,"version":1,"entity_id":"ACME"},
            {"id":"other.security","version":1,"entity_id":"OTHER"},{"id":"another.issuer.bond","version":1,"entity_id":"ACME"}]}})).unwrap();
    (p, state, scope)
}

#[test]
fn issuer_default_excludes_a_distinct_security_without_parent_contagion() {
    let (mut p, state, scope) = setup();
    let actual =
        h::solve_issuer_lifecycle_hedge_proposal(&p, std::slice::from_ref(&state), &scope).unwrap();
    assert_eq!(
        actual,
        h::solve_lifecycle_hedge_proposal(&p, std::slice::from_ref(&state)).unwrap()
    );
    p.candidates[0].risk.instrument.id =
        serde_json::from_value(json!("another.issuer.bond")).unwrap();
    assert!(
        h::solve_issuer_lifecycle_hedge_proposal(&p, &[state], &scope)
            .unwrap_err()
            .to_string()
            .contains("no fresh defaulted-issuer")
    );
}

#[test]
fn missing_mapping_wrong_versions_missing_native_state_and_rewritten_notice_fail_closed() {
    let (p, state, scope) = setup();
    assert!(h::solve_issuer_lifecycle_hedge_proposal(&p, &[], &scope).is_err());
    let mut wrong = scope.clone();
    wrong.directory.instruments.pop();
    wrong.directory.instruments.pop();
    assert!(
        h::solve_issuer_lifecycle_hedge_proposal(&p, std::slice::from_ref(&state), &wrong).is_err()
    );
    let mut wrong = scope.clone();
    wrong.directory.instruments[0].version = serde_json::from_value(json!(2)).unwrap();
    assert!(
        h::solve_issuer_lifecycle_hedge_proposal(&p, std::slice::from_ref(&state), &wrong).is_err()
    );
    let mut wrong = scope.clone();
    wrong.directory.issuers[0].default = None;
    assert!(h::solve_issuer_lifecycle_hedge_proposal(&p, &[state], &wrong).is_err());
    assert!(wrong.directory.preserves(&scope.directory).is_err());
    let mut wrong = scope.clone();
    wrong.directory.instruments[0].entity_id = serde_json::from_value(json!("OTHER")).unwrap();
    assert!(wrong.directory.preserves(&scope.directory).is_err());
    let mut duplicate = scope.directory.clone();
    duplicate.instruments.push(duplicate.instruments[0].clone());
    assert!(duplicate.validate().is_err());
}

#[test]
fn shared_library_action_and_schema_delegate_to_the_same_native_solver() {
    let (p, state, scope) = setup();
    let request = json!({"schema_version":1,"inputs":{"calendars":[],"helper_calendars":[],"curves":[],"calibration":null,"fixings":[],"publications":[]},
        "operation":{"kind":"hedge","problem":p,"action":{"kind":"solve_issuer_lifecycle","states":[state],"scope":scope}}});
    assert_eq!(
        calculate(request)["issuer_scope"],
        serde_json::to_value(scope).unwrap()
    );
    #[cfg(feature = "schemars")]
    assert!(
        serde_json::to_string(&schemars::schema_for!(LibraryRequest))
            .unwrap()
            .contains("solve_issuer_lifecycle")
    );
}
