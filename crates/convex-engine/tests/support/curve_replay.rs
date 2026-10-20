#![allow(dead_code)]
use crate::fixture;

use convex_core::contracts::*;
use convex_ports::snapshot::*;

pub fn record(recipe: serde_json::Value) -> SnapshotCurveRecipe {
    SnapshotCurveRecipe {
        schema_version: 1,
        curve_id: CurveId::new(recipe["key"]["id"].as_str().unwrap()).unwrap(),
        version: fixture::version(7),
        source_time: fixture::time("2026-10-02T15:00:00Z"),
        recorded_at: fixture::time("2026-10-02T15:00:00Z"),
        source: SourceId::new("fictional-recipe-source").unwrap(),
        model_id: ModelId::new("fictional-native-recipe-v1").unwrap(),
        input_versions: vec![InputVersion {
            kind: SourceId::new("fixture-definition").unwrap(),
            id: SourceId::new("original-terms").unwrap(),
            version: fixture::version(17),
        }],
        quality: DataQuality::Valid,
        max_age_ms: 10_000,
        recipe,
    }
}

pub fn recipe() -> serde_json::Value {
    serde_json::json!({
        "key":{"id":"USD-FIXTURE","version":"supplied.recipe.v7"},
        "reference_date":"2026-10-02", "currency":"USD", "time_basis":"Act360",
        "role":{"kind":"Discount","collateral_currency":"USD"},
        "representation":{"ZeroRates":{"compounding":"Continuous"}},
        "interpolation":"FlatForward", "extrapolation":"FlatForward", "dependencies":[],
        "calibration":"Direct", "source":{"FlatForward":{"nodes":[
            {"pillar":{"Date":"2026-10-02"},"quote":{"unit":"PercentRate","value":2.3}},
            {"pillar":{"Date":"2027-10-02"},"quote":{"unit":"DecimalRate","value":0.023}},
            {"pillar":{"Date":"2028-10-02"},"quote":{"unit":"PercentRate","value":2.8}}
        ]}}
    })
}

pub fn content(records: Vec<SnapshotCurveRecipe>) -> SnapshotContent {
    let mut content = fixture::content();
    content.schema_version = 2;
    content.curves.clear();
    content.required_curves = records.iter().map(|r| r.curve_id.clone()).collect();
    content.curve_recipes = records;
    content
}

pub fn cases() -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(include_str!(
        "../../../../tests/fixtures/library/curve-replay.json"
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
