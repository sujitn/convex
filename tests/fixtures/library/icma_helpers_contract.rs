//! Full independent economics; fresh native envelopes are transport evidence only.
#![allow(dead_code)]
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text(
        "tests/fixtures/library/icma-helpers-final-api.json",
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
pub fn exact(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(
                a.keys().collect::<Vec<_>>(),
                b.keys().collect::<Vec<_>>(),
                "{path}"
            );
            for (key, value) in a {
                exact(value, &b[key], &format!("{path}.{key}"));
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                exact(a, b, &format!("{path}[{i}]"));
            }
        }
        (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64(), "{path}"),
        _ => assert_eq!(actual, expected, "{path}"),
    }
}
fn number(v: &Value) -> f64 {
    v.as_f64()
        .unwrap_or_else(|| v.as_str().unwrap().parse().unwrap())
}
fn close(a: &Value, b: &Value, bound: f64, label: &str) {
    let (a, b) = (number(a), number(b));
    assert!(
        a.is_finite() && (a - b).abs() <= bound,
        "{label}: {a} != {b}, original bound {bound}"
    );
}
fn cash(projection: &Value, expected: &Value) {
    let mut actual: Vec<(String, String, f64)> = projection["coupons"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["entitled"].as_bool().unwrap())
        .map(|c| {
            (
                c["payment_date"].as_str().unwrap().into(),
                "coupon".into(),
                number(&c["amount"]),
            )
        })
        .collect();
    actual.extend(projection["principal"].as_array().unwrap().iter().map(|c| {
        (
            c["payment_date"].as_str().unwrap().into(),
            "principal".into(),
            number(&c["amount"]),
        )
    }));
    actual.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)).then(a.2.total_cmp(&b.2)));
    let mut original: Vec<_> = expected["cashflows"].as_array().unwrap().iter().collect();
    original.sort_by(|a, b| {
        (a["date"].as_str(), a["kind"].as_str())
            .cmp(&(b["date"].as_str(), b["kind"].as_str()))
            .then(number(&a["amount"]).total_cmp(&number(&b["amount"])))
    });
    assert_eq!(actual.len(), original.len(), "complete original cash count");
    for (a, b) in actual.iter().zip(original) {
        assert_eq!(a.0, b["date"].as_str().unwrap());
        assert_eq!(a.1, b["kind"].as_str().unwrap());
        close(&json!(a.2), &b["amount"], 1e-8, "original cash amount");
    }
}
fn quotation(q: &Value, h: &Value, expected: &Value, inverse: bool) {
    assert_eq!(q["terms"], h["terms"]);
    let input = &h["quote_kind"]["input"];
    for name in ["sources", "reference_context", "convention", "position"] {
        assert_eq!(q[name], input[name]);
    }
    assert_eq!(
        q["selection"],
        json!({"kind":"plain","settlement":h["settlement"],"on_payment":h["on_payment"]})
    );
    let raw = &q["quotation"]["value"];
    let v = if inverse {
        assert_eq!(q["quotation"]["kind"], "yield");
        assert_eq!(raw["settings"], input["inverse"]);
        close(
            &raw["decimal_parameter"],
            &expected["decimal_yield"],
            1e-9,
            "original contextual model yield",
        );
        assert!(
            number(&raw["native_residual"]).abs() <= number(&input["inverse"]["price_tolerance"])
        );
        close(
            &raw["target"]["value"],
            &expected["dirty_per100"],
            1e-8,
            "original model dirty target",
        );
        &raw["valuation"]
    } else {
        assert_eq!(q["quotation"]["kind"], "price");
        raw
    };
    for key in ["dirty_per100", "clean_per100", "accrued_per100"] {
        close(&v[key], &expected[key], 1e-8, key);
    }
    close(
        &v["present_value"]["value"],
        &expected["present_value"],
        1e-8,
        "complete quotation PV",
    );
    close(
        &q["consideration"]["value"],
        &expected["consideration"],
        1e-8,
        "original signed dirty consideration",
    );
    assert_eq!(
        q["payment_times"].as_object().unwrap().len(),
        expected["times"].as_object().unwrap().len()
    );
    for (date, time) in expected["times"].as_object().unwrap() {
        close(
            &q["payment_times"][date],
            time,
            1e-12,
            "original nominal-mesh time",
        );
    }
    cash(&v["cash"], expected);
}
fn conversion(c: &Value, h: &Value, expected: &Value) {
    assert_eq!(c["quote_kind"], h["quote_kind"]);
    assert_eq!(c["quote"], h["quote"]);
    assert_eq!(c["settlement"], h["settlement"]);
    for key in ["dirty_per100", "clean_per100", "accrued_per100"] {
        close(&c[key], &expected[key], 1e-8, key);
    }
    quotation(&c["icma"], h, expected, false);
    assert!(
        c["icma_work"]["reserved_cash_evaluations"]
            .as_u64()
            .unwrap()
            > 0
    );
}
fn valuation(v: &Value, h: &Value, expected: &Value) {
    let v = &v["Bond"];
    conversion(&v["observed"], h, &expected["observed"]);
    quotation(&v["icma"], h, &expected["model"], true);
    let p = &v["value"];
    for (key, e) in [
        ("present_value", "present_value"),
        ("clean_price", "clean_per100"),
        ("dirty_price", "dirty_per100"),
        ("accrued", "accrued_per100"),
    ] {
        close(
            &p[key],
            &expected["model"][e],
            1e-8,
            "original full model cash price",
        );
    }
    cash(&p["projection"], &expected["model"]);
    let work = &v["icma_work"];
    assert!(
        work["reserved_cash_evaluations"].as_u64().unwrap()
            <= work["max_cash_evaluations"].as_u64().unwrap()
    );
    assert_eq!(
        work["max_cash_evaluations"],
        h["quote_kind"]["input"]["max_cash_evaluations"]
    );
}
pub fn check(case: &Value, wire: &str) {
    let output: Value = serde_json::from_str(wire).unwrap();
    if !case["expected_error"].is_null() {
        assert_eq!(output["ok"], "false", "{}: {output}", case["id"]);
        assert_eq!(
            output["error"]["code"], case["expected_error"],
            "{}: {output}",
            case["id"]
        );
        assert!(output.get("result").is_none());
        return;
    }
    assert_eq!(output["ok"], "true", "{}: {output}", case["id"]);
    let mut refs = serde_json::from_str::<Value>(include_str!("icma-helpers-reference.json"))
        .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone();
    refs.extend(
        serde_json::from_str::<Value>(include_str!("icma-helpers-additional-reference.json"))
            .unwrap()["cases"]
            .as_array()
            .unwrap()
            .clone(),
    );
    refs.extend(
        serde_json::from_str::<Value>(include_str!("icma-helpers-terms-reference.json")).unwrap()
            ["cases"]
            .as_array()
            .unwrap()
            .clone(),
    );
    refs.extend(
        serde_json::from_str::<Value>(include_str!("icma-helpers-ready-reference.json")).unwrap()
            ["cases"]
            .as_array()
            .unwrap()
            .clone(),
    );
    refs.extend(
        serde_json::from_str::<Value>(include_str!("icma-helpers-boundaries.json")).unwrap()
            ["cases"]
            .as_array()
            .unwrap()
            .clone(),
    );
    refs.extend(
        serde_json::from_str::<Value>(include_str!("icma-helpers-final-reference.json")).unwrap()
            ["cases"]
            .as_array()
            .unwrap()
            .clone(),
    );
    let e = case
        .get("expected")
        .filter(|_| case["id"] != "icma_helpers.boundary.payment_include")
        .unwrap_or_else(|| {
            &refs
                .iter()
                .rev()
                .find(|r| r["id"] == case["id"])
                .expect("every success has an independent full answer")["expected"]
        });
    let op = &case["request"]["operation"];
    let data = &output["result"]["data"];
    if op["kind"] == "helper" {
        if op["input"]["kind"] == "cash_quote" {
            conversion(data, &op["input"]["helper"], &e["observed"]);
        } else {
            let h = &op["input"]["input"]["recipe"]["helpers"][0]["input"];
            exact(
                &data["recipe"],
                &op["input"]["input"]["recipe"],
                "complete original recipe",
            );
            assert_eq!(data["within_tolerance"], e["within_tolerance"]);
            valuation(&data["valuations"][0], h, e);
            close(
                &data["quotes"][0]["model"]["value"],
                &e["native_model"],
                1e-9 * match h["quote"]["unit"].as_str().unwrap() {
                    "BasisPoints" => 10000.0,
                    "PercentRate" => 100.0,
                    _ => 1.0,
                },
                "original native quote unit",
            );
            assert_eq!(data["quotes"][0]["input"], h["quote"]);
        }
    } else if let Some(risk) = e.get("risk") {
        for (name, value) in risk.as_object().unwrap() {
            close(&data[name], value, 1e-7, "independent five-PV signed risk");
        }
        for name in ["key", "settings", "position", "snapshot_id"] {
            assert_eq!(data[name], op["action"][name]);
        }
    } else {
        assert_eq!(data["build_order"], e["build_order"]);
        for fit in data["calibrations"].as_array().unwrap() {
            let key = fit["recipe"]["curve"]["key"]["id"].as_str().unwrap();
            for (a, b) in fit["annualized_parameters"]
                .as_array()
                .unwrap()
                .iter()
                .zip(e["parameters"][key].as_array().unwrap())
            {
                close(a, b, 1e-8, "original full cash fit parameter");
            }
            for (h, v) in fit["recipe"]["helpers"]
                .as_array()
                .unwrap()
                .iter()
                .zip(fit["repriced_helpers"].as_array().unwrap())
            {
                let h = &h["input"];
                valuation(v, h, &e["helpers"][h["id"].as_str().unwrap()]);
            }
            for q in fit["curve"]["diagnostics"]["quotes"].as_array().unwrap() {
                assert!(number(&q["residual"]).abs() <= number(&q["tolerance"]));
            }
        }
    }
}
