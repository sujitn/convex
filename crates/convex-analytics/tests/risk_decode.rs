//! Unapplied following controls; all independent declarations precede execution.
use convex_analytics::risk::{Convexity, Duration, KeyRateDurations, DV01};
use rust_decimal::Decimal;
use serde_json::{json, Value};

#[test]
fn independent_exact_risk_decode_cases_preserve_value_or_reject_rounding() {
    let reference: Value = serde_json::from_str(include_str!("fixtures/risk-decode.json")).unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 50);
    let mut captured = Vec::new();
    let (mut accepted, mut rejected) = (0, 0);
    for row in cases {
        let raw = row["input_json"].as_str().unwrap();
        let values = [
            (
                "Duration",
                serde_json::from_str::<Duration>(raw).map(|v| v.years()),
            ),
            (
                "Convexity",
                serde_json::from_str::<Convexity>(raw).map(|v| v.value()),
            ),
            ("DV01", serde_json::from_str::<DV01>(raw).map(|v| v.value())),
        ];
        for (risk_type, actual) in values {
            if row["expected"] == "success" {
                let value = actual.unwrap();
                let expected =
                    Decimal::from_str_exact(row["exact_decimal"].as_str().unwrap()).unwrap();
                assert_eq!(value, expected, "{} {risk_type}", row["id"]);
                captured.push(json!({"id":row["id"],"risk_type":risk_type,"actual_decimal":value.to_string(),"rejected":false}));
                accepted += 1;
            } else {
                assert!(actual.is_err(), "{} {risk_type}: {actual:?}", row["id"]);
                captured.push(json!({"id":row["id"],"risk_type":risk_type,"rejected":true}));
                rejected += 1;
            }
        }
    }
    assert_eq!(accepted, 72);
    assert_eq!(rejected, 78);
    println!("EXACT_RISK_DECODE_REFERENCE={}", json!(captured));
}

#[test]
fn healthy_signed_decimal_wire_representation_is_preserved() {
    for text in [
        "0",
        "-0",
        "4.50",
        "-4.5",
        "0.045",
        "0.0000000000000000000000000001",
    ] {
        let input = json!(text).to_string();
        let expected = Decimal::from_str_exact(text).unwrap();
        let duration: Duration = serde_json::from_str(&input).unwrap();
        let convexity: Convexity = serde_json::from_str(&input).unwrap();
        let dv01: DV01 = serde_json::from_str(&input).unwrap();
        let wire = serde_json::to_string(&expected).unwrap();
        assert_eq!(serde_json::to_string(&duration).unwrap(), wire);
        assert_eq!(serde_json::to_string(&convexity).unwrap(), wire);
        assert_eq!(serde_json::to_string(&dv01).unwrap(), wire);
        assert_eq!(duration.years(), expected);
        assert_eq!(convexity.value(), expected);
        assert_eq!(dv01.value(), expected);
    }
}

#[test]
fn nested_profiles_and_already_parsed_values_use_the_checked_owner() {
    for input in [json!("0.00000000000000000000000000001"), json!(1e-29)] {
        assert!(serde_json::from_value::<Duration>(input.clone()).is_err());
        assert!(serde_json::from_value::<Convexity>(input.clone()).is_err());
        assert!(serde_json::from_value::<DV01>(input.clone()).is_err());
        let profile = json!({"durations":[{"tenor":5.0,"duration":input}]});
        assert!(serde_json::from_value::<KeyRateDurations>(profile).is_err());
    }
    let value = json!({"durations":[{"tenor":5.0,"duration":"4.50"}]});
    let profile: KeyRateDurations = serde_json::from_value(value).unwrap();
    assert_eq!(
        profile.at_tenor(5.0).unwrap().duration.years(),
        Decimal::new(450, 2)
    );
}

#[test]
fn nonfinite_native_values_and_oversized_decimal_text_are_rejected() {
    use serde::de::value::{Error, F64Deserializer};
    use serde::Deserialize;
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e-29, -1e-29] {
        assert!(Duration::deserialize(F64Deserializer::<Error>::new(value)).is_err());
        assert!(Convexity::deserialize(F64Deserializer::<Error>::new(value)).is_err());
        assert!(DV01::deserialize(F64Deserializer::<Error>::new(value)).is_err());
    }
    let oversized = json!("0".repeat(8193)).to_string();
    assert!(serde_json::from_str::<Duration>(&oversized).is_err());
    assert!(serde_json::from_str::<Convexity>(&oversized).is_err());
    assert!(serde_json::from_str::<DV01>(&oversized).is_err());
}
