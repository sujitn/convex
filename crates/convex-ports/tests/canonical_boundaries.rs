//! Provider neutrality and publishing-unit compatibility at real port DTO boundaries.

use convex_ports::{output::BondQuoteOutput, reference_data::BondReferenceData};
use rust_decimal::Decimal;
use serde_json::json;

fn reference() -> BondReferenceData {
    serde_json::from_value(json!({
        "instrument_id":"internal-bond-1","isin":null,"cusip":null,"sedol":null,"bbgid":"fixture-vendor-a",
        "description":"Fictional boundary fixture","currency":"USD","issue_date":"2025-01-15","maturity_date":"2035-01-15",
        "coupon_rate":0.05375,"frequency":2,"day_count":"Thirty360US","face_value":100,"bond_type":"FixedBullet","issuer_type":"CorporateIG",
        "issuer_id":"fixture-issuer","issuer_name":"Fictional","seniority":"Senior","is_callable":false,"call_schedule":[],"is_putable":false,
        "is_sinkable":false,"floating_terms":null,"inflation_index":null,"inflation_base_index":null,"has_deflation_floor":false,
        "country_of_risk":"US","sector":"Fixture","amount_outstanding":null,"first_coupon_date":null,"last_updated":0,"source":"fixture-a"
    })).unwrap()
}
fn quote() -> BondQuoteOutput {
    // Option fields are absent on purpose; deserialization preserves them as None.
    serde_json::from_value(json!({
        "instrument_id":"internal-bond-1","currency":"USD","settlement_date":"2026-10-02",
        "clean_price_mid":98.5,"accrued_interest":-0.25,"ytm_mid":-0.005,
        "timestamp":0,"pricing_spec":"fixture","source":"fixture-a","is_stale":false,"quality":100
    }))
    .unwrap()
}

#[test]
fn supplier_symbol_remap_preserves_internal_identity_and_decimal_coupon() {
    let mut legacy = reference();
    let first = legacy.canonical_boundary().unwrap();
    legacy.bbgid = Some("fixture-vendor-b".into());
    legacy.source = "fixture-b".into();
    let remapped = legacy.canonical_boundary().unwrap();
    assert_eq!(first.instrument_id, remapped.instrument_id);
    assert_eq!(
        first.coupon.unwrap().value().decimal(),
        Decimal::new(5375, 5)
    );
    assert_eq!(first.face.value().decimal(), Decimal::from(100));
    let wire = serde_json::to_value(&first).unwrap();
    assert_eq!(wire["coupon"]["unit"], "decimal_rate");
    assert_eq!(wire["face"]["unit"], "face_amount");
    legacy.instrument_id = convex_core::ids::InstrumentId::new("");
    assert!(legacy.canonical_boundary().is_err());
    legacy = reference();
    legacy.face_value = Decimal::ZERO;
    assert!(legacy.canonical_boundary().is_err());
    legacy = reference();
    legacy.issue_date = legacy.maturity_date;
    assert!(legacy.canonical_boundary().is_err());
}

#[test]
fn publishing_retains_price_units_negative_yield_and_missing_accrued() {
    let mut legacy = quote();
    let values = legacy.canonical_mid_values().unwrap();
    assert_eq!(
        values.clean.unwrap().value().decimal(),
        Decimal::new(985, 1)
    );
    assert_eq!(
        values.dirty.unwrap().value().decimal(),
        Decimal::new(9825, 2)
    );
    assert_eq!(values.ytm.unwrap().value().decimal(), Decimal::new(-5, 3));
    legacy.accrued_interest = None;
    let values = legacy.canonical_mid_values().unwrap();
    assert!(values.clean.is_some());
    assert!(values.accrued.is_none());
    assert!(values.dirty.is_none());
    let wire = serde_json::to_value(values).unwrap();
    assert_eq!(wire["clean"]["unit"], "price_per_100");
    assert_eq!(wire["ytm"]["unit"], "decimal_rate");
    assert!(!wire.as_object().unwrap().contains_key("dv01"));
}

#[test]
fn exact_dirty_price_overflow_is_an_error() {
    let mut legacy = quote();
    legacy.clean_price_mid = Some(Decimal::MAX);
    legacy.accrued_interest = Some(Decimal::ONE);
    assert!(legacy.canonical_mid_values().is_err());
    legacy.accrued_interest = Some(Decimal::new(1, 28));
    assert!(legacy.canonical_mid_values().is_err());
}
