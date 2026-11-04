//! Independently declared principal-only default cash, earned claims and risk.
use convex::constrained_hedge as h;
use convex::library::{execute, LibraryRequest};
use serde_json::{json, Value};

fn control() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/bond-events-contract.json"
    ))
    .unwrap()
}
fn event<'a>(v: &'a mut Value, kind: &str) -> &'a mut Value {
    v["operation"]["lifecycle"]["events"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["event"]["action"]["kind"] == kind)
        .unwrap()
}
fn request(name: &str, fraction: &str) -> Value {
    let mut v = control()["requests"][name].clone();
    event(&mut v, "recovery")["event"]["action"]["terms"]["recovery_fraction"] = json!(fraction);
    v
}
fn calculate(v: &Value) -> Value {
    execute(&serde_json::from_value::<LibraryRequest>(v.clone()).unwrap()).unwrap()["data"].clone()
}
fn reject(v: Value) {
    assert!(execute(&serde_json::from_value::<LibraryRequest>(v).unwrap()).is_err());
}
fn number(v: &Value) -> f64 {
    v.as_str().unwrap().parse().unwrap()
}

#[test]
fn original_and_amortizing_recovery_preserve_earned_cash_and_five_price_risk() {
    let fixture = control();
    let bound = number(&fixture["absolute_bound"]);
    for name in ["original", "amortizing"] {
        for fraction in ["0", "0.35", "1"] {
            for sign in [-1.0, 1.0] {
                let mut v = request(name, fraction);
                if sign == -1.0 {
                    v["operation"]["lifecycle"]["position"]["value"] = json!("1200");
                    for point in v["operation"]["history"]["points"].as_array_mut().unwrap() {
                        for position in point["positions"].as_array_mut().unwrap() {
                            if position["identity"]["id"] == "mixed.bond" {
                                position["current"]["quantity"] = json!("1200");
                            }
                        }
                    }
                }
                let actual = calculate(&v);
                let expected = &fixture["expected"][name][fraction];
                let mark = &actual["lifecycle_mark"];
                assert_eq!(mark["state"]["status"], "default_cash_pending");
                assert_eq!(mark["state"]["remaining_fraction"], "0");
                assert_eq!(actual["lifecycle"]["default_entity"], "BOND.ISSUER");
                for (kind, field) in [
                    ("recovery", "recovery_cash"),
                    ("bond_coupon", "earned_coupon"),
                    ("principal", "earned_principal"),
                ] {
                    let claims: Vec<_> = mark["state"]["obligations"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|o| o["key"]["kind"] == kind)
                        .collect();
                    if kind == "principal" && name == "original" {
                        assert!(claims.is_empty());
                    } else {
                        assert_eq!(claims.len(), 1);
                        assert_eq!(
                            number(&claims[0]["amount"]["value"]),
                            sign * number(&expected[field])
                        );
                    }
                }
                assert!(!mark["state"]["obligations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|o| o["key"]["kind"] == "default_accrual"));
                for key in [
                    "base_pv",
                    "plus_pv",
                    "minus_pv",
                    "half_plus_pv",
                    "half_minus_pv",
                    "value",
                    "half_value",
                ] {
                    let answer = sign * number(&expected["prices"][key]);
                    let value = actual["lifecycle"]["current"][0][key].as_f64().unwrap();
                    assert!(
                        (value - answer).abs() <= bound,
                        "{name}/{fraction}/{sign}/{key}: {value} vs {answer}"
                    );
                }
                for row in actual["lifecycle"]["pending"].as_array().unwrap() {
                    assert_eq!(row["base_pv"], 0.0);
                    assert_eq!(row["value"], 0.0);
                }
                for row in actual["lifecycle"]["current"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .skip(1)
                {
                    assert_eq!(
                        row["value"], 0.0,
                        "actual cash cannot retain forecast projection/hazard/recovery risk"
                    );
                }
            }
        }
    }
}

#[test]
fn unknown_recovery_has_no_actual_price_risk_or_mark_and_requires_original_owner() {
    let mut v = request("original", "0.35");
    v["operation"]["lifecycle"]["events"]
        .as_array_mut()
        .unwrap()
        .pop();
    let data = calculate(&v);
    assert_eq!(
        data["lifecycle_unpriced"]["reason"],
        "actual_bond_recovery_unknown"
    );
    assert_eq!(
        data["lifecycle_unpriced"]["state"]["status"],
        "default_pending"
    );
    assert!(data["lifecycle_unpriced"]["state"]["recovery"].is_null());
    assert!(data.get("lifecycle").is_none());
    assert!(data.get("lifecycle_mark").is_none());
    let mut bad = v.clone();
    bad["operation"]["lifecycle"]["instrument"]["credit"] = Value::Null;
    reject(bad);
    let mut bad = v.clone();
    bad["operation"]["lifecycle"]["position"]["value"] = json!("-1199");
    reject(bad);
    let mut bad = v.clone();
    let at = bad["operation"]["history"]["causes"]
        .as_array()
        .unwrap()
        .len();
    bad["operation"]["history"]["points"][at]["positions"][0]["pending_quantity"] = json!("1");
    reject(bad);
    let mut bad = v;
    bad["operation"]["product"]["bond"]["terms"]["face_value"] = json!("5001");
    reject(bad);
}

#[test]
fn legal_scope_boundary_order_and_actual_recovery_cannot_be_substituted() {
    let original = request("original", "0.35");
    for (field, value) in [
        ("entity", json!("OTHER")),
        ("seniority", json!("Subordinated")),
        ("document", json!({"id":"other","version":1})),
        ("accrual_day_fraction", json!("0.5")),
    ] {
        let mut v = original.clone();
        event(&mut v, "credit_default")["event"]["action"]["terms"][field] = value;
        reject(v);
    }
    for (field, value) in [
        ("default_event", json!("other")),
        ("recovery_fraction", json!("1.01")),
        ("cash_date", json!("2026-10-06")),
    ] {
        let mut v = original.clone();
        event(&mut v, "recovery")["event"]["action"]["terms"][field] = value;
        reject(v);
    }
    let mut v = original;
    v["operation"]["lifecycle"]["events"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    event(&mut v, "credit_default")["event"]["expected_revision"] = json!(0);
    reject(v);
    #[cfg(feature = "schemars")]
    assert!(
        serde_json::to_string(&schemars::schema_for!(LibraryRequest))
            .unwrap()
            .contains("default_entity")
    );
}

fn payment(id: &str, revision: u64, kind: &str, amount: &str) -> Value {
    json!({"event":{"event_id":id,"trade_id":"mixed.bond","expected_revision":revision,
        "effective_date":"2026-10-07","known_at":"2026-10-07T12:00:00Z",
        "action":{"kind":"payment","terms":{"receipt_id":format!("{id}.cash"),
            "obligation":{"kind":kind,"effective_date":"2026-10-07","ordinal":0},
            "amount":{"value":amount,"currency":"USD","unit":"currency"},
            "value_date":"2026-10-07","source":"synthetic.actual-cash"}}},
        "credit_rounding":null,"futures":null,"delivery":null})
}

#[test]
fn actual_receipts_reduce_owned_claims_without_inventing_a_close_or_overpayment() {
    let mut original = request("original", "0.35");
    event(&mut original, "recovery")["event"]["action"]["terms"]["cash_date"] = json!("2026-10-07");
    let mut partial = original.clone();
    partial["operation"]["lifecycle"]["events"]
        .as_array_mut()
        .unwrap()
        .push(payment("partial.1", 3, "recovery", "-100"));
    let data = calculate(&partial);
    assert_eq!(
        data["lifecycle_mark"]["state"]["status"],
        "default_cash_pending"
    );
    assert_eq!(data["lifecycle_mark"]["local"]["current"]["value"], -341.35);
    let mut complete = partial.clone();
    complete["operation"]["lifecycle"]["events"]
        .as_array_mut()
        .unwrap()
        .extend([
            payment("recovery.2", 4, "recovery", "-320"),
            payment("coupon.1", 5, "bond_coupon", "-21.35"),
        ]);
    let data = calculate(&complete);
    assert_eq!(data["lifecycle_mark"]["state"]["status"], "closed");
    assert_eq!(data["lifecycle_mark"]["local"]["current"]["value"], 0.0);
    assert!(data["lifecycle_mark"]["state"]["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .all(|o| o["settled"] == true));
    for amount in ["100", "-420.01"] {
        let mut bad = original.clone();
        bad["operation"]["lifecycle"]["events"]
            .as_array_mut()
            .unwrap()
            .push(payment("wrong.1", 3, "recovery", amount));
        reject(bad);
    }
    let mut early = request("original", "0.35");
    early["operation"]["lifecycle"]["events"]
        .as_array_mut()
        .unwrap()
        .push(payment("early.1", 3, "recovery", "-100"));
    reject(early);
    // A zero recovery still needs an actual completion record, not a guessed close.
    let mut zero = original;
    event(&mut zero, "recovery")["event"]["action"]["terms"]["recovery_fraction"] = json!("0");
    let data = calculate(&zero);
    assert_eq!(
        data["lifecycle_mark"]["state"]["status"],
        "default_cash_pending"
    );
    zero["operation"]["lifecycle"]["events"]
        .as_array_mut()
        .unwrap()
        .extend([
            payment("zero.1", 3, "recovery", "0"),
            payment("coupon.1", 4, "bond_coupon", "-21.35"),
        ]);
    assert_eq!(
        calculate(&zero)["lifecycle_mark"]["state"]["status"],
        "closed"
    );
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
        "candidates":[{"risk":vector,"eligibility":{"eligible":true,"decision":{"id":"synthetic.paper","version":1},"reason":"synthetic only"},
            "trade_lower":"-1000000","trade_upper":"1000000","holding_lower":"-10000000","holding_upper":"10000000",
            "buy_available":"1000000","sell_available":"1000000","lot":null,
            "costs":{"currency":"USD","buy_per_reference":"0","sell_per_reference":"0","observation":{"id":"synthetic.zero-cost","version":1}}}],
        "max_cost":0.0,"settings":{"policy_version":1,"rank_relative_tolerance":1e-10,"max_condition":1e4,"solver_tolerance":1e-10,
            "factor_tolerance":1e-8,"cost_tolerance":1e-10,"cost_scale":1.0,"cost_weight":0.0,"max_work":1000,"svd_iterations":256}
    })).unwrap()
}

#[test]
fn defaulted_security_is_ineligible_even_under_another_trade_identity() {
    let data = calculate(&request("original", "0.35"));
    let mut p = problem(&data);
    let mut state: h::LifecycleHoldingRisk =
        serde_json::from_value(data["lifecycle"].clone()).unwrap();
    state.current.truncate(1);
    state.pending.truncate(1);
    for id in ["mixed.bond", "another.holding"] {
        p.candidates[0].risk.id = serde_json::from_value(json!(id)).unwrap();
        let error = h::solve_lifecycle_hedge_proposal(&p, &[state.clone()])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("no fresh defaulted-entity candidates"),
            "{error}"
        );
    }
    p.candidates[0].risk.instrument.version = serde_json::from_value(json!(2)).unwrap();
    let error = h::solve_lifecycle_hedge_proposal(&p, &[state])
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("no fresh defaulted-entity candidates"),
        "{error}"
    );
}
