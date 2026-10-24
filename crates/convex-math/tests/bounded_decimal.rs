//! Independent exact-rational bit-level final binary64 rounding controls.
use convex_math::decimal::BoundedDecimal;
use rust_decimal::Decimal;
use std::str::FromStr;
fn build(value: &serde_json::Value) -> BoundedDecimal {
    let mut number =
        BoundedDecimal::decimal(Decimal::from_str(value["initial"].as_str().unwrap()).unwrap());
    for operation in value["operations"].as_array().unwrap() {
        number = if let Some(text) = operation["multiply"].as_str() {
            number.multiply(Decimal::from_str(text).unwrap()).unwrap()
        } else {
            number
                .divide_power_of_ten(operation["divide_power_of_ten"].as_u64().unwrap() as u32)
                .unwrap()
        };
    }
    number
}
#[test]
fn independent_fraction_ratios_match_every_declared_binary64_bit() {
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/bounded-decimal-ratios.json")).unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 16);
    for case in cases {
        let numerator = build(&case["numerator"]);
        let denominator = build(&case["denominator"]);
        let actual = numerator.ratio_as_f64(&denominator);
        if case["expected"]["status"] == "finite" {
            let wanted =
                u64::from_str_radix(case["expected"]["bits"].as_str().unwrap(), 16).unwrap();
            assert_eq!(actual.unwrap().to_bits(), wanted, "{}", case["id"]);
        } else {
            assert!(actual.is_err(), "{}", case["id"]);
        }
    }
}
