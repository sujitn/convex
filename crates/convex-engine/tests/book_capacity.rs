//! Opt-in bounded definitions and source-independent recovery through real ports.
#![cfg(unix)]
#[path = "support/core_fixture.rs"]
mod fixture;
use convex_core::contracts::*;
use convex_engine::{
    core_workflow::CoreWorkflow,
    ports::{
        core_workflow::CoreResult, reference_master::*, reference_repository::*,
        runtime::RuntimeStore,
    },
};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn expanded_names_preserve_native_parity_and_restart_with_empty_sources() {
    let directory = tempfile::tempdir().unwrap();
    let (snapshot, result) = {
        let f = fixture::Fixture::new(directory.path()).await;
        let core = f.workflow();
        let old = core.assemble(f.plan.clone()).await.unwrap();
        let MasterDefinition::Calculation(mut definition) =
            old.content.definition.definition.clone()
        else {
            panic!("calculation")
        };
        let spec = definition.calculations["risk"].clone();
        definition.calculations = (0..40)
            .map(|i| (format!("risk.{i}"), spec.clone()))
            .collect();
        assert!(definition.validate().is_err());
        definition.reference_inputs["core_capacity"] = json!("book_128");
        assert!(definition.validate().is_ok());
        core.references
            .commit(
                &ReferenceCommand {
                    idempotency_key: RequestId::new("capacity.names").unwrap(),
                    changes: vec![],
                    master_changes: vec![MasterChange {
                        expected_version: Some(Version::new(1).unwrap()),
                        definition: MasterDefinition::Calculation(definition),
                    }],
                },
                fixture::time(),
            )
            .await
            .unwrap();
        let mut plan = f.plan.clone();
        plan.definition.version = Version::new(2).unwrap();
        let snapshot = core.assemble(plan).await.unwrap();
        let receipt = core
            .run(snapshot.id.clone(), "risk.39".into(), true)
            .await
            .unwrap();
        let result = core.result(&receipt.result_id).await.unwrap();
        let mut request = f.request.clone();
        if let convex::library::LibraryOperation::GraphHedgeRisk { input, .. } =
            &mut request.operation
        {
            input.snapshot_id = snapshot.id.clone();
        }
        assert_eq!(
            result.output,
            convex::library::execute_json(&serde_json::to_string(&request).unwrap())
        );
        assert_eq!(core.snapshot(&old.id).await.unwrap(), old);
        (snapshot, result)
    };
    let (_unused, results, _) =
        convex_ext_redb::create_redb_storage_with_core(directory.path().join("state.redb"))
            .unwrap();
    drop(_unused);
    let empty = tempfile::tempdir().unwrap();
    let (sources, _, _) =
        convex_ext_redb::create_redb_storage_with_core(empty.path().join("empty.redb")).unwrap();
    let core = CoreWorkflow::new(
        sources.bonds,
        sources.curves,
        Arc::new(convex_ext_file::JsonFixingRepository::empty()),
        Arc::new(
            convex_ext_file::FileSnapshotRepository::new(directory.path().join("snapshots"))
                .unwrap(),
        ),
        results,
        ModelId::new("core.test.build").unwrap(),
    );
    assert_eq!(core.snapshot(&snapshot.id).await.unwrap(), snapshot);
    assert!(
        core.run(snapshot.id.clone(), "risk.39".into(), false)
            .await
            .unwrap()
            .reused
    );
    let receipt = core.run(snapshot.id, "risk.39".into(), true).await.unwrap();
    assert!(!receipt.reused);
    assert_eq!(core.result(&receipt.result_id).await.unwrap(), result);
}

#[tokio::test]
async fn profile_selection_keeps_original_defaults_and_finite_new_limits() {
    let directory = tempfile::tempdir().unwrap();
    let f = fixture::Fixture::new(directory.path()).await;
    let snapshot = f.workflow().assemble(f.plan.clone()).await.unwrap();
    let MasterDefinition::Calculation(mut definition) = snapshot.content.definition.definition
    else {
        panic!("calculation")
    };
    for invalid in [
        json!(null),
        json!(true),
        json!("unbounded"),
        json!({"nodes":9999999}),
    ] {
        definition.reference_inputs["core_capacity"] = invalid;
        assert!(definition.validate().is_err());
    }
    definition
        .reference_inputs
        .as_object_mut()
        .unwrap()
        .remove("core_capacity");
    let spec = definition.calculations["risk"].clone();
    definition.calculations = (0..33)
        .map(|i| (format!("risk.{i}"), spec.clone()))
        .collect();
    assert!(definition.validate().is_err());
    definition.reference_inputs["core_capacity"] = json!("book_128");
    definition.calculations = (0..130)
        .map(|i| (format!("risk.{i}"), spec.clone()))
        .collect();
    assert!(definition.validate().is_ok());
    definition.calculations.insert("excess".into(), spec);
    assert!(definition.validate().is_err());
    definition.calculations.remove("excess");
    definition.reference_inputs["oversized"] = json!("x".repeat(8 * 1024 * 1024));
    assert!(definition.validate().is_err());
}

#[tokio::test]
async fn larger_result_storage_requires_the_saved_profile_and_survives_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let (identity, output) = {
        let f = fixture::Fixture::new(directory.path()).await;
        let core = f.workflow();
        let old = core.assemble(f.plan.clone()).await.unwrap();
        let build = ModelId::new("core.test.build").unwrap();
        let small = CoreResult::new(
            old.id.clone(),
            "risk".into(),
            build.clone(),
            r#"{"ok":"true","result":{}}"#.into(),
        )
        .unwrap();
        let encoded = serde_json::to_value(&small).unwrap();
        assert!(encoded.get("book_capacity").is_none());
        assert_eq!(
            serde_json::from_value::<CoreResult>(encoded).unwrap(),
            small
        );
        // Storage admission only: deliberately quote-heavy valid envelopes test
        // the separate escaped record bound without claiming financial values.
        let output = json!({"ok":"true","result":{"padding":"\"".repeat(3*1024*1024)}}).to_string();
        assert!(
            CoreResult::new(old.id.clone(), "risk".into(), build.clone(), output.clone()).is_err()
        );
        assert!(CoreResult::new_book(
            old.id.clone(),
            "risk".into(),
            build.clone(),
            " ".repeat(16 * 1024 * 1024 + 1)
        )
        .is_err());
        let wrong =
            CoreResult::new_book(old.id.clone(), "risk".into(), build.clone(), output.clone())
                .unwrap();
        f.results.save_core_result(&wrong).await.unwrap();
        assert!(core.result(&wrong.id).await.is_err());
        assert!(core
            .run(old.id.clone(), "risk".into(), false)
            .await
            .is_err());
        let MasterDefinition::Calculation(mut definition) = old.content.definition.definition
        else {
            panic!("calculation")
        };
        definition.reference_inputs["core_capacity"] = json!("book_128");
        core.references
            .commit(
                &ReferenceCommand {
                    idempotency_key: RequestId::new("capacity.result").unwrap(),
                    changes: vec![],
                    master_changes: vec![MasterChange {
                        expected_version: Some(Version::new(1).unwrap()),
                        definition: MasterDefinition::Calculation(definition),
                    }],
                },
                fixture::time(),
            )
            .await
            .unwrap();
        let mut plan = f.plan.clone();
        plan.definition.version = Version::new(2).unwrap();
        let snapshot = core.assemble(plan).await.unwrap();
        let result =
            CoreResult::new_book(snapshot.id.clone(), "risk".into(), build, output.clone())
                .unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() > 8 * 1024 * 1024);
        f.results.save_core_result(&result).await.unwrap();
        assert_eq!(core.result(&result.id).await.unwrap(), result);
        assert!(
            core.run(snapshot.id, "risk".into(), false)
                .await
                .unwrap()
                .reused
        );
        (result.id, output)
    };
    let (_, results, _) =
        convex_ext_redb::create_redb_storage_with_core(directory.path().join("state.redb"))
            .unwrap();
    let recovered = results.get_core_result(&identity).await.unwrap().unwrap();
    assert!(recovered.book_capacity);
    assert_eq!(recovered.output, output);
    recovered.verify().unwrap();
}
