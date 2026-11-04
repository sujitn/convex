//! Full explicit metadata roundtrips and rejected substitutions/ignored fields.
use convex_bonds::instruments::ZeroCouponBond;
use convex_core::types::Date;
use rust_decimal::Decimal;
use serde_json::{json, Value};

pub fn run() -> Value {
    let pack: Value = serde_json::from_str(include_str!("zero-terms-reference.json")).unwrap();
    for case in pack["positive_cases"].as_array().unwrap() {
        let input = case["terms"].clone();
        let bond: ZeroCouponBond = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(&bond).unwrap(),
            input,
            "{}",
            case["id"]
        );
        let text = serde_json::to_string(&bond).unwrap();
        let rebuilt: ZeroCouponBond = serde_json::from_str(&text).unwrap();
        assert_eq!(
            serde_json::to_value(rebuilt).unwrap(),
            input,
            "{}",
            case["id"]
        );
    }
    for case in pack["negative_cases"].as_array().unwrap() {
        assert!(
            serde_json::from_value::<ZeroCouponBond>(case["terms"].clone()).is_err(),
            "{} silently substitutes or omits supplied data",
            case["id"]
        );
    }
    assert!(
        serde_json::from_str::<ZeroCouponBond>(pack["duplicate_field_json"].as_str().unwrap())
            .is_err()
    );
    let boundaries: Value =
        serde_json::from_str(include_str!("zero-terms-boundaries.json")).unwrap();
    for case in boundaries["rejected_cases"].as_array().unwrap() {
        let mut terms = pack["positive_cases"][0]["terms"].clone();
        terms[case["field"].as_str().unwrap()] = case["value"].clone();
        assert!(
            serde_json::from_value::<ZeroCouponBond>(terms).is_err(),
            "{case}"
        );
    }
    // Metadata decoding and economic pricing admission remain separate owners.
    let mut raw = pack["positive_cases"][0]["terms"].clone();
    raw["face_value"] = json!("0");
    let invalid_economics: ZeroCouponBond = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(serde_json::to_value(&invalid_economics).unwrap(), raw);
    let settlement = Date::from_ymd(2026, 1, 1).unwrap();
    assert!(invalid_economics
        .price_from_yield(Decimal::ZERO, settlement)
        .is_err());
    assert!(invalid_economics
        .yield_from_price(Decimal::ONE_HUNDRED, settlement)
        .is_err());
    json!({"status":"passed","positive_roundtrips":151,"typed_rejections":40,"supplemental_exact_ingress_rejections":24,"certified":false})
}
