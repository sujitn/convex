//! Retired optional terms cannot become successful straight-bond convenience inputs.
use convex_wasm::BondParams;
use serde_json::{json, Value};

fn fixed() -> Value {
    json!({"coupon_rate":5.0,"maturity_date":"2030-06-15","issue_date":"2020-06-15",
        "settlement_date":"2026-06-15","face_value":100.0,"frequency":2,"day_count":"30/360",
        "currency":"USD","first_coupon_date":null,"market":null,"instrument_type":null,
        "yield_convention":null,"compounding":null,"settlement_days":null,
        "ex_dividend_days":null,"use_business_days":null})
}

#[test]
fn retained_fixed_fields_roundtrip_without_loss() {
    let original = fixed();
    let decoded: BondParams = serde_json::from_value(original.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), original);
}

#[test]
fn retired_optional_and_unknown_terms_are_rejected() {
    for (name, value) in [
        ("call_schedule", Value::Null),
        ("call_schedule", json!([])),
        (
            "call_schedule",
            json!([{"date":"2028-06-15","price":101.5}]),
        ),
        ("volatility", Value::Null),
        ("volatility", json!(1.0)),
        ("cap", json!(5.0)),
        ("floor", json!(0.0)),
        ("notice_days", json!(7)),
    ] {
        let mut input = fixed();
        input[name] = value;
        assert!(
            serde_json::from_value::<BondParams>(input).is_err(),
            "{name} was silently admitted"
        );
    }
}
