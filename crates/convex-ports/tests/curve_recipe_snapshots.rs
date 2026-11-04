#[path = "support/snapshot_fixtures.rs"]
mod fixture;
use convex_core::contracts::*;
use convex_ports::{snapshot::*, TraitError};

fn record() -> SnapshotCurveRecipe {
    SnapshotCurveRecipe {
        schema_version: 1,
        curve_id: CurveId::new("USD-FIXTURE").unwrap(),
        version: fixture::version(7),
        source_time: fixture::time("2026-10-02T15:00:00Z"),
        recorded_at: fixture::time("2026-10-02T15:00:00Z"),
        source: SourceId::new("fixture-source").unwrap(),
        model_id: ModelId::new("fixture-model").unwrap(),
        input_versions: vec![],
        quality: DataQuality::Valid,
        max_age_ms: 10_000,
        recipe: serde_json::json!({"key":{"id":"USD-FIXTURE","version":"actual-string-version"},"source":{"Nodes":[{},{}]}}),
    }
}
fn content(records: Vec<SnapshotCurveRecipe>) -> SnapshotContent {
    let mut c = fixture::content();
    c.schema_version = 2;
    c.curves.clear();
    c.required_curves = records.iter().map(|r| r.curve_id.clone()).collect();
    c.curve_recipes = records;
    c
}

/// D68: a schema-2 identity is SHA-256 over the content's plain serialization.
#[test]
fn recipe_identity_is_the_hash_of_the_plain_content_bytes() {
    fixture::assert_plain_identity(&MarketSnapshot::new(content(vec![record()])).unwrap());
}

/// D79: schema 4 holds recipes under the sectioned identity, and its wire form verifies.
#[test]
fn schema_four_holds_recipes() {
    let mut c = content(vec![record()]);
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
fn record_and_aggregate_bounds_are_shared_with_original_zero_records() {
    let mut r = record();
    r.recipe["source"]["Nodes"] = serde_json::json!(vec![serde_json::json!({}); 4096]);
    assert_eq!(r.input_count(), 4096);
    assert!(MarketSnapshot::new(content(vec![r.clone()])).is_ok());
    let mut extra = record();
    extra.curve_id = CurveId::new("other").unwrap();
    extra.recipe["key"]["id"] = "other".into();
    assert!(matches!(
        MarketSnapshot::new(content(vec![r, extra])),
        Err(TraitError::CapacityExceeded)
    ));
    let records: Vec<_> = (0..64)
        .map(|i| {
            let mut r = record();
            let id = format!("curve.{i}");
            r.curve_id = CurveId::new(&id).unwrap();
            r.recipe["key"]["id"] = id.into();
            r
        })
        .collect();
    let accepted = MarketSnapshot::new(content(records.clone())).unwrap();
    assert_eq!(accepted.content().curve_recipes.len(), 64);
    let mut too_many = content(records);
    too_many.curves.push(fixture::curve());
    assert!(matches!(
        MarketSnapshot::new(too_many),
        Err(TraitError::CapacityExceeded)
    ));
    let mut deep = record();
    let mut payload = serde_json::Value::Null;
    for _ in 0..65 {
        payload = serde_json::json!([payload]);
    }
    deep.recipe["nested"] = payload;
    assert!(matches!(deep.validate(), Err(TraitError::CapacityExceeded)));
    let mut long = record();
    long.recipe["text"] = "a".repeat(8193).into();
    assert!(matches!(long.validate(), Err(TraitError::CapacityExceeded)));
}

#[test]
fn schema_domain_identity_and_unchanged_legacy_content_are_explicit() {
    let legacy = MarketSnapshot::new(fixture::content()).unwrap();
    let wire: serde_json::Value =
        serde_json::from_slice(&legacy.canonical_bytes().unwrap()).unwrap();
    assert!(wire["content"].get("curve_recipes").is_none());
    let v2 = MarketSnapshot::new(content(vec![record()])).unwrap();
    let encoded = serde_json::to_vec(v2.content()).unwrap();
    assert_eq!(
        v2.id().as_str(),
        format!(
            "sha256:{}",
            content_digest(b"convex.snapshot.v2\0", &encoded)
        )
    );
    let mut wire = serde_json::to_value(v2).unwrap();
    wire["content"]["schema_version"] = 1.into();
    assert!(serde_json::from_value::<MarketSnapshot>(wire).is_err());
    let mut invalid = fixture::content();
    invalid.schema_version = 2;
    assert!(MarketSnapshot::new(invalid).is_err());
    let mut invalid = content(vec![record()]);
    invalid.curve_recipes[0].schema_version = 0;
    assert!(MarketSnapshot::new(invalid).is_err());
}
