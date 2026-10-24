//! Validation reuse must never hide adapter failure or changed captured inputs.
#![cfg(unix)]
#[path = "support/core_fixture.rs"]
mod fixture;

use async_trait::async_trait;
use convex_core::contracts::*;
use convex_engine::{
    core_workflow::*,
    ports::{core_workflow::*, reference_master::MasterDefinition, snapshot::*, TraitError},
};
use fixture::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, RwLock,
};

#[derive(Clone)]
enum Read {
    Saved,
    Replacement(Box<CoreSnapshot>),
    Missing,
    Offline,
}
struct Gate {
    inner: Arc<dyn SnapshotRepository>,
    read: RwLock<Read>,
    calls: AtomicUsize,
}
#[async_trait]
impl SnapshotRepository for Gate {
    async fn publish(&self, value: &MarketSnapshot) -> Result<(), TraitError> {
        self.inner.publish(value).await
    }
    async fn get(&self, id: &SnapshotId) -> Result<Option<MarketSnapshot>, TraitError> {
        self.inner.get(id).await
    }
    async fn publish_core(&self, value: &CoreSnapshot) -> Result<(), TraitError> {
        self.inner.publish_core(value).await
    }
    async fn get_core(&self, id: &SnapshotId) -> Result<Option<CoreSnapshot>, TraitError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let read = self.read.read().unwrap().clone();
        match read {
            Read::Saved => self.inner.get_core(id).await,
            Read::Replacement(value) => Ok(Some(*value)),
            Read::Missing => Ok(None),
            Read::Offline => Err(TraitError::SourceNotAvailable("test snapshot port".into())),
        }
    }
}
fn workflow(f: &Fixture) -> (CoreWorkflow, Arc<Gate>) {
    let gate = Arc::new(Gate {
        inner: f.snapshots.clone(),
        read: RwLock::new(Read::Saved),
        calls: AtomicUsize::new(0),
    });
    (
        CoreWorkflow::new(
            f.storage.bonds.clone(),
            f.storage.curves.clone(),
            f.fixings.clone(),
            gate.clone(),
            f.results.clone(),
            ModelId::new("core.test.build").unwrap(),
        ),
        gate,
    )
}
async fn rejects_every_path(core: &CoreWorkflow, id: &SnapshotId) {
    assert!(core.snapshot(id).await.is_err());
    for force in [false, true] {
        assert!(core.run(id.clone(), "risk".into(), force).await.is_err());
    }
}

#[tokio::test]
async fn warm_validation_still_reads_the_port_and_rejects_corrupt_missing_or_unavailable_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path()).await;
    let (core, gate) = workflow(&f);
    let saved = core.assemble(f.plan.clone()).await.unwrap();
    let receipt = core
        .run(saved.id.clone(), "risk".into(), false)
        .await
        .unwrap();
    let output = core.result(&receipt.result_id).await.unwrap();
    for mode in [Read::Missing, Read::Offline] {
        *gate.read.write().unwrap() = mode;
        let before = gate.calls.load(Ordering::SeqCst);
        rejects_every_path(&core, &saved.id).await;
        assert_eq!(gate.calls.load(Ordering::SeqCst) - before, 3);
    }
    let mut corrupt = saved.clone();
    corrupt.content.definition.recorded_at = UtcTimestamp::parse("2026-10-08T00:00:00Z").unwrap();
    assert!(corrupt.verify().is_err());
    *gate.read.write().unwrap() = Read::Replacement(Box::new(corrupt));
    rejects_every_path(&core, &saved.id).await;
    *gate.read.write().unwrap() = Read::Saved;
    assert_eq!(core.snapshot(&saved.id).await.unwrap(), saved);
    assert!(
        core.run(saved.id, "risk".into(), false)
            .await
            .unwrap()
            .reused
    );
    assert_eq!(core.result(&receipt.result_id).await.unwrap(), output);
}

#[tokio::test]
async fn new_digests_validate_the_whole_book_not_only_the_selected_calculation() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path()).await;
    let (core, gate) = workflow(&f);
    let saved = core.assemble(f.plan.clone()).await.unwrap();
    core.run(saved.id.clone(), "risk".into(), false)
        .await
        .unwrap();
    for change in 0..4 {
        let mut content = saved.content.clone();
        match change {
            0 => {
                let MasterDefinition::Calculation(ref mut definition) =
                    content.definition.definition
                else {
                    panic!("fixture calculation");
                };
                let mut unselected = definition.calculations["risk"].clone();
                unselected.request["schema_version"] = serde_json::json!(999);
                definition
                    .calculations
                    .insert("unselected".into(), unselected);
            }
            1 => content.curves.get_mut("base").unwrap()[0].quality = DataQuality::Stale,
            2 => content.fixings[0].entitled = false,
            3 => {
                content.definition.recorded_at =
                    UtcTimestamp::parse("2026-10-08T00:00:00Z").unwrap()
            }
            _ => unreachable!(),
        }
        let invalid = CoreSnapshot::new(content).unwrap();
        invalid.verify().unwrap(); // Structural/digest integrity alone is insufficient.
        assert_ne!(invalid.id, saved.id);
        *gate.read.write().unwrap() = Read::Replacement(Box::new(invalid.clone()));
        for _ in 0..2 {
            rejects_every_path(&core, &invalid.id).await;
        }
    }
    *gate.read.write().unwrap() = Read::Saved;
    assert!(
        core.run(saved.id, "risk".into(), false)
            .await
            .unwrap()
            .reused
    );
}

#[tokio::test]
async fn adapter_identity_and_unknown_calculation_checks_survive_warm_validation() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path()).await;
    let (core, gate) = workflow(&f);
    let saved = core.assemble(f.plan.clone()).await.unwrap();
    let mut content = saved.content.clone();
    content.definition.recorded_at = UtcTimestamp::parse("2026-10-07T11:59:59Z").unwrap();
    let other = CoreSnapshot::new(content).unwrap();
    *gate.read.write().unwrap() = Read::Replacement(Box::new(other.clone()));
    core.snapshot(&other.id).await.unwrap();
    rejects_every_path(&core, &saved.id).await;
    *gate.read.write().unwrap() = Read::Saved;
    assert!(core
        .run(saved.id.clone(), "absent".into(), false)
        .await
        .is_err());
    assert!(
        !core
            .run(saved.id.clone(), "risk".into(), false)
            .await
            .unwrap()
            .reused
    );
    assert!(
        core.run(saved.id, "risk".into(), false)
            .await
            .unwrap()
            .reused
    );
}

#[tokio::test]
async fn concurrent_replays_and_new_workflow_instances_retain_exact_durable_output() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path()).await;
    let (core, _) = workflow(&f);
    let saved = core.assemble(f.plan.clone()).await.unwrap();
    let receipt = core
        .run(saved.id.clone(), "risk".into(), false)
        .await
        .unwrap();
    let expected = core.result(&receipt.result_id).await.unwrap();
    let (cached, forced) = tokio::join!(
        core.run(saved.id.clone(), "risk".into(), false),
        core.run(saved.id.clone(), "risk".into(), true)
    );
    assert!(cached.unwrap().reused);
    assert!(!forced.unwrap().reused);
    let (restarted, gate) = workflow(&f);
    let recovered = restarted.run(saved.id, "risk".into(), false).await.unwrap();
    assert!(recovered.reused);
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        restarted.result(&recovered.result_id).await.unwrap(),
        expected
    );
}
