//! New independent recovery controls; prior full controls remain immutable.
#![allow(dead_code)]
#[path = "cash_basis_hedge_contract.rs"]
mod prior;
pub use prior::{check_owner, close, exact, number, request};
use serde_json::{json, Value};

pub fn cases() -> Vec<Value> {
    [
        include_str!("recovery-hedge-admitted.json"),
        include_str!("recovery-hedge-boundaries.json"),
    ]
    .into_iter()
    .flat_map(|text| {
        serde_json::from_str::<Value>(text).unwrap()["cases"]
            .as_array()
            .unwrap()
            .clone()
    })
    .collect()
}
fn answer(case: &Value) -> Value {
    let id = if case["control_id"].is_null() {
        &case["id"]
    } else {
        &case["control_id"]
    };
    [
        include_str!("recovery-hedge-reference.json"),
        include_str!("recovery-hedge-boundary-reference.json"),
    ]
    .into_iter()
    .flat_map(|text| {
        serde_json::from_str::<Value>(text).unwrap()["cases"]
            .as_array()
            .unwrap()
            .clone()
    })
    .find(|row| row["id"] == *id)
    .unwrap()["expected"]
        .clone()
}
fn cds(value: &Value, expected: &Value) {
    for (actual, key) in [
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
        close(&value[actual], &expected[key], 1e-7);
    }
    let rows = value["premiums"].as_array().unwrap();
    let original = expected["premiums"].as_array().unwrap();
    assert_eq!(rows.len(), original.len());
    for (row, expected) in rows.iter().zip(original) {
        for key in ["accrual_start", "accrual_end", "payment_date"] {
            exact(&row[key], &expected[key]);
        }
        for key in [
            "year_fraction",
            "survival_probability",
            "conditional_coupon_amount",
            "scheduled_pv",
            "accrual_on_default_pv",
        ] {
            close(&row[key], &expected[key], 1e-7);
        }
    }
}
pub fn check(case: &Value, output: &str) {
    let out: Value = serde_json::from_str(output).unwrap();
    if !case["expected_error"].is_null() {
        assert_eq!(out["ok"], "false", "{}: {out}", case["id"]);
        assert_eq!(
            out["error"]["code"], case["expected_error"],
            "{}: {out}",
            case["id"]
        );
        assert!(out.get("result").is_none());
        return;
    }
    assert_eq!(out["ok"], "true", "{}: {out}", case["id"]);
    if case["request"]["operation"]["kind"] == "calibration" {
        check_scenario(case, output);
        return;
    }
    let op = &case["request"]["operation"];
    let data = &out["result"]["data"];
    let is_cash = op["kind"] == "cash_basis_hedge_risk";
    let input = if is_cash {
        &op["input"]["hedge"]
    } else {
        &op["input"]
    };
    let vector = &data["vector"];
    let expected = answer(case);
    exact(&data["input"], input);
    exact(&vector["reference_position"], &input["reference_position"]);
    for key in ["id", "instrument", "economics", "calculation", "entity"] {
        exact(&vector[key], &input["identity"][key]);
    }
    assert!(vector["fx"].is_null());
    let factors = input["factors"].as_array().unwrap();
    let sensitivities = vector["sensitivities"].as_array().unwrap();
    assert_eq!(factors.len(), sensitivities.len());
    assert_eq!(factors.len(), expected["factors"].as_array().unwrap().len());
    assert_eq!(
        number(&data["reserved_factor_evaluations"]),
        5.0 * factors.len() as f64
    );
    for ((factor, sensitivity), control) in factors
        .iter()
        .zip(sensitivities)
        .zip(expected["factors"].as_array().unwrap())
    {
        exact(&sensitivity["key"], &factor["key"]);
        exact(&sensitivity["settings"], &factor["settings"]);
        exact(&sensitivity["position"], &input["reference_position"]);
        exact(&sensitivity["snapshot_id"], &input["snapshot_id"]);
        for (i, key) in [
            "base_pv",
            "plus_pv",
            "minus_pv",
            "half_plus_pv",
            "half_minus_pv",
        ]
        .iter()
        .enumerate()
        {
            close(&sensitivity[*key], &control["prices"][i], 1e-7);
        }
        let bound = if ["hazard", "recovery"]
            .contains(&factor["key"]["coordinate"]["kind"].as_str().unwrap())
        {
            1e-5
        } else {
            1e-6
        };
        close(&sensitivity["value"], &control["value"], bound);
        close(&sensitivity["half_value"], &control["half_value"], bound);
    }
    let baseline = &expected["factors"][0]["valuations"][0];
    if is_cash {
        let h = &op["security"]["helper"];
        exact(&data["helper"], h);
        exact(&data["model"], &op["input"]["model"]);
        exact(&data["basis"], &op["input"]["basis"]);
        exact(
            &vector["credit_scope"][0],
            &op["input"]["credit"]["cds"]["reference"],
        );
        exact(
            &data["credit"]["mapping"],
            &op["input"]["credit"]["mapping"],
        );
        for key in ["promised_pv", "recovery_pv"] {
            close(&data["valuation"][key], &baseline[key], 1e-8);
        }
        let price = &data["valuation"]["price"];
        for (key, expected_key) in [
            ("present_value", "present_value"),
            ("dirty_price", "dirty_per100"),
            ("clean_price", "clean_per100"),
        ] {
            close(&price[key], &baseline[expected_key], 1e-8);
        }
        close(&price["accrued"], &expected["cash"]["accrued_per100"], 1e-8);
        let projection = &price["projection"];
        for (name, fields) in [
            (
                "coupons",
                vec![
                    ("contractual_start", "start"),
                    ("contractual_end", "end"),
                    ("payment_date", "payment_date"),
                ],
            ),
            (
                "principal",
                vec![
                    ("effective_date", "effective_date"),
                    ("payment_date", "payment_date"),
                ],
            ),
        ] {
            let rows = projection[name].as_array().unwrap();
            let originals = expected["cash"][name].as_array().unwrap();
            assert_eq!(rows.len(), originals.len());
            for (row, original) in rows.iter().zip(originals) {
                for (key, expected_key) in &fields {
                    exact(&row[*key], &original[*expected_key]);
                }
                close(&row["amount"], &original["amount"], 1e-8);
                if name == "coupons" {
                    close(&row["accrued"], &original["accrued"], 1e-8);
                    assert_eq!(row["entitled"], true);
                }
            }
        }
        for key in ["clean_per100", "dirty_per100", "accrued_per100"] {
            close(&data["observed"][key], &expected["observed"][key], 1e-8);
        }
        close(
            &data["model_quote"]["value"],
            &expected["model_quote"],
            1e-8,
        );
        assert_eq!(data["model_quote"]["unit"], h["quote"]["unit"]);
    } else {
        match op["product"]["kind"].as_str().unwrap() {
            "cds" => {
                exact(
                    &vector["credit_scope"],
                    &json!([op["product"]["terms"]["reference"]]),
                );
                cds(&data["valuation"]["Cds"], baseline);
            }
            "swap" => {
                assert_eq!(data["native_position"]["quantity_unit"], "trade_copies");
                assert_eq!(data["native_position"]["quantity"], "1");
                let value = &data["valuation"]["Swap"];
                exact(&value["cashflows"]["terms"], &op["product"]["terms"]);
                for leg in ["fixed", "floating"] {
                    let rows = value[leg].as_array().unwrap();
                    let expected = baseline[leg].as_array().unwrap();
                    assert_eq!(rows.len(), expected.len());
                    for (row, expected) in rows.iter().zip(expected) {
                        exact(&row["payment_date"], &expected["payment"]);
                        close(&row["present_value"]["value"], &expected["pv"], 1e-7);
                    }
                }
            }
            "rate_future" => {
                assert_eq!(data["basis"], "FuturesQuoteLevel");
                let value = &data["valuation"]["RateFuture"];
                close(&value["model_price_points"], &baseline["price"], 1e-10);
                let rows = value["observations"].as_array().unwrap();
                let expected = baseline["observations"].as_array().unwrap();
                assert_eq!(rows.len(), expected.len());
                for (row, expected) in rows.iter().zip(expected) {
                    exact(
                        &row["requirement"]["observation_date"],
                        &expected["observation"],
                    );
                    close(&row["decimal_rate"], &expected["rate"], 1e-12);
                }
            }
            _ => panic!("Unexpected controlled product"),
        }
    }
}
pub fn owner_requests(case: &Value, output: &str) -> Vec<Value> {
    if case["request"]["operation"]["kind"] == "calibration" {
        vec![]
    } else {
        prior::owner_requests(case, output)
    }
}

/// Complete CDS scenario output is also compared directly with every independent
/// premium/protection/accrual/initial-cash diagnostic at all five actual moves.
pub fn scenarios(case: &Value) -> Vec<Value> {
    if !case["expected_error"].is_null() || case["request"]["operation"]["product"]["kind"] != "cds"
    {
        return vec![];
    }
    let op = &case["request"]["operation"];
    let expected = answer(case);
    let mut result = Vec::new();
    for (j, factor) in op["input"]["factors"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        if factor["key"]["coordinate"]["kind"] != "recovery" {
            continue;
        }
        let bump = number(&factor["settings"]["bump"]);
        for (i, delta) in [0.0, bump, -bump, bump / 2.0, -bump / 2.0]
            .iter()
            .enumerate()
        {
            result.push(json!({"id":format!("{}.scenario.{i}", case["id"].as_str().unwrap()), "expected_error":null,
                "request":{"schema_version":1,"inputs":case["request"]["inputs"],"operation":{"kind":"calibration","input":op["graph"],"action":{"kind":"product_scenario","product":op["product"],"key":factor["key"],"delta":delta,"position":op["input"]["reference_position"],"snapshot_id":op["input"]["snapshot_id"]}}},
                "expected":expected["factors"][j]["valuations"][i],"baseline":expected["factors"][j]["valuations"][0]}));
        }
    }
    result
}
pub fn check_scenario(case: &Value, output: &str) {
    let out: Value = serde_json::from_str(output).unwrap();
    assert_eq!(out["ok"], "true", "{}: {out}", case["id"]);
    let data = &out["result"]["data"]["valuation"];
    cds(&data["baseline"]["Cds"], &case["baseline"]);
    cds(&data["shocked"]["Cds"], &case["expected"]);
    exact(
        &data["position"],
        &case["request"]["operation"]["action"]["position"],
    );
    exact(
        &data["snapshot_id"],
        &case["request"]["operation"]["action"]["snapshot_id"],
    );
    close(
        &data["change"]["value"],
        &json!(number(&case["expected"]["pv"]) - number(&case["baseline"]["pv"])),
        1e-7,
    );
}

/// Reuse the preserved original full lot/cost/current/pending/failure builder.
/// Replace only its declared four-factor template with the new independent book.
pub fn mixed_requests(outputs: &[Value]) -> Vec<Value> {
    let aliases: Vec<_> = outputs
        .iter()
        .filter(|r| {
            r["id"]
                .as_str()
                .unwrap()
                .starts_with("recovery_hedge.book.")
        })
        .map(|r| {
            let mut r = r.clone();
            r["id"] = json!(r["id"]
                .as_str()
                .unwrap()
                .replace("recovery_hedge.book", "cash_basis_hedge.book"));
            r
        })
        .collect();
    let templates: Value =
        serde_json::from_str(include_str!("recovery-hedge-problems.json")).unwrap();
    prior::mixed_requests(&aliases)
        .into_iter()
        .map(|mut case| {
            let id = case["id"]
                .as_str()
                .unwrap()
                .replace("cash_basis_hedge", "recovery_hedge");
            let template = templates["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| id.starts_with(t["id"].as_str().unwrap()))
                .unwrap();
            case["id"] = json!(id);
            case["expected"] = template["expected"].clone();
            case["tolerances"] = templates["tolerances"].clone();
            let problem = &mut case["request"]["operation"]["problem"];
            problem["factors"] = template["factors"].clone();
            problem["snapshot_id"] = template["snapshot_id"].clone();
            for name in ["holdings", "candidates"] {
                for row in problem[name].as_array_mut().unwrap() {
                    if row["risk"]["fx"]["snapshot_id"] == "cash-basis-hedge.synthetic.snapshot.1" {
                        row["risk"]["fx"]["snapshot_id"] = template["snapshot_id"].clone();
                    }
                }
            }
            case
        })
        .collect()
}
pub fn check_mixed(case: &Value, output: &str) {
    prior::check_mixed(case, output);
    if case["expected_error"].is_null() {
        let out: Value = serde_json::from_str(output).unwrap();
        let data = &out["result"]["data"];
        let exposure = if data.get("exposure").is_some() {
            &data["exposure"]
        } else {
            data
        };
        for key in ["current", "pending", "before"] {
            assert_eq!(exposure[key].as_array().unwrap().len(), 5);
        }
        if !data["proposal"].is_null() {
            assert_eq!(data["proposal"]["after"].as_array().unwrap().len(), 5);
        }
    }
}
