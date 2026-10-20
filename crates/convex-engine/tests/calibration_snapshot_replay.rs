#[path = "../../convex-ports/tests/support/snapshot_fixtures.rs"]
mod fixture;
#[path = "../../convex/examples/support/rate_fixture.rs"]
mod original_rates;
#[path = "support/calibration_replay.rs"]
mod support;
use convex::library::*;
use convex_core::Date;
use convex_curves::contracts::*;
use convex_engine::{
    calibration_replay::*, curve_replay::replay_snapshot_curves, CalculationGraph,
};
use convex_ports::{snapshot::*, TraitError};
use serde_json::{json, Value};
use support::*;

#[test]
fn original_schema_one_and_two_identity_and_bytes_remain_exact() {
    for bytes in [
        include_str!("../../../tests/fixtures/library/curve-replay-schema-one.json"),
        include_str!("../../../tests/fixtures/library/curve-replay-schema-two.json"),
    ] {
        let snapshot: MarketSnapshot = serde_json::from_str(bytes).unwrap();
        assert_eq!(
            snapshot.canonical_bytes().unwrap(),
            bytes.trim_end().as_bytes()
        );
        replay_snapshot_curves(&snapshot).unwrap();
        assert!(snapshot.content().calibration.is_none());
    }
}
#[test]
fn schema_three_rebuilds_full_helpers_against_independent_dated_deposits() {
    let capture = runtime_capture();
    let snapshot = MarketSnapshot::new(content(fixture::content(), capture.clone())).unwrap();
    let wire = snapshot.canonical_bytes().unwrap();
    let reopened: MarketSnapshot = serde_json::from_slice(&wire).unwrap();
    let built = replay_snapshot_curves(&reopened).unwrap();
    let key = CurveKey {
        id: "USD-FIXTURE".into(),
        version: "deposits.v1".into(),
    };
    let curve = built.curves.get(&key).unwrap();
    assert!(matches!(curve.recipe().source, CurveSource::Helpers(_)));
    assert_eq!(reopened.content().calibration.as_ref().unwrap(), &capture);
    assert_eq!(
        snapshot.id().as_str(),
        format!(
            "sha256:{}",
            content_digest(
                b"convex.snapshot.v3\0",
                &serde_json::to_vec(snapshot.content()).unwrap()
            )
        )
    );
    let controls: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/calibration-replay-reference.json"
    ))
    .unwrap();
    for case in controls["cases"].as_array().unwrap() {
        let point = CurvePoint::Date(Date::parse(case["date"].as_str().unwrap()).unwrap());
        let expected = case["discount_factor"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let actual = curve
            .query(&CurveQuery::DiscountFactor { point })
            .unwrap()
            .value;
        assert!((actual - expected).abs() <= controls["absolute_tolerance"].as_f64().unwrap());
    }
    let fits = &capture.expected["calibrations"];
    assert_eq!(fits.as_array().unwrap().len(), 1);
    assert_eq!(fits[0]["repriced_helpers"].as_array().unwrap().len(), 2);
    assert_eq!(fits[0]["solver"]["roots"].as_array().unwrap().len(), 2);
    assert_eq!(
        capture.inputs["calibration"]["rates"][0]["quote_versions"],
        json!([21, 22])
    );
}
#[test]
fn missing_terms_pins_calendars_and_tampered_fitted_residuals_fail_atomically() {
    let original = runtime_capture();
    let as_of = original.source_time;
    let mut variants = Vec::new();
    let mut c = original.clone();
    c.curves[0].definition_version = "wrong-version".into();
    variants.push(c);
    let mut c = original.clone();
    c.inputs["calendars"] = json!([]);
    variants.push(c);
    let mut c = original.clone();
    c.inputs["calibration"]["rates"][0]["recipe"]["helpers"][0]["terms"]
        .as_object_mut()
        .unwrap()
        .remove("calendar");
    variants.push(c);
    let mut c = original.clone();
    c.expected["repricing"] = json!([]);
    variants.push(c);
    let mut c = original.clone();
    c.expected["calibrations"][0]["repriced_helpers"] = json!([]);
    variants.push(c);
    let mut c = original.clone();
    c.inputs["calibration"]["rates"][0]["recipe"]["bootstrap"]["max_iterations"] = 1.into();
    variants.push(c);
    let mut c = original.clone();
    c.inputs["calibration"]["rates"][0]["recipe"]["curve"]["source"] = json!({"Nodes":[]});
    variants.push(c);
    let mut c = original.clone();
    c.inputs["calibration"]["rates"][0]["recipe"]["helpers"][0]["terms"]["terms"]
        ["ignored_principal"] = 100.into();
    variants.push(c);
    for c in variants {
        assert!(replay_calibration(&c, as_of, as_of).is_err());
        let snapshot = MarketSnapshot::new(content(fixture::content(), c)).unwrap();
        let graph = CalculationGraph::new();
        assert!(graph
            .activate_snapshot(vec![], std::sync::Arc::new(snapshot))
            .is_err());
    }
    assert_eq!(
        replay_calibration(&original, as_of, as_of)
            .unwrap()
            .diagnostics(),
        &original.expected
    );
    assert!(matches!(
        capture_calibration(original, as_of, as_of),
        Err(CalibrationReplayError::Record(TraitError::Conflict(_)))
    ));
}
#[test]
fn complete_original_risk_requests_replay_without_current_resources() {
    let mut successes = 0;
    let mut failures = 0;
    for case in native::original_cases() {
        let inputs = native::original_inputs(&case["request"]);
        let parsed = decode_calibration_inputs(&inputs).unwrap();
        let calibration = parsed.calibration.as_ref().unwrap();
        let date = calibration
            .rate_context
            .as_ref()
            .map(|c| c.as_of)
            .unwrap_or_else(|| {
                let date = calibration
                    .credit
                    .first()
                    .map(|c| c.helpers[0].context.valuation_date)
                    .unwrap_or_else(|| {
                        calibration
                            .cash
                            .first()
                            .map(|c| c.recipe.curve.reference_date)
                            .unwrap_or(parsed.curves[0].reference_date)
                    });
                timestamp(&format!("{date}T12:00:00Z"))
            });
        let date = if calibration.rates.is_empty()
            && calibration.credit.is_empty()
            && calibration.cash.is_empty()
        {
            let mut pending = vec![&case["request"]["operation"]["action"]];
            let mut original_date = None;
            while let Some(value) = pending.pop() {
                match value {
                    Value::Object(fields) => {
                        if let Some(value) = fields.get("valuation_date").and_then(Value::as_str) {
                            original_date = Some(value);
                            break;
                        }
                        pending.extend(fields.values());
                    }
                    Value::Array(values) => pending.extend(values),
                    _ => {}
                }
            }
            original_date
                .map(|date| timestamp(&format!("{date}T00:00:00Z")))
                .unwrap_or(date)
        } else {
            date
        };
        // Restated captures can know later revisions while preserving the
        // original native calculation's earlier cutoff. No revision is invented.
        let mut cutoff = date;
        for fixing in &parsed.fixings {
            cutoff = cutoff.max(fixing.known_at).max(fixing.published_at);
        }
        let mut pending = vec![&case["request"]["operation"]["action"]];
        while let Some(value) = pending.pop() {
            match value {
                Value::Object(fields) => {
                    for field in ["known_at", "published_at"] {
                        if let Some(time) = fields.get(field).and_then(Value::as_str) {
                            cutoff = cutoff.max(timestamp(time));
                        }
                    }
                    pending.extend(fields.values());
                }
                Value::Array(values) => pending.extend(values),
                _ => {}
            }
        }
        let mut draft = unsealed(inputs, date);
        draft.recorded_at = cutoff;
        let result = capture_calibration(draft, date, cutoff).and_then(|capture| {
            let snapshot = MarketSnapshot::new(content(fixture::content(), capture))?;
            let mut action = case["request"]["operation"]["action"].clone();
            if action.get("snapshot_id").is_some() {
                action["snapshot_id"] = json!(snapshot.id());
            }
            if let Some(input) = action.get_mut("input") {
                if input.get("snapshot_id").is_some() {
                    input["snapshot_id"] = json!(snapshot.id());
                }
            }
            let action = decode_calibration_action(&parsed, &action.to_string())?;
            let mut wrong_id = serde_json::to_value(&action)?;
            if wrong_id.get("snapshot_id").is_some() {
                wrong_id["snapshot_id"] = "original.fixture.wrong-snapshot".into();
                let wrong = decode_calibration_action(&parsed, &wrong_id.to_string())?;
                assert!(matches!(
                    replay_calibration_operation(&snapshot, &wrong),
                    Err(CalibrationReplayError::Record(TraitError::Conflict(
                        "risk operation claims a different snapshot"
                    )))
                ));
            }
            let expected = CalibrationReplay::build(&parsed)?.evaluate(&action)?;
            let result = replay_calibration_operation(&snapshot, &action)?;
            assert_eq!(result["result"], expected, "{}", case["id"]);
            Ok(())
        });
        if case["expected_error"].is_null() {
            assert!(result.is_ok(), "{}: {result:?}", case["id"]);
            successes += 1;
        } else {
            match result {
                Err(CalibrationReplayError::Native(error)) => {
                    assert_eq!(
                        serde_json::to_value(error.code).unwrap(),
                        case["expected_error"],
                        "{}",
                        case["id"]
                    );
                    assert!(!error.details.is_null(), "{}", case["id"]);
                }
                other => panic!(
                    "original native failure changed for {}: {other:?}",
                    case["id"]
                ),
            }
            failures += 1;
        }
    }
    assert_eq!((successes, failures), (18, 14));
}

#[test]
fn original_scaled_global_fit_preserves_complete_solver_and_native_work_failure() {
    let original = original_rates::Fixture::load("nonflat-positive").unwrap();
    let case = native::original_cases()
        .into_iter()
        .find(|c| c["id"] == "calibration.mixed_rates")
        .unwrap();
    let mut inputs = native::original_inputs(&case["request"]);
    let global = original.global(convex_curves::InterpolationMethod::Linear);
    for (entry, recipe) in inputs["calibration"]["rates"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(&global)
    {
        let old: convex_analytics::rate_calibration::RateCurveRecipe =
            serde_json::from_value(entry["recipe"].clone()).unwrap();
        assert_eq!(
            old,
            original
                .recipes
                .iter()
                .find(|r| r.curve.key == recipe.curve.key)
                .unwrap()
                .clone()
        );
        entry["recipe"] = serde_json::to_value(recipe).unwrap();
    }
    let date = timestamp(
        inputs["calibration"]["rate_context"]["as_of"]
            .as_str()
            .unwrap(),
    );
    let capture = sealed(inputs.clone(), date);
    let actual = replay_calibration(&capture, date, date).unwrap();
    let fits = actual.diagnostics()["calibrations"].as_array().unwrap();
    assert_eq!(fits.len(), global.len());
    for fit in fits {
        assert_eq!(fit["solver"]["kind"], "global");
        assert!(
            fit["solver"]["diagnostics"]["evaluations"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(fit["solver"]["diagnostics"]["rank"].as_u64().unwrap() > 0);
        assert_eq!(
            fit["repriced_helpers"].as_array().unwrap().len(),
            fit["recipe"]["helpers"].as_array().unwrap().len()
        );
    }
    inputs["calibration"]["rates"][0]["recipe"]["curve"]["calibration"]["Global"]
        ["max_iterations"] = 1.into();
    let original_error =
        CalibrationReplay::build(&decode_calibration_inputs(&inputs).unwrap()).unwrap_err();
    assert_eq!(original_error.code, LibraryErrorCode::NoConvergence);
    assert!(!original_error.details["solver"].is_null());
    match capture_calibration(unsealed(inputs, date), date, date) {
        Err(CalibrationReplayError::Native(error)) => assert_eq!(error, original_error),
        other => panic!("native global work diagnostic changed: {other:?}"),
    }
}

#[test]
fn original_due_history_failure_survives_the_capture_boundary() {
    let case = native::original_cases()
        .into_iter()
        .find(|c| c["id"] == "calibration.mixed_rates")
        .unwrap();
    let mut inputs = native::original_inputs(&case["request"]);
    inputs["fixings"] = json!([]);
    let date = timestamp(
        inputs["calibration"]["rate_context"]["as_of"]
            .as_str()
            .unwrap(),
    );
    let draft = unsealed(inputs, date);
    match capture_calibration(draft, date, date) {
        Err(CalibrationReplayError::Native(error)) => {
            assert_eq!(error.code, LibraryErrorCode::MissingInput);
            assert!(!error.details.is_null());
        }
        other => panic!("original missing-history diagnostic was lost: {other:?}"),
    }
}
#[tokio::test]
async fn actual_file_reopen_and_failed_operation_leave_original_graph_unchanged() {
    use convex_ports::snapshot::SnapshotRepository;
    let snapshot = MarketSnapshot::new(content(fixture::content(), runtime_capture())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = convex_ext_file::FileSnapshotRepository::new(dir.path().join("snapshots")).unwrap();
    store.publish(&snapshot).await.unwrap();
    drop(store);
    let store = convex_ext_file::FileSnapshotRepository::new(dir.path().join("snapshots")).unwrap();
    let reopened = store.get(snapshot.id()).await.unwrap().unwrap();
    let before = replay_calibration_operation(&reopened, &CalibrationOperation::Build {}).unwrap();
    let mut bad = reopened.content().clone();
    bad.calibration.as_mut().unwrap().expected["build_order"] = json!([]);
    let bad = MarketSnapshot::new(bad).unwrap();
    assert!(replay_snapshot_curves(&bad).is_err());
    assert_eq!(
        replay_calibration_operation(&reopened, &CalibrationOperation::Build {}).unwrap(),
        before
    );
    assert_eq!(store.get(snapshot.id()).await.unwrap().unwrap(), snapshot);
    let mut invalid = snapshot.content().clone();
    let mut unrelated = fixture::curve();
    unrelated.curve_id = convex_core::contracts::CurveId::new("unrelated-original-record").unwrap();
    unrelated.extrapolation = ZeroExtrapolation::FlatForward;
    invalid.required_curves.push(unrelated.curve_id.clone());
    invalid.curves.push(unrelated);
    let invalid = MarketSnapshot::new(invalid).unwrap();
    assert!(replay_calibration_operation(&invalid, &CalibrationOperation::Build {}).is_err());
}
#[test]
fn later_known_history_wrong_economic_date_and_quality_deadlines_stay_explicit() {
    let original = runtime_capture();
    let time = original.source_time;
    let mut future = original.clone();
    future.inputs["fixings"] = json!([{"index_id":"USD.TEST","observation_date":"2026-10-02","rate":"0.02","published_at":"2026-10-02T16:00:00Z","known_at":"2026-10-02T16:00:00Z","revision":1,"status":"Final","source_id":"original"}]);
    assert!(matches!(
        replay_calibration(&future, time, time),
        Err(CalibrationReplayError::Record(TraitError::InvalidInput(
            "calibration input is later than captured cutoff"
        )))
    ));
    let mut wrong = original.clone();
    wrong.inputs["calibration"]["rate_context"]["cashflows"]["fixings"]["valuation_date"] =
        "2026-10-03".into();
    assert!(replay_calibration(&wrong, time, time).is_err());
    let snapshot = MarketSnapshot::new(content(fixture::content(), original)).unwrap();
    snapshot.require_execution_quality_at(time).unwrap();
    assert!(snapshot
        .require_execution_quality_at(timestamp("2026-10-02T15:00:10.000000001Z"))
        .is_err());
    let mut bad = snapshot.content().clone();
    bad.curves = fixture::content().curves;
    assert!(MarketSnapshot::new(bad).is_err());
    let mut bad = snapshot.content().clone();
    bad.schema_version = 2;
    assert!(MarketSnapshot::new(bad).is_err());
}
