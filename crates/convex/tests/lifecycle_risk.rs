//! Full native lifecycle risk and hedge admission. The independent reference is
//! declared by Decimal80, before comparison, in lifecycle-risk-contract.json.
use convex::constrained_hedge as h;
use convex::library::{execute, execute_json, LibraryRequest};
use serde_json::{json, Value};

fn control() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/lifecycle-risk-contract.json"
    ))
    .unwrap()
}
fn request(cash: &str) -> Value {
    let mut v = control()["request"].clone();
    let terms = v["operation"]["lifecycle"]["events"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    terms["event"]["action"]["terms"]["cash"]["value"] = json!(cash);
    terms["event"]["action"]["terms"]["agreement"]["id"] =
        json!(format!("independent.cash.{cash}"));
    v
}
fn calculate(v: &Value) -> Value {
    execute(&serde_json::from_value::<LibraryRequest>(v.clone()).unwrap()).unwrap()["data"].clone()
}
fn rejected(v: &Value) -> Value {
    serde_json::from_str(&execute_json(&v.to_string())).unwrap()
}

#[test]
fn independent_discounted_claim_and_five_price_risk() {
    let reference = control()["reference"].clone();
    let zero = calculate(&request("0"));
    let cash = calculate(&request("1000"));
    let a = &zero["lifecycle"]["current"][0];
    let b = &cash["lifecycle"]["current"][0];
    let bound: f64 = reference["absolute_bound"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    for name in [
        "base_pv",
        "plus_pv",
        "minus_pv",
        "half_plus_pv",
        "half_minus_pv",
        "value",
        "half_value",
    ] {
        let expected: f64 = reference["expected_increment"][name]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let actual = b[name].as_f64().unwrap() - a[name].as_f64().unwrap();
        assert!(
            (actual - expected).abs() <= bound,
            "{name}: actual={actual}, expected={expected}, bound={bound}"
        );
    }
    assert_eq!(cash["lifecycle_mark"]["state"]["remaining_fraction"], "0.5");
    assert_eq!(
        cash["lifecycle"]["pending"], zero["lifecycle"]["pending"],
        "owned cash must not enter pending orders"
    );
    assert_eq!(
        cash["vector"], zero["vector"],
        "owned cash must not enter fresh candidate risk"
    );
    assert_eq!(cash["reserved_state_evaluations"], 25);
    assert_eq!(cash["state_evaluations"], 21);
}

#[test]
fn complete_native_history_replay_and_bounds_reject() {
    let original = request("1000");
    let mut cases = Vec::new();
    let mut v = original.clone();
    v["operation"]["max_state_evaluations"] = json!(24);
    cases.push(v);
    let mut v = original.clone();
    v["operation"]["input"]["identity"]["id"] = json!("wrong");
    cases.push(v);
    let mut v = original.clone();
    v["operation"]["input"]["reference_position"]["reference_quantity"] = json!("999");
    cases.push(v);
    let mut v = original.clone();
    v["operation"]["lifecycle"]["events"][1]["event"]["expected_revision"] = json!(0);
    cases.push(v);
    let mut v = original.clone();
    v["operation"]["lifecycle"]["events"][1]["event"]["known_at"] = json!("2026-10-08T00:00:00Z");
    cases.push(v);
    let mut v = original.clone();
    v["operation"]["lifecycle"]["events"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    cases.push(v);
    let mut v = original.clone();
    v["operation"]["history"]["points"][2]["positions"][2]["current"]["quantity"] =
        json!("-199999");
    cases.push(v);
    let mut v = original.clone();
    v["operation"]["lifecycle"]["events"][1]["event"]["effective_date"] = json!("2026-10-08");
    cases.push(v);
    for v in cases {
        assert_eq!(
            rejected(&v)["ok"],
            "false",
            "accepted invalid lifecycle input"
        );
    }
}

fn problem(data: &Value) -> h::HedgeProblem {
    let mut vector = data["vector"].clone();
    vector["sensitivities"].as_array_mut().unwrap().truncate(1);
    let state = &data["lifecycle"];
    let pos = &state["position"];
    serde_json::from_value(json!({
        "snapshot_id":state["snapshot_id"], "entity":pos["identity"]["entity"],
        "settlement":state["settlement"],"reporting_currency":"USD", "position_state":state["manifest"],
        "expected_holdings":[vector["id"]],
        "holdings":[{"risk":vector,"current_quantity":pos["current"]["quantity"],"pending_quantity":pos["pending_quantity"]}],
        "factors":[{"key":data["vector"]["sensitivities"][0]["key"], "risk_settings":data["vector"]["sensitivities"][0]["settings"],
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
fn hedge_uses_actual_state_once_and_fresh_candidates() {
    let data = calculate(&request("1000"));
    let p = problem(&data);
    let mut state: h::LifecycleHoldingRisk =
        serde_json::from_value(data["lifecycle"].clone()).unwrap();
    state.current.truncate(1);
    state.pending.truncate(1);
    let report = h::solve_lifecycle_hedge_proposal(&p, &[state.clone()]).unwrap();
    assert_eq!(report.status, h::HedgeSolveStatus::Feasible);
    assert_eq!(report.exposure.current[0], state.current[0].value);
    assert_eq!(report.exposure.pending[0], state.pending[0].value);
    assert_eq!(report.exposure.problem.candidates[0], p.candidates[0]);
    assert!(report
        .proposal
        .unwrap()
        .constraints
        .iter()
        .all(|c| c.satisfied));
    for at in 0..7 {
        let mut bad = state.clone();
        match at {
            0 => bad.snapshot_id = convex::contracts::SnapshotId::new("wrong.snapshot").unwrap(),
            1 => bad.manifest.version = convex::contracts::Version::new(2).unwrap(),
            2 => {
                bad.position.pending_quantity = convex::contracts::ExactDecimal::parse("1").unwrap()
            }
            3 => bad.current.clear(),
            4 => {
                bad.current[0].position.quantity =
                    convex::contracts::ExactDecimal::parse("2").unwrap()
            }
            5 => bad.pending[0].settings.bump *= 2.0,
            _ => {
                bad.position.identity.instrument.version =
                    convex::contracts::Version::new(2).unwrap()
            }
        }
        assert!(h::solve_lifecycle_hedge_proposal(&p, &[bad]).is_err());
    }
    assert!(h::solve_lifecycle_hedge_proposal(&p, &[state.clone(), state]).is_err());
}

#[test]
fn strict_shared_wire_roundtrip_and_schema() {
    let v = request("1000");
    let typed: LibraryRequest = serde_json::from_value(v.clone()).unwrap();
    let canonical = serde_json::to_value(typed).unwrap();
    fn differences(a: &Value, b: &Value, at: &str, found: &mut Vec<String>) {
        if a == b || found.len() == 12 {
            return;
        }
        match (a, b) {
            // JSON integer/float spellings of a native f64 have the same value.
            // Exact financial decimals remain strings and must match byte for byte.
            (Value::Number(a), Value::Number(b)) if a.as_f64() == b.as_f64() => {}
            (Value::Object(a), Value::Object(b)) => {
                for (key, value) in a {
                    differences(
                        value,
                        b.get(key).unwrap_or(&Value::Null),
                        &format!("{at}.{key}"),
                        found,
                    );
                }
                for key in b.keys().filter(|key| !a.contains_key(*key)) {
                    found.push(format!("{at}.{key}: new field"));
                }
            }
            (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
                for (i, (a, b)) in a.iter().zip(b).enumerate() {
                    differences(a, b, &format!("{at}[{i}]"), found);
                }
            }
            _ => found.push(format!("{at}: {a} != {b}")),
        }
    }
    let mut changed = Vec::new();
    differences(&v, &canonical, "request", &mut changed);
    assert!(changed.is_empty(), "wire differences: {changed:?}");
    let mut extra = v;
    extra["operation"]["unrecognised"] = json!(true);
    assert!(serde_json::from_value::<LibraryRequest>(extra).is_err());
    #[cfg(feature = "schemars")]
    assert!(
        serde_json::to_string(&schemars::schema_for!(LibraryRequest))
            .unwrap()
            .contains("lifecycle_hedge_risk")
    );
}
