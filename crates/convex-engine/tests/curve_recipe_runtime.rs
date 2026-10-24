#[path = "support/configuration_setup.rs"]
mod configuration;
#[path = "support/curve_replay.rs"]
mod recipes;
use configuration::runtime::*;
use convex_core::contracts::*;
use convex_engine::{configuration::ConfigurationError, snapshot::SnapshotAssembler, RuntimeInput};
use convex_ports::snapshot::{MarketSnapshot, SnapshotRepository};
use std::sync::Arc;

#[tokio::test]
async fn actual_runtime_capture_value_and_restart_retain_complete_recipe_lineage() {
    let mut setup = setup().await;
    setup.plan.assembly.curves.clear();
    setup.plan.assembly.curve_recipes = vec![recipes::record(recipes::recipe())];
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
    assert_eq!(snapshot.content().schema_version, 2);
    assert_eq!(
        snapshot.content().curve_recipes[0].recipe,
        recipes::recipe()
    );
    assert!(snapshot.content().curves.is_empty());
    let lineage = &retained[0].result.provenance().input_versions;
    assert!(lineage.iter().any(|v| v.kind.as_str() == "curve"
        && v.id.as_str() == "USD-FIXTURE"
        && v.version.get() == 7));
    assert!(lineage
        .iter()
        .any(|v| v.kind.as_str() == "fixture-definition"
            && v.id.as_str() == "original-terms"
            && v.version.get() == 17));
    let context = setup
        .plan
        .context(&snapshot, &InstrumentId::new("BOND-001").unwrap())
        .unwrap();
    let replay = engine
        .evaluate_context(
            snapshot,
            context,
            retained[0].result.request_id().clone(),
            retained[0].result.provenance().calculated_at,
        )
        .await
        .unwrap();
    assert_eq!(replay.envelope(), &retained[0].result);
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
async fn actual_configuration_stage_activation_capture_and_pin_checks_keep_recipes() {
    let setup = setup().await;
    let service = configuration::cases(&setup);
    let mut staged = configuration::fixture::staged(1);
    staged.document.assembly.curves.clear();
    staged.document.assembly.curve_recipes = vec![recipes::record(recipes::recipe())];
    service
        .stage(&staged, &configuration::fixtures(&setup))
        .await
        .unwrap();
    service
        .activate(
            &configuration::fixture::activate(1, 0, "complete-recipe"),
            &configuration::fixtures(&setup),
            false,
        )
        .await
        .unwrap();
    let selected = service
        .active(&SourceId::new("CONFIG-RESTORE").unwrap())
        .await
        .unwrap()
        .unwrap();
    let request = selected
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
    let mut changed = snapshot.content().clone();
    changed.curve_recipes[0].recipe["source"]["FlatForward"]["nodes"][2]["quote"]["value"] =
        2.9.into();
    let changed = MarketSnapshot::new(changed).unwrap();
    assert!(matches!(
        service.verify_snapshot_selection(&changed).await,
        Err(ConfigurationError::Conflict)
    ));
    let mut bad = staged.document.clone();
    bad.assembly.curve_recipes[0].recipe["dependencies"] =
        serde_json::json!([{"id":"missing","version":"exact-v1"}]);
    let report = service
        .validate(&bad, &configuration::fixtures(&setup))
        .await
        .unwrap();
    assert!(!report.valid);
}
