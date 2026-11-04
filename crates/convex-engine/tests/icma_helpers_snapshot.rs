//! Full contextual recipes, sources and diagnostics survive real file reopen.
#[path = "../../../tests/fixtures/library/icma_helpers_contract.rs"]
mod controls;
#[path = "../../convex-ports/tests/support/snapshot_fixtures.rs"]
mod fixture;
#[path = "support/calibration_replay.rs"]
mod support;
use convex::library::*;
use convex_engine::{calibration_replay::*, curve_replay::replay_snapshot_curves};
use convex_ext_file::FileSnapshotRepository;
use convex_ports::snapshot::{MarketSnapshot, SnapshotRepository};
use serde_json::json;

#[tokio::test]
async fn original_icma_helper_graph_and_five_pv_risk_survive_file_reopen() {
    let case = controls::cases()
        .into_iter()
        .find(|c| c["id"] == "icma_helpers.graph.positive.risk.admitted.terms.role")
        .unwrap();
    let mut inputs = case["request"]["inputs"].clone();
    inputs["calibration"] = case["request"]["operation"]["input"].clone();
    let as_of = support::timestamp("2024-01-10T12:00:00Z");
    let mut draft = support::unsealed(inputs.clone(), as_of);
    // Every original contextual source is additionally retained in capture
    // provenance. Shared revisions are recorded once under their exact kind/id.
    let mut pins = std::collections::BTreeMap::new();
    for entry in inputs["calibration"]["cash"].as_array().unwrap() {
        for h in entry["recipe"]["helpers"].as_array().unwrap() {
            let i = &h["input"]["quote_kind"]["input"];
            for (kind, pin) in [
                ("security", &i["sources"]["security"]),
                ("quote", &i["sources"]["quote_revision"]),
                ("quotation_policy", &i["sources"]["quotation_policy"]),
                ("nominal_context", &i["reference_context"]["revision"]),
            ] {
                pins.insert(
                    (kind, pin["id"].as_str().unwrap()),
                    pin["version"].as_u64().unwrap(),
                );
            }
        }
    }
    draft
        .input_versions
        .extend(pins.into_iter().map(|((kind, id), version)| {
            convex_core::contracts::InputVersion {
                kind: convex_core::contracts::SourceId::new(kind).unwrap(),
                id: convex_core::contracts::SourceId::new(id).unwrap(),
                version: convex_core::contracts::Version::new(version).unwrap(),
            }
        }));
    let capture = capture_calibration(draft, as_of, as_of).unwrap();
    assert_eq!(capture.inputs, inputs);
    assert!(capture.expected["calibrations"][0]["repriced_helpers"][0]["Bond"]["icma"].is_object());
    let snapshot =
        MarketSnapshot::new(support::content(fixture::content(), capture.clone())).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let repository = FileSnapshotRepository::new(directory.path()).unwrap();
    repository.publish(&snapshot).await.unwrap();
    drop(repository);
    let reopened = FileSnapshotRepository::new(directory.path())
        .unwrap()
        .get(snapshot.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reopened.canonical_bytes().unwrap(),
        snapshot.canonical_bytes().unwrap()
    );
    assert_eq!(reopened.content().calibration.as_ref().unwrap(), &capture);
    let replayed = replay_snapshot_curves(&reopened).unwrap();
    assert_eq!(replayed.curves.len(), 2);
    let parsed = decode_calibration_inputs(&inputs).unwrap();
    let mut action = case["request"]["operation"]["action"].clone();
    action["snapshot_id"] = json!(snapshot.id());
    let native = decode_calibration_action(&parsed, &action.to_string()).unwrap();
    let result = replay_calibration_operation(&reopened, &native).unwrap();
    let mut actual = case.clone();
    actual["request"]["operation"]["action"] = action;
    controls::check(
        &actual,
        &json!({"ok":"true","result":{"data":result["result"]}}).to_string(),
    );
    // Any removal, stale context or original-work/diagnostic tampering fails;
    // native capture output is reproducibility evidence, never a financial oracle.
    for mutation in [
        "references",
        "calendar",
        "observed",
        "work",
        "source_revision",
    ] {
        let mut changed = capture.clone();
        let i = &mut changed.inputs["calibration"]["cash"][0]["recipe"]["helpers"][0]["input"]
            ["quote_kind"]["input"];
        match mutation {
            "references" => i["reference_context"]["periods"] = json!([]),
            "calendar" => i["sources"]["calendars"]["payment"]["version"] = json!(99),
            "work" => i["max_cash_evaluations"] = json!(1),
            "source_revision" => i["sources"]["security"]["version"] = json!(99),
            _ => {
                changed.expected["calibrations"][0]["repriced_helpers"][0]["Bond"]["observed"]
                    ["icma"]["consideration"]["value"] = json!(0)
            }
        }
        assert!(
            replay_calibration(&changed, as_of, as_of).is_err(),
            "{mutation}"
        );
    }
    let final_native = replay_calibration(&capture, as_of, as_of).unwrap();
    assert_eq!(final_native.diagnostics(), &capture.expected);
}
