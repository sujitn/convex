//! Full independent nonlinear answers; complete envelopes prove transport only.
use serde_json::{json, Value};
#[allow(dead_code)]
#[path = "delivery_hedge_contract.rs"]
mod delivery;
pub use delivery::{close, number};

pub fn cases() -> Vec<Value> {
    let mut cases = serde_json::from_str::<Value>(include_str!("mixed-scenario-publications.json"))
        .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone();
    cases.extend(
        serde_json::from_str::<Value>(include_str!("mixed-scenario-boundaries.json")).unwrap()
            ["cases"]
            .as_array()
            .unwrap()
            .clone(),
    );
    cases
}
fn amounts(actual: &Value, expected: &Value, bounds: [f64; 3]) {
    for (key, bound) in ["current", "pending", "projected"].iter().zip(bounds) {
        close(&actual[key]["value"], &expected[key], bound);
    }
}
fn attribution(actual: &Value, expected: &Value, bounds: [f64; 3], factors: usize) {
    for key in [
        "baseline",
        "scenario",
        "total_change",
        "interaction",
        "reconciliation_residual",
    ] {
        let scale = if key == "interaction" {
            2.0 * (factors + 1) as f64
        } else if matches!(key, "baseline" | "scenario") {
            1.0
        } else {
            2.0
        };
        amounts(&actual[key], &expected[key], bounds.map(|b| b * scale));
    }
    for key in ["ordered_effects", "standalone_effects"] {
        let rows = actual[key].as_array().unwrap();
        let expected = expected[key].as_array().unwrap();
        assert_eq!(rows.len(), factors);
        assert_eq!(expected.len(), factors);
        for (a, e) in rows.iter().zip(expected) {
            amounts(a, e, bounds.map(|b| b * 2.0));
        }
    }
    for key in ["current", "pending", "projected"] {
        assert!(number(&actual["reconciliation_residual"][key]["value"]).abs() <= 1e-7);
    }
}
fn cds(actual: &Value, expected: &Value) {
    for (a, e) in [
        ("net_pv", "pv"),
        ("protection_leg_pv", "protection"),
        ("premium_leg_pv", "premium"),
        ("initial_payment_pv", "initial"),
        ("accrued_days", "accrued_days"),
        ("accrued_year_fraction", "accrued_year_fraction"),
        ("accrued_premium", "accrued_premium"),
        ("dirty_risky_annuity", "dirty_risky_annuity"),
        ("clean_risky_annuity", "clean_risky_annuity"),
        ("par_spread_decimal", "par_spread_decimal"),
        ("clean_upfront_fraction", "clean_upfront_fraction"),
        ("dirty_upfront_fraction", "dirty_upfront_fraction"),
        ("cash_settlement_amount", "cash_settlement_amount"),
        ("clean_price_per100", "clean_price_per100"),
    ] {
        close(&actual[a], &expected[e], 1e-7);
    }
    let rows = actual["premiums"].as_array().unwrap();
    let reference = expected["premiums"].as_array().unwrap();
    assert_eq!(rows.len(), reference.len());
    for (a, e) in rows.iter().zip(reference) {
        for key in ["accrual_start", "accrual_end", "payment_date"] {
            assert_eq!(a[key], e[key]);
        }
        for key in [
            "year_fraction",
            "survival_probability",
            "conditional_coupon_amount",
            "scheduled_pv",
            "accrual_on_default_pv",
        ] {
            close(&a[key], &e[key], 1e-7);
        }
    }
}
fn native(actual: &Value, expected: &Value, holding: &Value) {
    let e = &expected["full"];
    let p = &holding["product"]["product"];
    match expected["kind"].as_str().unwrap() {
        "delivery" => delivery::evaluation(&actual["Delivery"], e, p),
        "cash_basis" => {
            let a = &actual["CashBasis"];
            for (field, source) in [
                ("promised_pv", "promised_pv"),
                ("recovery_pv", "recovery_pv"),
            ] {
                close(&a[field], &e["value"][source], 1e-7);
            }
            for (field, source) in [
                ("present_value", "present_value"),
                ("dirty_price", "dirty_per100"),
                ("clean_price", "clean_per100"),
            ] {
                close(&a["price"][field], &e["value"][source], 1e-7);
            }
            let coupons = a["price"]["projection"]["coupons"].as_array().unwrap();
            let reference = e["cash"]["coupons"].as_array().unwrap();
            assert_eq!(coupons.len(), reference.len());
            for (a, r) in coupons.iter().zip(reference) {
                for (field, source) in [
                    ("accrual_start", "start"),
                    ("accrual_end", "end"),
                    ("payment_date", "payment_date"),
                ] {
                    assert_eq!(a[field], r[source]);
                }
                close(&a["amount"], &r["amount"], 1e-10);
            }
        }
        "cds" => cds(&actual["Product"]["Cds"], e),
        "optional" => {
            let a = &actual["Product"]["Optional"];
            for (field, source) in [
                ("dirty_price", "dirty"),
                ("clean_price", "clean"),
                ("accrued", "accrued"),
            ] {
                close(
                    &a[field],
                    &e[source],
                    if field == "accrued" { 1e-10 } else { 0.006 },
                );
            }
            assert_eq!(a["context"], p["input"]["context"]);
            assert_eq!(a["discount_curve"], p["input"]["discount"]);
            assert_eq!(a["projection_curve"], p["input"]["projection"]);
            assert_eq!(a["model"]["key"], p["input"]["model"]["key"]);
            let fixings = a["fixings"].as_array().unwrap();
            let known = e["known"].as_array().unwrap();
            assert_eq!(fixings.len(), known.len());
            for (a, e) in fixings.iter().zip(known) {
                assert_eq!(a["origin"]["Published"], *e);
            }
            let future = a["future_reset_requirements"].as_array().unwrap();
            let reference = e["future"].as_array().unwrap();
            assert_eq!(future.len(), reference.len());
            for (a, e) in future.iter().zip(reference) {
                assert_eq!(a["observation_date"], *e);
            }
            for key in ["notice", "exercise", "settlement"] {
                assert_eq!(a["exercises"][0]["dates"][key], e["exercise"][key]);
            }
            for grid in a["grids"].as_array().unwrap().iter().skip(2) {
                assert!(
                    number(&grid["successive_difference"])
                        <= 0.004 + 1e-8 * number(&grid["dirty_price"]).abs()
                );
            }
        }
        "swap" => {
            let a = &actual["Product"]["Swap"];
            for leg in ["fixed", "floating"] {
                let cash = a["cashflows"][leg].as_array().unwrap();
                let pv = a[leg].as_array().unwrap();
                let reference = e[leg].as_array().unwrap();
                assert_eq!(cash.len(), reference.len());
                assert_eq!(pv.len(), reference.len());
                for ((c, v), r) in cash.iter().zip(pv).zip(reference) {
                    assert_eq!(c["period"]["payment_date"], r["payment"]);
                    close(&c["signed_amount"], &r["amount"], 1e-7);
                    close(&v["present_value"]["value"], &r["pv"], 1e-7);
                }
            }
        }
        "bond" => {
            let a = &actual["Product"]["Bond"]["cash"];
            let mut flows = a["coupons"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|v| v["entitled"] == true)
                .map(|v| json!({"date":v["payment_date"],"amount":v["amount"]}))
                .collect::<Vec<_>>();
            flows.extend(
                a["principal"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| json!({"date":v["payment_date"],"amount":v["amount"]})),
            );
            flows.sort_by_key(|v| v["date"].as_str().unwrap().to_owned());
            let reference = e["cashflows"].as_array().unwrap();
            assert_eq!(flows.len(), reference.len());
            for (a, e) in flows.iter().zip(reference) {
                assert_eq!(a["date"], e["date"]);
                close(&a["amount"], &e["amount"], 1e-10);
            }
        }
        "rate_future" => {
            let a = &actual["Product"]["RateFuture"];
            close(&a["model_price_points"], &e["price"], 1e-8);
            assert_eq!(
                a["contract"],
                json!({"id":p["input"]["contract"]["instrument_id"],"version":p["input"]["contract"]["stamp"]["version"]})
            );
            let observations = a["observations"].as_array().unwrap();
            let reference = e["observations"].as_array().unwrap();
            assert_eq!(observations.len(), reference.len());
            for (a, e) in observations.iter().zip(reference) {
                assert_eq!(a["requirement"]["observation_date"], e["observation"]);
                close(&a["decimal_rate"], &e["rate"], 1e-12);
            }
        }
        kind => panic!("No complete financial check for {kind}"),
    }
}
pub fn check(case: &Value, output: &str) {
    let out: Value = serde_json::from_str(output).unwrap();
    if !case["expected_error"].is_null() {
        assert_eq!(out["ok"], "false", "{}", case["id"]);
        assert_eq!(
            out["error"]["code"], case["expected_error"],
            "{}: {}",
            case["id"], out["error"]
        );
        assert!(out.get("result").is_none());
        return;
    }
    assert_eq!(out["ok"], "true", "{}: {}", case["id"], out["error"]);
    let input = &case["request"]["operation"]["action"]["input"];
    let book = &input["book"];
    let data = &out["result"]["data"];
    let report = &data["report"];
    let expected = &case["expected"];
    assert_eq!(data["request"], *input);
    assert_eq!(report["input"], *book);
    let f = book["moves"].as_array().unwrap().len();
    let n = book["positions"].as_array().unwrap().len();
    assert_eq!(number(&report["graph_evaluations"]), (2 * f + 1) as f64);
    assert_eq!(
        number(&report["product_evaluations"]),
        ((2 * f + 1) * n) as f64
    );
    let trades = report["trades"].as_array().unwrap();
    assert_eq!(trades.len(), n);
    let mut local_bounds: Vec<(Value, Value, [f64; 3])> = Vec::new();
    let mut report_bounds: Vec<(Value, [f64; 3])> = Vec::new();
    for (i, (trade, control)) in trades
        .iter()
        .zip(expected["trades"].as_array().unwrap())
        .enumerate()
    {
        let position = &book["positions"][i];
        assert_eq!(trade["input"], *position);
        assert_eq!(trade["basis"], control["basis"]);
        let reference_bound = if control["kind"] == "delivery" {
            1e-5
        } else {
            number(&control["reference_absolute"])
        };
        let q = number(&position["current"]["quantity"]);
        let pending = number(&position["pending_quantity"]);
        let reference = number(&position["current"]["reference_quantity"]);
        let bounds = [q, pending, q + pending].map(|v| v.abs() / reference * reference_bound);
        let currency = &position["current"]["currency"];
        let rate = book["fx"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["from"] == *currency)
            .map_or(1.0, |v| number(&v["rate"]));
        let values = trade["values"].as_array().unwrap();
        assert_eq!(values.len(), 2 * f + 1);
        for (value, e) in values.iter().zip(control["values"].as_array().unwrap()) {
            close(
                &value["reference_amount"]["value"],
                &e["pv"],
                reference_bound,
            );
            native(&value["valuation"], e, &input["holdings"][i]);
        }
        attribution(&trade["local"], &control["local"], bounds, f);
        attribution(
            &trade["reporting"],
            &control["reporting"],
            bounds.map(|v| v * rate),
            f,
        );
        if control["kind"] == "delivery" {
            let steps = trade["ctd_transitions"].as_array().unwrap();
            assert_eq!(steps.len(), f);
            for (j, step) in steps.iter().enumerate() {
                let before = &control["values"][j]["full"];
                let after = &control["values"][j + 1]["full"];
                for (state, e) in [(&step["before"], before), (&step["after"], after)] {
                    for key in ["selection_ties", "fair_ties"] {
                        assert_eq!(state[key], e[key]);
                    }
                }
                assert_eq!(
                    step["selection_changed"],
                    before["selection_ties"] != after["selection_ties"]
                );
                assert_eq!(
                    step["fair_deliverable_changed"],
                    before["fair_ties"] != after["fair_ties"]
                );
                assert_eq!(
                    step["ties_present"],
                    [before, after]
                        .iter()
                        .any(|v| v["selection_ties"].as_array().unwrap().len() > 1
                            || v["fair_ties"].as_array().unwrap().len() > 1)
                );
            }
        }
        if let Some((_, _, sum)) = local_bounds
            .iter_mut()
            .find(|(b, c, _)| *b == trade["basis"] && *c == *currency)
        {
            for i in 0..3 {
                sum[i] += bounds[i];
            }
        } else {
            local_bounds.push((trade["basis"].clone(), currency.clone(), bounds));
        }
        if let Some((_, sum)) = report_bounds.iter_mut().find(|(b, _)| *b == trade["basis"]) {
            for i in 0..3 {
                sum[i] += bounds[i] * rate;
            }
        } else {
            report_bounds.push((trade["basis"].clone(), bounds.map(|v| v * rate)));
        }
    }
    for (rows, reference, reporting) in [
        (&report["local_totals"], &expected["local_totals"], false),
        (
            &report["reporting_totals"],
            &expected["reporting_totals"],
            true,
        ),
    ] {
        let rows = rows.as_array().unwrap();
        let reference = reference.as_array().unwrap();
        assert_eq!(rows.len(), reference.len());
        for (a, e) in rows.iter().zip(reference) {
            assert_eq!(a["basis"], e["basis"]);
            assert_eq!(a["currency"], e["currency"]);
            let bounds = if reporting {
                report_bounds
                    .iter()
                    .find(|(b, _)| *b == a["basis"])
                    .unwrap()
                    .1
            } else {
                local_bounds
                    .iter()
                    .find(|(b, c, _)| *b == a["basis"] && *c == a["currency"])
                    .unwrap()
                    .2
            };
            attribution(&a["attribution"], &e["attribution"], bounds, f);
        }
    }
    let graphs = data["graphs"].as_array().unwrap();
    assert_eq!(graphs.len(), 2 * f + 1);
    for (point, expected) in graphs
        .iter()
        .zip(expected["graph_controls"].as_array().unwrap())
    {
        for (id, e) in expected.as_object().unwrap() {
            let curve = point["graph"]["result"]["curves"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["recipe"]["key"]["id"] == *id)
                .unwrap();
            close(
                curve["interpolation_grid"]["values"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap(),
                &e["zero"],
                1e-12,
            );
            for helper in point["graph"]["repricing"].as_array().unwrap() {
                assert!(number(&helper["residual"]).abs() <= number(&helper["tolerance"]));
            }
        }
    }
}
pub fn run(mut execute: impl FnMut(&str) -> String) -> Vec<Value> {
    cases().iter().map(|case| { let output = execute(&case["request"].to_string()); check(case, &output);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()}) }).collect()
}
