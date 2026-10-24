//! Frozen historical markets, fresh current risk, and durable date-roll replay.
#![cfg(unix)]
use convex_core::contracts::*;
use convex_engine::{
    core_workflow::CoreWorkflow,
    ports::{
        core_workflow::*, fixing::*, reference_master::*, reference_repository::*, snapshot::*,
    },
};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};

async fn seeded(
    directory: &Path,
    stale_current: bool,
    stale_history: bool,
) -> (Arc<CoreWorkflow>, CoreSnapshotPlan) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/library/date-rolls-core.json");
    let mut data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let (storage, results, fixings) =
        convex_ext_redb::create_redb_storage_with_core(directory.join("state.redb")).unwrap();
    for row in data["curves"].as_array_mut().unwrap() {
        if (stale_current && row["version"] == 4) || (stale_history && row["version"] == 1) {
            row["source_time"] = json!("2026-10-05T12:00:00Z");
        }
        storage
            .curves
            .save_recipe(&serde_json::from_value::<SnapshotCurveRecipe>(row.clone()).unwrap())
            .await
            .unwrap();
    }
    let observations: Vec<PublishedFixing> =
        serde_json::from_value(data["fixings"].clone()).unwrap();
    fixings.publish(&observations).await.unwrap();
    let definition: CoreDefinition = serde_json::from_value(data["definition"].clone()).unwrap();
    let mut plan: CoreSnapshotPlan = serde_json::from_value(data["plan"].clone()).unwrap();
    // The test database has one exact supplied definition; no old version is relabeled.
    plan.definition.version = Version::new(1).unwrap();
    storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("roll.test.seed").unwrap(),
                changes: vec![],
                master_changes: vec![MasterChange {
                    expected_version: None,
                    definition: MasterDefinition::Calculation(Box::new(definition)),
                }],
            },
            plan.known_at,
        )
        .await
        .unwrap();
    let core = Arc::new(CoreWorkflow::new(
        storage.bonds,
        storage.curves,
        fixings,
        Arc::new(
            convex_ext_file::FileSnapshotRepository::new(directory.join("snapshots")).unwrap(),
        ),
        results,
        ModelId::new("roll.test.build").unwrap(),
    ));
    (core, plan)
}

#[tokio::test]
async fn original_market_cutoffs_survive_roll_and_saved_recompute_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (core, plan) = seeded(dir.path(), false, false).await;
    let snapshot = core.assemble(plan).await.unwrap();
    assert!(snapshot.content.curves["market.0"][0].source_time < snapshot.content.plan.as_of);
    let receipt = core
        .run(snapshot.id.clone(), "risk.0".into(), false)
        .await
        .unwrap();
    let output = core.result(&receipt.result_id).await.unwrap();
    let value: Value = serde_json::from_str(&output.output).unwrap();
    assert_eq!(value["ok"], "true");
    assert_eq!(value["result"]["data"]["valuation"], "Matured");
    let pnl = core
        .run(snapshot.id.clone(), "pnl".into(), false)
        .await
        .unwrap();
    let original = core.result(&pnl.result_id).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&original.output).unwrap()["ok"],
        "true"
    );
    drop(core);
    let (_storage, results, _) =
        convex_ext_redb::create_redb_storage_with_core(dir.path().join("state.redb")).unwrap();
    let empty = tempfile::tempdir().unwrap();
    let (sources, _, _) =
        convex_ext_redb::create_redb_storage_with_core(empty.path().join("empty.redb")).unwrap();
    let recovered = CoreWorkflow::new(
        sources.bonds,
        sources.curves,
        Arc::new(convex_ext_file::JsonFixingRepository::empty()),
        Arc::new(
            convex_ext_file::FileSnapshotRepository::new(dir.path().join("snapshots")).unwrap(),
        ),
        results,
        ModelId::new("roll.test.build").unwrap(),
    );
    for (name, expected) in [("risk.0", output), ("pnl", original)] {
        let cached = recovered
            .run(snapshot.id.clone(), name.into(), false)
            .await
            .unwrap();
        assert!(cached.reused);
        assert_eq!(recovered.result(&cached.result_id).await.unwrap(), expected);
        let again = recovered
            .run(snapshot.id.clone(), name.into(), true)
            .await
            .unwrap();
        assert_eq!(recovered.result(&again.result_id).await.unwrap(), expected);
    }
}

#[tokio::test]
async fn stale_current_and_originally_stale_history_are_both_rejected() {
    for (current, history) in [(true, false), (false, true)] {
        let dir = tempfile::tempdir().unwrap();
        let (core, plan) = seeded(dir.path(), current, history).await;
        assert!(core.assemble(plan).await.is_err());
        assert_eq!(
            std::fs::read_dir(dir.path().join("snapshots"))
                .unwrap()
                .count(),
            0
        );
    }
}
