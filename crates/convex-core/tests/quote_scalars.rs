//! Independent exact Fraction expectations over the complete original requests.

#[path = "support/quote_scalar_consumer.rs"]
mod consumer;

use rust_decimal::Decimal;
use serde_json::Value;
use std::str::FromStr;

fn verify(start: usize, end: usize, values: usize, errors: usize) {
    let reference: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/native-quote-scalars.json"
    ))
    .unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 187);
    let mut actual_values = 0;
    let mut actual_errors = 0;
    for case in &cases[start..end] {
        let id = case["id"].as_str().unwrap();
        let result = consumer::consume(case);
        if case["expected_outcome"] == "typed_rejection" {
            let error = result.expect_err(id);
            assert!(!error.to_string().is_empty(), "{id}");
            actual_errors += 1;
            continue;
        }
        let actual = result.expect(id);
        assert_eq!(actual["typed_rejection"], false, "{id}");
        assert_eq!(actual["kind"], case["kind"], "{id}");
        match case["kind"].as_str().unwrap() {
            "decimal" => {
                let expected =
                    Decimal::from_str(reference["expected_decimals"][id].as_str().unwrap())
                        .unwrap();
                let actual = Decimal::from_str(actual["value"].as_str().unwrap()).unwrap();
                assert_eq!(actual, expected, "{id}");
            }
            "integer" => assert_eq!(actual["value"], case["expected_integer"], "{id}"),
            "text" => assert_eq!(actual["value"], case["expected_text"], "{id}"),
            _ => panic!("unknown independent output kind"),
        }
        let op = case["operation"].as_str().unwrap();
        if matches!(
            op,
            "price_from_decimal"
                | "price_to_dirty"
                | "price_to_clean"
                | "price_add"
                | "price_subtract"
        ) {
            assert_eq!(actual["currency"], "USD", "{id}");
        }
        if matches!(op, "spread_from_decimal" | "spread_add" | "spread_subtract") {
            assert_eq!(actual["spread_type"], "ZSpread", "{id}");
        }
        if op == "yield_from_percentage" {
            assert_eq!(actual["compounding"], "SemiAnnual", "{id}");
        }
        actual_values += 1;
    }
    assert_eq!(actual_values, values);
    assert_eq!(actual_errors, errors);
}

#[test]
fn independent_original_scalar_requests() {
    verify(0, 49, 26, 23);
}

#[test]
fn independent_precision_and_display_boundaries() {
    verify(49, 187, 92, 46);
}

#[test]
fn independent_total_price_display() {
    use convex_core::types::{Currency, Price};
    let reference: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/native-price-display.json"
    ))
    .unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 12);
    for case in cases {
        let value = Decimal::from_str(case["value"].as_str().unwrap()).unwrap();
        let currency = Currency::from_code(case["currency"].as_str().unwrap()).unwrap();
        let actual = Price::new(value, currency).to_string();
        assert_eq!(
            actual,
            case["expected_text"].as_str().unwrap(),
            "{}",
            case["id"]
        );
    }
}

#[test]
fn independent_binary64_spread_units() {
    use convex_core::types::{Spread, SpreadType};
    let reference: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/native-spread-float.json"
    ))
    .unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 25);
    for case in cases {
        let value = Decimal::from_str(case["value_bps"].as_str().unwrap()).unwrap();
        let expected =
            u64::from_str_radix(case["expected_binary64_bits"].as_str().unwrap(), 16).unwrap();
        let actual = Spread::new(value, SpreadType::ZSpread)
            .as_decimal_f64()
            .unwrap();
        assert_eq!(actual.to_bits(), expected, "{}", case["id"]);
    }
}
