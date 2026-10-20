//! Full independent causal before/after answers; native envelopes are transport only.
use serde_json::{json, Value};
#[allow(dead_code)]
#[path = "delivery_hedge_contract.rs"]
mod delivery;
pub use delivery::{close, number};
fn status(value: &Value) -> Value {
    let s = value.as_str().unwrap();
    let mut name = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && ch.is_ascii_uppercase() {
            name.push('_');
        }
        name.push(ch.to_ascii_lowercase());
    }
    json!(name)
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

pub fn cases() -> Vec<Value> {
    let mut cases = [
        include_str!("cohort-pnl-calendars.json"),
        include_str!("cohort-pnl-full-wire.json"),
    ]
    .iter()
    .flat_map(|raw| {
        serde_json::from_str::<Value>(raw).unwrap()["cases"]
            .as_array()
            .unwrap()
            .clone()
    })
    .collect::<Vec<_>>();
    if let Ok(path) = std::env::var("CVX_COHORT_CASE") {
        cases.retain(|c| c["id"] == path);
    }
    cases
}
fn amt(actual: &Value, expected: &Value, bound: f64) {
    for k in ["current", "pending", "projected"] {
        close(
            &actual["marking"][k]["value"],
            &expected["marking"][k],
            bound,
        );
    }
    close(&actual["cash"]["value"], &expected["cash"], bound);
    close(
        &actual["current_model_pnl"]["value"],
        &expected["current_model_pnl"],
        bound,
    );
}
fn explain(actual: &Value, expected: &Value, bound: f64, phases: usize) {
    amt(&actual["total"], &expected["total"], 2.0 * bound);
    for k in ["ordered", "standalone"] {
        let a = actual[k].as_array().unwrap();
        let e = expected[k].as_array().unwrap();
        assert_eq!(a.len(), phases);
        assert_eq!(e.len(), phases);
        for (a, e) in a.iter().zip(e) {
            if e.is_null() {
                assert!(a.is_null());
            } else {
                amt(a, e, 2.0 * bound);
            }
        }
    }
    if expected["interaction"].is_null() {
        assert!(actual["interaction"].is_null());
    } else {
        amt(
            &actual["interaction"],
            &expected["interaction"],
            2.0 * (phases + 1) as f64 * bound,
        );
    }
    amt(
        &actual["residual"],
        &expected["residual"],
        2.0 * (phases + 1) as f64 * bound,
    );
    for k in ["current", "pending", "projected"] {
        assert!(number(&actual["residual"]["marking"][k]["value"]).abs() <= 1e-7);
    }
    for k in ["cash", "current_model_pnl"] {
        assert!(number(&actual["residual"][k]["value"]).abs() <= 1e-7);
    }
}
pub fn check(case: &Value, output: &str) {
    let answer: Value = serde_json::from_str(output).unwrap();
    if !case["expected_error"].is_null() {
        assert_eq!(answer["ok"], "false", "{}: {answer}", case["id"]);
        assert_eq!(
            answer["error"]["code"], case["expected_error"],
            "{}: {answer}",
            case["id"]
        );
        if case["id"]
            .as_str()
            .unwrap()
            .starts_with("cohort.invalid.acquired_historically_")
        {
            assert!(
                answer["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("historically received cash"),
                "cash-transfer guard was masked by another failure: {answer}"
            );
        }
        return;
    }
    assert_eq!(answer["ok"], "true", "{}: {answer}", case["id"]);
    let specification = &case["request"]["operation"]["input"];
    let d = &specification["book"];
    let report = &answer["result"]["data"]["report"];
    let e = &case["expected"];
    assert_eq!(
        answer["result"]["data"]["delivery_comparisons"], e["delivery_comparisons"],
        "complete actual versus independently selected model CTD: {}",
        case["id"]
    );
    let phases = d["steps"].as_array().unwrap().len();
    let mut bindings = vec![&d["before"]];
    bindings.extend(d["steps"].as_array().unwrap().iter().map(|s| &s["after"]));
    bindings.extend(
        d["standalone"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["kind"] == "available")
            .map(|s| &s["information"]),
    );
    let sets=bindings.iter().map(|b|{let mut inputs=d["resources"].clone();let market=&d["markets"][b["market"].as_u64().unwrap() as usize];inputs["curves"]=market["curves"].clone();inputs["calibration"]=market["calibration"].clone();for k in ["fixings","publications"]{let extra=if k=="fixings"{"additional_fixings"}else{"additional_publications"};inputs[k].as_array_mut().unwrap().extend(b[extra].as_array().unwrap().clone());}json!({"point":b["point"],"inputs":inputs,"holdings":b["holdings"].as_array().unwrap().iter().map(|i|d["holdings"][i.as_u64().unwrap() as usize].clone()).collect::<Vec<_>>()})}).collect::<Vec<_>>();
    let trades = report["trades"].as_array().unwrap();
    let expected = e["trades"].as_array().unwrap();
    assert_eq!(trades.len(), expected.len());
    assert_eq!(
        report["product_evaluations"],
        json!(trades.len() * sets.len())
    );
    for (j, (a, e)) in trades.iter().zip(expected).enumerate() {
        assert_eq!(a["id"], e["id"]);
        let marks = a["marks"].as_array().unwrap();
        assert_eq!(marks.len(), sets.len());
        let reference = e["marks"].as_array().unwrap();
        let bound = number(&e["bound"]);
        for (i, (m, e)) in marks.iter().zip(reference).enumerate() {
            assert_eq!(m["basis"], e["basis"]);
            assert_eq!(m["state"]["status"], status(&e["status"]));
            close(&m["state"]["remaining_fraction"], &e["remaining"], 1e-12);
            assert!(
                (number(&m["reference_amount"]["value"]) - number(&e["reference"])).abs() <= bound,
                "{} trade={} point={i} reference actual={} expected={} bound={bound}",
                case["id"],
                a["id"],
                m["reference_amount"]["value"],
                e["reference"]
            );
            close(&m["reference_amount"]["value"], &e["reference"], bound);
            let p = &sets[i]["point"]["positions"][j];
            let refq = number(&p["current"]["reference_quantity"]);
            let scales = [
                number(&p["current"]["quantity"])
                    + e["physical"]["outstanding_face"]
                        .as_str()
                        .map_or(0.0, |q| q.parse::<f64>().unwrap()),
                number(&p["pending_quantity"]),
                number(&p["current"]["quantity"])
                    + number(&p["pending_quantity"])
                    + e["physical"]["outstanding_face"]
                        .as_str()
                        .map_or(0.0, |q| q.parse::<f64>().unwrap()),
            ]
            .map(|q| q.abs() / refq * bound);
            for (k, b) in ["current", "pending", "projected"].iter().zip(scales) {
                close(
                    &m["local"][k]["value"],
                    &e["local"][k],
                    b + 2e-7 * e["claims"].as_array().unwrap().len() as f64 + 1e-10,
                );
                close(
                    &m["reporting"][k]["value"],
                    &e["reporting"][k],
                    (b + 2e-7 * e["claims"].as_array().unwrap().len() as f64 + 1e-10)
                        * number(&e["fx"]),
                );
            }
            for (field, key) in [
                ("recorded_cash", "recorded"),
                ("proposed_cash", "proposed"),
                ("executed_marking", "executed_marking"),
                ("fill_consideration", "consideration"),
                ("fill_fees", "fees"),
            ] {
                close(
                    &m[field]["value"],
                    &e[key],
                    if field == "executed_marking" {
                        bound * number(&e["executed"]).abs() / refq + 1e-7
                    } else {
                        1e-10
                    },
                );
            }
            close(&m["executed_quantity"], &e["executed"], 1e-12);
            if e["terminal_quote"].is_null() {
                assert!(m["terminal_quote"].is_null());
            } else {
                close(&m["terminal_quote"]["value"], &e["terminal_quote"], 1e-7);
            }
            let mut claims = m["claims"].as_array().unwrap().iter().collect::<Vec<_>>();
            let mut expected = e["claims"].as_array().unwrap().iter().collect::<Vec<_>>();
            assert_eq!(claims.len(), expected.len());
            claims.sort_by_key(|v| v["obligation"]["key"].to_string());
            expected.sort_by_key(|v| v["key"].to_string());
            for (a, e) in claims.iter().zip(expected) {
                assert_eq!(a["entitled"], e["entitled"]);
                assert_eq!(a["obligation"]["key"], e["key"]);
                assert_eq!(a["obligation"]["due_date"], e["due"]);
                for (k, r) in [("amount", "amount"), ("recorded", "recorded")] {
                    close(&a["obligation"][k]["value"], &e[r], 1e-10);
                }
                close(&a["outstanding"]["value"], &e["outstanding"], 1e-10);
                if e["pv"].is_null() {
                    assert!(a["present_value"].is_null());
                    assert!(a["discount_ratio"].is_null());
                } else {
                    close(&a["discount_ratio"], &e["ratio"], 1e-12);
                    close(&a["present_value"]["value"], &e["pv"], 1e-7);
                }
                close(
                    &a["removed_from_active_model"]["value"],
                    &e["removed"],
                    1e-7,
                );
            }
            assert_eq!(m["cohort"], specification["cohorts"][j]);
            if e["physical"].is_null() {
                assert!(m["physical_delivery"].is_null());
            } else {
                let physical = &m["physical_delivery"];
                let reference = &e["physical"];
                for k in ["future", "delivery", "security_receipt", "instrument"] {
                    assert_eq!(physical[k], reference[k]);
                }
                for k in [
                    "proposed_face",
                    "received_face",
                    "outstanding_face",
                    "invoice_discount_ratio",
                ] {
                    close(&physical[k], &reference[k], 1e-12);
                }
                for k in [
                    "outstanding_invoice",
                    "invoice_present_value",
                    "outstanding_asset_present_value",
                ] {
                    close(
                        &physical[k]["value"],
                        &reference[k],
                        if k == "outstanding_asset_present_value" {
                            bound * number(&reference["outstanding_face"]).abs() / refq + 1e-7
                        } else {
                            1e-7
                        },
                    );
                }
                let claim = &physical["invoice"];
                let r = &reference["invoice"];
                assert_eq!(
                    claim["key"],
                    json!({"kind":r["kind"],"effective_date":r["effective"],"ordinal":r["ordinal"]})
                );
                assert_eq!(claim["due_date"], r["due"]);
                for k in ["amount", "recorded"] {
                    close(&claim[k]["value"], &r[k], 1e-10);
                }
            }
            match e["native"]["kind"].as_str().unwrap() {
                "ended" => assert_eq!(m["valuation"]["Ended"], status(&e["status"])),
                "notified" => {
                    let notice = sets[i]["holdings"][j]["lifecycle"]["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|s| s["event"]["action"]["kind"] == "exercise_notice")
                        .unwrap();
                    assert_eq!(m["valuation"]["Notified"]["notice"], notice["event"]);
                    close(
                        &m["valuation"]["Notified"]["valuation"]["present_value"]["value"],
                        &e["native"]["pv"],
                        1e-7,
                    );
                }
                _ => native(
                    &m["valuation"]["Active"],
                    &e["native"],
                    &sets[i]["holdings"][j],
                ),
            }
        }
        let max_bound = e["marks"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, mark)| {
                let p = &sets[i]["point"]["positions"][j];
                let q = number(&p["current"]["quantity"])
                    .abs()
                    .max(number(&p["pending_quantity"]).abs())
                    .max(
                        (number(&p["current"]["quantity"]) + number(&p["pending_quantity"])).abs(),
                    );
                bound
                    * (q + mark["physical"]["outstanding_face"]
                        .as_str()
                        .map_or(0.0, |q| q.parse::<f64>().unwrap().abs()))
                    / number(&p["current"]["reference_quantity"])
                    + 2e-7 * mark["claims"].as_array().unwrap().len() as f64
            })
            .fold(1e-7_f64, f64::max);
        explain(&a["local"], &e["local"], max_bound, phases);
        let max_fx = reference
            .iter()
            .map(|m| number(&m["fx"]))
            .fold(0.0_f64, f64::max);
        explain(&a["reporting"], &e["reporting"], max_bound * max_fx, phases);
        let transitions = a["ctd_transitions"].as_array().unwrap();
        assert_eq!(transitions.len(), phases);
        for (i, t) in transitions.iter().enumerate() {
            if e["marks"][i]["native"]["kind"] == "delivery"
                && e["marks"][i + 1]["native"]["kind"] == "delivery"
            {
                for (side, mark) in [("before", i), ("after", i + 1)] {
                    for key in ["selection_ties", "fair_ties"] {
                        assert_eq!(t[side][key], e["marks"][mark]["native"]["full"][key]);
                    }
                }
                assert_eq!(
                    t["selection_changed"],
                    json!(t["before"]["selection_ties"] != t["after"]["selection_ties"])
                );
                assert_eq!(
                    t["ties_present"],
                    json!(["before", "after"]
                        .iter()
                        .any(
                            |side| ["selection_ties", "fair_ties"]
                                .iter()
                                .any(|key| t[side][key].as_array().unwrap().len() > 1)
                        ))
                );
                assert_eq!(
                    t["fair_deliverable_changed"],
                    json!(t["before"]["fair_ties"] != t["after"]["fair_ties"])
                );
            } else {
                assert!(t.is_null());
            }
        }
    }
    for key in ["local_totals", "reporting_totals"] {
        let rows = report[key].as_array().unwrap();
        let totals = e[key].as_array().unwrap();
        assert_eq!(rows.len(), totals.len());
        for (a, total) in rows.iter().zip(totals) {
            assert_eq!(a["basis"], total["basis"]);
            assert_eq!(a["currency"], total["currency"]);
            let bound = expected
                .iter()
                .enumerate()
                .filter(|(j, t)| {
                    t["marks"][0]["basis"] == total["basis"]
                        && (key == "reporting_totals"
                            || sets[0]["point"]["positions"][*j]["current"]["currency"]
                                == total["currency"])
                })
                .map(|(j, t)| {
                    t["marks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .enumerate()
                        .map(|(i, m)| {
                            let p = &sets[i]["point"]["positions"][j];
                            let q = number(&p["current"]["quantity"])
                                .abs()
                                .max(number(&p["pending_quantity"]).abs())
                                .max(
                                    (number(&p["current"]["quantity"])
                                        + number(&p["pending_quantity"]))
                                    .abs(),
                                );
                            let b = number(&t["bound"])
                                * (q + m["physical"]["outstanding_face"]
                                    .as_str()
                                    .map_or(0.0, |q| q.parse::<f64>().unwrap().abs()))
                                / number(&p["current"]["reference_quantity"])
                                + 2e-7 * m["claims"].as_array().unwrap().len() as f64
                                + 1e-10;
                            b * if key == "reporting_totals" {
                                number(&m["fx"])
                            } else {
                                1.0
                            }
                        })
                        .fold(1e-10_f64, f64::max)
                })
                .sum::<f64>();
            explain(&a["attribution"], &total["attribution"], bound, phases);
        }
    }
}
pub fn run(mut execute: impl FnMut(&str) -> String) -> Vec<Value> {
    cases()
        .into_iter()
        .map(|case| {
            let request = case["request"].to_string();
            let answer = execute(&request);
            check(&case, &answer);
            // Preserve actual bytes: parser error locations are wire-specific.
            json!({"id":case["id"],"request":case["request"],"request_json":request,"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&answer).unwrap()})
        })
        .collect()
}
