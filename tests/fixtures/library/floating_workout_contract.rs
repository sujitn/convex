//! Independent financial controls are separate from fresh transport envelopes.
#![allow(dead_code)]
#[path = "contract.rs"]
mod original;
use serde_json::Value;
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("floating-workout-qualified.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(c: &Value) -> String {
    original::request(c)
}
pub fn check(c: &Value, output: &str) {
    original::check(c, output);
}
fn number(v: &Value) -> f64 {
    v.as_f64()
        .unwrap_or_else(|| v.as_str().unwrap().parse().unwrap())
}
fn close(actual: &Value, expected: &Value, tolerance: f64, name: &str) {
    let a = number(actual);
    let b = number(expected);
    assert!(
        a.is_finite() && (a - b).abs() <= tolerance,
        "{name}: {a} vs {b}, bound {tolerance}"
    );
}
fn assert_path(q: &Value, expected: &Value) {
    let value = &q["quotation"]["value"];
    let inverse = q["quotation"]["kind"] == "discount_margin";
    let v = if inverse { &value["valuation"] } else { value };
    for name in ["clean_per100", "dirty_per100", "accrued_per100"] {
        close(&v[name], &expected[name], 1e-8, name);
    }
    close(&v["present_value"]["value"], &expected["pv"], 1e-8, "PV");
    close(
        &q["consideration"]["value"],
        &expected["consideration"],
        1e-8,
        "consideration",
    );
    if inverse {
        close(
            &value["decimal_parameter"],
            &expected["decimal_dm"],
            1e-9,
            "DM",
        );
        assert!(
            number(&value["native_residual"]).abs()
                <= number(&value["settings"]["price_tolerance"])
        );
    }
    if !expected["exercise"].is_null() {
        let e = &q["exercise"];
        let x = &expected["exercise"];
        for name in ["notice", "exercise", "settlement"] {
            assert_eq!(e["dates"][name], x[name]);
        }
        for (name, original) in [
            ("outstanding_principal", "outstanding"),
            ("redemption", "redemption"),
            ("accrued_cash", "partial"),
            ("settlement_cash", "total"),
        ] {
            close(&e[name], &x[original], 1e-8, name);
        }
    } else {
        assert!(q["exercise"].is_null());
    }
    let mut actual: Vec<(String, String, Value)> = v["cash"]["coupons"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["entitled"].as_bool().unwrap())
        .map(|c| {
            (
                c["payment_date"].as_str().unwrap().into(),
                "coupon".into(),
                c["amount"].clone(),
            )
        })
        .collect();
    actual.extend(v["cash"]["principal"].as_array().unwrap().iter().map(|c| {
        (
            c["payment_date"].as_str().unwrap().into(),
            "principal".into(),
            c["amount"].clone(),
        )
    }));
    actual.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    let mut x: Vec<_> = expected["cashflows"].as_array().unwrap().iter().collect();
    x.sort_by_key(|c| (c["date"].as_str().unwrap(), c["kind"].as_str().unwrap()));
    assert_eq!(actual.len(), x.len(), "full original cash count");
    for (a, b) in actual.iter().zip(x) {
        assert_eq!(a.0, b["date"].as_str().unwrap());
        assert_eq!(a.1, b["kind"].as_str().unwrap());
        close(&a.2, &b["amount"], 1e-8, "original cash");
    }
}
pub fn assert_financial(case: &Value, output: &Value) {
    let refs: Value =
        serde_json::from_str(include_str!("floating-workout-reference.json")).unwrap();
    if !case["expected_error"].is_null() {
        return;
    }
    let mut refs = refs;
    let extra: Value =
        serde_json::from_str(include_str!("floating-workout-additional-reference.json")).unwrap();
    refs["cases"]
        .as_array_mut()
        .unwrap()
        .extend(extra["cases"].as_array().unwrap().clone());
    let id = case["id"].as_str().unwrap();
    let row = refs["cases"].as_array().unwrap().iter().find(|r| {
        r["id"] == case["id"]
            || r["id"].as_str() == Some(id.strip_prefix("floating.").unwrap_or(id))
    });
    let row =
        row.expect("every successful floating workout requires independent financial controls");
    assert_eq!(output["ok"], "true", "{}: {output}", case["id"]);
    let q = &output["result"]["data"]["quotation"];
    let x = &row["expected"];
    if x.get("quotes").is_some() {
        let quotes = q["quotes"].as_array().unwrap();
        let refs = x["quotes"].as_array().unwrap();
        assert_eq!(quotes.len(), refs.len());
        assert_eq!(q["examined_dates"], x["examined_dates"]);
        for (q, r) in quotes.iter().zip(refs) {
            assert_eq!(q["selection"], r["selection"]);
            assert_path(q, &r["expected"]);
        }
        assert!(
            q.get("selected_index").is_none(),
            "no invented worst-DM meaning"
        );
    } else {
        assert_path(&q["quote"], x);
    }
}
