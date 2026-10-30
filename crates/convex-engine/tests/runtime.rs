#[path = "support/subscription_source.rs"]
mod subscription_source;
#[path = "support/runtime_setup.rs"]
mod support;
use async_trait::async_trait;
use convex_core::contracts::*;
use convex_engine::*;
use convex_ext_file::*;
use convex_ports::{
    market_data::*,
    reference_master::{
        ExternalIdentifier, IdentifierResolution, MasterKey, MasterRecord, ReferenceLookup,
    },
    reference_repository::{ReferenceCommand, ReferenceCommit, ReferenceError, ReferenceRecord},
    runtime::*,
    snapshot::SnapshotRepository,
    TraitError,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use subscription_source::SubscriptionSource;
use support::*;
use tokio::sync::Notify;
async fn update(
    engine: &PricingEngine,
    event: QuoteEvent,
    milliseconds: i64,
) -> PublishedValuation {
    let mut results = engine.subscribe_results();
    let receipt = engine
        .submit_update(event, time(milliseconds), time(milliseconds))
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), results.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.generation, receipt.generation);
    result
}
#[tokio::test]
async fn cvx005_ac1_actual_quote_to_snapshot_graph_and_durable_publication() {
    let s = setup().await;
    let engine = make_engine(
        &s,
        archive(&s),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    assert!(engine.runtime_health().ready);
    let first = engine.runtime_results()[0].clone();
    let next = update(&engine, event(2, 1, "98.25"), 1).await;
    assert_eq!(next.result.status(), ResultStatus::Complete);
    assert_ne!(
        first.result.provenance().snapshot_id,
        next.result.provenance().snapshot_id
    );
    assert_ne!(first.context_key, next.context_key);
    assert_ne!(
        first.result.results()[&MeasureId::new("clean_price_mid").unwrap()],
        next.result.results()[&MeasureId::new("clean_price_mid").unwrap()]
    );
    assert_ne!(
        first.result.results()[&MeasureId::new("ytm_mid").unwrap()],
        next.result.results()[&MeasureId::new("ytm_mid").unwrap()]
    );
    assert_eq!(next.generation.get(), 2);
    assert!(next.revision > first.revision);
    let id = NodeId::BondPrice {
        instrument_id: "BOND-001".into(),
    };
    let cached = engine.calc_graph().get_cached(&id).unwrap();
    assert_eq!(
        cached.snapshot_id.as_ref(),
        Some(&next.result.provenance().snapshot_id)
    );
    assert_eq!(s.results.len().unwrap(), 2);
    for published in [first, next] {
        assert_eq!(
            s.results
                .get(&published.runtime_id, published.revision)
                .unwrap(),
            Some(published)
        );
    }
    let checkpoint = s.store.load(&s.plan.runtime_id).await.unwrap().unwrap();
    assert!(!checkpoint.pending);
    assert!(checkpoint.outbox.is_empty());
    assert_eq!(checkpoint.events.len(), 2);
    engine.shutdown().await.unwrap();
    assert_eq!(engine.runtime_health().phase, RuntimePhase::Stopped);
}
#[tokio::test]
async fn cvx005_ac2_curve_expiry_and_cleared_mark_retain_original_last_good() {
    for clear in [false, true] {
        let s = setup().await;
        // A curve expires only for a context that reads it (D58), so this plan asks for a
        // spread as well.
        let mut plan = s.plan.clone();
        plan.measures.push(MeasureId::new("z_spread_mid").unwrap());
        let engine = make_engine(
            &s,
            archive(&s),
            plan.clone(),
            s.store.clone(),
            RuntimeInput::Captured,
        );
        engine.start().await.unwrap();
        let good = engine.runtime_results()[0].clone();
        let elapsed = if clear { 1 } else { 10_001 };
        let mut event = event(2, elapsed, "98.25");
        if clear {
            event.event = QuoteEventKind::Delta {
                fields: vec![QuoteChange {
                    field: QuoteField::MidPrice,
                    update: FieldUpdate::Clear,
                }],
            };
        }
        let failed = update(&engine, event, elapsed).await;
        assert_eq!(failed.result.status(), ResultStatus::Failed);
        assert!(failed.result.results().is_empty());
        assert!(!failed.result.errors().is_empty());
        assert_eq!(failed.last_good.as_ref(), Some(&good.result));
        assert_eq!(failed.retained_quality, Some(DataQuality::Stale));
        assert_eq!(
            failed
                .last_good
                .as_ref()
                .unwrap()
                .provenance()
                .calculated_at,
            good.result.provenance().calculated_at
        );
        assert!(!engine.runtime_health().ready);
        let id = NodeId::BondPrice {
            instrument_id: "BOND-001".into(),
        };
        assert!(engine.calc_graph().get_cached(&id).is_none());
        assert!(matches!(
            engine.calc_graph().inspect_node(&id).unwrap().phase,
            NodePhase::Failed | NodePhase::Stale
        ));
        engine.shutdown().await.unwrap();
        let reopened = make_engine(
            &s,
            archive(&s),
            plan,
            s.store.clone(),
            RuntimeInput::Captured,
        );
        reopened.start().await.unwrap();
        assert!(!reopened.runtime_health().ready);
        assert_eq!(reopened.runtime_results(), vec![failed]);
        reopened.shutdown().await.unwrap();
    }
}
struct GatePublisher {
    inner: Arc<dyn ResultPublisher>,
    blocked: AtomicBool,
    entered: Notify,
    release: Notify,
    fail: AtomicBool,
}
impl GatePublisher {
    fn new(inner: Arc<dyn ResultPublisher>) -> Self {
        Self {
            inner,
            blocked: AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
            fail: AtomicBool::new(false),
        }
    }
}
#[async_trait]
impl ResultPublisher for GatePublisher {
    async fn publish(&self, result: &PublishedValuation) -> Result<(), TraitError> {
        if self.blocked.load(Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(TraitError::SourceNotAvailable("injected outage".into()));
        }
        self.inner.publish(result).await
    }
}
#[tokio::test]
async fn cvx005_ac3_shutdown_joins_publication_and_restart_preserves_exact_dependencies_and_revisions(
) {
    let s = setup().await;
    let gate = Arc::new(GatePublisher::new(archive(&s)));
    let engine = make_engine(
        &s,
        gate.clone(),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    gate.blocked.store(true, Ordering::SeqCst);
    let receipt = engine
        .submit_update(event(2, 1, "98.25"), time(1), time(1))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
    let copy = engine.clone();
    let shutdown = tokio::spawn(async move { copy.shutdown().await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(!shutdown.is_finished());
    assert!(!engine.runtime_health().ready);
    gate.blocked.store(false, Ordering::SeqCst);
    gate.release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), shutdown)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let retained = engine.runtime_results();
    assert_eq!(retained[0].generation, receipt.generation);
    assert!(engine
        .submit_update(event(3, 2, "97.25"), time(2), time(2))
        .await
        .is_err());
    assert_eq!(engine.calc_graph().in_flight_count(), 0);
    let reopened = make_engine(
        &s,
        archive(&s),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    assert_eq!(reopened.runtime_health().phase, RuntimePhase::New);
    reopened.start().await.unwrap();
    assert!(reopened.runtime_health().ready);
    assert_eq!(reopened.runtime_results(), retained);
    assert_eq!(reopened.calc_graph().node_count(), 1);
    let next = update(&reopened, event(3, 2, "97.25"), 2).await;
    assert!(next.revision > retained[0].revision);
    reopened.shutdown().await.unwrap();
}
#[tokio::test]
async fn publication_failure_is_checkpointed_and_retried_before_restart_readiness() {
    let s = setup().await;
    let gate = Arc::new(GatePublisher::new(archive(&s)));
    let engine = make_engine(
        &s,
        gate.clone(),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    gate.fail.store(true, Ordering::SeqCst);
    let receipt = engine
        .submit_update(event(2, 1, "98.25"), time(1), time(1))
        .await
        .unwrap();
    let error = engine.shutdown().await.unwrap_err();
    assert!(matches!(
        error,
        EngineError::Runtime(RuntimeError::Publication)
    ));
    let saved = s.store.load(&s.plan.runtime_id).await.unwrap().unwrap();
    assert_eq!(saved.generation, receipt.generation);
    assert_eq!(saved.outbox.len(), 1);
    assert!(!saved.pending);
    let reopened = make_engine(
        &s,
        archive(&s),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    reopened.start().await.unwrap();
    assert!(reopened.runtime_health().ready);
    let saved_results: Vec<_> = saved
        .results
        .iter()
        .map(|record| PublishedValuation::clone(record))
        .collect();
    assert_eq!(reopened.runtime_results(), saved_results);
    // Shared records (G30) are stored as the plain records, and read back equal.
    let json = serde_json::to_value(&saved).unwrap();
    assert_eq!(
        json["results"],
        serde_json::to_value(&saved_results).unwrap()
    );
    assert_eq!(
        serde_json::from_value::<RuntimeCheckpoint>(json).unwrap(),
        saved
    );
    assert!(s
        .store
        .load(&s.plan.runtime_id)
        .await
        .unwrap()
        .unwrap()
        .outbox
        .is_empty());
    reopened.shutdown().await.unwrap();
}
#[tokio::test]
async fn accepted_pending_checkpoint_is_recalculated_on_restart_before_ready() {
    let s = setup().await;
    let engine = make_engine(
        &s,
        archive(&s),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    engine.shutdown().await.unwrap();
    let mut saved = s.store.load(&s.plan.runtime_id).await.unwrap().unwrap();
    let previous = saved.revision;
    saved.revision += 1;
    saved.generation = fixture::version(2);
    saved.events.push(event(2, 1, "98.25"));
    saved.as_of = time(1);
    saved.known_at = time(1);
    saved.pending = true;
    s.store.checkpoint(previous, &saved).await.unwrap();
    let reopened = make_engine(
        &s,
        archive(&s),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    reopened.start().await.unwrap();
    assert!(reopened.runtime_health().ready);
    let records = reopened.runtime_results();
    assert_eq!(records[0].generation.get(), 2);
    assert!(records[0].revision > saved.result_revision);
    assert!(
        !s.store
            .load(&s.plan.runtime_id)
            .await
            .unwrap()
            .unwrap()
            .pending
    );
    reopened.shutdown().await.unwrap();
}
#[tokio::test]
async fn actual_file_replay_and_prefix_checked_restart_are_deterministic() {
    let s = setup().await;
    let path = s.dir.path().join("quotes.jsonl");
    std::fs::write(
        &path,
        [event(1, 0, "99.25"), event(2, 1, "98.25")]
            .iter()
            .map(|e| serde_json::to_string(e).unwrap() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    let mut plan = s.plan.clone();
    plan.mode = RuntimeMode::Replay;
    plan.seed_events.clear();
    let engine = make_engine(
        &s,
        archive(&s),
        plan.clone(),
        s.store.clone(),
        RuntimeInput::Replay(Box::new(FileQuoteReplay::open(&path).unwrap())),
    );
    engine.start().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !engine.runtime_health().replay_complete {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let records = engine.runtime_results();
    assert!(engine.runtime_health().ready);
    engine.shutdown().await.unwrap();
    let reopened = make_engine(
        &s,
        archive(&s),
        plan.clone(),
        s.store.clone(),
        RuntimeInput::Replay(Box::new(FileQuoteReplay::open(&path).unwrap())),
    );
    reopened.start().await.unwrap();
    assert_eq!(reopened.runtime_results(), records);
    reopened.shutdown().await.unwrap();
    std::fs::write(
        &path,
        serde_json::to_string(&event(1, 0, "80.25")).unwrap() + "\n",
    )
    .unwrap();
    let changed = make_engine(
        &s,
        archive(&s),
        plan,
        s.store.clone(),
        RuntimeInput::Replay(Box::new(FileQuoteReplay::open(&path).unwrap())),
    );
    assert!(changed.start().await.is_err());
    assert_eq!(changed.runtime_health().phase, RuntimePhase::Failed);
    assert!(!changed.runtime_health().ready);
    changed.shutdown().await.unwrap();
}
#[tokio::test]
async fn changed_plan_and_missing_reference_never_become_ready_and_checkpoint_cas_conflicts() {
    let s = setup().await;
    let engine = make_engine(
        &s,
        archive(&s),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    engine.shutdown().await.unwrap();
    let mut plan = s.plan.clone();
    plan.settlement_date = BusinessDate::parse("2026-10-07").unwrap();
    let changed = make_engine(
        &s,
        archive(&s),
        plan,
        s.store.clone(),
        RuntimeInput::Captured,
    );
    assert!(changed.start().await.is_err());
    assert!(!changed.runtime_health().ready);
    changed.shutdown().await.unwrap();
    let saved = s.store.load(&s.plan.runtime_id).await.unwrap().unwrap();
    let mut bad = saved.clone();
    bad.revision += 1;
    assert!(matches!(
        s.store.checkpoint(0, &bad).await,
        Err(TraitError::InvalidInput(_))
    ));
    bad.revision += 1;
    assert!(matches!(
        s.store.checkpoint(saved.revision + 1, &bad).await,
        Err(TraitError::Conflict(_))
    ));
    assert_eq!(s.store.load(&s.plan.runtime_id).await.unwrap(), Some(saved));
    let mut missing = s.plan.clone();
    missing.runtime_id = SourceId::new("missing-reference").unwrap();
    missing.assembly.instruments[0].version = fixture::version(99);
    let absent = make_engine(
        &s,
        archive(&s),
        missing,
        s.store.clone(),
        RuntimeInput::Captured,
    );
    assert!(absent.start().await.is_err());
    assert!(!absent.runtime_health().ready);
    absent.shutdown().await.unwrap();
}
#[tokio::test]
async fn concurrent_start_has_one_owner_and_shutdown_is_idempotent() {
    let s = setup().await;
    let engine = make_engine(
        &s,
        archive(&s),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    let (one, two) = tokio::join!(engine.start(), engine.start());
    assert_ne!(one.is_ok(), two.is_ok());
    let (one, two) = tokio::join!(engine.shutdown(), engine.shutdown());
    one.unwrap();
    two.unwrap();
    assert_eq!(engine.runtime_health().phase, RuntimePhase::Stopped);
    assert!(engine.start().await.is_err());
}
#[tokio::test]
async fn an_engine_without_a_runtime_needs_explicit_demo_and_is_never_ready() {
    let s = setup().await;
    let engine = PricingEngineBuilder::new()
        .with_storage(s.storage.clone())
        .build()
        .unwrap();
    assert!(engine.start().await.is_err());
    assert!(!engine.runtime_health().ready);
    engine.shutdown().await.unwrap();
    let demo = PricingEngineBuilder::new()
        .with_storage(s.storage.clone())
        .with_demo_mode()
        .build()
        .unwrap();
    demo.start().await.unwrap();
    assert_eq!(demo.runtime_health().phase, RuntimePhase::Demo);
    assert_eq!(demo.runtime_health().quality, DataQuality::Demo);
    assert!(!demo.runtime_health().ready);
    demo.shutdown().await.unwrap();
}

struct FaultStore {
    inner: Arc<dyn RuntimeStore>,
    fail_next: AtomicBool,
    committed_unknown: AtomicBool,
}
#[async_trait]
impl RuntimeStore for FaultStore {
    async fn load(&self, id: &SourceId) -> Result<Option<RuntimeCheckpoint>, TraitError> {
        self.inner.load(id).await
    }
    async fn checkpoint(&self, expected: u64, image: &RuntimeCheckpoint) -> Result<(), TraitError> {
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(TraitError::SourceNotAvailable(
                "injected checkpoint outage".into(),
            ));
        }
        self.inner.checkpoint(expected, image).await?;
        if self.committed_unknown.swap(false, Ordering::SeqCst) {
            return Err(TraitError::UnknownOutcome);
        }
        Ok(())
    }
}
#[tokio::test]
async fn durable_acceptance_failure_and_unknown_outcome_have_distinct_receipts() {
    for committed in [false, true] {
        let s = setup().await;
        let fault = Arc::new(FaultStore {
            inner: s.store.clone(),
            fail_next: AtomicBool::new(false),
            committed_unknown: AtomicBool::new(false),
        });
        let engine = make_engine(
            &s,
            archive(&s),
            s.plan.clone(),
            fault.clone(),
            RuntimeInput::Captured,
        );
        engine.start().await.unwrap();
        if committed {
            fault.committed_unknown.store(true, Ordering::SeqCst);
        } else {
            fault.fail_next.store(true, Ordering::SeqCst);
        }
        let receipt = engine
            .submit_update(event(2, 1, "98.25"), time(1), time(1))
            .await;
        assert_eq!(receipt.is_ok(), committed);
        let shutdown = engine.shutdown().await;
        assert_eq!(shutdown.is_ok(), committed);
        let saved = s.store.load(&s.plan.runtime_id).await.unwrap().unwrap();
        assert_eq!(saved.events.len(), if committed { 2 } else { 1 });
        assert!(!saved.pending);
        assert!(saved.outbox.is_empty());
    }
}
#[tokio::test]
async fn bounded_input_queue_rejects_overload_and_shutdown_drains_every_queued_receipt() {
    let s = setup().await;
    let gate = Arc::new(GatePublisher::new(archive(&s)));
    let engine = make_engine(
        &s,
        gate.clone(),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    gate.blocked.store(true, Ordering::SeqCst);
    engine
        .submit_update(event(2, 1, "98.25"), time(1), time(1))
        .await
        .unwrap();
    gate.entered.notified().await;
    let full = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut jobs = Vec::new();
    for _ in 0..130 {
        let engine = engine.clone();
        let full = full.clone();
        jobs.push(tokio::spawn(async move {
            let result = engine
                .submit_update(event(3, 2, "97.25"), time(2), time(2))
                .await;
            if result == Err(RuntimeError::CapacityExceeded) {
                full.fetch_add(1, Ordering::SeqCst);
            }
            result
        }));
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while full.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let copy = engine.clone();
    let shutdown = tokio::spawn(async move { copy.shutdown().await });
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(!shutdown.is_finished());
    gate.blocked.store(false, Ordering::SeqCst);
    gate.release.notify_one();
    tokio::time::timeout(Duration::from_secs(20), shutdown)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let mut accepted = 0;
    for job in jobs {
        match job.await.unwrap() {
            Ok(_) => accepted += 1,
            Err(error) => assert_eq!(error, RuntimeError::CapacityExceeded),
        }
    }
    assert_eq!(accepted, 128);
    let saved = s.store.load(&s.plan.runtime_id).await.unwrap().unwrap();
    assert_eq!(saved.events.len(), 130);
    assert!(!saved.pending);
    assert!(saved.outbox.is_empty());
    assert_eq!(engine.calc_graph().in_flight_count(), 0);
}

#[tokio::test]
async fn live_subscription_requires_new_image_expires_without_polling_and_closure_publishes_stale_failure(
) {
    for closed in [false, true] {
        let s = setup().await;
        let source = Arc::new(SubscriptionSource::new());
        let now = UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap();
        let mut plan = s.plan.clone();
        plan.mode = RuntimeMode::Live;
        plan.seed_events.clear();
        plan.assembly.as_of = now;
        plan.assembly.known_at = now;
        plan.assembly.curves[0].source_time = now;
        plan.assembly.curves[0].recorded_at = now;
        plan.assembly.curves[0].max_age_ms = 5000;
        plan.assembly.quotes[0].max_age_ms = 500;
        let engine = PricingEngineBuilder::new()
            .with_storage(s.storage.clone())
            .with_runtime(RuntimeServices {
                plan,
                snapshots: s.snapshots.clone(),
                fixings: Arc::new(JsonFixingRepository::empty()),
                store: s.store.clone(),
                publisher: archive(&s),
                input: RuntimeInput::Live(source.clone()),
            })
            .build()
            .unwrap();
        engine.start().await.unwrap();
        assert!(!engine.runtime_health().ready);
        let mut updates = engine.subscribe_results();
        let stamp = chrono::Utc::now().timestamp_millis();
        source.send(fixture::event(1, stamp, stamp, "99.25"));
        let good = tokio::time::timeout(Duration::from_secs(5), updates.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(good.result.status(), ResultStatus::Complete);
        assert!(good.live_valid_until.is_some());
        if closed {
            source.close();
        }
        let failed = tokio::time::timeout(Duration::from_secs(5), updates.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(failed.result.status(), ResultStatus::Failed);
        assert!(failed.result.results().is_empty());
        assert_eq!(failed.retained_quality, Some(DataQuality::Stale));
        assert_eq!(failed.last_good.as_ref(), Some(&good.result));
        assert!(!engine.runtime_health().ready);
        if closed {
            assert!(engine.shutdown().await.is_err());
        } else {
            engine.shutdown().await.unwrap();
        }
        assert!(source.unsubscribed.load(Ordering::SeqCst));
        assert_eq!(engine.calc_graph().in_flight_count(), 0);
    }
}

struct CountingStore {
    inner: Arc<dyn RuntimeStore>,
    writes: std::sync::atomic::AtomicUsize,
}
#[async_trait]
impl RuntimeStore for CountingStore {
    async fn load(&self, id: &SourceId) -> Result<Option<RuntimeCheckpoint>, TraitError> {
        self.inner.load(id).await
    }
    async fn checkpoint(&self, expected: u64, image: &RuntimeCheckpoint) -> Result<(), TraitError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.checkpoint(expected, image).await
    }
}
fn live_plan(s: &Setup) -> RuntimePlan {
    let now = UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap();
    let mut plan = s.plan.clone();
    plan.mode = RuntimeMode::Live;
    plan.seed_events.clear();
    plan.assembly.as_of = now;
    plan.assembly.known_at = now;
    plan.assembly.curves[0].source_time = now;
    plan.assembly.curves[0].recorded_at = now;
    plan.assembly.curves[0].max_age_ms = 600_000;
    plan.assembly.quotes[0].max_age_ms = 600_000;
    plan
}
fn live_engine(
    s: &Setup,
    plan: RuntimePlan,
    store: Arc<dyn RuntimeStore>,
    source: Arc<SubscriptionSource>,
) -> PricingEngine {
    PricingEngineBuilder::new()
        .with_storage(s.storage.clone())
        .with_runtime(RuntimeServices {
            plan,
            snapshots: s.snapshots.clone(),
            fixings: Arc::new(JsonFixingRepository::empty()),
            store,
            publisher: archive(s),
            input: RuntimeInput::Live(source),
        })
        .build()
        .unwrap()
}
#[tokio::test]
async fn live_ticks_stay_in_memory_and_restart_continues_above_reserved_counters() {
    let s = setup().await;
    let store = Arc::new(CountingStore {
        inner: s.store.clone(),
        writes: Default::default(),
    });
    let source = Arc::new(SubscriptionSource::new());
    let plan = live_plan(&s);
    let engine = live_engine(&s, plan.clone(), store.clone(), source.clone());
    engine.start().await.unwrap();
    // First start stores the initial image, then reserves counters for its first result.
    assert_eq!(store.writes.load(Ordering::SeqCst), 2);
    let mut updates = engine.subscribe_results();
    let mut last = None;
    for sequence in 1..=20u64 {
        let stamp = chrono::Utc::now().timestamp_millis();
        source.send(fixture::event(sequence, stamp, stamp, "99.25"));
        let result = tokio::time::timeout(Duration::from_secs(5), updates.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.result.status(), ResultStatus::Complete);
        assert!(s
            .snapshots
            .get(&result.result.provenance().snapshot_id)
            .await
            .unwrap()
            .is_none());
        last = Some(result);
    }
    assert!(engine.runtime_health().ready);
    assert_eq!(store.writes.load(Ordering::SeqCst), 2);
    engine.shutdown().await.unwrap();
    let last = last.unwrap();
    let stored = s.store.load(&s.plan.runtime_id).await.unwrap().unwrap();
    assert!(stored.events.is_empty() && stored.results.is_empty() && stored.outbox.is_empty());
    assert!(stored.pending && stored.snapshot_id.is_none());
    assert!(stored.result_revision > last.revision);
    assert!(stored.generation > last.generation);

    let source = Arc::new(SubscriptionSource::new());
    let restarted = live_engine(&s, plan, store.clone(), source.clone());
    restarted.start().await.unwrap();
    let restored = restarted.runtime_results();
    assert_eq!(restored.len(), 1);
    assert!(restored[0].revision > stored.result_revision);
    assert!(restored[0].generation > stored.generation);
    assert_ne!(restored[0].result.status(), ResultStatus::Complete);
    assert!(!restarted.runtime_health().ready);
    let mut updates = restarted.subscribe_results();
    let stamp = chrono::Utc::now().timestamp_millis();
    source.send(fixture::event(1, stamp, stamp, "99.50"));
    let fresh = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fresh.result.status(), ResultStatus::Complete);
    assert!(fresh.revision > restored[0].revision);
    assert!(restarted.runtime_health().ready);
    restarted.shutdown().await.unwrap();
}

/// The two records one tick of a two-bond plan publishes, in plan order.
async fn two_bond_tick(
    updates: &mut tokio::sync::broadcast::Receiver<PublishedValuation>,
) -> [PublishedValuation; 2] {
    let mut records = Vec::with_capacity(2);
    for _ in 0..2 {
        records.push(
            tokio::time::timeout(Duration::from_secs(5), updates.recv())
                .await
                .unwrap()
                .unwrap(),
        );
    }
    assert_eq!(records[0].generation, records[1].generation);
    records.try_into().unwrap()
}

/// D58: each bond is judged only on its own inputs. Two bonds ticked further apart than
/// the skew policy both value. When one bond's quote ages out, only that bond is valued
/// and fails (D55), and the runtime stays ready (D58a); its past expiry is never armed,
/// so the tick does not re-run.
#[tokio::test]
async fn live_judges_each_bond_only_on_its_own_inputs() {
    let s = setup().await;
    let mut plan = live_plan(&s);
    widen(&s, &mut plan, 2).await;
    plan.assembly.max_source_skew_ms = 500;
    plan.assembly.quotes[1].max_age_ms = 3_000;
    let source = Arc::new(SubscriptionSource::new());
    let engine = live_engine(&s, plan, s.store.clone(), source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    // This current-thread test runs the actor only while it awaits, so both quotes are
    // valued in one tick (D54).
    let stamp = chrono::Utc::now().timestamp_millis();
    let mut other = fixture::event(1, stamp - 600, stamp - 600, "99.00");
    other.key.instrument_id = convex_core::ids::InstrumentId::new("BOND-W001");
    source.send(other);
    source.send(fixture::event(1, stamp, stamp, "99.25"));
    let both = two_bond_tick(&mut updates).await;
    for record in &both {
        assert_eq!(
            record.result.status(),
            ResultStatus::Complete,
            "{:?}",
            record.result.errors()
        );
        assert_eq!(record.result.quality(), DataQuality::Valid);
    }
    let health = engine.runtime_health();
    assert!(health.ready && health.failed_instruments == 0, "{health:?}");

    // The second bond's quote ages out three seconds after it was observed. Only that
    // bond is valued and published; the first keeps its record as valued (D60).
    let aged = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(updates.is_empty());
    assert_eq!(aged.result.status(), ResultStatus::Failed);
    assert_eq!(aged.result.quality(), DataQuality::Stale);
    assert!(aged
        .result
        .errors()
        .iter()
        .all(|e| e.code() == ErrorCode::StaleInput));
    assert_eq!(aged.retained_quality, Some(DataQuality::Stale));
    assert_eq!(aged.last_good.as_ref(), Some(&both[1].result));
    assert_eq!(engine.runtime_results(), [both[0].clone(), aged.clone()]);
    let health = engine.runtime_health();
    assert_eq!(health.generation, aged.generation.get());
    assert!(health.ready, "{health:?}");
    assert_eq!(health.failed_instruments, 1);
    assert_eq!(health.quality, DataQuality::Stale);

    // Only the valid bond's expiry is armed, ten minutes away: no further tick.
    assert!(
        tokio::time::timeout(Duration::from_millis(500), updates.recv())
            .await
            .is_err()
    );
    assert_eq!(engine.runtime_health().generation, health.generation);
    engine.shutdown().await.unwrap();
}

/// D55: a tick values only the bonds whose inputs changed. A quote on one of two bonds
/// values and publishes that bond alone; the other keeps its record, with the revision,
/// generation and snapshot it was valued under (D60), and the runtime stays ready.
#[tokio::test]
async fn live_values_only_the_bonds_whose_inputs_changed() {
    let s = setup().await;
    let mut plan = live_plan(&s);
    widen(&s, &mut plan, 2).await;
    let source = Arc::new(SubscriptionSource::new());
    let engine = live_engine(&s, plan, s.store.clone(), source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    let quote = |sequence, id: &str, value| {
        let stamp = chrono::Utc::now().timestamp_millis();
        let mut event = fixture::event(sequence, stamp, stamp, value);
        event.key.instrument_id = convex_core::ids::InstrumentId::new(id);
        source.send(event);
    };
    // Every bond's first image values both, in one tick.
    quote(1, "BOND-001", "99.25");
    quote(1, "BOND-W001", "99.00");
    let both = two_bond_tick(&mut updates).await;
    for sequence in 2..=4 {
        quote(sequence, "BOND-001", "99.50");
        let single = tokio::time::timeout(Duration::from_secs(5), updates.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(updates.is_empty());
        assert_eq!(single.result.status(), ResultStatus::Complete);
        assert!(single.generation > both[0].generation);
        assert!(single.revision > both[1].revision);
        assert_ne!(
            single.result.provenance().snapshot_id,
            both[1].result.provenance().snapshot_id
        );
        assert_eq!(engine.runtime_results(), [single.clone(), both[1].clone()]);
        let health = engine.runtime_health();
        assert!(health.ready, "{health:?}");
        assert_eq!(health.generation, single.generation.get());
        assert_eq!(health.revision, single.revision);
        assert_eq!(
            health.snapshot_id.as_ref(),
            Some(&single.result.provenance().snapshot_id)
        );
    }
    engine.shutdown().await.unwrap();
}

/// D58: a curve counts only for a context that reads curves. A stale curve fails a
/// spread measure's context and leaves a mid-only context valid.
#[tokio::test]
async fn a_stale_curve_fails_only_a_context_that_reads_curves() {
    for (measure, read) in [("ytm_mid", false), ("z_spread_mid", true)] {
        let s = setup().await;
        let mut plan = s.plan.clone();
        let stale = fixture::time("2026-10-02T14:59:00Z");
        plan.assembly.curves[0].source_time = stale;
        plan.assembly.curves[0].recorded_at = stale;
        plan.measures = vec![MeasureId::new(measure).unwrap()];
        let engine = make_engine(
            &s,
            archive(&s),
            plan,
            s.store.clone(),
            RuntimeInput::Captured,
        );
        engine.start().await.unwrap();
        let results = engine.runtime_results();
        assert_eq!(results.len(), 1);
        let result = &results[0].result;
        let health = engine.runtime_health();
        if read {
            assert_eq!(result.status(), ResultStatus::Failed);
            assert_eq!(result.quality(), DataQuality::Stale);
            assert_eq!(result.errors()[0].code(), ErrorCode::StaleInput);
            assert!(
                !health.ready && health.failed_instruments == 1,
                "{health:?}"
            );
        } else {
            assert_eq!(
                result.status(),
                ResultStatus::Complete,
                "{:?}",
                result.errors()
            );
            assert_eq!(result.quality(), DataQuality::Valid);
            assert!(health.ready && health.failed_instruments == 0, "{health:?}");
        }
        engine.shutdown().await.unwrap();
    }
}

/// G20: every tick is a new snapshot, so a Live runtime values more contexts than its
/// 256-entry result cache holds. The cache keeps one snapshot's results (D51).
#[tokio::test]
async fn live_ticks_stay_complete_past_the_result_cache_capacity() {
    let s = setup().await;
    let source = Arc::new(SubscriptionSource::new());
    let engine = live_engine(&s, live_plan(&s), s.store.clone(), source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    for sequence in 1..=300u64 {
        let stamp = chrono::Utc::now().timestamp_millis();
        source.send(fixture::event(sequence, stamp, stamp, "99.25"));
        let result = tokio::time::timeout(Duration::from_secs(5), updates.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            result.result.status(),
            ResultStatus::Complete,
            "tick {sequence}: {:?}",
            result.result.errors()
        );
    }
    engine.shutdown().await.unwrap();
}

/// D54: quotes queued while the runtime is busy are valued in one tick. Every quote
/// still advances the generation, and the result reflects the newest.
#[tokio::test]
async fn live_quotes_queued_together_are_valued_in_one_tick() {
    let s = setup().await;
    let source = Arc::new(SubscriptionSource::new());
    let engine = live_engine(&s, live_plan(&s), s.store.clone(), source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    let start = engine.runtime_health().generation;
    let stamp = chrono::Utc::now().timestamp_millis();
    // This current-thread test runs the actor only while it awaits, so all ten quotes
    // are queued before the actor takes the first.
    for sequence in 1..=10u64 {
        let price = if sequence == 10 { "99.50" } else { "99.00" };
        source.send(fixture::event(sequence, stamp, stamp, price));
    }
    let batched = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(batched.generation.get(), start + 10);
    assert_eq!(batched.result.status(), ResultStatus::Complete);
    let stamp = chrono::Utc::now().timestamp_millis().max(stamp);
    source.send(fixture::event(11, stamp, stamp, "99.50"));
    let single = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(single.generation.get(), start + 11);
    let clean = MeasureId::new("clean_price_mid").unwrap();
    assert_eq!(
        batched.result.results()[&clean].value,
        single.result.results()[&clean].value
    );
    assert!(engine.runtime_health().ready);
    engine.shutdown().await.unwrap();
}

/// D56: a Live runtime reads its pinned references once per selection, at start, and
/// not at each tick.
#[tokio::test]
async fn live_ticks_reuse_the_pinned_references() {
    let s = setup().await;
    let references = Arc::new(CountingReferences {
        inner: s.storage.bonds.clone(),
        reads: Default::default(),
    });
    let storage = Arc::new(convex_ports::storage::StorageAdapter {
        bonds: references.clone(),
        curves: s.storage.curves.clone(),
        configs: s.storage.configs.clone(),
        configuration: s.storage.configuration.clone(),
        audit: s.storage.audit.clone(),
    });
    let source = Arc::new(SubscriptionSource::new());
    let engine = PricingEngineBuilder::new()
        .with_storage(storage)
        .with_runtime(RuntimeServices {
            plan: live_plan(&s),
            snapshots: s.snapshots.clone(),
            fixings: Arc::new(JsonFixingRepository::empty()),
            store: s.store.clone(),
            publisher: archive(&s),
            input: RuntimeInput::Live(source.clone()),
        })
        .build()
        .unwrap();
    engine.start().await.unwrap();
    assert_eq!(references.reads.load(Ordering::SeqCst), 1);
    let mut updates = engine.subscribe_results();
    for sequence in 1..=5u64 {
        let stamp = chrono::Utc::now().timestamp_millis();
        source.send(fixture::event(sequence, stamp, stamp, "99.25"));
        let result = tokio::time::timeout(Duration::from_secs(5), updates.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.result.status(), ResultStatus::Complete);
    }
    assert_eq!(references.reads.load(Ordering::SeqCst), 1);
    engine.shutdown().await.unwrap();
}

/// Counts exact-version reference reads, the read a snapshot makes for each pinned instrument.
struct CountingReferences {
    inner: Arc<dyn convex_ports::reference_repository::ReferenceRepository>,
    reads: std::sync::atomic::AtomicUsize,
}
#[async_trait]
impl convex_ports::reference_repository::ReferenceRepository for CountingReferences {
    async fn get(
        &self,
        id: &convex_core::ids::InstrumentId,
    ) -> Result<Option<ReferenceRecord>, ReferenceError> {
        self.inner.get(id).await
    }
    async fn get_version(
        &self,
        id: &convex_core::ids::InstrumentId,
        version: Version,
    ) -> Result<Option<ReferenceRecord>, ReferenceError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.get_version(id, version).await
    }
    async fn list(
        &self,
        filter: &convex_ports::storage::BondFilter,
        pagination: &convex_ports::storage::Pagination,
    ) -> Result<convex_ports::storage::Page<ReferenceRecord>, ReferenceError> {
        self.inner.list(filter, pagination).await
    }
    async fn resolve(
        &self,
        id: &ExternalIdentifier,
    ) -> Result<Option<ReferenceRecord>, ReferenceError> {
        self.inner.resolve(id).await
    }
    async fn commit(
        &self,
        command: &ReferenceCommand,
        time: UtcTimestamp,
    ) -> Result<ReferenceCommit, ReferenceError> {
        self.inner.commit(command, time).await
    }
    async fn get_commit(&self, key: &RequestId) -> Result<Option<ReferenceCommit>, ReferenceError> {
        self.inner.get_commit(key).await
    }
    async fn get_at(
        &self,
        id: &convex_core::ids::InstrumentId,
        lookup: ReferenceLookup,
    ) -> Result<Option<ReferenceRecord>, ReferenceError> {
        self.inner.get_at(id, lookup).await
    }
    async fn get_master(&self, key: &MasterKey) -> Result<Option<MasterRecord>, ReferenceError> {
        self.inner.get_master(key).await
    }
    async fn get_master_version(
        &self,
        key: &MasterKey,
        version: Version,
    ) -> Result<Option<MasterRecord>, ReferenceError> {
        self.inner.get_master_version(key, version).await
    }
    async fn get_master_at(
        &self,
        key: &MasterKey,
        lookup: ReferenceLookup,
    ) -> Result<Option<MasterRecord>, ReferenceError> {
        self.inner.get_master_at(key, lookup).await
    }
    async fn resolve_at(
        &self,
        id: &ExternalIdentifier,
        lookup: ReferenceLookup,
    ) -> Result<Option<IdentifierResolution>, ReferenceError> {
        self.inner.resolve_at(id, lookup).await
    }
}

#[tokio::test]
async fn live_plan_refuses_seed_events() {
    let s = setup().await;
    let mut plan = s.plan.clone();
    plan.mode = RuntimeMode::Live;
    assert!(!plan.seed_events.is_empty());
    assert!(matches!(
        plan.validate(),
        Err(RuntimeError::Configuration(_))
    ));
}

#[tokio::test]
async fn result_archive_reconciles_retry_keys_and_refuses_corrupt_tails() {
    let s = setup().await;
    let path = s.dir.path().join("results.jsonl");
    let engine = make_engine(
        &s,
        Arc::new(JsonlResultPublisher::new(&path).unwrap()),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let first = engine.runtime_results()[0].clone();
    engine.shutdown().await.unwrap();
    let reopened = JsonlResultPublisher::new(&path).unwrap();
    reopened.publish(&first).await.unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);
    let mut changed = first.clone();
    changed.context_key = "f".repeat(64);
    assert!(matches!(
        reopened.publish(&changed).await,
        Err(TraitError::Conflict(_))
    ));
    drop(reopened);
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{torn")
        .unwrap();
    assert!(JsonlResultPublisher::new(&path).is_err());
}

#[tokio::test]
async fn plan_admits_each_published_measure_once_and_nothing_else() {
    let s = setup().await;
    let mut plan = s.plan.clone();
    plan.measures = convex_engine::snapshot::RUNTIME_MEASURES
        .iter()
        .map(|m| MeasureId::new(*m).unwrap())
        .collect();
    plan.validate().unwrap();
    let mut repeated = plan.clone();
    repeated.measures.push(MeasureId::new("ytm_mid").unwrap());
    assert!(matches!(
        repeated.validate(),
        Err(RuntimeError::Configuration(_))
    ));
    let mut unknown = s.plan.clone();
    unknown
        .measures
        .push(MeasureId::new("g_spread_mid").unwrap());
    assert!(matches!(
        unknown.validate(),
        Err(RuntimeError::Configuration(_))
    ));
}

#[tokio::test]
async fn runtime_results_feed_portfolio_analytics_through_the_quote_adapter() {
    use rust_decimal::Decimal;
    let s = setup().await;
    // Version 2 gives the fixture curve the swap role too, so every runtime measure is defined.
    let mut config = fixture::config();
    config.analytics_curves.swap_curve = Some(config.analytics_curves.discount_curve.clone());
    config.version = 2;
    s.storage.configs.save(&config).await.unwrap();
    let mut plan = s.plan.clone();
    plan.assembly.configuration.version = fixture::version(2);
    plan.measures = convex_engine::snapshot::RUNTIME_MEASURES
        .iter()
        .map(|m| MeasureId::new(*m).unwrap())
        .collect();
    let engine = make_engine(
        &s,
        archive(&s),
        plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let published = engine.runtime_results()[0].clone();
    engine.shutdown().await.unwrap();
    assert_eq!(engine.runtime_settlement_date(), Some(plan.settlement_date));
    let quote = runtime_bond_quote(
        &published,
        plan.settlement_date.date(),
        convex_core::Currency::USD,
    )
    .unwrap();
    // Each analyzer input is the runtime measure itself.
    let measure = |name: &str| {
        published.result.results()[&MeasureId::new(name).unwrap()]
            .value
            .clone()
    };
    let model = |value: MeasureValue| match value {
        MeasureValue::ModelPrice(v) => v.value().get(),
        MeasureValue::ModelRate(v) => v.value().get(),
        MeasureValue::ModelYears(v) => v.value().get(),
        MeasureValue::ModelYearsSquared(v) => v.value().get(),
        MeasureValue::ModelBps(v) => v.value().get(),
        other => panic!("unexpected {other:?}"),
    };
    let decimal = |value: f64| Some(Decimal::from_f64_retain(value).unwrap());
    let MeasureValue::Price(clean) = measure("clean_price_mid") else {
        panic!("clean mid is an exact price")
    };
    assert_eq!(quote.clean_price_mid, Some(clean.value().decimal()));
    assert_eq!(
        quote.accrued_interest,
        decimal(model(measure("accrued_interest")))
    );
    assert_eq!(quote.ytm_mid, decimal(model(measure("ytm_mid"))));
    assert_eq!(
        quote.modified_duration,
        decimal(model(measure("modified_duration_mid")))
    );
    assert_eq!(quote.convexity, decimal(model(measure("convexity_mid"))));
    assert_eq!(quote.dv01, decimal(model(measure("dv01_mid"))));
    assert_eq!(quote.z_spread_mid, decimal(model(measure("z_spread_mid"))));
    for (field, name) in [
        (quote.i_spread_mid, "i_spread_mid"),
        (quote.asw_mid, "asw_mid"),
        (quote.macaulay_duration, "macaulay_duration_mid"),
        (quote.effective_duration, "effective_duration_mid"),
        (quote.effective_convexity, "effective_convexity_mid"),
        (quote.spread_duration, "spread_duration_mid"),
        (quote.pv01, "pv01_mid"),
        (quote.cs01, "cs01_mid"),
    ] {
        assert_eq!(field, decimal(model(measure(name))), "{name}");
    }
    let key_rates = quote.key_rate_durations.clone().unwrap();
    assert_eq!(
        key_rates
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>(),
        ["3M", "6M", "1Y", "2Y", "3Y", "5Y", "7Y", "10Y", "20Y", "30Y"]
    );
    assert_eq!(
        key_rates[7].1,
        decimal(model(measure("krd_10y_mid"))).unwrap()
    );
    assert_eq!(quote.source, "runtime-test");

    // A one-position book reproduces each measure; market value is dirty price times face.
    let face = Decimal::new(1_000_000, 0);
    let portfolio = Portfolio {
        portfolio_id: convex_core::ids::PortfolioId::new("ONE"),
        name: "one position".into(),
        currency: convex_core::Currency::USD,
        positions: vec![Position {
            instrument_id: quote.instrument_id.clone(),
            notional: face,
            sector: None,
            rating: None,
        }],
    };
    let output = PortfolioAnalyzer::new()
        .calculate(&portfolio, std::slice::from_ref(&quote))
        .unwrap();
    // Weighted ratios divide by the market value in 28-digit decimal arithmetic.
    let close = |left: Decimal, right: Decimal| {
        assert!(
            (left - right).abs() <= right.abs() * Decimal::new(1, 12),
            "{left} vs {right}"
        )
    };
    let dirty = quote.clean_price_mid.unwrap() + quote.accrued_interest.unwrap();
    close(output.market_value, dirty * face / Decimal::ONE_HUNDRED);
    close(output.duration, quote.modified_duration.unwrap());
    close(output.convexity, quote.convexity.unwrap());
    close(output.yield_value, quote.ytm_mid.unwrap());
    close(output.spread, quote.z_spread_mid.unwrap());
    close(
        output.dv01,
        quote.dv01.unwrap() * face / Decimal::ONE_HUNDRED,
    );

    // A result that is not complete is refused, not adapted.
    assert!(runtime_bond_quote(
        &published_with_status(&published, ResultStatus::Partial),
        plan.settlement_date.date(),
        convex_core::Currency::USD,
    )
    .is_err());
}

fn published_with_status(
    published: &PublishedValuation,
    status: ResultStatus,
) -> PublishedValuation {
    let envelope = &published.result;
    let error = StructuredError::new(
        ErrorCode::MissingMarketData,
        Some("z_spread_mid".into()),
        "fixture gap",
        false,
    )
    .unwrap();
    let mut results = envelope.results().clone();
    results.retain(|m, _| m.as_str() != "z_spread_mid");
    PublishedValuation {
        result: ResultEnvelope::new(
            envelope.request_id().clone(),
            status,
            envelope.quality(),
            results,
            vec![error],
            envelope.provenance().clone(),
        )
        .unwrap(),
        ..published.clone()
    }
}
