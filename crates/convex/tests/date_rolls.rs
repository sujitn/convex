//! Independent scheduled cash and maturity-risk controls. Original packs stay intact.
use convex::constrained_hedge as h;
use convex::library::{execute, execute_json, LibraryRequest};
use serde_json::{json, Value};

fn controls() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/date-rolls-requests.json"
    ))
    .unwrap()
}
fn reference() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/date-rolls-reference.json"
    ))
    .unwrap()
}
fn calculate(request: &Value) -> Value {
    let request: LibraryRequest = serde_json::from_value(request.clone()).unwrap();
    execute(&request).unwrap()["data"].clone()
}
fn rejected(request: &Value) {
    let result: Value = serde_json::from_str(&execute_json(&request.to_string())).unwrap();
    assert_eq!(result["ok"], "false", "{result}");
}

#[test]
fn scheduled_cash_is_not_received_and_partial_principal_is_applied_once() {
    let c = controls();
    let opening = calculate(&c["requests"]["opening"]);
    let coupon = calculate(&c["requests"]["coupon"]);
    assert_eq!(
        opening["lifecycle_mark"]["state"]["remaining_fraction"],
        "1"
    );
    assert_eq!(coupon["lifecycle_mark"]["state"]["remaining_fraction"], "1");
    assert_eq!(
        coupon["lifecycle"]["position"]["current"]["quantity"],
        "-1000"
    );
    assert_eq!(coupon["lifecycle_mark"]["recorded_cash"]["value"], "0");
    assert_eq!(
        coupon["lifecycle_mark"]["state"]["obligations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let mut request = c["requests"]["coupon"].clone();
    request["operation"] = json!({"kind":"lifecycle","input":request["operation"]["lifecycle"]});
    let replay = calculate(&request);
    let effects = replay["transitions"][0]["effects"].as_array().unwrap();
    let change = effects
        .iter()
        .find_map(|e| e.get("PositionChanged"))
        .unwrap();
    assert_eq!(change["before"], "-1000");
    assert_eq!(change["after"], "-600");
    assert_ne!(
        coupon["lifecycle"]["current"],
        opening["lifecycle"]["current"]
    );
    assert_ne!(
        coupon["lifecycle"]["pending"],
        opening["lifecycle"]["pending"]
    );
}

#[test]
fn maturity_retains_exact_outstanding_claims_and_independent_collateral_risk() {
    let c = controls();
    let expected = reference();
    let result = calculate(&c["requests"]["maturity"]);
    assert_eq!(result["valuation"], "Matured");
    assert_eq!(result["lifecycle_mark"]["state"]["status"], "matured");
    assert_eq!(result["lifecycle_mark"]["recorded_cash"]["value"], "0");
    assert_eq!(
        result["lifecycle"]["matured_instrument"],
        result["vector"]["instrument"]
    );
    let obligations = result["lifecycle_mark"]["state"]["obligations"]
        .as_array()
        .unwrap();
    for claim in expected["claims"].as_array().unwrap() {
        let obligation = obligations
            .iter()
            .find(|o| o["key"]["kind"] == claim["kind"] && o["key"]["ordinal"] == claim["ordinal"])
            .unwrap();
        assert_eq!(obligation["key"]["effective_date"], claim["effective"]);
        assert_eq!(obligation["due_date"], claim["due"]);
        assert_eq!(
            obligation["amount"]["value"]
                .as_str()
                .unwrap()
                .parse::<rust_decimal::Decimal>()
                .unwrap(),
            claim["amount"]
                .as_str()
                .unwrap()
                .parse::<rust_decimal::Decimal>()
                .unwrap()
        );
        assert_eq!(obligation["settled"], false);
    }
    let bound: f64 = expected["absolute_bound"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    for (field, value) in expected["expected_maturity_current"].as_object().unwrap() {
        let actual = result["lifecycle"]["current"][0][field].as_f64().unwrap();
        let value: f64 = value.as_str().unwrap().parse().unwrap();
        assert!(
            (actual - value).abs() <= bound,
            "{field}: {actual} != {value}"
        );
    }
    for row in result["lifecycle"]["pending"]
        .as_array()
        .unwrap()
        .iter()
        .chain(result["vector"]["sensitivities"].as_array().unwrap())
    {
        for key in [
            "base_pv",
            "plus_pv",
            "minus_pv",
            "half_plus_pv",
            "half_minus_pv",
            "value",
            "half_value",
        ] {
            assert_eq!(row[key].as_f64(), Some(0.0));
        }
    }
    let due = calculate(&c["requests"]["due"]);
    for row in due["lifecycle"]["current"].as_array().unwrap() {
        assert_eq!(row["base_pv"], json!(-1003.06));
        assert_eq!(row["value"], json!(0.0));
    }
}

#[test]
fn missing_boundaries_wrong_revisions_and_pending_maturity_fail() {
    let original = controls()["requests"]["maturity"].clone();
    for at in 0..5 {
        let mut bad = original.clone();
        match at {
            0 => {
                bad["operation"]["lifecycle"]["events"]
                    .as_array_mut()
                    .unwrap()
                    .remove(0);
            }
            1 => {
                bad["operation"]["lifecycle"]["events"][3]["event"]["expected_revision"] = json!(1);
            }
            2 => {
                let closing = bad["operation"]["history"]["causes"]
                    .as_array()
                    .unwrap()
                    .len();
                bad["operation"]["history"]["points"][closing]["positions"][0]
                    ["pending_quantity"] = json!("1");
            }
            3 => {
                bad["operation"]["max_state_evaluations"] = json!(24);
            }
            _ => {
                bad["operation"]["product"]["bond"]["terms"]["redemption_multiplier"] = json!("2");
            }
        }
        rejected(&bad);
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
        "holdings":[{"risk":vector,"current_quantity":pos["current"]["quantity"],"pending_quantity":"0"}],
        "factors":[{"key":vector["sensitivities"][0]["key"],"risk_settings":vector["sensitivities"][0]["settings"],
            "credit":null,"scale":1.0,"weight":1.0,"target":0.0,"lower":-1e12,"upper":1e12}],
        "candidates":[],"max_cost":0.0,
        "settings":{"policy_version":1,"rank_relative_tolerance":1e-10,"max_condition":1e4,"solver_tolerance":1e-10,
            "factor_tolerance":1e-8,"cost_tolerance":1e-10,"cost_scale":1.0,"cost_weight":0.0,"max_work":1000,"svd_iterations":256}
    })).unwrap()
}

#[test]
fn native_hedge_excludes_matured_security_across_candidate_aliases_and_revisions() {
    let result = calculate(&controls()["requests"]["maturity"]);
    let mut state: h::LifecycleHoldingRisk =
        serde_json::from_value(result["lifecycle"].clone()).unwrap();
    state.current.truncate(1);
    state.pending.truncate(1);
    let p = problem(&result);
    // A book with no active candidates retains the original solver's failure/status.
    // Never invent a feasible hedge for a zero-column problem.
    for alias in [false, true] {
        let mut bad = serde_json::to_value(&p).unwrap();
        let mut vector = bad["holdings"][0]["risk"].clone();
        if alias {
            vector["id"] = json!("another.trade");
            vector["instrument"]["version"] = json!(2);
        }
        bad["candidates"] = json!([{"risk":vector,"eligibility":{"eligible":true,"decision":{"id":"synthetic.paper","version":1},"reason":"synthetic"},
            "trade_lower":"-1000","trade_upper":"1000","holding_lower":"-10000","holding_upper":"10000",
            "buy_available":"1000","sell_available":"1000","lot":null,
            "costs":{"currency":"USD","buy_per_reference":"0","sell_per_reference":"0","observation":{"id":"zero.cost","version":1}}}]);
        let bad = serde_json::from_value::<h::HedgeProblem>(bad).unwrap();
        let error = h::solve_lifecycle_hedge_proposal(&bad, &[state.clone()]).unwrap_err();
        assert!(error.to_string().contains("maturity risk"), "{error}");
    }
}

fn matured_coupon_request(kind: &str) -> Value {
    let core: Value = serde_json::from_slice(
        &std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/library/date-rolls-core.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let data = &core["definition"]["calculations"]["pnl"]["request"]["operation"]["input"];
    let id = if kind == "swap" {
        "stepped-amortizing-IRS"
    } else {
        "graph-hedge.CDS"
    };
    let close = &data["steps"].as_array().unwrap().last().unwrap()["after"];
    let holding = close["holdings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|index| &data["holdings"][index.as_u64().unwrap() as usize])
        .find(|h| h["id"] == id)
        .unwrap();
    let mut product = holding["product"]["product"].clone();
    let terms = &mut product["terms"];
    terms["version"] = json!(1);
    let mut history = json!([]);
    if kind == "swap" {
        terms["effective_date"] = json!("2026-09-08");
        terms["maturity_date"] = json!("2026-10-09");
        for side in ["fixed", "floating"] {
            let leg = &mut terms[side]["leg"];
            leg["schedule"] = json!({"start_date":"2026-09-08","end_date":"2026-10-09","frequency":"Annual","stub_type":"ShortLast","end_of_month":false,"first_regular_date":null,"penultimate_date":null});
            leg["notionals"] = json!([{"start":"2026-09-08","end":"2026-10-09","value":"1000"}]);
            leg["rounding"] = json!({"NearestEven":2});
            leg["calendars"] = json!({"accrual":{"id":"WEEKEND","version":1},"payment":{"id":"WEEKEND","version":1}});
            leg["dates"]["payment_delay"] = json!({"CalendarDays":1});
        }
        terms["fixed"]["rates"] =
            json!([{"start":"2026-09-08","end":"2026-10-09","value":"0.036"}]);
        terms["floating"]["profiles"] = json!({"gearings":[{"start":"2026-09-08","end":"2026-10-09","value":"1"}],"spreads":[{"start":"2026-09-08","end":"2026-10-09","value":"0"}]});
        terms["floating"]["resets"]["schedule"] = terms["floating"]["leg"]["schedule"].clone();
        terms["floating"]["resets"]["fixing_offset"] = json!({"CalendarDays":0});
        history = json!([{"index_id":"USD.TERM3M","observation_date":"2026-09-08","source_id":"engineering-CVX057","rate":"0.04","revision":1,"published_at":"2026-09-09T08:00:00Z","known_at":"2026-09-09T08:00:00Z","status":"Final"}]);
    } else {
        terms["notional"] = json!("1000");
        terms["trade_date"] = json!("2026-09-07");
        terms["protection_start_date"] = json!("2026-09-08");
        terms["step_in_date"] = json!("2026-09-08");
        terms["cash_settlement_date"] = json!("2026-09-10");
        terms["initial_clean_upfront_fraction"] = Value::Null;
        terms["maturity_rule"] = json!("ExplicitDate");
        terms["premium_schedule"]["start_date"] = json!("2026-09-08");
        terms["premium_schedule"]["end_date"] = json!("2026-10-09");
        terms["premium_schedule"]["stub_type"] = json!("ShortLast");
        terms["premium_calendar"] = json!({"id":"WEEKEND","version":1});
        product["context"]["initial_payment_state"] = json!("NotContracted");
    }
    let mut request = controls()["requests"]["maturity"].clone();
    let operation = &mut request["operation"];
    operation["product"] = product.clone();
    let points = operation["history"]["points"].as_array_mut().unwrap();
    for point in points.iter_mut() {
        let mut position = point["positions"][0].clone();
        position["identity"]["id"] = json!(id);
        position["identity"]["instrument"] = json!({"id":id,"version":1});
        position["current"]["quantity"] = json!("1000");
        position["current"]["reference_quantity"] = json!("1000");
        position["pending_quantity"] = json!("0");
        point["positions"] = json!([position]);
    }
    let closing = operation["history"]["causes"].as_array().unwrap().len();
    operation["input"]["identity"] =
        operation["history"]["points"][closing]["positions"][0]["identity"].clone();
    let instrument = json!({"kind":kind,"terms":product["terms"]});
    operation["lifecycle"] = json!({"trade_id":id,"version":1,"instrument":instrument,
        "position":{"unit":if kind=="swap" {"trade_copies"} else {"cds_notional"},"value":if kind=="swap" {"1"} else {"1000"}},
        "known_at":"2026-10-07T12:00:00Z","history":history,
        "events":[{"event":{"event_id":"maturity.actual.coupon","trade_id":id,"expected_revision":0,
            "effective_date":"2026-10-09","known_at":"2026-10-09T12:00:00Z","action":{"kind":"contractual_cash"}},
            "credit_rounding":if kind=="cds" {json!({"NearestEven":2})} else {Value::Null},"futures":null,"delivery":null}]});
    request["inputs"]["fixings"]
        .as_array_mut()
        .unwrap()
        .extend(history.as_array().unwrap().iter().cloned());
    request
}

#[test]
fn swap_and_cds_maturity_use_native_coupons_and_current_only_claims() {
    for kind in ["swap", "cds"] {
        let result = calculate(&matured_coupon_request(kind));
        assert_eq!(result["valuation"], "Matured");
        assert_eq!(result["lifecycle_mark"]["state"]["remaining_fraction"], "0");
        assert_eq!(result["lifecycle_mark"]["recorded_cash"]["value"], "0");
        let obligations = result["lifecycle_mark"]["state"]["obligations"]
            .as_array()
            .unwrap();
        let expected = reference()["expected_native_coupon_cash"].clone();
        let expected: Vec<_> = if kind == "swap" {
            vec![
                expected["swap_fixed"].as_str().unwrap(),
                expected["swap_floating"].as_str().unwrap(),
            ]
        } else {
            vec![expected["cds_final"].as_str().unwrap()]
        };
        assert_eq!(
            obligations
                .iter()
                .map(|o| o["amount"]["value"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        for row in result["lifecycle"]["pending"].as_array().unwrap() {
            assert_eq!(row["base_pv"], 0.0);
            assert_eq!(row["value"], 0.0);
        }
    }
}

#[test]
fn actual_default_claim_survives_contractual_maturity_without_a_fresh_cds_price() {
    let mut request = matured_coupon_request("cds");
    let terms = &request["operation"]["product"]["terms"];
    let legal = terms["reference"].clone();
    let id = terms["trade_id"].clone();
    request["operation"]["lifecycle"]["events"] = json!([
        {"event":{"event_id":"expired.default","trade_id":id,"expected_revision":0,"effective_date":"2026-10-08","known_at":"2026-10-08T12:00:00Z",
          "action":{"kind":"credit_default","terms":{"entity":legal["entity_id"],"seniority":legal["seniority"],"document":legal["document"],"accrual_day_fraction":"0.5","rounding":{"NearestEven":2}}}},"credit_rounding":null,"futures":null,"delivery":null},
        {"event":{"event_id":"expired.recovery","trade_id":id,"expected_revision":1,"effective_date":"2026-10-09","known_at":"2026-10-09T12:00:00Z",
          "action":{"kind":"recovery","terms":{"default_event":"expired.default","recovery_fraction":"0.4","reference":{"id":"actual.auction","version":1},"cash_date":"2026-10-13"}}},"credit_rounding":null,"futures":null,"delivery":null}]);
    let result = calculate(&request);
    assert_eq!(result["valuation"], "Matured");
    assert_eq!(result["lifecycle"]["default_entity"], "ACME");
    assert_eq!(
        result["lifecycle"]["matured_instrument"],
        result["vector"]["instrument"]
    );
    assert!(
        result["lifecycle"]["current"][0]["base_pv"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    assert_ne!(result["lifecycle"]["current"][0]["value"], 0.0);
    assert_eq!(result["lifecycle_mark"]["recorded_cash"]["value"], "0");
    request["operation"]["lifecycle"]["events"]
        .as_array_mut()
        .unwrap()
        .pop();
    let pending = calculate(&request);
    assert!(pending.get("lifecycle").is_none());
    assert!(pending.get("lifecycle_unpriced").is_some());
}
