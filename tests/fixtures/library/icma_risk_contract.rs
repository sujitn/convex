//! Complete independent financial controls; generated envelopes prove transport only.
#![allow(dead_code)]
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use serde_json::{json, Value};

pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text(
        "tests/fixtures/library/icma-risk-owner-api.json",
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
pub fn exact(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
            for (key, value) in a {
                exact(value, &b[key]);
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
pub fn number(v: &Value) -> f64 {
    v.as_f64()
        .unwrap_or_else(|| v.as_str().unwrap().parse().unwrap())
}
pub fn close(a: &Value, b: &Value, bound: f64) {
    let (a, b) = (number(a), number(b));
    assert!(
        a.is_finite() && (a - b).abs() <= bound,
        "{a} != {b}, bound {bound}"
    );
}
pub fn reference(id: &Value) -> Value {
    for source in [
        include_str!("icma-risk-admission-reference.json"),
        packed::text("tests/fixtures/library/icma-risk-reference.json"),
    ] {
        if let Some(row) = serde_json::from_str::<Value>(source).unwrap()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| &r["id"] == id)
        {
            return row["expected"].clone();
        }
    }
    panic!("Missing full independent control {id}");
}
fn cash(actual: &Value, expected: &Value) {
    let mut flows: Vec<_> = actual["coupons"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["entitled"] == true)
        .map(|c| {
            (
                c["payment_date"].as_str().unwrap(),
                "coupon",
                number(&c["amount"]),
            )
        })
        .collect();
    flows.extend(actual["principal"].as_array().unwrap().iter().map(|c| {
        (
            c["payment_date"].as_str().unwrap(),
            "principal",
            number(&c["amount"]),
        )
    }));
    flows.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)).then(a.2.total_cmp(&b.2)));
    let mut controls: Vec<_> = expected["cashflows"].as_array().unwrap().iter().collect();
    controls.sort_by(|a, b| {
        (a["date"].as_str(), a["kind"].as_str())
            .cmp(&(b["date"].as_str(), b["kind"].as_str()))
            .then(number(&a["amount"]).total_cmp(&number(&b["amount"])))
    });
    assert_eq!(
        flows.len(),
        controls.len(),
        "every original coupon/principal component"
    );
    for (a, b) in flows.iter().zip(controls) {
        assert_eq!(a.0, b["date"].as_str().unwrap());
        assert_eq!(a.1, b["kind"].as_str().unwrap());
        close(&json!(a.2), &b["amount"], 1e-8);
    }
}
pub fn check(case: &Value, wire: &str) {
    let out: Value = serde_json::from_str(wire).unwrap();
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
    let op = &case["request"]["operation"];
    let expected = reference(&case["id"]);
    let data = &out["result"]["data"];
    let hedge = op["action"]["kind"] == "hedge_vector";
    let data = if hedge {
        assert_eq!(data["identity"], op["action"]["identity"]);
        let vector = &data["vector"];
        assert_eq!(vector["instrument"], op["input"]["security"]["reference"]);
        assert_eq!(vector["reference_position"], op["input"]["position"]);
        assert_eq!(vector["settlement"], op["cash"]["settlement"]);
        assert_eq!(
            vector["sensitivities"],
            json!([data["yield_risk"]["sensitivity"]])
        );
        assert!(vector["fx"].is_null());
        assert_eq!(vector["credit_scope"], json!([]));
        &data["yield_risk"]
    } else {
        data
    };
    assert_eq!(data["input"], op["action"]["input"]);
    let s = &data["sensitivity"];
    for key in ["key", "snapshot_id", "settings"] {
        assert_eq!(s[key], op["action"]["input"][key]);
    }
    assert_eq!(s["position"], op["input"]["position"]);
    assert_eq!(s["method"], "central");
    for (key, value) in expected["sensitivity"].as_object().unwrap() {
        close(&s[key], value, 1e-7);
    }
    let values = data["valuations"].as_array().unwrap();
    assert_eq!(values.len(), 5);
    let h = number(&op["action"]["input"]["settings"]["bump"]);
    let moves = [0.0, h, -h, h / 2.0, -h / 2.0];
    let unit = op["action"]["input"]["observed_quote"]["unit"]
        .as_str()
        .unwrap();
    let multiplier = match unit {
        "PercentRate" => 100.0,
        "BasisPoints" => 10000.0,
        _ => 1.0,
    };
    for ((actual, e), delta) in values
        .iter()
        .zip(expected["valuations"].as_array().unwrap())
        .zip(moves)
    {
        assert_eq!(actual["decimal_move"], delta);
        assert_eq!(actual["native_quote"]["unit"], unit);
        close(
            &actual["native_quote"]["value"],
            &json!(number(&op["action"]["input"]["observed_quote"]["value"]) + delta * multiplier),
            1e-10,
        );
        let q = &actual["quotation"];
        assert_eq!(q["terms"], op["input"]["security"]["bond"]["terms"]);
        assert_eq!(q["reference_context"], op["input"]["reference_context"]);
        assert_eq!(q["convention"], op["convention"]);
        assert_eq!(q["selection"], op["cash"]);
        assert_eq!(q["position"], op["input"]["position"]);
        assert_eq!(
            q["sources"]["security"],
            op["input"]["security"]["reference"]
        );
        assert_eq!(
            q["sources"]["quote_revision"],
            op["input"]["security"]["quote_revision"]
        );
        assert_eq!(
            q["sources"]["quotation_policy"],
            op["input"]["quotation_policy"]
        );
        assert_eq!(
            q["sources"]["calendars"],
            op["input"]["security"]["bond"]["calendars"]
        );
        assert_eq!(
            q["reserved_reference_evaluations"],
            data["reserved_reference_evaluations"]
        );
        assert_eq!(q["quotation"]["kind"], "price");
        let p = &q["quotation"]["value"];
        for key in ["clean_per100", "dirty_per100", "accrued_per100"] {
            close(&p[key], &e[key], 1e-8);
        }
        close(&p["present_value"]["value"], &e["present_value"], 1e-8);
        close(&q["consideration"]["value"], &e["consideration"], 1e-8);
        assert_eq!(
            q["payment_times"].as_object().unwrap().len(),
            e["times"].as_object().unwrap().len()
        );
        for (date, time) in e["times"].as_object().unwrap() {
            close(&q["payment_times"][date], time, 1e-12);
        }
        cash(&p["cash"], e);
    }
    let visits = expected["valuations"][0]["cashflows"]
        .as_array()
        .unwrap()
        .len() as u64;
    assert_eq!(data["reserved_cash_evaluations"], (visits + 2) * 5);
    assert!(
        data["reserved_cash_evaluations"].as_u64().unwrap()
            <= op["action"]["input"]["max_cash_evaluations"]
                .as_u64()
                .unwrap()
    );
}

/// Real newly calculated full column enters the existing hedge owner unchanged.
/// The independent same-factor control closes current+pending, with one FX conversion.
pub fn hedge_requests(case: &Value, wire: &str) -> Vec<Value> {
    let out: Value = serde_json::from_str(wire).unwrap();
    let data = &out["result"]["data"];
    let vector = &data["vector"];
    let s = &vector["sensitivities"][0];
    let mut result = Vec::new();
    for foreign in [false, true] {
        let mut v = vector.clone();
        if foreign {
            v["fx"] = json!({"from":"USD","to":"EUR","rate":"0.9","observation":{"id":"ICMA.FX","version":71},"snapshot_id":s["snapshot_id"]});
        }
        let problem = json!({"snapshot_id":s["snapshot_id"],"entity":v["entity"],"settlement":v["settlement"],
            "reporting_currency":if foreign {"EUR"} else {"USD"},"position_state":{"id":"ICMA.POSITIONS","version":81},
            "expected_holdings":[v["id"]],"holdings":[{"risk":v,"current_quantity":"2500","pending_quantity":"-500"}],
            "factors":[{"key":s["key"],"risk_settings":s["settings"],"credit":null,"scale":1.0,"weight":1.0,"target":0.0,"lower":-100.0,"upper":100.0}],
            "candidates":[{"risk":v,"eligibility":{"eligible":true,"decision":{"id":"ICMA.ELIGIBLE","version":91},"reason":"supplied same-security calculation"},
                "trade_lower":"-3000","trade_upper":"3000","holding_lower":"-3000","holding_upper":"3000","buy_available":"3000","sell_available":"3000","lot":"1000",
                "costs":{"currency":"USD","buy_per_reference":"1","sell_per_reference":"1","observation":{"id":"ICMA.COST","version":101}}}],
            "max_cost":10.0,"settings":{"policy_version":21,"rank_relative_tolerance":1e-10,"max_condition":100.0,"solver_tolerance":1e-10,
                "factor_tolerance":1e-8,"cost_tolerance":1e-8,"cost_scale":1.0,"cost_weight":0.0,"max_work":10000,"svd_iterations":1000}});
        for action in ["aggregate", "solve"] {
            result.push(json!({"id":format!("{}.hedge.{foreign}.{action}",case["id"].as_str().unwrap()),"request":{"schema_version":1,
                "inputs":case["request"]["inputs"],"operation":{"kind":"hedge","problem":problem,"action":{"kind":action}}},"expected_error":null,
                "original_reference":reference(&case["id"]),"fx":if foreign {0.9} else {1.0}}));
        }
    }
    for (name, path, error) in [
        ("missing_factor", "sensitivities", "missing_input"),
        ("mixed_snapshot", "snapshot_id", "invalid_input"),
        ("wrong_quantity", "quantity", "invalid_input"),
        ("wrong_key", "key", "invalid_input"),
        ("missing_fx", "fx", "missing_input"),
    ] {
        let mut c = result[if path == "fx" { 3 } else { 1 }].clone();
        c["id"] = json!(format!("{}.invalid.{name}", case["id"].as_str().unwrap()));
        c["expected_error"] = json!(error);
        let v = &mut c["request"]["operation"]["problem"]["candidates"][0]["risk"];
        match path {
            "sensitivities" => v["sensitivities"] = json!([]),
            "snapshot_id" => v["sensitivities"][0]["snapshot_id"] = json!("OTHER"),
            "quantity" => v["reference_position"]["quantity"] = json!("500"),
            "key" => v["sensitivities"][0]["key"]["factor"]["model_version"] = json!("OTHER"),
            "fx" => v["fx"] = Value::Null,
            _ => unreachable!(),
        }
        result.push(c);
    }
    result
}
pub fn check_hedge(case: &Value, wire: &str) {
    let out: Value = serde_json::from_str(wire).unwrap();
    if !case["expected_error"].is_null() {
        assert_eq!(out["ok"], "false", "{}: {out}", case["id"]);
        assert_eq!(
            out["error"]["code"], case["expected_error"],
            "{}: {out}",
            case["id"]
        );
        return;
    }
    assert_eq!(out["ok"], "true", "{}: {out}", case["id"]);
    let d = &out["result"]["data"];
    let solve = case["request"]["operation"]["action"]["kind"] == "solve";
    let e = if solve { &d["exposure"] } else { d };
    assert_eq!(e["problem"], case["request"]["operation"]["problem"]);
    let r = number(&case["original_reference"]["sensitivity"]["value"]) * number(&case["fx"]);
    for (key, multiple) in [("current", 2.5), ("pending", -0.5), ("before", 2.0)] {
        close(&e[key][0], &json!(r * multiple), 1e-7);
    }
    if solve {
        assert_eq!(d["status"], "feasible");
        assert_eq!(d["authority"], "proposal_only");
        let p = &d["proposal"];
        close(&p["quantities"][0], &json!(-2000.0), 1e-8);
        close(&p["final_holdings"][0], &json!(0.0), 1e-8);
        close(&p["after"][0], &json!(0.0), 1e-8);
        close(&p["total_cost"], &json!(2.0 * number(&case["fx"])), 1e-8);
        assert!(p["constraints"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["satisfied"] == true));
    }
}
