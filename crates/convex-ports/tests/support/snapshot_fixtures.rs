//! Fictional resolved fixtures; never production construction defaults.
#![allow(dead_code)] // Suites use different subsets of these shared constructors.

use convex_core::contracts::*;
use convex_ports::{
    fixing::*, quote_event::*, reference_data::BondReferenceData, reference_master::*,
    reference_repository::ReferenceRecord, snapshot::*, storage::*,
};
use std::collections::BTreeMap;

#[path = "reference_terms.rs"]
pub mod reference_terms;

pub fn time(value: &str) -> UtcTimestamp {
    UtcTimestamp::parse(value).unwrap()
}
pub fn version(value: u64) -> Version {
    Version::new(value).unwrap()
}
pub fn bond() -> BondReferenceData {
    serde_json::from_str(include_str!("../fixtures/reference_bond.json")).unwrap()
}
pub fn config() -> BondPricingConfig {
    serde_json::from_str(include_str!("../fixtures/pricing_config.json")).unwrap()
}
pub fn event(sequence: u64, source_ms: i64, receive_ms: i64, mid: &str) -> QuoteEvent {
    QuoteEvent {
        schema_version: 1,
        key: QuoteKey {
            instrument_id: bond().instrument_id,
            source: "feed-a".into(),
        },
        session: "session-1".into(),
        sequence,
        source_time_ms: source_ms,
        receive_time_ms: receive_ms,
        quality: QuoteQuality::Live,
        entitled: true,
        entitlement_tags: vec!["fictional-research".into()],
        evidence_reference: Some("fixture/quote".into()),
        event: QuoteEventKind::Image {
            fields: vec![QuoteChange {
                field: QuoteField::MidPrice,
                update: FieldUpdate::Set(QuoteValue {
                    value: ExactDecimal::parse(mid).unwrap().decimal(),
                    unit: QuoteUnit::PricePer100,
                }),
            }],
        },
    }
}
pub fn quote_requirement() -> QuoteRequirement {
    QuoteRequirement {
        key: QuoteKey {
            instrument_id: bond().instrument_id,
            source: "feed-a".into(),
        },
        fields: vec![QuoteField::MidPrice],
        max_age_ms: 10_000,
    }
}
pub fn fixing_key() -> FixingKey {
    FixingKey {
        index_id: IndexId::new("USD-SOFR").unwrap(),
        observation_date: BusinessDate::parse("2026-10-01").unwrap(),
        source: SourceId::new("administrator").unwrap(),
    }
}
pub fn fixing(revision: u64, published: &str, recorded: &str, rate: &str) -> PublishedFixing {
    PublishedFixing {
        schema_version: 1,
        key: fixing_key(),
        version: version(revision),
        rate: DecimalRate::new(ExactDecimal::parse(rate).unwrap()),
        published_at: time(published),
        recorded_at: time(recorded),
        is_preliminary: false,
        entitled: true,
        evidence_reference: format!("fixture/fixing/{revision}"),
    }
}
pub fn curve() -> SnapshotCurve {
    SnapshotCurve {
        curve_id: CurveId::new("USD-FIXTURE").unwrap(),
        version: version(1),
        reference_date: BusinessDate::parse("2026-10-02").unwrap(),
        source_time: time("2026-10-02T15:00:00Z"),
        recorded_at: time("2026-10-02T15:00:00Z"),
        source: SourceId::new("supplied-fixture").unwrap(),
        model_id: ModelId::new("supplied-zero-nodes-v1").unwrap(),
        input_versions: vec![],
        quality: DataQuality::Valid,
        max_age_ms: 10_000,
        interpolation: ZeroInterpolation::MonotoneConvex,
        extrapolation: ZeroExtrapolation::FlatForward,
        points: vec![1.0, 2.0, 5.0, 10.0]
            .into_iter()
            .map(|tenor| ZeroRatePoint {
                tenor_years: FiniteF64::new(tenor).unwrap(),
                continuous_zero: FiniteF64::new(0.04).unwrap(),
            })
            .collect(),
    }
}
pub fn content() -> SnapshotContent {
    let bond = bond();
    let resources: Vec<_> = reference_terms::resources(&bond)
        .into_iter()
        .map(|change| MasterRecord {
            schema_version: SchemaVersion::V2,
            version: version(1),
            recorded_at: time("2025-01-01T00:00:00Z"),
            definition: change.definition,
        })
        .collect();
    let as_of = time("2026-10-02T15:00:00Z");
    let event = event(
        1,
        as_of.datetime().timestamp_millis(),
        as_of.datetime().timestamp_millis(),
        "99.25",
    );
    let fields = BTreeMap::from([(
        QuoteField::MidPrice,
        FieldObservation {
            value: QuoteValue {
                value: ExactDecimal::parse("99.25").unwrap().decimal(),
                unit: QuoteUnit::PricePer100,
            },
            source_time_ms: event.source_time_ms,
            receive_time_ms: event.receive_time_ms,
            sequence: 1,
        },
    )]);
    SnapshotContent {
        schema_version: 1,
        as_of,
        known_at: as_of,
        generation: version(1),
        configuration: config(),
        configuration_capture: None,
        references: vec![ResolvedBondReference {
            record: ReferenceRecord {
                schema_version: SchemaVersion::V2,
                instrument_id: bond.instrument_id.clone(),
                version: version(1),
                recorded_at: time("2025-01-01T00:00:00Z"),
                bond: Some(bond),
            },
            issuer: resources[0].clone(),
            calendar: resources[1].clone(),
            convention: resources[2].clone(),
        }],
        quotes: vec![SnapshotQuote {
            requirement: quote_requirement(),
            delivery_watermark: Some(event.clone()),
            image: Some(QuoteSnapshot {
                watermark: event,
                fields,
                state: DataState::Live,
            }),
        }],
        fixings: vec![],
        required_curves: vec![curve().curve_id],
        curves: vec![curve()],
        max_source_skew_ms: 1000,
    }
}
pub fn snapshot() -> MarketSnapshot {
    MarketSnapshot::new(content()).unwrap()
}
