//! Canonical ingress, precision, signed-factor and result-state acceptance fixtures.

use convex_core::{contracts::*, Currency};
use proptest::prelude::*;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

fn exact(value: &str) -> ExactDecimal {
    ExactDecimal::parse(value).unwrap()
}
fn provenance() -> Provenance {
    Provenance {
        subject: ValuationSubject::InstrumentVersion {
            instrument_id: InstrumentId::new("internal-bond-1").unwrap(),
            version: Version::new(3).unwrap(),
        },
        snapshot_id: SnapshotId::new("snapshot-fixture").unwrap(),
        configuration_version: Version::new(7).unwrap(),
        valuation_time: UtcTimestamp::parse("2026-10-02T15:00:00Z").unwrap(),
        known_at: UtcTimestamp::parse("2026-10-02T15:00:00Z").unwrap(),
        calculated_at: UtcTimestamp::parse("2026-10-02T15:00:00.010Z").unwrap(),
        model_version: ModelId::new("fixture-model-v1").unwrap(),
        build_id: ModelId::new("fixture-build").unwrap(),
        input_versions: vec![],
    }
}
fn measures() -> BTreeMap<MeasureId, MeasureResult> {
    BTreeMap::from([(
        MeasureId::new("pv").unwrap(),
        MeasureResult {
            value: MeasureValue::Money(Money::new(exact("-1000150.00"), Currency::USD)),
            quality: DataQuality::Valid,
            method: ModelId::new("observed_dirty_mark").unwrap(),
            source_observation_id: Some(ObservationId::new("fixture-observation").unwrap()),
        },
    )])
}
fn error() -> StructuredError {
    StructuredError::new(
        ErrorCode::MissingFixing,
        Some("market_quote_pv01".into()),
        "Required known-at fixing is absent",
        false,
    )
    .unwrap()
}
fn factor() -> FactorDefinition {
    FactorDefinition {
        family: FactorFamily::InterestRate,
        target: FactorTarget::Curve {
            curve_id: CurveId::new("USD-SOFR-OIS").unwrap(),
        },
        currency: Currency::USD,
        tenor: FactorTenor::new("5Y").unwrap(),
        quote_type: QuoteType::OisParRate,
        basis: RiskBasis::MarketQuoteRecalibrated,
        convention_version: Version::new(2).unwrap(),
        model_version: ModelId::new("fixture-v1").unwrap(),
    }
}

#[test]
fn documented_wire_examples_roundtrip() {
    let partial: ResultEnvelope =
        serde_json::from_str(include_str!("fixtures/canonical_partial_result.json")).unwrap();
    assert_eq!(partial.status(), ResultStatus::Partial);
    let risk: RiskSensitivity =
        serde_json::from_str(include_str!("fixtures/canonical_risk.json")).unwrap();
    assert_eq!(risk.value().get(), -425.0);
    assert_eq!(risk.measure(), SignedRiskMeasure::Pv01Signed);
    let recovered: ResultEnvelope =
        serde_json::from_value(serde_json::to_value(&partial).unwrap()).unwrap();
    assert_eq!(recovered, partial);
    assert_eq!(
        serde_json::from_value::<RiskSensitivity>(serde_json::to_value(&risk).unwrap()).unwrap(),
        risk
    );
}

#[test]
fn duplicate_measures_and_oversized_results_fail_ingress() {
    let partial = include_str!("fixtures/canonical_partial_result.json");
    let duplicate = partial.replace("\"pv\": {", "\"pv\": {\"value\":{\"value\":\"1\",\"unit\":\"currency\",\"currency\":\"USD\"},\"quality\":\"Valid\",\"method\":\"fixture\",\"source_observation_id\":null}, \"pv\": {");
    assert!(serde_json::from_str::<ResultEnvelope>(&duplicate).is_err());
    let mut wire: serde_json::Value = serde_json::from_str(partial).unwrap();
    let measure = wire["results"]["pv"].clone();
    wire["results"] = serde_json::Value::Object(
        (0..4097)
            .map(|i| (format!("m{i}"), measure.clone()))
            .collect(),
    );
    assert!(serde_json::from_value::<ResultEnvelope>(wire).is_err());
}

#[test]
fn units_reject_interchange_and_ambiguous_wire_values() {
    let price = json!({"value":"100.00", "unit":"price_per_100"});
    let rate = json!({"value":"0.05", "unit":"decimal_rate"});
    let bp = json!({"value":"100.00", "unit":"bp"});
    let face = json!({"value":"1000000.00", "unit":"face_amount"});
    let contracts = json!({"value":"12", "unit":"futures_contracts"});
    assert!(serde_json::from_value::<PricePer100>(price.clone()).is_ok());
    assert!(serde_json::from_value::<DecimalRate>(rate.clone()).is_ok());
    assert!(serde_json::from_value::<BasisPoints>(bp.clone()).is_ok());
    assert!(serde_json::from_value::<FaceAmount>(face.clone()).is_ok());
    assert!(serde_json::from_value::<FuturesContracts>(contracts.clone()).is_ok());
    for (i, value) in [price, rate, bp, face, contracts].into_iter().enumerate() {
        assert_eq!(
            serde_json::from_value::<PricePer100>(value.clone()).is_ok(),
            i == 0
        );
        assert_eq!(
            serde_json::from_value::<DecimalRate>(value.clone()).is_ok(),
            i == 1
        );
        assert_eq!(
            serde_json::from_value::<BasisPoints>(value.clone()).is_ok(),
            i == 2
        );
        assert_eq!(
            serde_json::from_value::<FaceAmount>(value.clone()).is_ok(),
            i == 3
        );
        assert_eq!(
            serde_json::from_value::<FuturesContracts>(value).is_ok(),
            i == 4
        );
    }
    for value in [
        json!(100),
        json!("100"),
        json!({"value":"100"}),
        json!({"value":"100","unit":"percent"}),
        json!({"value":100,"unit":"price_per_100"}),
        json!({"value":"100","unit":"price_per_100","currency":"USD"}),
    ] {
        assert!(serde_json::from_value::<PricePer100>(value).is_err());
    }
    assert!(serde_json::from_value::<FuturesContracts>(
        json!({"value":1.25,"unit":"futures_contracts"})
    )
    .is_err());
    for count in [
        json!("1.25"),
        json!("9223372036854775808"),
        json!(12),
        json!("01"),
    ] {
        assert!(serde_json::from_value::<FuturesContracts>(
            json!({"value":count,"unit":"futures_contracts"})
        )
        .is_err());
    }
    assert!(
        serde_json::from_value::<FaceAmount>(json!({"value":"-1","unit":"face_amount"})).is_err()
    );
    assert!(serde_json::from_value::<Money>(json!({"value":"1","unit":"currency"})).is_err());
    assert!(serde_json::from_value::<Money>(
        json!({"value":"1","unit":"currency","currency":"ZZZ"})
    )
    .is_err());
}

#[test]
fn exact_precision_rounding_and_explicit_unit_conversion() {
    let precise = exact("12345678901234567890.12345678");
    assert_eq!(
        serde_json::to_value(precise).unwrap(),
        json!("12345678901234567890.12345678")
    );
    assert_eq!(
        serde_json::from_value::<ExactDecimal>(serde_json::to_value(precise).unwrap()).unwrap(),
        precise
    );
    for value in [
        "NaN",
        "Infinity",
        "1e2",
        "+1",
        "1_000",
        " 1",
        "1.",
        ".1",
        "0.00000000000000000000000000001",
        "79228162514264337593543950336",
    ] {
        assert!(ExactDecimal::parse(value).is_err(), "{value}");
    }
    let bp = BasisPoints::new(exact("-125"));
    assert_eq!(
        bp.to_decimal_rate().unwrap().value().decimal(),
        Decimal::new(-125, 4)
    );
    assert_eq!(
        bp.to_decimal_rate()
            .unwrap()
            .to_basis_points()
            .unwrap()
            .value(),
        bp.value()
    );
    assert!(BasisPoints::new(exact("0.0000000000000000000000000001"))
        .to_decimal_rate()
        .is_err());
    assert!(DecimalRate::new(ExactDecimal::new(Decimal::MAX))
        .to_basis_points()
        .is_err());
    let model = FiniteF64::new(1.125).unwrap();
    assert_eq!(
        ExactDecimal::from_model(
            model,
            DecimalRounding::new(2, RoundingMode::HalfEven).unwrap()
        )
        .unwrap()
        .decimal(),
        Decimal::new(112, 2)
    );
    assert_eq!(
        ExactDecimal::from_model(
            model,
            DecimalRounding::new(2, RoundingMode::HalfAwayFromZero).unwrap()
        )
        .unwrap()
        .decimal(),
        Decimal::new(113, 2)
    );
    assert!(DecimalRounding::new(29, RoundingMode::HalfEven).is_err());
    assert!(ExactDecimal::from_model(
        FiniteF64::new(f64::MAX).unwrap(),
        DecimalRounding::new(2, RoundingMode::TowardZero).unwrap()
    )
    .is_err());
    assert!(ExactDecimal::from_model(
        FiniteF64::new(f64::MIN_POSITIVE).unwrap(),
        DecimalRounding::new(28, RoundingMode::TowardZero).unwrap()
    )
    .is_err());
}

#[test]
fn nonfinite_values_fail_native_and_serde_ingress() {
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(FiniteF64::new(number).is_err());
        let de = serde::de::value::F64Deserializer::<serde::de::value::Error>::new(number);
        assert!(FiniteF64::deserialize(de).is_err());
    }
    for input in ["NaN", "Infinity", "1e309", "null", "\"NaN\""] {
        assert!(serde_json::from_str::<FiniteF64>(input).is_err());
    }
    assert_eq!(
        FiniteF64::new(-0.0).unwrap().get().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert!(FiniteF64::new(-0.005).is_ok());
    let pv = MeasureValue::ModelMoney(ModelMoney::new(
        FiniteF64::new(-123.456789).unwrap(),
        Currency::USD,
    ));
    assert_eq!(
        serde_json::from_value::<MeasureValue>(serde_json::to_value(&pv).unwrap()).unwrap(),
        pv
    );
    assert!(serde_json::from_str::<ModelMoney>(
        r#"{"value":1e309,"unit":"currency","currency":"USD"}"#
    )
    .is_err());
}

#[test]
fn dates_utc_precision_and_id_versions_fail_closed() {
    assert!(BusinessDate::parse("2024-02-29").is_ok());
    for date in [
        "2023-02-29",
        "2026-02-30",
        "2026-1-02",
        "0000-01-01",
        "2026-10-02T00:00:00Z",
    ] {
        assert!(BusinessDate::parse(date).is_err());
    }
    let timestamp = UtcTimestamp::parse("2026-10-02T15:00:00.123456789Z").unwrap();
    assert_eq!(
        serde_json::to_value(timestamp).unwrap(),
        json!("2026-10-02T15:00:00.123456789Z")
    );
    for time in [
        "2026-10-02T15:00:00",
        "2026-10-02T15:00:00+00:00",
        "2026-10-02T15:00:00.1234567891Z",
        "2026-02-30T15:00:00Z",
        "2026-10-02T25:00:00Z",
        "2016-12-31T23:59:60Z",
    ] {
        assert!(UtcTimestamp::parse(time).is_err(), "{time}");
    }
    for id in ["", " bond ", "a\n", "a secret", "🦀"] {
        assert!(InstrumentId::new(id).is_err());
    }
    assert!(InstrumentId::new("x".repeat(129)).is_err());
    assert!(InstrumentId::try_from(convex_core::ids::InstrumentId::new("")).is_err());
    let id = InstrumentId::new("internal-bond-1").unwrap();
    let legacy: convex_core::ids::InstrumentId = id.clone().into();
    assert_eq!(InstrumentId::try_from(legacy).unwrap(), id);
    assert!(serde_json::from_str::<Version>("0").is_err());
    assert!(serde_json::from_str::<SchemaVersion>("\"1.0\"").is_err());
    assert!(serde_json::from_str::<ValuationSubject>(
        r#"{"kind":"trade_version","trade_id":"t1","version":1,"quantity":12}"#
    )
    .is_err());
}

#[test]
fn factor_keys_separate_bases_and_signed_measure_labels() {
    let quote = RiskFactorKey::new(factor()).unwrap();
    let mut node = factor();
    node.quote_type = QuoteType::ZeroRate;
    node.basis = RiskBasis::ZeroNode;
    let zero = RiskFactorKey::new(node).unwrap();
    assert_ne!(quote, zero);
    let mut changed = factor();
    changed.model_version = ModelId::new("fixture-v2").unwrap();
    assert_ne!(quote, RiskFactorKey::new(changed).unwrap());
    for tenor in ["0Y", "01Y", "5y", "5.5Y", "", "5🦀"] {
        assert!(FactorTenor::new(tenor).is_err());
    }
    let mut invalid = factor();
    invalid.quote_type = QuoteType::CdsSpread;
    assert!(RiskFactorKey::new(invalid).is_err());
    let sensitivity = RiskSensitivity::new(
        quote.clone(),
        SignedRiskMeasure::Pv01Signed,
        FiniteF64::new(-425.0).unwrap(),
        Currency::USD,
        BasisPoints::new(exact("1")),
        RiskMethod::Central,
        SnapshotId::new("snapshot-fixture").unwrap(),
    )
    .unwrap();
    let mut value = serde_json::to_value(&sensitivity).unwrap();
    assert_eq!(value["measure"], "pv01_signed");
    assert_eq!(value["value"], -425.0);
    assert_eq!(value["unit"], "currency_per_bp");
    assert_eq!(
        serde_json::from_value::<RiskSensitivity>(value.clone()).unwrap(),
        sensitivity
    );
    value["measure"] = json!("cs01_signed");
    assert!(serde_json::from_value::<RiskSensitivity>(value).is_err());
    assert!(RiskSensitivity::new(
        quote,
        SignedRiskMeasure::Pv01Signed,
        FiniteF64::new(-425.0).unwrap(),
        Currency::USD,
        BasisPoints::new(exact("0")),
        RiskMethod::Central,
        SnapshotId::new("snapshot-fixture").unwrap()
    )
    .is_err());
}

#[test]
fn complete_partial_failed_status_and_lineage_are_checked() {
    let request = RequestId::new("request-fixture").unwrap();
    let complete = ResultEnvelope::new(
        request.clone(),
        ResultStatus::Complete,
        DataQuality::Demo,
        measures(),
        vec![],
        provenance(),
    )
    .unwrap();
    let partial = ResultEnvelope::new(
        request.clone(),
        ResultStatus::Partial,
        DataQuality::Invalid,
        measures(),
        vec![error()],
        provenance(),
    )
    .unwrap();
    let failed = ResultEnvelope::new(
        request.clone(),
        ResultStatus::Failed,
        DataQuality::Invalid,
        BTreeMap::new(),
        vec![error()],
        provenance(),
    )
    .unwrap();
    for envelope in [complete, partial, failed] {
        let value = serde_json::to_value(&envelope).unwrap();
        assert_eq!(
            serde_json::from_value::<ResultEnvelope>(value).unwrap(),
            envelope
        );
    }
    assert!(ResultEnvelope::new(
        request.clone(),
        ResultStatus::Complete,
        DataQuality::Valid,
        measures(),
        vec![error()],
        provenance()
    )
    .is_err());
    assert!(ResultEnvelope::new(
        request.clone(),
        ResultStatus::Complete,
        DataQuality::Valid,
        BTreeMap::new(),
        vec![],
        provenance()
    )
    .is_err());
    assert!(ResultEnvelope::new(
        request.clone(),
        ResultStatus::Partial,
        DataQuality::Invalid,
        BTreeMap::new(),
        vec![error()],
        provenance()
    )
    .is_err());
    assert!(ResultEnvelope::new(
        request.clone(),
        ResultStatus::Failed,
        DataQuality::Invalid,
        measures(),
        vec![error()],
        provenance()
    )
    .is_err());
    let mut value = serde_json::to_value(
        ResultEnvelope::new(
            request,
            ResultStatus::Complete,
            DataQuality::Valid,
            measures(),
            vec![],
            provenance(),
        )
        .unwrap(),
    )
    .unwrap();
    value["snapshot_id"] = json!("different-snapshot");
    assert!(serde_json::from_value::<ResultEnvelope>(value).is_err());
    let risk = RiskSensitivity::new(
        RiskFactorKey::new(factor()).unwrap(),
        SignedRiskMeasure::Pv01Signed,
        FiniteF64::new(-425.0).unwrap(),
        Currency::USD,
        BasisPoints::new(exact("1")),
        RiskMethod::Central,
        SnapshotId::new("other-snapshot").unwrap(),
    )
    .unwrap();
    let mut risk_measures = measures();
    risk_measures.insert(
        MeasureId::new("market_quote_pv01").unwrap(),
        MeasureResult {
            value: MeasureValue::Risk(risk),
            quality: DataQuality::Valid,
            method: ModelId::new("central").unwrap(),
            source_observation_id: None,
        },
    );
    assert!(ResultEnvelope::new(
        RequestId::new("request-fixture").unwrap(),
        ResultStatus::Complete,
        DataQuality::Valid,
        risk_measures,
        vec![],
        provenance()
    )
    .is_err());
    let mut late = provenance();
    late.known_at = UtcTimestamp::parse("2026-10-03T15:00:00Z").unwrap();
    assert!(late.validate().is_err());
    assert!(
        StructuredError::new(ErrorCode::UnknownOutcome, None, "Reconcile first", true).is_err()
    );
    assert_eq!(
        serde_json::to_value(error()).unwrap()["code"],
        "MISSING_FIXING"
    );
}

proptest! {
    #[test]
    fn exact_units_and_negative_rates_roundtrip(mantissa in -1_000_000_000_i64..1_000_000_000, scale in 0_u32..=12) {
        let decimal = ExactDecimal::new(Decimal::new(mantissa,scale));
        let rate = DecimalRate::new(decimal);
        let wire = serde_json::to_value(rate).unwrap();
        prop_assert_eq!(serde_json::from_value::<DecimalRate>(wire).unwrap(),rate);
        prop_assert_eq!(rate.to_basis_points().unwrap().to_decimal_rate().unwrap().value(),decimal);
    }
    #[test]
    fn integer_contracts_roundtrip_exactly(count in any::<i64>()) {
        let value = FuturesContracts::new(count);
        prop_assert_eq!(serde_json::from_value::<FuturesContracts>(serde_json::to_value(value).unwrap()).unwrap(),value);
    }
    #[test]
    fn arbitrary_text_never_panics_ingress(value in ".{0,160}") {
        let _ = UtcTimestamp::parse(&value);
        let _ = BusinessDate::parse(&value);
        let _ = FactorTenor::new(value.clone());
        let _ = InstrumentId::new(value.clone());
        let _ = ExactDecimal::parse(&value);
    }
}

#[cfg(feature = "schemars")]
#[test]
fn generated_contract_schemas_match_snapshot() {
    let schemas = convex_core::contracts::json_schemas();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/canonical-contracts.json")).unwrap();
    assert_eq!(serde_json::to_value(schemas).unwrap(), expected);
}
