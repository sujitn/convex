#![allow(dead_code)]
#[path = "../../../../tests/fixtures/library/calibration_replay.rs"]
pub mod native;
use convex_core::contracts::*;
use convex_engine::calibration_replay::*;
use convex_ports::{calibration::*, snapshot::*};
use serde_json::Value;

pub fn timestamp(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse(s).unwrap()
}
pub fn unsealed(inputs: Value, as_of: UtcTimestamp) -> SnapshotCalibration {
    let mut keys = Vec::new();
    for curve in inputs["curves"].as_array().unwrap() {
        keys.push(curve["key"].clone());
    }
    for family in ["rates", "credit", "cash"] {
        for entry in inputs["calibration"][family].as_array().unwrap() {
            let recipe = entry.get("recipe").unwrap_or(entry);
            keys.push(recipe["curve"]["key"].clone());
            if let Some(key) = recipe.pointer("/overnight_projection/key") {
                keys.push(key.clone());
            }
        }
    }
    SnapshotCalibration {
        schema_version: 1,
        graph_id: SourceId::new("fictional-complete-graph").unwrap(),
        version: Version::new(11).unwrap(),
        source_time: as_of,
        recorded_at: as_of,
        source: SourceId::new("caller-supplied-graph").unwrap(),
        model_id: ModelId::new("original-product-calibration-v1").unwrap(),
        input_versions: inputs["calendars"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| InputVersion {
                kind: SourceId::new("calendar").unwrap(),
                id: SourceId::new(c["pin"]["id"].as_str().unwrap()).unwrap(),
                version: Version::new(c["pin"]["version"].as_u64().unwrap()).unwrap(),
            })
            .collect(),
        quality: DataQuality::Valid,
        max_age_ms: 10_000,
        curves: keys
            .iter()
            .map(|key| CalibrationCurvePin {
                curve_id: CurveId::new(key["id"].as_str().unwrap()).unwrap(),
                definition_version: key["version"].as_str().unwrap().into(),
                version: Version::new(7).unwrap(),
            })
            .collect(),
        inputs,
        expected: serde_json::json!({}),
    }
}
pub fn sealed(inputs: Value, as_of: UtcTimestamp) -> SnapshotCalibration {
    capture_calibration(unsealed(inputs, as_of), as_of, as_of).unwrap()
}
pub fn runtime_capture() -> SnapshotCalibration {
    sealed(native::runtime_inputs(), timestamp("2026-10-02T15:00:00Z"))
}
pub fn content(mut content: SnapshotContent, capture: SnapshotCalibration) -> SnapshotContent {
    content.schema_version = 3;
    // This standalone graph consumes no R0 bond reference or market mark.
    content.references.make_mut().clear();
    content.quotes.clear();
    // Explicit fictional capture configuration, valid at each original fixture instant.
    content.configuration.created_at = 0;
    content.configuration.updated_at = 0;
    content.as_of = capture.source_time;
    content.known_at = capture.recorded_at;
    content.curves.clear();
    content.curve_recipes.clear();
    content.required_curves = capture.curves.iter().map(|p| p.curve_id.clone()).collect();
    content.calibration = Some(capture);
    content
}
