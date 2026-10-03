//! Legacy economic units and signs survive explicit canonical adapters.

use convex_analytics::{
    contracts::{CanonicalPricingValues, ValidatedBondSpec, ValidatedLegacyRisk},
    dto::*,
};
use convex_core::contracts::{ExactDecimal, FiniteF64};
use proptest::prelude::*;
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/legacy_contracts.json")).unwrap()
}

#[test]
fn all_legacy_bond_shapes_roundtrip_without_rate_conversion_or_term_loss() {
    for raw in fixture()["bonds"].as_array().unwrap() {
        let legacy: BondSpec = serde_json::from_value(raw.clone()).unwrap();
        // Compare the existing DTO's serialization, including its legacy defaults.
        let expected = serde_json::to_value(&legacy).unwrap();
        let validated = ValidatedBondSpec::try_from(legacy).unwrap();
        if let Some(coupon) = validated.coupon_rate() {
            assert_eq!(
                coupon.value().decimal().to_string(),
                raw["coupon_rate"].as_f64().unwrap().to_string()
            );
        }
        if let Some(spread) = validated.spread_bps() {
            assert_eq!(spread.value(), ExactDecimal::parse("-12.5").unwrap());
        }
        assert!(validated.face_amount().value().decimal() > rust_decimal::Decimal::ZERO);
        let wire = serde_json::to_value(&validated).unwrap();
        assert_eq!(wire, expected);
        let recovered: ValidatedBondSpec = serde_json::from_value(wire).unwrap();
        assert_eq!(
            serde_json::to_value(BondSpec::from(recovered)).unwrap(),
            expected
        );
    }
}

#[test]
fn pricing_roundtrip_preserves_bits_negative_yield_spread_and_optional_absence() {
    let expected: PricingResponse = serde_json::from_value(fixture()["pricing"].clone()).unwrap();
    let canonical = CanonicalPricingValues::try_from(expected.clone()).unwrap();
    let wire = serde_json::to_value(&canonical).unwrap();
    assert_eq!(wire["clean_price"]["unit"], "price_per_100");
    assert_eq!(wire["ytm_decimal"]["unit"], "decimal_rate");
    assert_eq!(wire["z_spread_bps"]["unit"], "bp");
    let recovered =
        PricingResponse::from(serde_json::from_value::<CanonicalPricingValues>(wire).unwrap());
    for (actual, original) in [
        (recovered.clean_price, expected.clean_price),
        (recovered.dirty_price, expected.dirty_price),
        (recovered.accrued, expected.accrued),
        (recovered.ytm_decimal, expected.ytm_decimal),
        (
            recovered.z_spread_bps.unwrap(),
            expected.z_spread_bps.unwrap(),
        ),
    ] {
        assert_eq!(actual.to_bits(), original.to_bits());
    }
    let mut without_spread = expected;
    without_spread.z_spread_bps = None;
    let canonical = CanonicalPricingValues::try_from(without_spread).unwrap();
    assert!(!serde_json::to_value(&canonical)
        .unwrap()
        .as_object()
        .unwrap()
        .contains_key("z_spread_bps"));
    assert!(PricingResponse::from(canonical).z_spread_bps.is_none());
}

#[test]
fn legacy_risk_retains_original_dv01_sign_and_basis() {
    let expected: RiskResponse = serde_json::from_value(fixture()["risk"].clone()).unwrap();
    let validated = ValidatedLegacyRisk::try_from(expected.clone()).unwrap();
    let wire = serde_json::to_value(&validated).unwrap();
    assert_eq!(wire, serde_json::to_value(&expected).unwrap());
    assert!(!wire.as_object().unwrap().contains_key("pv01_signed"));
    let recovered =
        RiskResponse::from(serde_json::from_value::<ValidatedLegacyRisk>(wire).unwrap());
    assert_eq!(recovered.dv01.to_bits(), expected.dv01.to_bits());
    assert_eq!(
        recovered.key_rates[0].duration.to_bits(),
        expected.key_rates[0].duration.to_bits()
    );
}

#[test]
fn compatibility_ingress_rejects_invalid_dates_quantities_and_model_numbers() {
    let mut invalid = fixture()["bonds"][0].clone();
    invalid["issue"] = json!("2025-02-30");
    assert!(serde_json::from_value::<ValidatedBondSpec>(invalid).is_err());
    let mut invalid = fixture()["bonds"][0].clone();
    invalid["issue"] = json!("2035-01-15");
    assert!(serde_json::from_value::<ValidatedBondSpec>(invalid).is_err());
    let mut invalid = fixture()["bonds"][0].clone();
    invalid["face_value"] = json!(-1);
    assert!(serde_json::from_value::<ValidatedBondSpec>(invalid).is_err());
    let mut bond: BondSpec = serde_json::from_value(fixture()["bonds"][1].clone()).unwrap();
    if let BondSpec::Callable(ref mut v) = bond {
        v.call_schedule[0].price = f64::NAN;
    }
    assert!(ValidatedBondSpec::try_from(bond).is_err());
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut pricing: PricingResponse =
            serde_json::from_value(fixture()["pricing"].clone()).unwrap();
        pricing.z_spread_bps = Some(number);
        assert!(CanonicalPricingValues::try_from(pricing).is_err());
        let mut risk: RiskResponse = serde_json::from_value(fixture()["risk"].clone()).unwrap();
        risk.dv01 = number;
        assert!(ValidatedLegacyRisk::try_from(risk).is_err());
    }
    let mut wire = serde_json::to_value(
        CanonicalPricingValues::try_from(
            serde_json::from_value::<PricingResponse>(fixture()["pricing"].clone()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    wire["ytm_decimal"]["unit"] = json!("percent");
    assert!(serde_json::from_value::<CanonicalPricingValues>(wire).is_err());
}

proptest! {
    #[test]
    fn native_pricing_adapters_preserve_every_finite_f64_bit(value in any::<f64>().prop_filter("finite", |v| v.is_finite())) {
        let original = PricingResponse { clean_price:value,dirty_price:value,accrued:value,ytm_decimal:value,z_spread_bps:Some(value) };
        let recovered = PricingResponse::from(CanonicalPricingValues::try_from(original).unwrap());
        prop_assert_eq!(recovered.clean_price.to_bits(),value.to_bits());
        prop_assert_eq!(recovered.ytm_decimal.to_bits(),value.to_bits());
        prop_assert_eq!(recovered.z_spread_bps.unwrap().to_bits(),value.to_bits());
        prop_assert_eq!(FiniteF64::new(value).unwrap().get().to_bits(),value.to_bits());
    }
}
