//! A corporate's pricing rule in force against a set of benchmark notes, built in memory
//! for the level stage's bench and tests: no store and no Live runtime.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;

use convex_core::contracts::{ReferenceDataId, SnapshotId, Version};
use convex_core::ids::CurveId;
use convex_core::Date;
use convex_engine::curve_builder::BuiltCurve;
use convex_engine::level_pricing::{BenchmarkQuote, BenchmarkSetQuotes, PricingRuleInForce};
use convex_engine::ports::level_pricing::BenchmarkSecurity;
use convex_engine::ports::reference_data::{BondReferenceData, IssuerType};

pub const SETTLEMENT: &str = "2026-10-06";

/// Each benchmark note's Live clean price mid.
pub const BENCHMARK_CLEAN_PRICE: Decimal = dec!(98.25);

/// A USD discount curve at `reference`.
pub fn curve(reference: Date) -> BuiltCurve {
    let mut built = BuiltCurve {
        curve_id: CurveId::new("USD-SOFR"),
        reference_date: reference,
        points: vec![
            (0.5, 0.040),
            (2.0, 0.038),
            (5.0, 0.039),
            (10.0, 0.042),
            (30.0, 0.045),
        ],
        built_at: 0,
        inputs_hash: "fixture".to_string(),
        extrapolation: convex_curves::ExtrapolationMethod::Flat,
        inner: None,
    };
    built.rebuild_inner().unwrap();
    built
}

/// A corporate fixed bullet paying semi-annually.
pub fn corporate(id: &str, maturity: &str, coupon: &str) -> BondReferenceData {
    let mut bond: BondReferenceData = serde_json::from_str(include_str!(
        "../../../convex-ports/tests/fixtures/reference_bond.json"
    ))
    .unwrap();
    bond.instrument_id = convex_core::ids::InstrumentId::new(id);
    bond.maturity_date = Date::parse(maturity).unwrap();
    bond.coupon_rate = Some(coupon.parse().unwrap());
    bond
}

/// A government fixed bullet paying semi-annually, as a benchmark set holds one.
pub fn government_note(id: &str, maturity: &str, coupon: &str, cusip: &str) -> BondReferenceData {
    let mut note = corporate(id, maturity, coupon);
    note.issuer_type = IssuerType::Sovereign;
    note.cusip = Some(cusip.into());
    note
}

/// `CORP`'s rule quoting it by `method` against the `UST-OTR` set of `notes`, each with
/// its tenor and priced at [`BENCHMARK_CLEAN_PRICE`].
pub fn rule_in_force(
    method: serde_json::Value,
    notes: &[(BondReferenceData, &str)],
) -> PricingRuleInForce {
    let effective = json!({"from": "2025-01-01T00:00:00Z", "until": null});
    let provenance = json!({"source": "desk", "evidence_reference": "fixture"});
    let rule = serde_json::from_value(json!({
        "instrument_id": "CORP", "method": method, "benchmark_set_id": "UST-OTR",
        "effective": effective, "provenance": provenance,
    }))
    .unwrap();
    let securities: Vec<BenchmarkSecurity> = notes
        .iter()
        .map(|(note, tenor)| {
            serde_json::from_value(
                json!({"tenor": tenor, "instrument_id": note.instrument_id.as_str(), "version": 1}),
            )
            .unwrap()
        })
        .collect();
    let quotes = notes
        .iter()
        .zip(securities)
        .map(|((note, _), security)| BenchmarkQuote {
            security,
            bond: note.clone(),
            clean_price_mid: BENCHMARK_CLEAN_PRICE,
            snapshot_id: SnapshotId::new("sha256:fixture".to_owned()).unwrap(),
        })
        .collect();
    PricingRuleInForce {
        rule,
        rule_version: Version::new(1).unwrap(),
        benchmark_set: Some(BenchmarkSetQuotes {
            benchmark_set_id: ReferenceDataId::new("UST-OTR").unwrap(),
            version: Version::new(1).unwrap(),
            quotes,
        }),
    }
}
