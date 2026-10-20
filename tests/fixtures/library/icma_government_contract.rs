//! Complete original independent cash/quotation/context and error controls.
#![allow(dead_code)]
use serde_json::Value;
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("icma-government-qualified.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
fn number(v: &Value) -> f64 {
    v.as_f64()
        .unwrap_or_else(|| v.as_str().unwrap().parse().unwrap())
}
fn close(a: &Value, b: &Value, tolerance: f64, label: &str) {
    let x = number(a);
    let y = number(b);
    assert!(
        x.is_finite() && (x - y).abs() <= tolerance,
        "{label}: {x} != {y}, bound {tolerance}"
    );
}
fn icma(q: &Value, expected: &Value, context: &Value, terms: &Value, inverse: bool) {
    let raw = &q["quotation"]["value"];
    let v = if inverse { &raw["valuation"] } else { raw };
    assert_eq!(
        q["quotation"]["kind"],
        if inverse { "yield" } else { "price" }
    );
    for name in ["clean_per100", "dirty_per100", "accrued_per100"] {
        close(&v[name], &expected[name], 1e-8, name);
    }
    close(
        &v["present_value"]["value"],
        &expected["present_value"],
        1e-8,
        "PV",
    );
    close(
        &q["consideration"]["value"],
        &expected["consideration"],
        1e-8,
        "original signed consideration",
    );
    if inverse {
        close(
            &raw["decimal_parameter"],
            &expected["decimal_yield"],
            1e-9,
            "same-pricer yield",
        );
        assert!(
            number(&raw["native_residual"]).abs() <= number(&raw["settings"]["price_tolerance"])
        );
        assert!(raw["price_evaluations"].as_u64().unwrap() > 0);
    }
    assert_eq!(
        q["payment_times"].as_object().unwrap().len(),
        expected["times"].as_object().unwrap().len()
    );
    for (date, time) in expected["times"].as_object().unwrap() {
        close(
            &q["payment_times"][date],
            time,
            1e-12,
            "contextual payment time",
        );
    }
    let mut actual: Vec<_> = v["cash"]["coupons"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["entitled"].as_bool().unwrap())
        .map(|c| {
            (
                c["payment_date"].as_str().unwrap().to_owned(),
                "coupon",
                c["amount"].clone(),
            )
        })
        .collect();
    actual.extend(v["cash"]["principal"].as_array().unwrap().iter().map(|c| {
        (
            c["payment_date"].as_str().unwrap().to_owned(),
            "principal",
            c["amount"].clone(),
        )
    }));
    actual.sort_by(|a, b| {
        (&a.0, a.1)
            .cmp(&(&b.0, b.1))
            .then_with(|| number(&a.2).partial_cmp(&number(&b.2)).unwrap())
    });
    let flows = expected["cashflows"].as_array().unwrap();
    assert_eq!(actual.len(), flows.len(), "full cash count");
    for (a, b) in actual.iter().zip(flows) {
        assert_eq!(a.0, b["date"].as_str().unwrap());
        assert_eq!(a.1, b["kind"].as_str().unwrap());
        close(&a.2, &b["amount"], 1e-8, "full original cash amount");
    }
    assert_eq!(q["terms"], *terms);
    for name in ["reference_context", "convention", "position"] {
        assert_eq!(q[name], context[name], "full {name}");
    }
    assert_eq!(q["sources"]["calendars"], context["calendars"]);
    assert_eq!(
        q["sources"]["quotation_policy"],
        context["quotation_policy"]
    );
    assert_eq!(
        q["sources"]["security"],
        context["reference_context"]["security"]
    );
}
pub fn check(case: &Value, output: &str) {
    let v: Value = serde_json::from_str(output).unwrap();
    if !case["expected_error"].is_null() {
        assert_eq!(v["ok"], "false", "{}: {v}", case["id"]);
        assert_eq!(
            v["error"]["code"], case["expected_error"],
            "{}: {v}",
            case["id"]
        );
        assert!(v.get("result").is_none(), "partial accepted output");
        return;
    }
    assert_eq!(v["ok"], "true", "{}: {v}", case["id"]);
    assert!(v.get("error").is_none());
    let mut refs: Value =
        serde_json::from_str(include_str!("icma-government-reference.json")).unwrap();
    let extra: Value =
        serde_json::from_str(include_str!("icma-government-additional-reference.json")).unwrap();
    refs["cases"]
        .as_array_mut()
        .unwrap()
        .extend(extra["cases"].as_array().unwrap().clone());
    let x = &refs["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == case["id"])
        .expect("every admitted composition needs independent full answers")["expected"];
    let boundaries: Value =
        serde_json::from_str(include_str!("icma-government-entitlement-reference.json")).unwrap();
    let x = boundaries["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == case["id"])
        .map_or(x, |r| &r["expected"]);
    let op = &case["request"]["operation"];
    let q = &v["result"]["data"];
    let price = op["kind"] == "icma_government_price";
    for name in ["benchmark_decimal_yield", "decimal_spread"] {
        close(&q[name], &x[name], 1e-9, name);
    }
    close(
        &q["basis_points"],
        &x["basis_points"],
        1e-5,
        "fractional basis points",
    );
    for name in [
        "lower_index",
        "upper_index",
        "target_maturity",
        "target_remaining_days",
    ] {
        assert_eq!(q["selected"][name], x["selected"][name]);
    }
    for name in ["lower_weight", "upper_weight"] {
        close(&q["selected"][name], &x["selected"][name], 1e-12, name);
    }
    assert_eq!(q["settings"], op["settings"]);
    assert_eq!(q["reference_date"], op["reference_date"]);
    let common = if price {
        &op["settings"]["government"]
    } else {
        &op["settings"]
    };
    let target = &op["target"];
    let t = if price {
        &target["target"]["terms"]
    } else {
        &target["helper"]["terms"]
    };
    let target_quote = if price {
        &q["valuation"]
    } else {
        &q["target"]["quotation"]
    };
    icma(target_quote, &x["target"], &target["icma"], t, !price);
    let mut reserved = target_quote["reserved_reference_evaluations"]
        .as_u64()
        .unwrap();
    assert_eq!(
        q["benchmarks"].as_array().unwrap().len(),
        op["benchmarks"].as_array().unwrap().len()
    );
    for ((b, original), answer) in q["benchmarks"]
        .as_array()
        .unwrap()
        .iter()
        .zip(op["benchmarks"].as_array().unwrap())
        .zip(x["benchmarks"].as_array().unwrap())
    {
        assert_eq!(
            b["source"], *original,
            "full benchmark including unselected sources"
        );
        assert_eq!(
            b["quotation"]["source"]["helper"],
            original["quote"]["helper"]
        );
        let conversion = &b["quotation"]["conversion"];
        close(
            &conversion["dirty_per100"],
            &answer["observed_dirty_per100"],
            1e-8,
            "original benchmark dirty",
        );
        close(
            &conversion["clean_per100"],
            &answer["observed_clean_per100"],
            1e-8,
            "original benchmark clean",
        );
        assert_eq!(conversion["quote"], original["quote"]["helper"]["quote"]);
        icma(
            &b["quotation"]["quotation"],
            answer,
            &original["icma"],
            &original["quote"]["helper"]["terms"],
            true,
        );
        assert_eq!(
            b["quotation"]["quotation"]["quotation"]["value"]["settings"],
            common["inverse"]
        );
        assert_eq!(
            b["quotation"]["quotation"]["sources"]["quote_revision"]["version"],
            original["quote"]["helper"]["quote_version"]
        );
        reserved += b["quotation"]["quotation"]["reserved_reference_evaluations"]
            .as_u64()
            .unwrap();
    }
    assert_eq!(
        q["reserved_reference_evaluations"].as_u64().unwrap(),
        reserved
    );
    assert!(reserved <= common["max_reference_evaluations"].as_u64().unwrap());
    if price {
        assert_eq!(q["target"], *target);
        assert_eq!(q["quote"], op["quote"]);
        close(
            &q["target_decimal_yield"],
            &x["target"]["decimal_yield"],
            1e-9,
            "requested target yield",
        );
        let inverse = &q["yield_inverse"]["quotation"]["value"];
        assert_eq!(inverse["settings"], common["inverse"]);
        assert_eq!(
            inverse["target"]["value"],
            q["valuation"]["quotation"]["value"]["dirty_per100"]
        );
        close(
            &q["yield_inverse"]["consideration"]["value"],
            &q["valuation"]["consideration"]["value"],
            0.0,
            "original computed target consideration",
        );
        assert!(
            number(&inverse["native_residual"]).abs()
                <= number(&common["inverse"]["price_tolerance"])
        );
        assert!(
            number(&q["decimal_spread_residual"]).abs()
                <= number(&op["settings"]["decimal_spread_tolerance"])
        );
        close(
            &q["roundtrip_decimal_spread"],
            &x["decimal_spread"],
            1e-9,
            "spread roundtrip",
        );
    } else {
        assert_eq!(q["target"]["source"], *target);
        close(
            &q["target"]["conversion"]["dirty_per100"],
            &x["target"]["observed_dirty_per100"],
            1e-8,
            "original target observation",
        );
        assert_eq!(
            target_quote["quotation"]["value"]["settings"],
            common["inverse"]
        );
    }
}
