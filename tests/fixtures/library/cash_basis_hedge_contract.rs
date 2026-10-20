//! Full independent financial controls and original native owner equality.
#![allow(dead_code)]
#[path = "graph_hedge_contract.rs"]
mod common;
pub use common::{close, exact, number, request};
use serde_json::{json, Value};

pub fn cases() -> Vec<Value> {
    [
        include_str!("cash-basis-hedge-api.json"),
        include_str!("cash-basis-hedge-precise.json"),
        include_str!("cash-basis-hedge-book-columns.json"),
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
pub fn check(case: &Value, output: &str) {
    if case["request"]["operation"]["kind"] == "graph_hedge_risk" {
        check_graph_book(case, output);
        return;
    }
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
    let d = &out["result"]["data"];
    let op = &case["request"]["operation"];
    let input = &op["input"];
    let hedge = &input["hedge"];
    let helper = &op["security"]["helper"];
    exact(&d["helper"], helper);
    exact(&d["input"], hedge);
    exact(&d["basis"], &input["basis"]);
    exact(&d["model"], &input["model"]);
    let v = &d["vector"];
    assert_eq!(v["product"], "cash_bond");
    assert_eq!(v["settlement"], helper["settlement"]);
    assert!(v["fx"].is_null());
    for key in ["id", "instrument", "economics", "calculation", "entity"] {
        exact(&v[key], &hedge["identity"][key]);
    }
    exact(&v["reference_position"], &hedge["reference_position"]);
    exact(&v["credit_scope"][0], &input["credit"]["cds"]["reference"]);
    exact(&d["credit"]["mapping"], &input["credit"]["mapping"]);
    exact(&d["credit"]["basis"], &input["credit"]["basis"]);
    let reference: Value =
        serde_json::from_str(include_str!("cash-basis-hedge-reference.json")).unwrap();
    let extra: Value =
        serde_json::from_str(include_str!("cash-basis-hedge-rebuild-reference.json")).unwrap();
    let book: Value =
        serde_json::from_str(include_str!("cash-basis-hedge-book-reference.json")).unwrap();
    let a = &reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(extra["cases"].as_array().unwrap())
        .chain(book["cases"].as_array().unwrap())
        .find(|r| r["id"] == case["id"])
        .unwrap()["expected"];
    let tol = &reference["tolerances"];
    let value = &d["valuation"];
    for key in ["promised_pv", "recovery_pv"] {
        close(&value[key], &a["valuation"][key], number(&tol["pv"]));
    }
    let price = &value["price"];
    for (native, key) in [
        ("present_value", "present_value"),
        ("dirty_price", "dirty_per100"),
        ("clean_price", "clean_per100"),
    ] {
        close(&price[native], &a["valuation"][key], number(&tol["pv"]));
    }
    close(
        &price["accrued"],
        &a["cash"]["accrued_per100"],
        number(&tol["cash"]),
    );
    let p = &price["projection"];
    let expected = &a["cash"];
    assert_eq!(
        p["coupons"].as_array().unwrap().len(),
        expected["coupons"].as_array().unwrap().len()
    );
    for (x, y) in p["coupons"]
        .as_array()
        .unwrap()
        .iter()
        .zip(expected["coupons"].as_array().unwrap())
    {
        for (native, key) in [
            ("contractual_start", "start"),
            ("contractual_end", "end"),
            ("payment_date", "payment_date"),
        ] {
            assert_eq!(x[native], y[key]);
        }
        for key in ["amount", "accrued"] {
            close(&x[key], &y[key], number(&tol["cash"]));
        }
        assert_eq!(x["entitled"], true);
        assert!(x["rates"].as_array().unwrap().is_empty());
    }
    assert_eq!(
        p["principal"].as_array().unwrap().len(),
        expected["principal"].as_array().unwrap().len()
    );
    for (x, y) in p["principal"]
        .as_array()
        .unwrap()
        .iter()
        .zip(expected["principal"].as_array().unwrap())
    {
        for key in ["effective_date", "payment_date"] {
            assert_eq!(x[key], y[key]);
        }
        close(&x["amount"], &y["amount"], number(&tol["cash"]));
    }
    for key in ["clean_per100", "dirty_per100", "accrued_per100"] {
        close(
            &d["observed"][key],
            &a["observed"][key],
            number(&tol["price"]),
        );
    }
    close(
        &d["model_quote"]["value"],
        &a["model_quote"],
        number(&tol["native_quote"]),
    );
    assert_eq!(d["model_quote"]["unit"], helper["quote"]["unit"]);
    close(
        &d["model_residual"],
        &json!(number(&d["model_quote"]["value"]) - number(&helper["quote"]["value"])),
        number(&tol["native_quote"]),
    );
    let fs = hedge["factors"].as_array().unwrap();
    let actual = v["sensitivities"].as_array().unwrap();
    assert_eq!(fs.len(), actual.len());
    assert_eq!(
        number(&d["reserved_factor_evaluations"]),
        5.0 * fs.len() as f64
    );
    for ((f, s), answer) in fs.iter().zip(actual).zip(a["factors"].as_array().unwrap()) {
        exact(&s["key"], &f["key"]);
        exact(&s["settings"], &f["settings"]);
        exact(&s["position"], &hedge["reference_position"]);
        exact(&s["snapshot_id"], &hedge["snapshot_id"]);
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
            close(&s[*key], &answer["prices"][i], number(&tol["pv"]));
        }
        close(&s["value"], &answer["value"], number(&tol["risk"]));
        assert!(number(&s["half_value"]).is_finite());
    }
    if helper["quote_kind"]["kind"] == "ContextualIcmaYield" {
        let obs = &d["observed"]["icma"];
        let m = &d["model_icma"];
        let quote = &m["quotation"]["value"];
        close(
            &quote["decimal_parameter"],
            &a["model_icma"]["decimal_yield"],
            number(&tol["yield_rate"]),
        );
        for (actual, expected) in [(obs, &a["observed"]["icma"]), (m, &a["model_icma"])] {
            close(
                &actual["consideration"]["value"],
                &expected["consideration"],
                number(&tol["cash"]),
            );
        }
        assert!(
            number(&d["icma_work"]["reserved_cash_evaluations"])
                > number(&d["observed"]["icma_work"]["reserved_cash_evaluations"])
        );
    } else {
        assert!(d["model_icma"].is_null());
        assert!(d["icma_work"].is_null());
    }
}
pub fn owner_requests(case: &Value, output: &str) -> Vec<Value> {
    if case["request"]["operation"]["kind"] == "graph_hedge_risk" {
        return common::owner_requests(case, output);
    }
    if !case["expected_error"].is_null() {
        return vec![];
    }
    let out: Value = serde_json::from_str(output).unwrap();
    let d = &out["result"]["data"];
    let op = &case["request"]["operation"];
    let h = &op["security"]["helper"];
    let mut owner_inputs = case["request"]["inputs"].clone();
    let mut curve = if op["graph"]["cash"].as_array().unwrap().is_empty() {
        owner_inputs["curves"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["key"] == op["input"]["basis"])
            .unwrap()
            .clone()
    } else {
        owner_inputs["calibration"] = op["graph"].clone();
        op["graph"]["cash"][0]["recipe"]["curve"].clone()
    };
    curve["calibration"] = json!("Piecewise");
    curve["source"] = json!({"Helpers":[h["id"]]});
    curve["dependencies"] = if op["input"]["model"]["kind"] == "CreditParRecovery" {
        json!([
            op["input"]["model"]["discount"],
            op["input"]["model"]["survival"]
        ])
    } else {
        json!([op["input"]["model"]["reference"]])
    };
    let recipe = json!({"curve":curve,"pillars":[h["pillar"]],"helpers":[{"kind":"Bond","input":h}],"bootstrap":{"lower_parameter":-0.05,"upper_parameter":0.1,"max_iterations":128},"basis_model":op["input"]["model"],"overlap":"Reject"});
    let request = json!({"schema_version":1,"inputs":owner_inputs,"operation":{"kind":"helper","input":{"kind":"cash_reprice","input":{"recipe":recipe,"calendars":[{"kind":"bond","pins":op["security"]["calendars"]}]}}}});
    vec![
        json!({"id":format!("{}.original_cash_owner",case["id"].as_str().unwrap()),"request":request,"expected_error":null,"valuation":d["valuation"],"observed":d["observed"],"model_quote":d["model_quote"],"model_icma":d["model_icma"]}),
    ]
}
pub fn check_owner(case: &Value, output: &str) {
    if !case["expected_risk"].is_null() {
        common::check_owner(case, output);
        return;
    }
    let out: Value = serde_json::from_str(output).unwrap();
    assert_eq!(out["ok"], "true", "{}: {out}", case["id"]);
    let d = &out["result"]["data"];
    let value = &d["valuations"][0]["Bond"];
    exact(&value["basis"], &case["valuation"]);
    exact(&value["observed"], &case["observed"]);
    exact(&d["quotes"][0]["model"], &case["model_quote"]);
    if !case["model_icma"].is_null() {
        exact(&value["icma"], &case["model_icma"]);
    }
}

pub fn mixed_requests(outputs: &[Value]) -> Vec<Value> {
    let templates =
        serde_json::from_str::<Value>(include_str!("cash-basis-hedge-book-problems.json")).unwrap();
    let mut cases = Vec::new();
    for template in templates["cases"].as_array().unwrap() {
        let column = |id: &Value| {
            let row = outputs.iter().find(|r| r["id"] == *id).unwrap();
            let mut vector = row["expected_envelope"]["result"]["data"]["vector"].clone();
            if !template["fx"].is_null() {
                let mut fx = template["fx"].clone();
                let from = fx.as_object_mut().unwrap().remove("from_").unwrap();
                fx["from"] = from;
                vector["fx"] = fx;
            }
            vector
        };
        let holdings:Vec<_>=template["holdings"].as_array().unwrap().iter().map(|h|json!({"risk":column(&h["column"]),"current_quantity":h["current"],"pending_quantity":h["pending"]})).collect();
        let candidates:Vec<_>=template["candidates"].as_array().unwrap().iter().map(|c|{
            let v=column(&c["column"]);let n=number(&c["limit"]);json!({"risk":v,"eligibility":{"eligible":true,"decision":{"id":"SUPPLIED.ELIGIBILITY","version":1},"reason":"Explicit synthetic decision; proposal only"},"trade_lower":format!("-{n}"),"trade_upper":format!("{n}"),"holding_lower":format!("-{}",2.0*n),"holding_upper":format!("{}",2.0*n),"buy_available":c["limit"],"sell_available":c["limit"],"lot":c["lot"],"costs":{"currency":"USD","buy_per_reference":c["buy_per_reference"],"sell_per_reference":c["sell_per_reference"],"observation":{"id":"FULL.SIDE.COSTS","version":1}}})
        }).collect();
        let expected_holdings: Vec<_> = holdings.iter().map(|h| h["risk"]["id"].clone()).collect();
        let problem = json!({"snapshot_id":template["snapshot_id"],"entity":template["entity"],"settlement":template["settlement"],"reporting_currency":template["reporting_currency"],"position_state":{"id":"FULL.CURRENT.AND.PENDING","version":1},"expected_holdings":expected_holdings,"holdings":holdings,"factors":template["factors"],"candidates":candidates,"max_cost":template["max_cost"],"settings":template["settings"]});
        let empty = json!({"calendars":[],"curves":[],"fixings":[],"publications":[],"helper_calendars":[],"calibration":null});
        for action in ["aggregate", "solve"] {
            cases.push(json!({"id":format!("{}.{}",template["id"].as_str().unwrap(),action),"request":{"schema_version":1,"inputs":empty,"operation":{"kind":"hedge","problem":problem,"action":{"kind":action}}},"expected_error":null,"expected":template["expected"],"tolerances":templates["tolerances"]}));
        }
        let base = cases.last().unwrap().clone();
        for kind in [
            "factor_missing",
            "snapshot",
            "policy",
            "credit_scope",
            "fx",
            "fractional_contract",
            "credit_cash_scope",
            "work",
        ] {
            let mut c = base.clone();
            c["id"] = json!(format!("{}.{}", template["id"].as_str().unwrap(), kind));
            c["expected_error"] = json!(if kind == "factor_missing" {
                "missing_input"
            } else {
                "invalid_input"
            });
            let p = &mut c["request"]["operation"]["problem"];
            match kind {
                "factor_missing" => {
                    p["holdings"][0]["risk"]["sensitivities"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }
                "snapshot" => {
                    p["holdings"][0]["risk"]["sensitivities"][0]["snapshot_id"] =
                        json!("DIFFERENT.SNAPSHOT")
                }
                "policy" => {
                    p["holdings"][0]["risk"]["sensitivities"][0]["settings"]["policy_version"] =
                        json!(2)
                }
                "credit_scope" => p["holdings"][1]["risk"]["credit_scope"] = json!([]),
                "credit_cash_scope" => p["holdings"][0]["risk"]["credit_scope"] = json!([]),
                "fx" => {
                    p["holdings"][0]["risk"]["fx"] = json!({"from":"USD","to":"EUR","rate":"0.9","observation":{"id":"FX","version":1},"snapshot_id":"DIFFERENT.SNAPSHOT"})
                }
                "fractional_contract" => {
                    p["candidates"][3]["risk"]["reference_position"]["quantity"] = json!("1.5")
                }
                "work" => {
                    p["settings"]["max_work"] = json!(1);
                    c["expected_error"] = Value::Null;
                    c["expected_status"] = json!("work_exhausted");
                }
                _ => unreachable!(),
            }
            cases.push(c);
        }
    }
    cases
}
pub fn check_mixed(case: &Value, output: &str) {
    let out: Value = serde_json::from_str(output).unwrap();
    if !case["expected_error"].is_null() {
        assert_eq!(out["ok"], "false", "{}: {out}", case["id"]);
        assert_eq!(out["error"]["code"], case["expected_error"]);
        assert!(out.get("result").is_none());
        return;
    }
    assert_eq!(out["ok"], "true", "{}: {out}", case["id"]);
    let data = &out["result"]["data"];
    let solve = case["request"]["operation"]["action"]["kind"] == "solve";
    let exposure = if solve { &data["exposure"] } else { data };
    assert_eq!(exposure["problem"], case["request"]["operation"]["problem"]);
    for name in ["current", "pending", "before"] {
        for (i, (a, b)) in exposure[name]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["expected"][name].as_array().unwrap())
            .enumerate()
        {
            close(
                a,
                b,
                if i == 1 {
                    number(&case["tolerances"]["hazard_risk_absolute"])
                } else {
                    number(&case["tolerances"]["rate_risk_absolute"])
                },
            );
        }
    }
    if solve {
        assert_eq!(data["authority"], "proposal_only");
        if !case["expected_status"].is_null() {
            assert_eq!(data["status"], case["expected_status"]);
            assert!(data["proposal"].is_null());
            return;
        }
        assert_eq!(data["status"], "feasible", "{data}");
        let p = &data["proposal"];
        for name in ["quantities", "final_holdings"] {
            for (a, b) in p[name]
                .as_array()
                .unwrap()
                .iter()
                .zip(case["expected"][name].as_array().unwrap())
            {
                close(a, b, number(&case["tolerances"]["quantity_absolute"]));
            }
        }
        assert_eq!(p["quantities"][3], "2");
        close(
            &p["total_cost"],
            &case["expected"]["total_cost"],
            number(&case["tolerances"]["cost_absolute"]),
        );
        for (i, (a, b)) in p["after"]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["expected"]["after"].as_array().unwrap())
            .enumerate()
        {
            close(a, b, if i == 1 { 1e-5 } else { 1e-6 });
        }
        assert!(p["constraints"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["satisfied"] == true));
    }
}
fn check_graph_book(case: &Value, output: &str) {
    let out: Value = serde_json::from_str(output).unwrap();
    assert_eq!(out["ok"], "true", "{}: {out}", case["id"]);
    let op = &case["request"]["operation"];
    let d = &out["result"]["data"];
    let v = &d["vector"];
    exact(&d["input"], &op["input"]);
    exact(&v["reference_position"], &op["input"]["reference_position"]);
    for name in ["id", "instrument", "economics", "calculation", "entity"] {
        exact(&v[name], &op["input"]["identity"][name]);
    }
    let reference: Value =
        serde_json::from_str(include_str!("cash-basis-hedge-book-reference.json")).unwrap();
    let a = &reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == case["id"])
        .unwrap()["expected"];
    let fs = op["input"]["factors"].as_array().unwrap();
    let sensitivities = v["sensitivities"].as_array().unwrap();
    assert_eq!(sensitivities.len(), 4);
    for ((f, s), answer) in fs
        .iter()
        .zip(sensitivities)
        .zip(a["factors"].as_array().unwrap())
    {
        exact(&s["key"], &f["key"]);
        exact(&s["settings"], &f["settings"]);
        exact(&s["position"], &op["input"]["reference_position"]);
        exact(&s["snapshot_id"], &op["input"]["snapshot_id"]);
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
            close(&s[*key], &answer["valuations"][i]["pv"], 1e-7);
        }
        close(&s["value"], &answer["value"], 1e-5);
        close(&s["half_value"], &answer["half_value"], 1e-5);
    }
    let answer = &a["factors"][0]["valuations"][0];
    let value = &d["valuation"];
    match op["product"]["kind"].as_str().unwrap() {
        "cds" => {
            assert_eq!(v["product"], "cds");
            exact(
                &v["credit_scope"],
                &json!([op["product"]["terms"]["reference"]]),
            );
            let data = &value["Cds"];
            close(&data["protection_leg_pv"], &answer["protection"], 1e-7);
            close(&data["premium_leg_pv"], &answer["premium"], 1e-7);
            assert_eq!(
                data["premiums"].as_array().unwrap().len(),
                answer["premiums"].as_array().unwrap().len()
            );
            for (x, y) in data["premiums"]
                .as_array()
                .unwrap()
                .iter()
                .zip(answer["premiums"].as_array().unwrap())
            {
                for key in ["accrual_start", "accrual_end", "payment_date"] {
                    exact(&x[key], &y[key]);
                }
                for key in [
                    "conditional_coupon_amount",
                    "scheduled_pv",
                    "accrual_on_default_pv",
                ] {
                    close(&x[key], &y[key], 1e-7);
                }
            }
        }
        "swap" => {
            assert_eq!(v["product"], "irs");
            let data = &value["Swap"];
            exact(&data["cashflows"]["terms"], &op["product"]["terms"]);
            for leg in ["fixed", "floating"] {
                assert_eq!(
                    data[leg].as_array().unwrap().len(),
                    answer[leg].as_array().unwrap().len()
                );
                for (x, y) in data[leg]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(answer[leg].as_array().unwrap())
                {
                    exact(&x["payment_date"], &y["payment"]);
                    close(&x["present_value"]["value"], &y["pv"], 1e-7);
                }
            }
        }
        "rate_future" => {
            assert_eq!(v["product"], "futures");
            assert_eq!(d["basis"], "FuturesQuoteLevel");
            let data = &value["RateFuture"];
            close(&data["model_price_points"], &answer["price"], 1e-10);
            assert_eq!(
                data["observations"].as_array().unwrap().len(),
                answer["observations"].as_array().unwrap().len()
            );
            for (x, y) in data["observations"]
                .as_array()
                .unwrap()
                .iter()
                .zip(answer["observations"].as_array().unwrap())
            {
                exact(&x["requirement"]["observation_date"], &y["observation"]);
                close(&x["decimal_rate"], &y["rate"], 1e-12);
            }
        }
        _ => panic!("Uncontrolled book product"),
    }
}
