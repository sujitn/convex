//! Independent complete-cash controls, plus unchanged-owner rebuilt agreement.
#![allow(dead_code)]
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use serde_json::{json, Value};

pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text("tests/fixtures/library/graph-hedge-api.json"))
        .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
pub fn number(v: &Value) -> f64 {
    v.as_f64()
        .unwrap_or_else(|| v.as_str().unwrap().parse().unwrap())
}
pub fn close(a: &Value, b: &Value, bound: f64) {
    let (a, b) = (number(a), number(b));
    assert!(
        a.is_finite() && (a - b).abs() <= bound,
        "{a} != {b}; bound {bound}"
    );
}
pub fn exact(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
            for (k, v) in a {
                exact(v, &b[k]);
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                exact(a, b);
            }
        }
        (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64()),
        _ => assert_eq!(a, b),
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
    let data = &out["result"]["data"];
    let op = &case["request"]["operation"];
    let input = &op["input"];
    let vector = &data["vector"];
    assert_eq!(data["input"], *input);
    for field in ["id", "instrument", "economics", "calculation", "entity"] {
        assert_eq!(vector[field], input["identity"][field]);
    }
    assert_eq!(vector["reference_position"], input["reference_position"]);
    assert_eq!(vector["settlement"], "2026-10-07");
    assert!(vector["fx"].is_null());
    let factors = input["factors"].as_array().unwrap();
    let sensitivities = vector["sensitivities"].as_array().unwrap();
    assert_eq!(factors.len(), sensitivities.len());
    assert_eq!(
        number(&data["reserved_factor_evaluations"]),
        5.0 * factors.len() as f64
    );
    let swap = op["product"]["kind"] == "swap";
    assert_eq!(
        data["native_position"]["quantity_unit"],
        if swap {
            json!("trade_copies")
        } else {
            input["reference_position"]["quantity_unit"].clone()
        }
    );
    if swap {
        assert_eq!(data["native_position"]["quantity"], "1");
        assert_eq!(data["native_position"]["reference_quantity"], "1");
    }
    let reference = serde_json::from_str::<Value>(packed::text(
        "tests/fixtures/library/graph-hedge-actual-cash-reference.json",
    ))
    .unwrap();
    let control = reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == case["id"]);
    for (i, (factor, sensitivity)) in factors.iter().zip(sensitivities).enumerate() {
        assert_eq!(sensitivity["key"], factor["key"]);
        assert_eq!(sensitivity["settings"], factor["settings"]);
        assert_eq!(sensitivity["snapshot_id"], input["snapshot_id"]);
        assert_eq!(sensitivity["position"], input["reference_position"]);
        if let Some(control) = control {
            let answer = &control["expected"]["factors"][i];
            for (field, j) in [
                ("base_pv", 0),
                ("plus_pv", 1),
                ("minus_pv", 2),
                ("half_plus_pv", 3),
                ("half_minus_pv", 4),
            ] {
                close(
                    &sensitivity[field],
                    &answer["valuations"][j]["pv"],
                    number(&reference["tolerances"]["pv_absolute"]),
                );
            }
            let bound = number(
                &reference["tolerances"][if factor["key"]["coordinate"]["kind"] == "hazard" {
                    "hazard_risk_absolute"
                } else {
                    "rate_risk_absolute"
                }],
            );
            close(&sensitivity["value"], &answer["value"], bound);
            close(&sensitivity["half_value"], &answer["half_value"], bound);
        }
    }
    if let Some(control) = control {
        let answer = &control["expected"]["factors"][0]["valuations"][0];
        let valuation = &data["valuation"];
        match op["product"]["kind"].as_str().unwrap() {
            "bond" => {
                assert_eq!(vector["product"], "cash_bond");
                assert!(vector["credit_scope"].as_array().unwrap().is_empty());
                let cash = &valuation["Bond"]["cash"];
                let mut actual: Vec<_> = cash["coupons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| r["entitled"] == true)
                    .map(|r| json!({"date":r["payment_date"],"amount":r["amount"]}))
                    .collect();
                actual.extend(
                    cash["principal"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| json!({"date":r["payment_date"],"amount":r["amount"]})),
                );
                actual.sort_by(|a, b| {
                    a["date"]
                        .as_str()
                        .cmp(&b["date"].as_str())
                        .then(number(&a["amount"]).total_cmp(&number(&b["amount"])))
                });
                let mut expected = answer["cashflows"].as_array().unwrap().clone();
                expected.sort_by(|a, b| {
                    a["date"]
                        .as_str()
                        .cmp(&b["date"].as_str())
                        .then(number(&a["amount"]).total_cmp(&number(&b["amount"])))
                });
                assert_eq!(actual.len(), expected.len());
                for (a, b) in actual.iter().zip(expected) {
                    assert_eq!(a["date"], b["date"]);
                    close(&a["amount"], &b["amount"], 1e-7);
                }
            }
            "swap" => {
                assert_eq!(vector["product"], "irs");
                assert_eq!(
                    valuation["Swap"]["cashflows"]["terms"],
                    op["product"]["terms"]
                );
                for leg in ["fixed", "floating"] {
                    let a = valuation["Swap"][leg].as_array().unwrap();
                    let b = answer[leg].as_array().unwrap();
                    assert_eq!(a.len(), b.len());
                    for (a, b) in a.iter().zip(b) {
                        assert_eq!(a["payment_date"], b["payment"]);
                        close(&a["present_value"]["value"], &b["pv"], 1e-7);
                    }
                }
            }
            "cds" => {
                assert_eq!(vector["product"], "cds");
                assert_eq!(
                    vector["credit_scope"],
                    json!([op["product"]["terms"]["reference"]])
                );
                let v = &valuation["Cds"];
                close(&v["protection_leg_pv"], &answer["protection"], 1e-7);
                close(&v["premium_leg_pv"], &answer["premium"], 1e-7);
                let rows = v["premiums"].as_array().unwrap();
                let expected = answer["premiums"].as_array().unwrap();
                assert_eq!(rows.len(), expected.len());
                for (a, b) in rows.iter().zip(expected) {
                    for k in ["accrual_start", "accrual_end", "payment_date"] {
                        assert_eq!(a[k], b[k]);
                    }
                    for k in [
                        "conditional_coupon_amount",
                        "scheduled_pv",
                        "accrual_on_default_pv",
                    ] {
                        close(&a[k], &b[k], 1e-7);
                    }
                }
            }
            "rate_future" => {
                assert_eq!(vector["product"], "futures");
                assert_eq!(data["basis"], "FuturesQuoteLevel");
                let v = &valuation["RateFuture"];
                close(&v["model_price_points"], &answer["price"], 1e-10);
                let rows = v["observations"].as_array().unwrap();
                let expected = answer["observations"].as_array().unwrap();
                assert_eq!(rows.len(), expected.len());
                for (a, b) in rows.iter().zip(expected) {
                    assert_eq!(a["requirement"]["observation_date"], b["observation"]);
                    close(&a["decimal_rate"], &b["rate"], 1e-12);
                }
            }
            _ => panic!("Uncontrolled product"),
        }
    }
}

/// Original single-factor graph API equality, including every actual rebuilt PV.
pub fn owner_requests(case: &Value, output: &str) -> Vec<Value> {
    if !case["expected_error"].is_null() {
        return Vec::new();
    }
    let out: Value = serde_json::from_str(output).unwrap();
    let data = &out["result"]["data"];
    let op = &case["request"]["operation"];
    op["input"]["factors"].as_array().unwrap().iter().zip(data["vector"]["sensitivities"].as_array().unwrap()).enumerate().map(|(i,(f,s))| {
        let mut request=case["request"].clone();request["operation"]=json!({"kind":"calibration","input":op["graph"],"action":{"kind":"product_risk","product":op["product"],"key":f["key"],"position":data["native_position"],"snapshot_id":op["input"]["snapshot_id"],"settings":f["settings"]}});
        let mut expected=s.clone();expected["position"]=data["native_position"].clone();
        json!({"id":format!("{}.owner.{i}",case["id"].as_str().unwrap()),"request":request,"expected_error":null,"expected_risk":expected})
    }).collect()
}
pub fn check_owner(case: &Value, output: &str) {
    let out: Value = serde_json::from_str(output).unwrap();
    assert_eq!(out["ok"], "true", "{out}");
    exact(&out["result"]["data"], &case["expected_risk"]);
}

/// Complete mixed-book problems use freshly calculated full native columns.
pub fn mixed_requests(outputs: &[Value]) -> Vec<Value> {
    let templates = serde_json::from_str::<Value>(include_str!("graph-hedge-limits.json")).unwrap();
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
                "fx" => {
                    p["holdings"][0]["risk"]["fx"] = json!({"from":"USD","to":"EUR","rate":"0.9","observation":{"id":"FX","version":1},"snapshot_id":"DIFFERENT.SNAPSHOT"})
                }
                "fractional_contract" => {
                    p["candidates"][1]["risk"]["reference_position"]["quantity"] = json!("1.5")
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
                if i == 3 {
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
        assert_eq!(p["quantities"][1], "2");
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
            close(a, b, if i == 3 { 0.001 } else { 1e-6 });
        }
        assert!(p["constraints"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["satisfied"] == true));
    }
}
