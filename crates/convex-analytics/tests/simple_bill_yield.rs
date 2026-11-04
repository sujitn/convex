//! Full fixed-365 short-bill controls; long bills require explicit original dates.
use convex_analytics::{error::AnalyticsError, yields::bond_equivalent_yield};
use rust_decimal::Decimal;
use serde_json::Value;
use std::str::FromStr;

fn decimal(text: &str) -> Decimal {
    Decimal::from_scientific(text)
        .or_else(|_| Decimal::from_str(text))
        .unwrap()
}

#[test]
fn independent_short_bill_values_and_long_term_rejection() {
    let reference: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/simple-bill-reference.json"
    ))
    .unwrap();
    for case in reference["cases"].as_array().unwrap() {
        let actual = bond_equivalent_yield(
            decimal(case["price"].as_str().unwrap()),
            decimal(case["face"].as_str().unwrap()),
            case["days"].as_u64().unwrap() as u32,
        )
        .unwrap();
        let expected = decimal(case["expected_percent"].as_str().unwrap());
        let tolerance = decimal(reference["absolute_tolerance"].as_str().unwrap())
            + decimal(reference["relative_tolerance"].as_str().unwrap()) * expected.abs();
        assert!((actual - expected).abs() <= tolerance, "{case}: {actual}");
    }
    for days in reference["rejected_days"].as_array().unwrap() {
        let error =
            bond_equivalent_yield(decimal("97"), decimal("100"), days.as_u64().unwrap() as u32)
                .unwrap_err();
        assert!(matches!(error, AnalyticsError::InvalidInput(_)));
        assert!(error
            .to_string()
            .contains("original issue, settlement and maturity"));
    }
}

#[test]
fn invalid_cash_and_decimal_overflow_are_errors() {
    for (price, face, days) in [
        ("0", "100", 90),
        ("-1", "100", 90),
        ("97", "0", 90),
        ("97", "-100", 90),
        ("97", "100", 0),
    ] {
        assert!(matches!(
            bond_equivalent_yield(decimal(price), decimal(face), days),
            Err(AnalyticsError::InvalidInput(_))
        ));
    }
    for (price, face) in [
        (Decimal::ONE, Decimal::MAX),
        (Decimal::new(1, 28), Decimal::from(100)),
    ] {
        assert!(matches!(
            bond_equivalent_yield(price, face, 1),
            Err(AnalyticsError::CalculationFailed(_))
        ));
    }
}
