#[path = "../../../tests/fixtures/library/product_parametric.rs"]
mod native;
#[path = "support/calibration_replay.rs"]
mod replay;
#[path = "support/runtime_setup.rs"]
mod runtime;
use convex::curve_contracts::{CurvePoint, CurveQuery};
use convex::library::*;
use convex_curves::RateCurveDyn;
use convex_engine::{calibration_replay::*, curve_replay::replay_snapshot_curves, RuntimeInput};
use convex_ports::snapshot::{MarketSnapshot, SnapshotRepository};
use serde_json::json;

#[tokio::test]
async fn complete_product_parameters_and_full_diagnostics_survive_file_reopen_and_risk() {
    let directory = tempfile::tempdir().unwrap();
    for case in native::cases() {
        let time = replay::timestamp("2026-10-07T12:00:00Z");
        let record = replay::sealed(case["inputs"].clone(), time);
        let snapshot =
            MarketSnapshot::new(replay::content(runtime::fixture::content(), record.clone()))
                .unwrap();
        let bytes = snapshot.canonical_bytes().unwrap();
        let repo = convex_ext_file::FileSnapshotRepository::new(directory.path()).unwrap();
        repo.publish(&snapshot).await.unwrap();
        drop(repo);
        let reopened = convex_ext_file::FileSnapshotRepository::new(directory.path())
            .unwrap()
            .get(snapshot.id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reopened.canonical_bytes().unwrap(), bytes);
        let curves = replay_snapshot_curves(&reopened).unwrap();
        assert!(curves.curves.keys().all(|key| curves
            .curves
            .get(key)
            .unwrap()
            .interpolation_grid()
            .is_err()));
        for key in curves.curves.keys() {
            let native = curves.curves.get(key).unwrap();
            let expected = native
                .query(&CurveQuery::InstantaneousForward {
                    point: CurvePoint::Tenor(0.5),
                })
                .unwrap()
                .value;
            let bridge = convex_engine::curve_replay::RecipeRateCurve::new(native.clone()).unwrap();
            assert_eq!(bridge.instantaneous_forward(0.5).unwrap(), expected);
        }
        let mut tampered = record.clone();
        tampered.expected["calibrations"][0]["solver"]["parameters"]["beta0"] = json!(0.5);
        assert!(replay_calibration(&tampered, time, time).is_err());
        tampered = record.clone();
        tampered.expected["calibrations"][0]["repriced_helpers"] = json!([]);
        assert!(replay_calibration(&tampered, time, time).is_err());
        assert_eq!(
            replay_calibration(&record, time, time)
                .unwrap()
                .diagnostics(),
            &record.expected
        );
        if case["id"] == "nelson_siegel.rate.positive" {
            let parsed = decode_calibration_inputs(&record.inputs).unwrap();
            let mut wire = native::risk_action(&case);
            wire["snapshot_id"] = json!(reopened.id());
            let action = decode_calibration_action(&parsed, &wire.to_string()).unwrap();
            let risk = replay_calibration_operation(&reopened, &action).unwrap();
            assert!(risk.is_object());
            wire["key"]["coordinate"]["quote_version"] = json!(99);
            let wrong = decode_calibration_action(&parsed, &wire.to_string()).unwrap();
            assert!(replay_calibration_operation(&reopened, &wrong).is_err());
            assert_eq!(
                risk,
                replay_calibration_operation(&reopened, &action).unwrap()
            );
        }
    }
}
#[tokio::test]
async fn actual_runtime_uses_exact_product_parametric_capture_and_recovery() {
    let mut setup = runtime::setup().await;
    let mut inputs = native::cases().remove(0)["inputs"].clone();
    let rate = &mut inputs["calibration"]["rates"][0]["recipe"];
    // Explicit fictional application mapping; parameters/quotes/terms unchanged.
    rate["curve"]["key"]["id"] = json!("USD-FIXTURE");
    for helper in rate["helpers"].as_array_mut().unwrap() {
        helper["terms"]["curve"]["id"] = json!("USD-FIXTURE");
    }
    let time = replay::timestamp("2026-10-07T12:00:00Z");
    setup.plan.assembly.as_of = time;
    setup.plan.assembly.known_at = time;
    setup.plan.assembly.curves.clear();
    setup.plan.assembly.calibration = Some(replay::sealed(inputs, time));
    for event in &mut setup.plan.seed_events {
        event.source_time_ms = time.datetime().timestamp_millis();
        event.receive_time_ms = event.source_time_ms;
    }
    setup.plan.settlement_date = convex_core::contracts::BusinessDate::parse("2026-10-07").unwrap();
    let engine = runtime::make_engine(
        &setup,
        runtime::archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    assert!(engine.runtime_health().ready);
    let original = engine.runtime_results();
    assert_eq!(original.len(), 1);
    assert_eq!(
        original[0].result.status(),
        convex_core::contracts::ResultStatus::Complete
    );
    let snapshot = setup
        .snapshots
        .get(&original[0].result.provenance().snapshot_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.content().calibration.as_ref().unwrap().expected["calibrations"][0]["solver"]
            ["kind"],
        "parametric"
    );
    engine.shutdown().await.unwrap();
    let reopened = runtime::make_engine(
        &setup,
        runtime::archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    reopened.start().await.unwrap();
    assert_eq!(reopened.runtime_results(), original);
    reopened.shutdown().await.unwrap();
}
