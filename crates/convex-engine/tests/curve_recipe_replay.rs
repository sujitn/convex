#[path = "../../convex-ports/tests/support/snapshot_fixtures.rs"]
mod fixture;
#[path = "support/curve_replay.rs"]
mod support;
use convex_core::{contracts::*, Date};
use convex_curves::{contracts::*, Compounding, RateCurveDyn};
use convex_engine::{curve_replay::*, BuiltCurve, CalculationGraph};
use convex_ext_file::FileSnapshotRepository;
use convex_ports::{snapshot::*, TraitError};
use std::sync::Arc;
use support::*;

#[test]
fn original_schema_one_identity_and_canonical_bytes_survive() {
    let snapshot = MarketSnapshot::new(fixture::content()).unwrap();
    let before =
        include_str!("../../../tests/fixtures/library/curve-replay-schema-one.json").trim_end();
    assert_eq!(snapshot.canonical_bytes().unwrap(), before.as_bytes());
    assert_eq!(snapshot.content().schema_version, 1);
    assert!(snapshot.content().curve_recipes.is_empty());
}

#[test]
fn complete_original_native_pack_roundtrips_and_replays_exactly() {
    let cases = cases();
    assert_eq!(cases.len(), 2563);
    let mut successes = 0;
    let mut failures = 0;
    for case in cases {
        let run = || -> Result<(), Box<dyn std::error::Error>> {
            let recipes = case["request"]["inputs"]["curves"].as_array().unwrap();
            let snapshot =
                MarketSnapshot::new(content(recipes.iter().cloned().map(record).collect()))?;
            let reopened: MarketSnapshot = serde_json::from_slice(&snapshot.canonical_bytes()?)?;
            assert_eq!(snapshot, reopened, "{}", case["id"]);
            let built = replay_snapshot_curves(&reopened)?;
            let original: Vec<CurveRecipe> = recipes
                .iter()
                .cloned()
                .map(serde_json::from_value)
                .collect::<Result<_, _>>()?;
            let direct = build_curve_set(&original, &CurveSet::new())?;
            assert_eq!(built.build_order, direct.build_order);
            for recipe in &original {
                assert_eq!(built.curves.get(&recipe.key)?.recipe(), recipe);
                assert_eq!(
                    built.curves.get(&recipe.key)?.diagnostics(),
                    direct.curves.get(&recipe.key)?.diagnostics()
                );
            }
            let operation = &case["request"]["operation"];
            if operation["kind"] == "curve_query" {
                let key: CurveKey = serde_json::from_value(operation["curve"].clone())?;
                let query: CurveQuery = serde_json::from_value(operation["query"].clone())?;
                let actual = built.curves.get(&key)?.query(&query)?;
                assert_eq!(actual, direct.curves.get(&key)?.query(&query)?);
                assert!(actual.value.is_finite());
            }
            Ok(())
        };
        let actual = run();
        assert_eq!(
            actual.is_ok(),
            case["expected_error"].is_null(),
            "{}: {:?}",
            case["id"],
            actual
        );
        if actual.is_ok() {
            successes += 1;
        } else {
            failures += 1;
        }
    }
    assert_eq!((successes, failures), (2208, 355));
}

#[tokio::test]
async fn actual_file_reopen_preserves_recipe_quotes_models_and_queries() {
    let dir = tempfile::tempdir().unwrap();
    let repository = FileSnapshotRepository::new(dir.path().join("snapshots")).unwrap();
    let snapshot = MarketSnapshot::new(content(vec![record(recipe())])).unwrap();
    repository.publish(&snapshot).await.unwrap();
    drop(repository);
    let reopened = FileSnapshotRepository::new(dir.path().join("snapshots"))
        .unwrap()
        .get(snapshot.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.canonical_bytes().unwrap(),
        reopened.canonical_bytes().unwrap()
    );
    assert_eq!(reopened.content().curve_recipes[0].recipe, recipe());
    let built = replay_snapshot_curves(&reopened).unwrap();
    let key: CurveKey = serde_json::from_value(recipe()["key"].clone()).unwrap();
    let native = built.curves.get(&key).unwrap();
    let curve =
        BuiltCurve::from_recipe(&reopened.content().curve_recipes[0], native.clone()).unwrap();
    let date = Date::parse("2028-10-02").unwrap();
    assert_eq!(
        curve.date_to_tenor(date).to_bits(),
        native.date_to_tenor(date).unwrap().to_bits()
    );
    assert_ne!(curve.date_to_tenor(date), 731.0 / 365.0);
    for t in [0.0, 0.25, 1.0, 731.0 / 360.0, 5.0, 149.0] {
        assert_eq!(
            curve.discount_factor(t).unwrap().to_bits(),
            native
                .query(&CurveQuery::DiscountFactor {
                    point: CurvePoint::Tenor(t)
                })
                .unwrap()
                .value
                .to_bits()
        );
    }
    assert!(curve.zero_rate(0.0, Compounding::Continuous).is_err());
    assert!(curve.instantaneous_forward(1.0).is_err());
    assert!(curve.to_points().is_err());
    assert!(curve.points.is_empty());
}

#[test]
fn schema_versions_dependency_integrity_and_quality_fail_explicitly() {
    let record = record(recipe());
    let snapshot = MarketSnapshot::new(content(vec![record.clone()])).unwrap();
    assert!(snapshot.quality().complete);
    snapshot
        .require_execution_quality_at(snapshot.content().known_at)
        .unwrap();
    assert!(snapshot
        .require_execution_quality_at(fixture::time("2026-10-02T15:00:10.000000001Z"))
        .is_err());
    for index in 0..9 {
        let mut c = content(vec![record.clone()]);
        match index {
            0 => c.schema_version = 1,
            1 => c.curve_recipes[0].schema_version = 2,
            2 => c.curve_recipes[0].recorded_at = fixture::time("2026-10-02T15:00:00.000000001Z"),
            3 => c.curve_recipes[0].source_time = fixture::time("2026-10-02T15:00:01Z"),
            4 => c.curve_recipes.push(record.clone()),
            5 => c.curves.push(fixture::curve()),
            6 => c.curve_recipes[0].recipe["key"]["id"] = "other".into(),
            7 => c.curve_recipes[0].max_age_ms = 0,
            8 => c.curve_recipes[0]
                .input_versions
                .push(record.input_versions[0].clone()),
            _ => unreachable!(),
        }
        assert!(MarketSnapshot::new(c).is_err(), "{index}");
    }
    let mut stale = content(vec![record.clone()]);
    stale.curve_recipes[0].quality = DataQuality::Indicative;
    let stale = MarketSnapshot::new(stale).unwrap();
    assert_eq!(stale.quality().quality, DataQuality::Indicative);
    assert!(stale
        .require_execution_quality_at(stale.content().known_at)
        .is_err());
    let mut missing = content(vec![record.clone()]);
    missing
        .required_curves
        .push(convex_core::contracts::CurveId::new("missing").unwrap());
    assert!(!MarketSnapshot::new(missing).unwrap().quality().complete);
    for mutation in ["unknown", "missing_version", "unsupported_helpers"] {
        let mut record = record.clone();
        match mutation {
            "unknown" => record.recipe["ignored_terms"] = true.into(),
            "missing_version" => record.recipe["key"]
                .as_object_mut()
                .unwrap()
                .remove("version")
                .map(|_| ())
                .unwrap(),
            "unsupported_helpers" => {
                record.recipe["source"] = serde_json::json!({"Helpers":["original.irs.helper"]});
                record.recipe["calibration"] = "Piecewise".into();
            }
            _ => unreachable!(),
        }
        let snapshot = Arc::new(MarketSnapshot::new(content(vec![record])).unwrap());
        assert!(replay_snapshot_curves(&snapshot).is_err());
        assert!(CalculationGraph::new()
            .activate_snapshot(vec![], snapshot)
            .is_err());
    }
}

#[test]
fn dependency_metadata_identity_and_atomic_native_failures_survive() {
    let mut base = recipe();
    base["key"]["id"] = "base".into();
    let base_key = base["key"].clone();
    let mut derived = recipe();
    derived["dependencies"] = serde_json::json!([base_key]);
    derived["source"] = serde_json::json!({"DiscountEquivalent":{"base":base["key"]}});
    let records = vec![record(derived.clone()), record(base.clone())];
    let first = MarketSnapshot::new(content(records.clone())).unwrap();
    let mut reversed = records.clone();
    reversed.reverse();
    let second = MarketSnapshot::new(content(reversed)).unwrap();
    assert_eq!(first.id(), second.id());
    let original = replay_snapshot_curves(&first).unwrap();
    assert_eq!(original.build_order[0].id, "base");
    assert!(rebuild_curve_recipes(&records[..1]).is_err());
    let mut wrong = records.clone();
    wrong[1].recipe["key"]["version"] = "other".into();
    assert!(rebuild_curve_recipes(&wrong).is_err());
    let mut cyclic = records.clone();
    cyclic[1].recipe["dependencies"] = serde_json::json!([derived["key"]]);
    cyclic[1].recipe["source"] = serde_json::json!({"DiscountEquivalent":{"base":derived["key"]}});
    assert!(rebuild_curve_recipes(&cyclic).is_err());
    for field in ["version", "model_id", "source", "max_age_ms"] {
        let mut value = serde_json::to_value(&records[0]).unwrap();
        value[field] = match field {
            "version" => 8.into(),
            "max_age_ms" => 9999.into(),
            _ => "other".into(),
        };
        let changed: SnapshotCurveRecipe = serde_json::from_value(value).unwrap();
        let snapshot = MarketSnapshot::new(content(vec![changed, records[1].clone()])).unwrap();
        assert_ne!(first.id(), snapshot.id(), "{field}");
    }
    let mut bad = records.clone();
    bad[0].recipe["extrapolation"] = "unsupported".into();
    assert!(rebuild_curve_recipes(&bad).is_err());
    assert_eq!(original.curves.len(), 2);
    let mut wrong_record = records[0].clone();
    wrong_record.recipe["extrapolation"] = "Flat".into();
    let key: CurveKey = serde_json::from_value(derived["key"].clone()).unwrap();
    assert!(matches!(
        BuiltCurve::from_recipe(&wrong_record, original.curves.get(&key).unwrap().clone()),
        Err(TraitError::Conflict(_))
    ));
}

#[test]
fn failed_legacy_rebuild_clears_old_owner_and_never_substitutes_values() {
    let mut curve = BuiltCurve::from_snapshot(&fixture::curve()).unwrap();
    curve.points[0].1 = -0.01;
    assert!(curve.rebuild_inner().is_err());
    assert!(curve.inner.is_none());
    assert!(curve.interpolate_rate(1.0).is_nan());
    assert!(curve.discount_factor(1.0).is_err());
    assert!(curve.forward_rate(1.0, 2.0).is_err());
    let mut curve = BuiltCurve::from_snapshot(&fixture::curve()).unwrap();
    curve.points[0].0 = 0.1001;
    assert!(curve.to_points().is_err());
    let mut curve = BuiltCurve::from_snapshot(&fixture::curve()).unwrap();
    curve.points = vec![(3.0 / 365.0, 0.02), (100.0 / 365.0, 0.03)];
    curve.rebuild_inner().unwrap();
    assert_eq!(curve.to_points().unwrap(), vec![(3, 0.02), (100, 0.03)]);
    curve.points[0].1 = 0.025;
    assert!(curve.to_points().is_err());
    curve.points = vec![(0.1001, 0.02), (1.0, 0.03)];
    curve.rebuild_inner().unwrap();
    assert!(curve.to_points().is_err());
}
