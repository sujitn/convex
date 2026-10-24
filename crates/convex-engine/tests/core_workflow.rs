//! Actual backend snapshots/results, native parity and uncertain-commit recovery.
#![cfg(unix)] // The existing durable filesystem snapshot adapter explicitly supports local Unix.
#[path = "support/core_fixture.rs"]
mod fixture;
use async_trait::async_trait;
use convex_core::contracts::*;
use convex_engine::{
    core_workflow::*,
    ports::{core_workflow::*, fixing::*, runtime::*, snapshot::*, TraitError},
};
use fixture::*;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

#[tokio::test]
async fn native_parity_exact_pins_and_restart_replay_do_not_read_current_sources() {
    let directory = tempfile::tempdir().unwrap();
    let (id, receipt, output) = {
        let f = Fixture::new(directory.path()).await;
        let core = f.workflow();
        let snapshot = core.assemble(f.plan.clone()).await.unwrap();
        assert_eq!(core.assemble(f.plan.clone()).await.unwrap(), snapshot);
        let receipt = core
            .run(snapshot.id.clone(), "risk".into(), false)
            .await
            .unwrap();
        assert!(!receipt.reused);
        let output = core.result(&receipt.result_id).await.unwrap();
        let mut native = f.request.clone();
        if let convex::library::LibraryOperation::GraphHedgeRisk { input, .. } =
            &mut native.operation
        {
            input.snapshot_id = snapshot.id.clone();
        }
        let expected = convex::library::execute_json(&serde_json::to_string(&native).unwrap());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&expected).unwrap()["ok"],
            "true"
        );
        assert_eq!(output.output, expected);
        assert!(
            core.run(snapshot.id.clone(), "risk".into(), false)
                .await
                .unwrap()
                .reused
        );
        (snapshot.id, receipt, output)
    };
    // Reopen the real database. Empty alternative input ports prove recovery
    // uses captured records rather than silently querying current providers.
    let (_storage, results, _) =
        convex_ext_redb::create_redb_storage_with_core(directory.path().join("state.redb"))
            .unwrap();
    let sources = tempfile::tempdir().unwrap();
    let (empty, _, _) =
        convex_ext_redb::create_redb_storage_with_core(sources.path().join("empty.redb")).unwrap();
    let core = CoreWorkflow::new(
        empty.bonds,
        empty.curves,
        Arc::new(convex_ext_file::JsonFixingRepository::empty()),
        Arc::new(
            convex_ext_file::FileSnapshotRepository::new(directory.path().join("snapshots"))
                .unwrap(),
        ),
        results,
        ModelId::new("core.test.build").unwrap(),
    );
    assert_eq!(core.result(&receipt.result_id).await.unwrap(), output);
    assert!(
        core.run(id.clone(), "risk".into(), false)
            .await
            .unwrap()
            .reused
    );
    let recomputed = core.run(id, "risk".into(), true).await.unwrap();
    assert!(!recomputed.reused);
    assert_eq!(core.result(&recomputed.result_id).await.unwrap(), output);
}

#[tokio::test]
async fn missing_sources_and_future_reference_versions_never_publish_a_partial_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path()).await;
    let core = f.workflow();
    let mut missing = f.plan.clone();
    missing.curve_sets.get_mut("base").unwrap()[0].version = Version::new(999).unwrap();
    assert!(matches!(
        core.assemble(missing).await,
        Err(CoreWorkflowError::NotFound)
    ));
    assert_eq!(
        std::fs::read_dir(dir.path().join("snapshots"))
            .unwrap()
            .count(),
        0
    );
    let mut missing = f.plan.clone();
    missing.fixings[0].source = SourceId::new("absent.source").unwrap();
    assert!(matches!(
        core.assemble(missing).await,
        Err(CoreWorkflowError::NotFound)
    ));
    let mut early = f.plan.clone();
    early.known_at = UtcTimestamp::parse("2026-10-07T11:59:59Z").unwrap();
    assert!(core.assemble(early).await.is_err());
    assert_eq!(
        std::fs::read_dir(dir.path().join("snapshots"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn source_updates_create_new_snapshots_and_preserve_all_original_inputs_and_results() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path()).await;
    let core = f.workflow();
    let original = core.assemble(f.plan.clone()).await.unwrap();
    let run = core
        .run(original.id.clone(), "risk".into(), false)
        .await
        .unwrap();
    let result = core.result(&run.result_id).await.unwrap();
    let mut recipe = f.recipe.clone();
    recipe.version = Version::new(2).unwrap();
    recipe.model_id = ModelId::new("new.captured.source").unwrap();
    f.storage.curves.save_recipe(&recipe).await.unwrap();
    f.storage.curves.save_recipe(&recipe).await.unwrap();
    let mut conflict = recipe.clone();
    conflict.source = SourceId::new("different.source").unwrap();
    assert!(matches!(
        f.storage.curves.save_recipe(&conflict).await,
        Err(TraitError::Conflict(_))
    ));
    let mut changed = f.plan.clone();
    changed.curve_sets.get_mut("base").unwrap()[0].version = recipe.version;
    assert_ne!(core.assemble(changed).await.unwrap().id, original.id);
    let mut revised = f.published[0].clone();
    revised.version = Version::new(2).unwrap();
    revised.published_at = time();
    revised.recorded_at = time();
    f.fixings
        .publish(std::slice::from_ref(&revised))
        .await
        .unwrap();
    let corrected = core.assemble(f.plan.clone()).await.unwrap();
    assert_ne!(corrected.id, original.id);
    assert_eq!(
        corrected.content.fixings.len(),
        original.content.fixings.len() + 1
    );
    assert!(corrected.content.fixings.contains(&f.published[0]));
    assert!(corrected.content.fixings.contains(&revised));
    assert_eq!(core.snapshot(&original.id).await.unwrap(), original);
    let replay = core.run(original.id, "risk".into(), true).await.unwrap();
    assert_eq!(core.result(&replay.result_id).await.unwrap(), result);
}

#[tokio::test]
async fn fixing_batch_conflicts_roll_back_and_future_known_corrections_stay_out_of_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path()).await;
    let core = f.workflow();
    let original = core.assemble(f.plan.clone()).await.unwrap();
    let mut added = f.published[0].clone();
    added.version = Version::new(2).unwrap();
    added.published_at = time();
    added.recorded_at = time();
    let mut conflict = f.published.last().unwrap().clone();
    conflict.evidence_reference = "conflicting original revision".into();
    assert!(matches!(
        f.fixings.publish(&[added.clone(), conflict]).await,
        Err(TraitError::Conflict(_))
    ));
    assert_eq!(core.assemble(f.plan.clone()).await.unwrap(), original);
    let mut reversed = f.published[0].clone();
    reversed.version = Version::new(2).unwrap();
    reversed.published_at = UtcTimestamp::parse("2026-09-16T12:00:00Z").unwrap();
    reversed.recorded_at = reversed.published_at;
    assert!(matches!(
        f.fixings.publish(&[reversed]).await,
        Err(TraitError::Conflict(_))
    ));
    added.published_at = UtcTimestamp::parse("2026-10-07T12:01:00Z").unwrap();
    added.recorded_at = added.published_at;
    f.fixings.publish(&[added]).await.unwrap();
    assert_eq!(core.assemble(f.plan.clone()).await.unwrap(), original);
}

struct SnapshotLostAck {
    inner: Arc<dyn SnapshotRepository>,
    once: std::sync::atomic::AtomicBool,
}
#[async_trait]
impl SnapshotRepository for SnapshotLostAck {
    async fn publish(&self, snapshot: &MarketSnapshot) -> Result<(), TraitError> {
        self.inner.publish(snapshot).await
    }
    async fn get(&self, id: &SnapshotId) -> Result<Option<MarketSnapshot>, TraitError> {
        self.inner.get(id).await
    }
    async fn get_core(&self, id: &SnapshotId) -> Result<Option<CoreSnapshot>, TraitError> {
        self.inner.get_core(id).await
    }
    async fn publish_core(&self, snapshot: &CoreSnapshot) -> Result<(), TraitError> {
        self.inner.publish_core(snapshot).await?;
        if self.once.swap(false, Ordering::SeqCst) {
            Err(TraitError::UnknownOutcome)
        } else {
            Ok(())
        }
    }
}
#[tokio::test]
async fn lost_snapshot_ack_exposes_the_actual_published_identity_and_idempotent_retry() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path()).await;
    let snapshots = Arc::new(SnapshotLostAck {
        inner: f.snapshots.clone(),
        once: std::sync::atomic::AtomicBool::new(true),
    });
    let core = CoreWorkflow::new(
        f.storage.bonds.clone(),
        f.storage.curves.clone(),
        f.fixings.clone(),
        snapshots,
        f.results.clone(),
        ModelId::new("core.test.build").unwrap(),
    );
    let Err(CoreWorkflowError::SnapshotUnknown(id)) = core.assemble(f.plan.clone()).await else {
        panic!("lost snapshot ack must retain its actual identity")
    };
    assert_eq!(core.snapshot(&id).await.unwrap().id, id);
    assert_eq!(core.assemble(f.plan.clone()).await.unwrap().id, id);
}

struct LostAck {
    inner: Arc<dyn RuntimeStore>,
    mode: AtomicU8,
}
#[async_trait]
impl RuntimeStore for LostAck {
    async fn load(&self, id: &SourceId) -> Result<Option<RuntimeCheckpoint>, TraitError> {
        self.inner.load(id).await
    }
    async fn checkpoint(&self, expected: u64, image: &RuntimeCheckpoint) -> Result<(), TraitError> {
        self.inner.checkpoint(expected, image).await
    }
    async fn get_core_result(&self, id: &str) -> Result<Option<CoreResult>, TraitError> {
        self.inner.get_core_result(id).await
    }
    async fn save_core_result(&self, result: &CoreResult) -> Result<(), TraitError> {
        match self.mode.swap(0, Ordering::SeqCst) {
            1 => Err(TraitError::SourceNotAvailable(
                "injected precommit failure".into(),
            )),
            2 => {
                self.inner.save_core_result(result).await?;
                Err(TraitError::UnknownOutcome)
            }
            _ => self.inner.save_core_result(result).await,
        }
    }
}
#[tokio::test]
async fn lost_result_ack_and_precommit_failure_remain_recoverable_without_false_success() {
    for mode in [1, 2] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path()).await;
        let store = Arc::new(LostAck {
            inner: f.results.clone(),
            mode: AtomicU8::new(mode),
        });
        let core = CoreWorkflow::new(
            f.storage.bonds.clone(),
            f.storage.curves.clone(),
            f.fixings.clone(),
            f.snapshots.clone(),
            store,
            ModelId::new("core.test.build").unwrap(),
        );
        let snapshot = core.assemble(f.plan.clone()).await.unwrap();
        let first = core.run(snapshot.id.clone(), "risk".into(), false).await;
        if mode == 2 {
            let Err(CoreWorkflowError::ResultUnknown(id)) = first else {
                panic!("committed lost ack must expose its recovery key")
            };
            assert!(core.result(&id).await.is_ok());
        } else {
            assert!(matches!(
                first,
                Err(CoreWorkflowError::Port(TraitError::SourceNotAvailable(_)))
            ));
        }
        let retried = core.run(snapshot.id, "risk".into(), false).await.unwrap();
        assert_eq!(retried.reused, mode == 2);
    }
}

#[tokio::test]
async fn corrupt_saved_inputs_are_rejected_even_when_a_result_is_cached() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path()).await;
    let core = f.workflow();
    let snapshot = core.assemble(f.plan.clone()).await.unwrap();
    core.run(snapshot.id.clone(), "risk".into(), false)
        .await
        .unwrap();
    let path = dir.path().join("snapshots").join(format!(
        "{}.core.json",
        snapshot.id.as_str().strip_prefix("sha256:").unwrap()
    ));
    let mut stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    stored["content"]["plan"]["definition"]["version"] = serde_json::json!(999);
    std::fs::write(path, serde_json::to_vec(&stored).unwrap()).unwrap();
    assert!(core.run(snapshot.id, "risk".into(), false).await.is_err());
}
