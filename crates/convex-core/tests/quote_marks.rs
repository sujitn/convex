//! Exact quote values and checked native mark admission.

use convex_core::types::{Currency, Mark, Price, Spread, SpreadType, Yield};
use rust_decimal::Decimal;
use serde_json::Value;
use std::str::FromStr;

fn exact(text: &str) -> Decimal {
    Decimal::from_str_exact(text).unwrap()
}

#[test]
fn textual_marks_conserve_every_original_exact_digit_or_reject() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/quote-admission.json")).unwrap();
    for case in fixture["mark_cases"].as_array().unwrap() {
        let parsed = Mark::from_str(case["text"].as_str().unwrap());
        if case["expected"] == "rejected" {
            assert!(parsed.is_err(), "{}", case["id"]);
            continue;
        }
        let parsed = parsed.unwrap();
        let actual = match &parsed {
            Mark::Price { value, .. } | Mark::Yield { value, .. } => *value,
            Mark::Spread { value, .. } => value.as_bps(),
        };
        assert_eq!(
            actual,
            exact(case["exact_decimal"].as_str().unwrap()),
            "{}",
            case["id"]
        );
        let wire = serde_json::to_string(&parsed).unwrap();
        let restored: Mark = serde_json::from_str(&wire).unwrap();
        assert_eq!(parsed, restored, "{}", case["id"]);
        println!(
            "{}",
            serde_json::json!({"id": case["id"], "wire": wire, "exact": actual.to_string()})
        );
    }
    assert!(Mark::from_str(&"1".repeat(8193)).is_err());
}

#[test]
fn typed_price_yield_spread_and_tagged_marks_retain_exact_decimal_values() {
    let price = Price::new(exact("99.12345678901234567890123456"), Currency::USD);
    let yield_value = Yield::new(
        exact("0.0412345678901234567890123456"),
        convex_core::types::Compounding::SemiAnnual,
    );
    let spread = Spread::new(exact("125.12345678901234567890123456"), SpreadType::ZSpread);
    assert_eq!(
        price,
        serde_json::from_str::<Price>(&serde_json::to_string(&price).unwrap()).unwrap()
    );
    assert_eq!(
        yield_value,
        serde_json::from_str::<Yield>(&serde_json::to_string(&yield_value).unwrap()).unwrap()
    );
    assert_eq!(
        spread,
        serde_json::from_str::<Spread>(&serde_json::to_string(&spread).unwrap()).unwrap()
    );
    for request in [
        serde_json::json!({"mark":"price","value":"99.12345678901234567890123456789","kind":"Clean"}),
        serde_json::json!({"mark":"yield","value":"0.00000000000000000000000000001","frequency":"SemiAnnual"}),
        serde_json::json!({"mark":"price","value":99.5,"kind":"Clean","coupon_steps":[]}),
        serde_json::json!({"mark":"spread","value":{"value_bps":125,"spread_type":"ZSpread","recovery":0.4},"benchmark":"USD.SOFR"}),
    ] {
        assert!(
            serde_json::from_value::<Mark>(request.clone()).is_err(),
            "{request}"
        );
    }
    assert!(
        serde_json::to_value(Price::new(exact("99.5"), Currency::USD)).unwrap()["value"]
            .is_number()
    );
    assert!(serde_json::to_value(price).unwrap()["value"].is_string());
}
