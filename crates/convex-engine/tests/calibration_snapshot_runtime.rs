#[path = "support/configuration_setup.rs"]
mod configuration;
#[path = "support/calibration_replay.rs"]
mod replay;
use configuration::runtime::*;
use convex_core::contracts::*;
use convex_engine::{snapshot::SnapshotAssembler, RuntimeInput};
use convex_ports::snapshot::SnapshotRepository;
use std::sync::Arc;

#[tokio::test]
async fn full_original_product_graph_is_used_by_actual_runtime_and_restart() {
    let mut setup = setup().await;
    setup.plan.assembly.curves.clear();
    setup.plan.assembly.calibration = Some(replay::runtime_capture());
    let engine = make_engine(
        &setup,
        archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    assert!(engine.runtime_health().ready);
    let retained = engine.runtime_results();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].result.status(), ResultStatus::Complete);
    let snapshot = setup
        .snapshots
        .get(&retained[0].result.provenance().snapshot_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.content().schema_version, 3);
    assert!(snapshot.content().curve_recipes.is_empty() && snapshot.content().curves.is_empty());
    assert_eq!(
        snapshot.content().calibration.as_ref().unwrap().inputs,
        replay::native::runtime_inputs()
    );
    let lineage = &retained[0].result.provenance().input_versions;
    for (kind, id, version) in [
        ("calibration", "fictional-complete-graph", 11),
        ("curve", "USD-FIXTURE", 7),
        ("calendar", "actual-calendar", 17),
    ] {
        assert!(lineage
            .iter()
            .any(|p| p.kind.as_str() == kind && p.id.as_str() == id && p.version.get() == version));
    }
    engine.shutdown().await.unwrap();
    let reopened = make_engine(
        &setup,
        archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    reopened.start().await.unwrap();
    assert!(reopened.runtime_health().ready);
    assert_eq!(reopened.runtime_results(), retained);
    reopened.shutdown().await.unwrap();
}
#[tokio::test]
async fn real_configuration_stage_activation_and_snapshot_selection_retain_complete_capture() {
    let setup = setup().await;
    let service = configuration::cases(&setup);
    let mut staged = configuration::fixture::staged(1);
    staged.document.assembly.curves.clear();
    staged.document.assembly.calibration = Some(replay::runtime_capture());
    service
        .stage(&staged, &configuration::fixtures(&setup))
        .await
        .unwrap();
    service
        .activate(
            &configuration::fixture::activate(1, 0, "full-calibration"),
            &configuration::fixtures(&setup),
        )
        .await
        .unwrap();
    let active = service
        .active(&SourceId::new("CONFIG-RESTORE").unwrap())
        .await
        .unwrap()
        .unwrap();
    let request = active
        .capture(time(0), time(0), Version::new(1).unwrap())
        .unwrap();
    let assembler = SnapshotAssembler::new(
        setup.storage.bonds.clone(),
        setup.storage.configs.clone(),
        Arc::new(convex_ext_file::JsonFixingRepository::empty()),
        setup.snapshots.clone(),
    );
    let snapshot = assembler
        .resolve(&request, &setup.plan.seed_events)
        .await
        .unwrap();
    service.verify_snapshot_selection(&snapshot).await.unwrap();
    let mut invalid = staged.document.clone();
    invalid.assembly.calibration.as_mut().unwrap().expected["repricing"] = serde_json::json!([]);
    assert!(
        !service
            .validate(&invalid, &configuration::fixtures(&setup))
            .await
            .unwrap()
            .valid
    );
    let mut changed = snapshot.content().clone();
    changed.calibration.as_mut().unwrap().version = Version::new(12).unwrap();
    assert!(service
        .verify_snapshot_selection(&convex_ports::snapshot::MarketSnapshot::new(changed).unwrap())
        .await
        .is_err());
}
