//! Complete independent full-member financial answers; native envelopes prove
//! transport only. Financial tolerances were declared before native execution.
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use serde_json::{json, Value};

pub fn cases() -> Vec<Value> {
    [
        packed::text("tests/fixtures/library/delivery-hedge-decimal.json"),
        include_str!("delivery-hedge-rebuilt.json"),
        packed::text("tests/fixtures/library/delivery-hedge-frontier.json"),
    ]
    .iter()
    .flat_map(|raw| {
        serde_json::from_str::<Value>(raw).unwrap()["cases"]
            .as_array()
            .unwrap()
            .clone()
    })
    .collect()
}
pub fn number(value: &Value) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| value.as_str().unwrap().parse().unwrap())
}
pub fn close(actual: &Value, expected: &Value, bound: f64) {
    let a = number(actual);
    let b = number(expected);
    assert!(
        a.is_finite() && b.is_finite() && (a - b).abs() <= bound,
        "actual={a:.12} expected={b:.12} bound={bound}"
    );
}
fn projection(actual: &Value, expected: &Value) {
    let coupons = actual["coupons"].as_array().unwrap();
    let original = expected["coupons"].as_array().unwrap();
    assert_eq!(coupons.len(), original.len());
    for (a, b) in coupons.iter().zip(original) {
        assert_eq!(a["accrual_start"], b["start"]);
        assert_eq!(a["accrual_end"], b["end"]);
        assert_eq!(a["payment_date"], b["payment"]);
        assert_eq!(a["entitled"], true);
        close(&a["amount"], &b["currency_amount"], 1e-10);
        assert!(a["rates"].as_array().unwrap().is_empty());
        assert!(a["overnight"].is_null());
    }
    assert_eq!(actual["principal"].as_array().unwrap().len(), 1);
    let cash = expected["cashflows"].as_array().unwrap().last().unwrap();
    assert_eq!(actual["principal"][0]["payment_date"], cash["date"]);
    close(&actual["principal"][0]["amount"], &cash["amount"], 1e-10);
    assert_eq!(actual["conditional_on_no_exercise"], false);
}
pub fn evaluation(actual: &Value, expected: &Value, product: &Value) {
    let a = &actual["valuation"];
    let future = &product["input"];
    assert_eq!(
        a["contract"],
        json!({"id":future["contract"]["instrument_id"],"version":future["contract"]["stamp"]["version"]})
    );
    assert_eq!(
        a["basket"],
        json!({"id":future["basket"]["stamp"]["id"],"version":future["basket"]["stamp"]["version"]})
    );
    assert_eq!(a["market"], future["market"]);
    assert_eq!(a["selection"], future["selection"]);
    assert_eq!(a["settlement"], future["settlement"]);
    assert_eq!(a["delivery"], future["delivery"]);
    assert_eq!(a["selected"], expected["selected"]);
    assert_eq!(a["fair_deliverable"], expected["fair_deliverable"]);
    close(
        &a["fair_price_points"],
        &expected["fair_price_points"],
        1e-8,
    );
    if !expected["selection_gap"].is_null() {
        close(&a["selection_gap"], &expected["selection_gap"], 1e-8);
    }
    assert_eq!(actual["ctd"]["selection_ties"], expected["selection_ties"]);
    assert_eq!(actual["ctd"]["fair_ties"], expected["fair_ties"]);
    assert_eq!(a["model_limitations"].as_array().unwrap().len(), 2);
    assert_eq!(a["inputs"]["context"], future["context"]);
    assert_eq!(a["inputs"]["execution_status"], "indicative_only");
    assert_eq!(a["inputs"]["assessment"]["eligible"], false);
    let alternatives = a["alternatives"].as_array().unwrap();
    let prices = actual["members"].as_array().unwrap();
    assert_eq!(
        alternatives.len(),
        future["members"].as_array().unwrap().len()
    );
    assert_eq!(prices.len(), alternatives.len());
    for (alternative, reference) in alternatives
        .iter()
        .zip(expected["alternatives"].as_array().unwrap())
    {
        assert_eq!(alternative["instrument"], reference["instrument"]);
        let original = future["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["instrument"] == alternative["instrument"])
            .unwrap();
        let mut financing = original["financing"].clone();
        financing["repo_decimal"] = alternative["financing"]["repo_decimal"].clone();
        assert_eq!(alternative["financing"], financing);
        close(
            &alternative["financing"]["repo_decimal"],
            &reference["repo_rate"],
            1e-12,
        );
        assert_eq!(alternative["own_cash"], original["own_cash"]);
        assert_eq!(alternative["coupon_carry"], original["coupon_carry"]);
        for field in [
            "conversion_factor",
            "spot_clean_per100",
            "spot_dirty_per100",
            "settlement_accrued_per100",
            "delivery_accrued_per100",
            "coupons_per100",
            "coupon_delivery_value_per100",
            "repo_cost_per100",
            "own_cash_cost_per100",
            "forward_price_points",
            "gross_basis_per100",
            "net_basis_per100",
            "implied_repo_decimal",
        ] {
            close(
                &alternative[field],
                &reference[field],
                if field == "implied_repo_decimal" {
                    1e-12
                } else {
                    1e-8
                },
            );
        }
        close(
            &alternative["delivery_invoice_per_contract"]["value"],
            &reference["delivery_invoice_per_contract"],
            0.0,
        );
        close(
            &alternative["unrounded_basis_profit_per_contract"]["value"],
            &reference["unrounded_basis_profit_per_contract"],
            1e-5,
        );
        let model = prices
            .iter()
            .find(|m| m["instrument"] == alternative["instrument"])
            .unwrap();
        let financial = expected["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["instrument"] == alternative["instrument"])
            .unwrap();
        assert_eq!(
            model["observed_clean_per100"],
            original["clean_price_per100"]
        );
        for field in ["frozen_residual_basis_per100", "scenario_clean_per100"] {
            close(&model[field], &financial[field], 1e-8);
        }
        for (native, independent) in [
            ("baseline_model", "baseline"),
            ("scenario_model", "scenario"),
        ] {
            let m = &model[native];
            let r = &financial[independent];
            for (x, y) in [
                ("present_value", "pv"),
                ("clean_price", "clean"),
                ("dirty_price", "dirty"),
                ("accrued", "accrued"),
            ] {
                close(&m[x], &r[y], if x == "present_value" { 1e-5 } else { 1e-8 });
            }
            assert_eq!(m["discount_curve"], product["discount"]["curve"]);
            projection(&m["projection"], r);
        }
    }
    let stamps = a["inputs"]["stamps"].as_array().unwrap();
    assert!(stamps.contains(&product["discount"]["stamp"]));
    for m in future["members"].as_array().unwrap() {
        for stamp in [&m["stamp"], &m["quote_stamp"], &m["financing"]["stamp"]] {
            assert!(stamps.contains(stamp));
        }
    }
}
pub fn risk(actual: &Value, expected: &Value, action: &Value) {
    let sensitivity = &actual["sensitivity"];
    for field in [
        "base_pv",
        "plus_pv",
        "minus_pv",
        "half_plus_pv",
        "half_minus_pv",
        "value",
        "half_value",
    ] {
        close(&sensitivity[field], &expected[field], 1e-5);
    }
    assert_eq!(sensitivity["key"], action["key"]);
    assert_eq!(sensitivity["position"], action["position"]);
    assert_eq!(sensitivity["settings"], action["settings"]);
    assert_eq!(sensitivity["snapshot_id"], action["snapshot_id"]);
    assert_eq!(sensitivity["method"], "central");
    for field in [
        "frozen_deliverable_sensitivity",
        "frozen_deliverable_half_sensitivity",
        "one_sided_plus",
        "one_sided_minus",
    ] {
        close(&actual[field], &expected[field], 1e-5);
    }
    for field in [
        "selection_switch",
        "fair_deliverable_switch",
        "ties_present",
        "ctd_stable",
    ] {
        assert_eq!(actual[field], expected[field]);
    }
    let values = actual["evaluations"].as_array().unwrap();
    assert_eq!(values.len(), 5);
    for (a, b) in values
        .iter()
        .zip(expected["evaluations"].as_array().unwrap())
    {
        evaluation(a, b, &action["product"]);
    }
    let members = action["product"]["input"]["members"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(actual["reserved_member_evaluations"], members * 12);
}
fn column(actual: &Value, expected: &Value, request: &Value) {
    let op = &request["operation"];
    let input = &op["input"];
    let vector = &actual["vector"];
    for field in ["id", "instrument", "economics", "calculation", "entity"] {
        assert_eq!(vector[field], input["identity"][field]);
    }
    assert_eq!(vector["reference_position"], input["reference_position"]);
    assert_eq!(vector["settlement"], "2026-10-07");
    assert_eq!(vector["credit_scope"], json!([]));
    assert!(vector["fx"].is_null());
    let factors = input["factors"].as_array().unwrap();
    let sensitivities = vector["sensitivities"].as_array().unwrap();
    assert_eq!(factors.len(), sensitivities.len());
    assert_eq!(actual["reserved_factor_evaluations"], factors.len() * 5);
    for (i, (s, reference)) in sensitivities
        .iter()
        .zip(expected.as_array().unwrap())
        .enumerate()
    {
        let action = json!({"product":op["product"],"position":input["reference_position"],"key":factors[i]["key"],"settings":factors[i]["settings"],"snapshot_id":input["snapshot_id"]});
        for field in ["key", "position", "settings", "snapshot_id"] {
            assert_eq!(s[field], action[field]);
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
            close(&s[field], &reference[field], 1e-5);
        }
        if op["product"]["kind"] == "bond_future" {
            risk(&actual["factors"][i], reference, &action);
        }
    }
    if op["product"]["kind"] == "bond_future" {
        assert_eq!(actual["basis"], "FuturesQuoteLevel");
        assert_eq!(
            actual["reserved_member_evaluations"],
            factors.len() * 3 * 12
        );
        assert_eq!(
            actual["valuation"],
            actual["factors"][0]["evaluations"][0]["valuation"]
        );
    } else {
        assert_eq!(actual["basis"], "CurrencyPresentValue");
    }
}
pub fn book_cases(columns: &[Value]) -> Vec<Value> {
    let controls: Value = serde_json::from_str(packed::text(
        "tests/fixtures/library/delivery-hedge-decimal.json",
    ))
    .unwrap();
    let mut cases = Vec::new();
    for book in controls["books"].as_array().unwrap() {
        let mut problem = book["problem"].clone();
        for group in ["holdings", "candidates"] {
            for row in problem[group].as_array_mut().unwrap() {
                let column = &columns.iter().find(|v| v["id"] == row["column"]).unwrap()
                    ["expected_envelope"]["result"]["data"];
                row["risk"] = column["vector"].clone();
                row["risk"]["fx"] = book["fx"].clone();
                row.as_object_mut().unwrap().remove("column");
            }
        }
        for kind in ["aggregate", "solve"] {
            cases.push(json!({"id":format!("{}.{}",book["id"].as_str().unwrap(),kind),"expected_error":null,
                "expected":book["expected"],"request":{"schema_version":1,"inputs":{"calendars":[],"curves":[],"calibration":null,"helper_calendars":[],"fixings":[],"publications":[]},
                    "operation":{"kind":"hedge","problem":problem,"action":{"kind":kind}}}}));
        }
        let original_case = cases.last().unwrap().clone();
        if book["id"] == "delivery.book.USD" {
            for negative in controls["book_protocol"].as_array().unwrap().iter().take(9) {
                let mut case = original_case.clone();
                let name = negative["name"].as_str().unwrap();
                case["id"] = json!(format!("delivery.book.invalid.{name}"));
                case["expected_error"] = negative["error"].clone();
                case["expected_status"] = negative["status"].clone();
                let p = &mut case["request"]["operation"]["problem"];
                match name {
                    "missing_holding" => {
                        p["holdings"].as_array_mut().unwrap().pop();
                    }
                    "duplicate_factor" => p["factors"][1] = p["factors"][0].clone(),
                    "factor_revision" => {
                        p["factors"][0]["key"]["coordinate"]["curve"]["version"] = json!("8")
                    }
                    "wrong_reference" => {
                        p["holdings"][0]["risk"]["reference_position"]["reference_quantity"] =
                            json!("700")
                    }
                    "wrong_pending_snapshot" => {
                        p["holdings"][0]["risk"]["sensitivities"][0]["snapshot_id"] =
                            json!("S.OTHER")
                    }
                    "missing_lot" => p["candidates"][1]["lot"] = Value::Null,
                    "ineligible" => p["candidates"][1]["eligibility"]["eligible"] = json!(false),
                    "liquidity" => p["candidates"][1]["sell_available"] = json!("0"),
                    "cost_budget" => p["max_cost"] = json!(0.0),
                    _ => panic!("undeclared protocol"),
                }
                cases.push(case);
            }
        } else {
            for name in ["missing_fx", "stale_fx"] {
                let mut case = original_case.clone();
                case["id"] = json!(format!("delivery.book.invalid.{name}"));
                case["expected_error"] = json!(if name == "missing_fx" {
                    "missing_input"
                } else {
                    "invalid_input"
                });
                let p = &mut case["request"]["operation"]["problem"];
                for group in ["holdings", "candidates"] {
                    for row in p[group].as_array_mut().unwrap() {
                        if name == "missing_fx" {
                            row["risk"]["fx"] = Value::Null;
                        } else {
                            row["risk"]["fx"]["snapshot_id"] = json!("S.OTHER");
                        }
                    }
                }
                cases.push(case);
            }
        }
    }
    cases
}
fn check_book(case: &Value, result: &Value) {
    let solve = case["request"]["operation"]["action"]["kind"] == "solve";
    let exposure = if solve { &result["exposure"] } else { result };
    let expected = &case["expected"];
    assert_eq!(exposure["problem"], case["request"]["operation"]["problem"]);
    for field in ["current", "pending", "before"] {
        for (a, b) in exposure[field]
            .as_array()
            .unwrap()
            .iter()
            .zip(expected[field].as_array().unwrap())
        {
            close(a, b, 1e-5);
        }
    }
    if solve {
        assert_eq!(result["authority"], "proposal_only");
        if !case["expected_status"].is_null() {
            assert_eq!(result["status"], case["expected_status"]);
            assert!(result["proposal"].is_null());
            return;
        }
        assert_eq!(result["status"], "feasible", "{result}");
        assert_eq!(result["rank"]["rank"], expected["rank"]);
        let proposal = &result["proposal"];
        for (a, b) in proposal["quantities"]
            .as_array()
            .unwrap()
            .iter()
            .zip(expected["quantities"].as_array().unwrap())
        {
            close(a, b, 1e-8);
        }
        for (a, b) in proposal["final_holdings"]
            .as_array()
            .unwrap()
            .iter()
            .zip(expected["holdings"].as_array().unwrap())
        {
            close(a, b, 1e-8);
        }
        for (a, b) in proposal["after"]
            .as_array()
            .unwrap()
            .iter()
            .zip(expected["after"].as_array().unwrap())
        {
            close(a, b, 1e-8);
        }
        close(&proposal["total_cost"], &expected["cost"], 1e-8);
        assert!(proposal["constraints"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["satisfied"] == true));
    }
}
pub fn run(mut execute: impl FnMut(&str) -> String) -> Vec<Value> {
    let mut outputs = Vec::new();
    for case in cases() {
        let request = &case["request"];
        let envelope: Value = serde_json::from_str(&execute(&request.to_string())).unwrap();
        if !case["expected_error"].is_null() {
            assert_eq!(envelope["ok"], "false", "{} {envelope}", case["id"]);
            assert_eq!(
                envelope["error"]["code"], case["expected_error"],
                "{} {envelope}",
                case["id"]
            );
        } else {
            assert_eq!(envelope["ok"], "true", "{} {envelope}", case["id"]);
            let result = &envelope["result"]["data"];
            let action = &request["operation"]["action"];
            if action["kind"] == "product_scenario" {
                evaluation(
                    &result["baseline"],
                    &case["expected"]["baseline"],
                    &action["product"],
                );
                evaluation(
                    &result["shocked"],
                    &case["expected"]["shocked"],
                    &action["product"],
                );
                let multiplier =
                    number(&action["product"]["input"]["contract"]["currency_per_price_point"])
                        * number(&action["position"]["quantity"]);
                let base = number(&case["expected"]["baseline"]["fair_price_points"]) * multiplier;
                let shock = number(&case["expected"]["shocked"]["fair_price_points"]) * multiplier;
                close(&result["baseline_amount"]["value"], &json!(base), 1e-5);
                close(&result["scenario_amount"]["value"], &json!(shock), 1e-5);
                close(&result["change"]["value"], &json!(shock - base), 1e-5);
            } else {
                risk(result, &case["expected"], action);
            }
        }
        outputs.push(json!({"id":case["id"],"request":request,"expected_error":case["expected_error"],"expected_envelope":envelope}));
    }
    let books: Value = serde_json::from_str(packed::text(
        "tests/fixtures/library/delivery-hedge-decimal.json",
    ))
    .unwrap();
    for c in books["books"][0]["columns"].as_array().unwrap() {
        let envelope: Value = serde_json::from_str(&execute(&c["request"].to_string())).unwrap();
        assert_eq!(envelope["ok"], "true", "{} {envelope}", c["id"]);
        column(&envelope["result"]["data"], &c["expected"], &c["request"]);
        outputs.push(json!({"id":c["id"],"request":c["request"],"expected_error":null,"expected_envelope":envelope}));
    }
    for case in book_cases(&outputs) {
        let envelope: Value = serde_json::from_str(&execute(&case["request"].to_string())).unwrap();
        if !case["expected_error"].is_null() {
            assert_eq!(envelope["ok"], "false", "{} {envelope}", case["id"]);
            assert_eq!(
                envelope["error"]["code"], case["expected_error"],
                "{} {envelope}",
                case["id"]
            );
        } else {
            assert_eq!(envelope["ok"], "true", "{} {envelope}", case["id"]);
            check_book(&case, &envelope["result"]["data"]);
        }
        outputs.push(json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":envelope}));
    }
    outputs
}
