//! Independent exact Decimal inputs and checked boundary admission.
use convex_core::contracts::{json, numeric_decimal};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct Scalar(#[serde(deserialize_with = "numeric_decimal::deserialize")] Decimal);

#[test]
fn independent_numeric_and_exact_string_cases_reject_erasure_and_rounding() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/numeric-decimal.json")).unwrap();
    assert_eq!(reference["absolute_bound"], "0");
    assert_eq!(reference["relative_bound"], "0");
    let mut observations = Vec::new();
    for row in reference["cases"].as_array().unwrap() {
        let parsed = json::parse(row["input_json"].as_str().unwrap())
            .and_then(|value| serde_json::from_value::<Scalar>(value).map_err(|e| e.to_string()));
        assert_eq!(
            parsed.is_ok(),
            row["expected"] == "ok",
            "{}: {parsed:?}",
            row["id"]
        );
        let value = parsed.ok().map(|value| value.0);
        if let Some(value) = value {
            assert_eq!(
                value,
                Decimal::from_str_exact(row["exact_decimal"].as_str().unwrap()).unwrap(),
                "{}",
                row["id"]
            );
        }
        observations.push(serde_json::json!({"id":row["id"],"available":value.is_some(),"exact_decimal":value.map(|value|value.to_string())}));
    }
    assert_eq!(
        observations.len(),
        reference["case_count"].as_u64().unwrap() as usize
    );
    println!(
        "NUMERIC_DECIMAL_OBSERVATIONS={}",
        serde_json::to_string(&observations).unwrap()
    );
}

#[test]
fn missing_null_and_present_zero_remain_distinct() {
    #[derive(Deserialize)]
    struct Optional {
        #[serde(default, deserialize_with = "numeric_decimal::optional::deserialize")]
        value: Option<Decimal>,
    }
    for text in ["{}", r#"{"value":null}"#] {
        assert!(serde_json::from_str::<Optional>(text)
            .unwrap()
            .value
            .is_none());
    }
    for text in [r#"{"value":0}"#, r#"{"value":"0"}"#] {
        assert_eq!(
            serde_json::from_str::<Optional>(text).unwrap().value,
            Some(Decimal::ZERO)
        );
    }
    for text in [r#"{"value":1e-100}"#, r#"{"value":"1e-100"}"#] {
        assert!(serde_json::from_str::<Optional>(text).is_err());
    }
}

#[test]
fn nonfinite_foreign_scalars_and_unbounded_diagnostic_context_are_rejected() {
    use serde::de::value::{Error, F64Deserializer};
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(numeric_decimal::deserialize(F64Deserializer::<Error>::new(value)).is_err());
    }
    const LONG_CONTEXT: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
    assert!(
        numeric_decimal::deserialize_named(F64Deserializer::<Error>::new(1.0), LONG_CONTEXT)
            .is_err()
    );
}
