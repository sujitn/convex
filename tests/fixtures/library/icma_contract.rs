//! Declared full contextual ICMA requests and independent financial answers.
#![allow(dead_code)]
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use serde_json::Value;
pub fn cases() -> Vec<Value> {
    let v: Value =
        serde_json::from_str(packed::text("tests/fixtures/library/icma-qualified.json")).unwrap();
    v["cases"].as_array().unwrap().clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
pub fn check(case: &Value, output: &str) {
    let v: Value = serde_json::from_str(output).unwrap();
    if case["expected_error"].is_null() {
        assert_eq!(v["ok"], "true", "{}: {v}", case["id"]);
        assert!(v["error"].is_null());
        assert_financial(case, &v);
    } else {
        assert_eq!(v["ok"], "false", "{}: {v}", case["id"]);
        assert_eq!(
            v["error"]["code"], case["expected_error"],
            "{}: {v}",
            case["id"]
        );
        assert!(v.get("result").is_none(), "partial accepted quote leaked");
    }
}
fn number(v: &Value) -> f64 {
    v.as_f64()
        .unwrap_or_else(|| v.as_str().unwrap().parse().unwrap())
}
fn close(a: &Value, b: &Value, tol: f64, label: &str) {
    let x = number(a);
    let y = number(b);
    assert!(
        x.is_finite() && (x - y).abs() <= tol,
        "{label}: {x} != {y}, bound {tol}"
    );
}
pub fn assert_financial(case: &Value, output: &Value) {
    if !case["expected_error"].is_null() {
        return;
    }
    let mut refs: Value = serde_json::from_str(include_str!("icma-reference.json")).unwrap();
    let extra: Value =
        serde_json::from_str(include_str!("icma-additional-reference.json")).unwrap();
    refs["cases"]
        .as_array_mut()
        .unwrap()
        .extend(extra["cases"].as_array().unwrap().clone());
    let components: Value =
        serde_json::from_str(include_str!("icma-cash-components.json")).unwrap();
    let component = components["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == case["id"]);
    let entitlement: Value =
        serde_json::from_str(include_str!("icma-entitlement-reference.json")).unwrap();
    let boundary = entitlement["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == case["id"]);
    let original = &refs["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == case["id"])
        .expect("Every successful quote needs an original independent answer")["expected"];
    let x = boundary.map_or(original, |r| &r["expected"]);
    let q = &output["result"]["data"];
    let raw = &q["quotation"]["value"];
    let v = if q["quotation"]["kind"] == "yield" {
        &raw["valuation"]
    } else {
        raw
    };
    for name in ["clean_per100", "dirty_per100", "accrued_per100"] {
        close(&v[name], &x[name], 1e-8, name);
    }
    close(
        &v["present_value"]["value"],
        &x["present_value"],
        1e-8,
        "PV",
    );
    close(
        &q["consideration"]["value"],
        &x["consideration"],
        1e-8,
        "original signed invoice",
    );
    if q["quotation"]["kind"] == "yield" {
        close(
            &raw["decimal_parameter"],
            &x["decimal_yield"],
            1e-9,
            "original same-cash yield",
        );
        assert!(number(&raw["native_residual"]).abs() <= 1e-8);
        assert_eq!(
            raw["target"],
            case["request"]["operation"]["action"]["target"]
        );
        assert_eq!(
            raw["settings"],
            case["request"]["operation"]["action"]["settings"]
        );
    }
    assert_eq!(
        q["payment_times"].as_object().unwrap().len(),
        x["times"].as_object().unwrap().len()
    );
    for (date, time) in x["times"].as_object().unwrap() {
        close(
            &q["payment_times"][date],
            time,
            1e-12,
            "original contextual time",
        );
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
    let expected = component
        .map_or(&x["cashflows"], |r| &r["expected"]["cashflows"])
        .as_array()
        .unwrap();
    assert_eq!(
        actual.len(),
        expected.len(),
        "full original cash count {}: actual {actual:?} expected {expected:?}",
        case["id"]
    );
    for (a, b) in actual.iter().zip(expected) {
        assert_eq!(a.0, b["date"].as_str().unwrap());
        assert_eq!(a.1, b["kind"].as_str().unwrap());
        close(&a.2, &b["amount"], 1e-8, "cash amount");
    }
    let op = &case["request"]["operation"];
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
        q["sources"]["calendars"],
        op["input"]["security"]["bond"]["calendars"]
    );
    assert_eq!(
        q["sources"]["quotation_policy"],
        op["input"]["quotation_policy"]
    );
}
