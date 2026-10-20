//! Predeclared independent full-path financial expectations, separate from parity.
#![allow(dead_code)]
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use serde_json::Value;

pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text(
        "tests/fixtures/library/icma-workout-qualified.json",
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
fn number(value: &Value) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| value.as_str().unwrap().parse().unwrap())
}
fn close(actual: &Value, expected: &Value, tolerance: f64, label: &str) {
    let (a, b) = (number(actual), number(expected));
    assert!(
        a.is_finite() && (a - b).abs() <= tolerance,
        "{label}: {a} != {b}, bound {tolerance}"
    );
}
fn cashflows(value: &Value) -> Vec<(String, String, f64)> {
    let mut rows: Vec<_> = value["coupons"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["entitled"] == true)
        .map(|c| {
            (
                c["payment_date"].as_str().unwrap().to_owned(),
                "coupon".to_owned(),
                number(&c["amount"]),
            )
        })
        .chain(value["principal"].as_array().unwrap().iter().map(|c| {
            (
                c["payment_date"].as_str().unwrap().to_owned(),
                "principal".to_owned(),
                number(&c["amount"]),
            )
        }))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2)));
    rows
}
fn check_cash(actual: &Value, expected: &Value, label: &str) {
    let rows = cashflows(actual);
    let expected = expected.as_array().unwrap();
    assert_eq!(
        rows.len(),
        expected.len(),
        "{label}: cash component omitted"
    );
    for ((date, kind, amount), reference) in rows.iter().zip(expected) {
        assert_eq!(date, reference["date"].as_str().unwrap(), "{label}");
        assert_eq!(kind, reference["kind"].as_str().unwrap(), "{label}");
        close(&Value::from(*amount), &reference["amount"], 1e-8, label);
    }
}
pub fn check(case: &Value, output: &str) {
    let output: Value = serde_json::from_str(output).unwrap();
    let label = case["id"].as_str().unwrap();
    if !case["expected_error"].is_null() {
        assert_eq!(output["ok"], "false", "{label}: {output}");
        assert_eq!(
            output["error"]["code"], case["expected_error"],
            "{label}: {output}"
        );
        assert!(
            output.get("result").is_none(),
            "accepted partial paths leaked"
        );
        return;
    }
    assert_eq!(output["ok"], "true", "{label}: {output}");
    let mut references: Value = serde_json::from_str(packed::text(
        "tests/fixtures/library/icma-workout-reference.json",
    ))
    .unwrap();
    let additional: Value =
        serde_json::from_str(include_str!("icma-workout-additional-reference.json")).unwrap();
    references["cases"]
        .as_array_mut()
        .unwrap()
        .extend(additional["cases"].as_array().unwrap().clone());
    let expected = &references["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == case["id"])
        .expect("complete independent answer required")["expected"];
    let q = &output["result"]["data"];
    let op = &case["request"]["operation"];
    assert_eq!(q["context"], op["context"]);
    assert_eq!(q["settings"], op["action"]["settings"]);
    assert_eq!(q["examined_dates"], expected["examined_dates"], "{label}");
    assert_eq!(
        q["forced_by_notice"], expected["forced_by_notice"],
        "{label}"
    );
    let worst = op["action"]["kind"] == "worst";
    if worst {
        assert_eq!(q["target"], op["action"]["target"]);
        assert_eq!(q["selected_index"], expected["selected_index"], "{label}");
    } else {
        assert_eq!(q["decimal_yield"], op["action"]["decimal_yield"]);
    }
    let quotes = q["quotes"].as_array().unwrap();
    let expected_quotes = expected["quotes"].as_array().unwrap();
    assert_eq!(
        quotes.len(),
        expected_quotes.len(),
        "{label}: legal path dropped"
    );
    let mut evaluations = 0_u64;
    let mut cash_evaluations = 0_u64;
    let mut references = 0_u64;
    for (quote, expected) in quotes.iter().zip(expected_quotes) {
        assert_eq!(quote["terms"], op["input"]["security"]["bond"]["terms"]);
        assert_eq!(
            quote["sources"]["security"],
            op["input"]["security"]["reference"]
        );
        assert_eq!(
            quote["sources"]["quote_revision"],
            op["input"]["security"]["quote_revision"]
        );
        assert_eq!(
            quote["sources"]["calendars"],
            op["input"]["security"]["bond"]["calendars"]
        );
        assert_eq!(
            quote["sources"]["quotation_policy"],
            op["input"]["quotation_policy"]
        );
        assert_eq!(quote["reference_context"], op["input"]["reference_context"]);
        assert_eq!(quote["convention"], op["convention"]);
        assert_eq!(quote["position"], op["input"]["position"]);
        assert_eq!(quote["selection"]["kind"], "workout");
        assert_eq!(quote["selection"]["context"], op["context"]);
        assert_eq!(quote["selection"]["selection"], expected["selection"]);
        let inverse = &quote["quotation"]["value"];
        let valuation = if worst {
            &inverse["valuation"]
        } else {
            inverse
        };
        assert_eq!(
            quote["quotation"]["kind"],
            if worst { "yield" } else { "price" }
        );
        for field in ["clean_per100", "dirty_per100", "accrued_per100"] {
            close(&valuation[field], &expected[field], 1e-8, label);
        }
        close(
            &valuation["present_value"]["value"],
            &expected["present_value"],
            1e-8,
            label,
        );
        close(
            &quote["consideration"]["value"],
            &expected["consideration"],
            1e-8,
            label,
        );
        assert_eq!(
            quote["consideration"]["currency"],
            op["input"]["position"]["currency"]
        );
        check_cash(&valuation["cash"], &expected["cashflows"], label);
        check_cash(&quote["source_cash"], &expected["source_cashflows"], label);
        if expected["exercise"].is_null() {
            assert!(quote["exercise"].is_null());
        } else {
            assert_eq!(quote["exercise"]["dates"], expected["exercise"]["dates"]);
            assert_eq!(quote["exercise"]["right"], expected["exercise"]["right"]);
            for field in [
                "outstanding_principal",
                "redemption",
                "accrued_cash",
                "settlement_cash",
            ] {
                close(
                    &quote["exercise"][field],
                    &expected["exercise"][field],
                    1e-8,
                    label,
                );
            }
        }
        assert_eq!(
            quote["payment_times"].as_object().unwrap().len(),
            expected["times"].as_object().unwrap().len()
        );
        for (date, time) in expected["times"].as_object().unwrap() {
            close(&quote["payment_times"][date], time, 1e-12, label);
        }
        let count = if worst {
            assert_eq!(inverse["target"], op["action"]["target"]);
            assert_eq!(inverse["settings"], op["action"]["settings"]["inverse"]);
            close(
                &inverse["decimal_parameter"],
                &expected["decimal_yield"],
                1e-9,
                label,
            );
            assert!(
                number(&inverse["native_residual"]).abs()
                    <= number(&op["action"]["settings"]["inverse"]["price_tolerance"])
            );
            inverse["price_evaluations"].as_u64().unwrap()
        } else {
            1
        };
        evaluations += count;
        cash_evaluations += cashflows(&valuation["cash"]).len() as u64 * count;
        references += quote["reserved_reference_evaluations"].as_u64().unwrap();
    }
    assert_eq!(q["price_evaluations"].as_u64(), Some(evaluations));
    assert_eq!(
        q["cash_discount_evaluations"].as_u64(),
        Some(cash_evaluations)
    );
    assert_eq!(
        q["reserved_reference_evaluations"].as_u64(),
        Some(references)
    );
    let bounds = if worst {
        &q["settings"]["paths"]
    } else {
        &q["settings"]
    };
    assert!(references <= bounds["max_reference_evaluations"].as_u64().unwrap());
    let reserved = q["reserved_cash_evaluations"].as_u64().unwrap();
    assert!(
        cash_evaluations <= reserved
            && reserved <= bounds["max_cash_evaluations"].as_u64().unwrap()
    );
    let source_count = cashflows(&quotes[0]["source_cash"]).len() as u64 + 2;
    let ceiling = if worst {
        q["settings"]["inverse"]["max_iterations"].as_u64().unwrap() + 8
    } else {
        1
    };
    assert_eq!(reserved, quotes.len() as u64 * source_count * ceiling);
}
