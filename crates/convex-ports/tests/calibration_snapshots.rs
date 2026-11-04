#[path = "support/snapshot_fixtures.rs"]
mod fixture;
use convex_core::contracts::*;
use convex_ports::{calibration::*, snapshot::*, TraitError};
use serde_json::json;
fn record() -> SnapshotCalibration {
    SnapshotCalibration {
        schema_version: 1,
        graph_id: SourceId::new("graph").unwrap(),
        version: fixture::version(11),
        source_time: fixture::time("2026-10-02T15:00:00Z"),
        recorded_at: fixture::time("2026-10-02T15:00:00Z"),
        source: SourceId::new("original").unwrap(),
        model_id: ModelId::new("model").unwrap(),
        input_versions: vec![],
        quality: DataQuality::Valid,
        max_age_ms: 10_000,
        curves: vec![CalibrationCurvePin {
            curve_id: CurveId::new("USD-FIXTURE").unwrap(),
            definition_version: "original.v1".into(),
            version: fixture::version(7),
        }],
        inputs: json!({"curves":[],"calibration":{"rates":[],"credit":[],"cash":[]}}),
        expected: json!({}),
    }
}
fn content(capture: SnapshotCalibration) -> SnapshotContent {
    let mut content = fixture::content();
    content.curves.clear();
    content.schema_version = 3;
    content.calibration = Some(capture);
    content
}
/// D68: a schema-3 identity is SHA-256 over the content's plain serialization.
#[test]
fn calibration_identity_is_the_hash_of_the_plain_content_bytes() {
    fixture::assert_plain_identity(&MarketSnapshot::new(content(record())).unwrap());
}

/// D79: schema 4 holds a calibration under the sectioned identity, and its wire form
/// verifies.
#[test]
fn schema_four_holds_a_calibration() {
    let mut c = content(record());
    c.schema_version = SNAPSHOT_SCHEMA;
    let snapshot = MarketSnapshot::new(c).unwrap();
    fixture::assert_sectioned_identity(&snapshot);
    let wire = snapshot.canonical_bytes().unwrap();
    assert_eq!(
        serde_json::from_slice::<MarketSnapshot>(&wire).unwrap(),
        snapshot
    );
}

#[test]
fn original_bounds_and_identity_pins_apply_to_the_whole_calibration() {
    let original = record();
    let snapshot = MarketSnapshot::new(content(original.clone())).unwrap();
    assert_eq!(snapshot.content().schema_version, 3);
    let mut c = original.clone();
    c.curves.push(c.curves[0].clone());
    assert!(c.validate().is_err());
    let mut c = original.clone();
    c.curves[0].definition_version = " ".into();
    assert!(c.validate().is_err());
    let mut c = original.clone();
    c.inputs["curves"] = json!([{"source":{"Nodes":vec![json!({});4096]}}]);
    assert!(c.validate().is_ok());
    let mut content = content(c);
    content.curve_recipes = vec![SnapshotCurveRecipe {
        schema_version: 1,
        curve_id: CurveId::new("other").unwrap(),
        version: fixture::version(8),
        source_time: original.source_time,
        recorded_at: original.recorded_at,
        source: original.source,
        model_id: original.model_id,
        input_versions: vec![],
        quality: DataQuality::Valid,
        max_age_ms: 10_000,
        recipe: json!({"key":{"id":"other"},"source":{"Nodes":[{}]}}),
    }];
    assert!(matches!(
        MarketSnapshot::new(content),
        Err(TraitError::CapacityExceeded)
    ));
}
#[test]
fn schema_metadata_time_resource_and_unknown_field_failures_stay_explicit() {
    let original = record();
    let mut c = content(original.clone());
    c.schema_version = 2;
    assert!(MarketSnapshot::new(c).is_err());
    let mut c = original.clone();
    c.schema_version = 2;
    assert!(c.validate().is_err());
    let mut c = original.clone();
    c.recorded_at = fixture::time("2026-10-02T15:00:00.000000001Z");
    assert!(MarketSnapshot::new(content(c)).is_err());
    let mut c = original.clone();
    c.inputs["ignored_text"] = json!("a".repeat(8193));
    assert!(matches!(c.validate(), Err(TraitError::CapacityExceeded)));
    let mut c = original.clone();
    let mut deep = json!({});
    for _ in 0..65 {
        deep = json!([deep]);
    }
    c.expected["deep"] = deep;
    assert!(matches!(c.validate(), Err(TraitError::CapacityExceeded)));
    let mut value = serde_json::to_value(original).unwrap();
    value["silent_default"] = json!(1);
    assert!(serde_json::from_value::<SnapshotCalibration>(value).is_err());
}
