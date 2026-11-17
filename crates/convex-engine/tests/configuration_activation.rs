#[path = "support/configuration_fault.rs"]
mod fault;
#[path = "support/subscription_source.rs"]
mod subscription_source;
#[path = "support/configuration_setup.rs"]
mod support;
use convex_core::contracts::*;
use convex_engine::configuration::*;
use convex_ports::{configuration::*, snapshot::SnapshotRepository, storage::Pagination};
use std::time::Duration;
use support::*;

#[tokio::test]
async fn captured_selection_accepts_canonical_manifest_order_and_rejects_changed_curve_pin() {
    let mut setup = runtime::setup().await;
    let service = cases(&setup);
    let mut staged = fixture::staged(1);
    let mut extra = staged.document.assembly.curves[0].clone();
    extra.curve_id = CurveId::new("AAA-FIXTURE").unwrap();
    extra.input_versions = ["zeta", "alpha"]
        .into_iter()
        .map(|id| InputVersion {
            kind: SourceId::new("fixture-input").unwrap(),
            id: SourceId::new(id).unwrap(),
            version: Version::new(1).unwrap(),
        })
        .collect();
    staged
        .document
        .assembly
        .required_curves
        .push(extra.curve_id.clone());
    staged.document.assembly.curves.push(extra);
    service.stage(&staged, &fixtures(&setup)).await.unwrap();
    service
        .activate(
            &fixture::activate(1, 0, "ordered-manifest"),
            &fixtures(&setup),
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
    let assembler = convex_engine::snapshot::SnapshotAssembler::new(
        setup.storage.bonds.clone(),
        setup.storage.configs.clone(),
        std::sync::Arc::new(convex_ext_file::JsonFixingRepository::empty()),
        setup.snapshots.clone(),
    );
    let snapshot = assembler
        .resolve(&request, &setup.plan.seed_events)
        .await
        .unwrap();
    assert_eq!(
        snapshot.content().curves[0].curve_id.as_str(),
        "AAA-FIXTURE"
    );
    assert_eq!(
        snapshot.content().curves[0].input_versions[0].id.as_str(),
        "alpha"
    );
    service.verify_snapshot_selection(&snapshot).await.unwrap();
    setup.plan = plan(&staged.document, setup.plan.seed_events.clone());
    let engine = engine(&setup, service.clone());
    engine.start().await.unwrap();
    let context = setup
        .plan
        .context(&snapshot, &InstrumentId::new("BOND-001").unwrap())
        .unwrap();
    let result = engine
        .evaluate_context(
            snapshot.clone(),
            context,
            RequestId::new("ordered-valuation").unwrap(),
            time(0),
        )
        .await
        .unwrap();
    assert_eq!(result.envelope().status(), ResultStatus::Complete);
    let mut changed = snapshot.content().clone();
    changed.curves[0].version = Version::new(2).unwrap();
    let false_snapshot = convex_ports::snapshot::MarketSnapshot::new(changed).unwrap();
    assert!(matches!(
        service.verify_snapshot_selection(&false_snapshot).await,
        Err(ConfigurationError::Conflict)
    ));
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn cvx010_ac3_dry_run_reports_missing_curves_providers_and_never_publishes_or_stages() {
    let setup = runtime::setup().await;
    let service = cases(&setup);
    let fixtures = fixtures(&setup);
    let mut document = fixture::document(1);
    let before = setup.snapshots.market_snapshot_count().unwrap();
    let valid = service.validate(&document, &fixtures).await.unwrap();
    assert!(valid.valid, "{valid:?}");
    assert_eq!(valid.fixtures.len(), 1);
    document.assembly.curves.clear();
    document.providers[0].source = SourceId::new("missing-provider").unwrap();
    document.providers[0].credential =
        Some(SecretReference::new("env://PRIVATE_MARKER".into()).unwrap());
    let invalid = service.validate(&document, &fixtures).await.unwrap();
    assert!(!invalid.valid);
    assert!(invalid.issues.iter().any(|i| i.code == "MISSING_CURVE"));
    assert!(invalid.issues.iter().any(|i| i.code == "MISSING_PROVIDER"));
    assert!(!serde_json::to_string(&invalid)
        .unwrap()
        .contains("PRIVATE_MARKER"));
    assert!(!format!("{document:?}").contains("PRIVATE_MARKER"));
    let id = SourceId::new("CONFIG-RESTORE").unwrap();
    assert!(service.active(&id).await.unwrap().is_none());
    assert!(service
        .repository()
        .get_staged(&id, fixture::snapshot::version(1))
        .await
        .unwrap()
        .is_none());
    assert_eq!(setup.snapshots.market_snapshot_count().unwrap(), before);
    let mut wrong = fixtures.clone();
    wrong[0].expected[0].value = FiniteF64::new(0.0).unwrap();
    let mismatch = service
        .validate(&fixture::document(1), &wrong)
        .await
        .unwrap();
    assert!(!mismatch.valid);
    assert!(mismatch.issues.iter().any(|i| i.code == "FIXTURE_MISMATCH"));
}

/// G20: one service validates on a shared fixture valuator whose result cache holds 256
/// contexts. 33 validations of 8 fixtures value 264 distinct snapshots.
#[tokio::test]
async fn fixture_validation_stays_valid_past_the_result_cache_capacity() {
    let setup = runtime::setup().await;
    let service = cases(&setup);
    let document = fixture::document(1);
    let case = fixtures(&setup).remove(0);
    for validation in 0..33i64 {
        let cases: Vec<_> = (0..8i64)
            .map(|index| ConfigurationFixture {
                id: SourceId::new(format!("bond-mid-regression-{index}")).unwrap(),
                as_of: time(validation * 8 + index),
                known_at: time(validation * 8 + index),
                ..case.clone()
            })
            .collect();
        let report = service.validate(&document, &cases).await.unwrap();
        assert!(report.valid, "validation {validation}: {:?}", report.issues);
    }
}

#[test]
fn cvx010_ac1_in_flight_real_valuation_keeps_its_original_selection_across_activation() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let setup = runtime::setup().await;
        let service = cases(&setup);
        let fixtures = fixtures(&setup);
        service.stage(&fixture::staged(1), &fixtures).await.unwrap();
        service
            .activate(&fixture::activate(1, 0, "first"), &fixtures)
            .await
            .unwrap();
        let engine = engine(&setup, service.clone());
        engine.start().await.unwrap();
        let original = engine.runtime_results()[0].clone();
        let snapshot = setup
            .snapshots
            .get(&original.result.provenance().snapshot_id)
            .await
            .unwrap()
            .unwrap();
        let context = setup
            .plan
            .context(&snapshot, &InstrumentId::new("BOND-001").unwrap())
            .unwrap();
        let mut altered = snapshot.content().clone();
        altered.max_source_skew_ms = 999;
        let altered = convex_ports::snapshot::MarketSnapshot::new(altered).unwrap();
        let altered_context = setup
            .plan
            .context(&altered, &InstrumentId::new("BOND-001").unwrap())
            .unwrap();
        assert!(matches!(
            engine
                .evaluate_context(
                    altered,
                    altered_context,
                    RequestId::new("false-policy").unwrap(),
                    time(0)
                )
                .await,
            Err(convex_ports::TraitError::Conflict(_))
        ));
        let (release, occupied) = std::sync::mpsc::sync_channel::<()>(1);
        let (started, acquired) = tokio::sync::oneshot::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            started.send(()).unwrap();
            occupied.recv().unwrap();
        });
        acquired.await.unwrap();
        let request = engine.evaluate_context(
            snapshot,
            context.clone(),
            RequestId::new("in-flight-v1").unwrap(),
            time(0),
        );
        tokio::pin!(request);
        std::future::poll_fn(|cx| {
            use std::{future::Future, task::Poll};
            assert!(request.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        let mut second = fixture::staged(2);
        second.document.assembly.quotes[0].max_age_ms = 2000;
        service.stage(&second, &fixtures).await.unwrap();
        let activation = service
            .activate(&fixture::activate(2, 1, "second"), &fixtures)
            .await
            .unwrap();
        assert_eq!(activation.command.configuration_version.get(), 2);
        assert_eq!(
            service
                .active(&activation.command.id)
                .await
                .unwrap()
                .unwrap()
                .activation,
            activation
        );
        release.send(()).unwrap();
        blocker.await.unwrap();
        let result = request.await.unwrap();
        assert_eq!(
            result.envelope().provenance().configuration_version.get(),
            1
        );
        assert_eq!(result.context(), &context);
        assert_eq!(result.envelope().results(), original.result.results());
        let mut updates = engine.subscribe_results();
        let outcome = engine
            .activate_configuration(&activation.command, &fixtures)
            .await
            .unwrap();
        assert_eq!(
            outcome.accepted_activation_revision,
            Some(activation.revision)
        );
        let next = tokio::time::timeout(Duration::from_secs(5), updates.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.result.provenance().configuration_version.get(), 2);
        assert_ne!(next.context_key, original.context_key);
        let snapshot = setup
            .snapshots
            .get(&next.result.provenance().snapshot_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot
                .content()
                .configuration_capture
                .as_ref()
                .unwrap()
                .activation,
            activation
        );
        assert!(next
            .result
            .provenance()
            .input_versions
            .iter()
            .any(|v| v.kind.as_str() == "configuration_activation"
                && v.version == activation.revision));
        let before_retry = engine.runtime_health().revision;
        let retried = engine
            .activate_configuration(&activation.command, &fixtures)
            .await
            .unwrap();
        assert_eq!(retried.activation, activation);
        assert_eq!(engine.runtime_health().revision, before_retry);
        engine.shutdown().await.unwrap();
    });
}

#[tokio::test]
async fn cvx010_ac2_override_expiry_is_exclusive_preserves_history_last_good_and_restart() {
    let setup = runtime::setup().await;
    let service = cases(&setup);
    let fixtures = fixtures(&setup);
    service.stage(&fixture::staged(1), &fixtures).await.unwrap();
    service
        .activate(&fixture::activate(1, 0, "first"), &fixtures)
        .await
        .unwrap();
    let engine = engine(&setup, service.clone());
    engine.start().await.unwrap();
    let mut second = fixture::staged(2);
    second.staged_at = time(0);
    second.document.pricing.updated_at = time(0).datetime().timestamp_millis();
    second.document.assembly.quotes[0].max_age_ms = 1000;
    second.document.overrides = vec![PolicyOverride {
        id: SourceId::new("approved-age-exception").unwrap(),
        scope: second.document.assembly.quotes[0].key.clone(),
        max_age_ms: 5000,
        actor: "fixture-operator".into(),
        reason: "Explicit fixture freshness exception".into(),
        created_at: fixture::snapshot::time("2026-10-02T14:59:58Z"),
        approved_by: "fixture-approver".into(),
        approved_at: fixture::snapshot::time("2026-10-02T14:59:59Z"),
        expires_at: time(2000),
    }];
    service.stage(&second, &fixtures).await.unwrap();
    let mut updates = engine.subscribe_results();
    let command = fixture::activate(2, 1, "override");
    engine
        .activate_configuration(&command, &fixtures)
        .await
        .unwrap();
    updates.recv().await.unwrap();
    let mut good_event = runtime::event(2, 0, "99.25");
    good_event.receive_time_ms = time(0).datetime().timestamp_millis();
    engine
        .submit_update(good_event.clone(), time(1999), time(1999))
        .await
        .unwrap();
    let good = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(good.result.status(), ResultStatus::Complete);
    assert_eq!(good.result.quality(), DataQuality::Valid);
    let good_snapshot = setup
        .snapshots
        .get(&good.result.provenance().snapshot_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        good_snapshot.content().quotes[0].requirement.max_age_ms,
        5000
    );
    assert_eq!(
        good_snapshot
            .content()
            .configuration_capture
            .as_ref()
            .unwrap()
            .applied_overrides,
        second.document.overrides
    );
    assert!(good_snapshot
        .require_execution_quality_at(time(2000))
        .is_err());
    good_event.sequence = 3;
    engine
        .submit_update(good_event, time(2000), time(2000))
        .await
        .unwrap();
    let expired = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(expired.result.status(), ResultStatus::Failed);
    assert!(expired.result.results().is_empty());
    assert_eq!(expired.last_good, Some(good.result.clone()));
    let expired_snapshot = setup
        .snapshots
        .get(&expired.result.provenance().snapshot_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        expired_snapshot.content().quotes[0].requirement.max_age_ms,
        1000
    );
    assert!(expired_snapshot
        .content()
        .configuration_capture
        .as_ref()
        .unwrap()
        .applied_overrides
        .is_empty());
    assert_eq!(
        service
            .repository()
            .get_staged(&command.id, command.configuration_version)
            .await
            .unwrap()
            .unwrap(),
        second
    );
    assert_eq!(
        service
            .history(&command.id, &Pagination::new(0, 10))
            .await
            .unwrap()
            .total,
        2
    );
    engine.shutdown().await.unwrap();
    drop(engine);
    let restarted = support::engine(&setup, service);
    restarted.start().await.unwrap();
    assert!(!restarted.runtime_health().ready);
    assert_eq!(restarted.runtime_results(), [(*expired).clone()]);
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn cvx010_ac3_rollback_revalidates_content_and_records_new_activation_and_runtime_version() {
    let setup = runtime::setup().await;
    let service = cases(&setup);
    let fixtures = fixtures(&setup);
    service.stage(&fixture::staged(1), &fixtures).await.unwrap();
    let first = service
        .activate(&fixture::activate(1, 0, "first"), &fixtures)
        .await
        .unwrap();
    let engine = engine(&setup, service.clone());
    engine.start().await.unwrap();
    let mut second = fixture::staged(2);
    second.document.assembly.max_source_skew_ms = 100;
    service.stage(&second, &fixtures).await.unwrap();
    let mut updates = engine.subscribe_results();
    engine
        .activate_configuration(&fixture::activate(2, 1, "second"), &fixtures)
        .await
        .unwrap();
    updates.recv().await.unwrap();
    let mut rollback = fixture::activate(1, 2, "rollback");
    rollback.action = ActivationKind::Rollback {
        to_revision: first.revision,
    };
    let outcome = engine
        .activate_configuration(&rollback, &fixtures)
        .await
        .unwrap();
    assert_eq!(outcome.activation.revision.get(), 3);
    assert_eq!(
        outcome.activation.previous_version,
        Some(fixture::snapshot::version(2))
    );
    let result = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.result.provenance().configuration_version.get(), 1);
    assert_eq!(
        engine.runtime_health().activation_revision.unwrap().get(),
        3
    );
    assert_eq!(
        engine.runtime_health().configuration_id,
        Some(rollback.id.clone())
    );
    assert_eq!(
        service
            .history(&rollback.id, &Pagination::new(0, 10))
            .await
            .unwrap()
            .items
            .last()
            .unwrap()
            .command,
        rollback
    );
    let mut restart = fixture::staged(3);
    restart.document.process.max_concurrent_calcs = 6;
    service.stage(&restart, &fixtures).await.unwrap();
    // A restart-required change cannot be applied hot by the running runtime.
    let mut command = fixture::activate(3, 3, "restart");
    command.mode = ReloadClass::Hot;
    assert_eq!(
        engine
            .activate_configuration(&command, &fixtures)
            .await
            .unwrap_err(),
        ConfigurationError::RestartRequired
    );
    // It commits as restart-required while the runtime keeps its accepted selection.
    let mut command = fixture::activate(3, 3, "restart-later");
    command.mode = ReloadClass::RestartRequired;
    let outcome = engine
        .activate_configuration(&command, &fixtures)
        .await
        .unwrap();
    assert!(outcome.restart_required);
    assert_eq!(outcome.accepted_generation, None);
    assert_eq!(outcome.activation.revision.get(), 4);
    assert_eq!(
        engine.runtime_health().activation_revision.unwrap().get(),
        3
    );
    assert_eq!(
        service
            .active(&rollback.id)
            .await
            .unwrap()
            .unwrap()
            .activation
            .revision
            .get(),
        4
    );
    // Later quotes keep pricing under the accepted revision until the next start.
    engine
        .submit_update(runtime::event(2, 0, "99.25"), time(1000), time(1000))
        .await
        .unwrap();
    let kept = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept.result.status(), ResultStatus::Complete);
    assert_eq!(kept.result.provenance().configuration_version.get(), 1);
    let health = engine.runtime_health();
    assert_eq!(health.activation_revision.unwrap().get(), 3);
    assert_eq!(health.phase, convex_engine::RuntimePhase::Ready);
    // A hot change on top of the deferred selection is still deferred, and readiness
    // keeps targeting the accepted revision instead of waiting for a restart.
    let mut hot = fixture::staged(4);
    hot.document.process.max_concurrent_calcs = 6;
    hot.document.assembly.max_source_skew_ms = 200;
    service.stage(&hot, &fixtures).await.unwrap();
    let outcome = engine
        .activate_configuration(&fixture::activate(4, 4, "hot-on-deferred"), &fixtures)
        .await
        .unwrap();
    assert!(outcome.restart_required);
    assert_eq!(outcome.activation.revision.get(), 5);
    engine
        .submit_update(runtime::event(3, 0, "99.5"), time(1001), time(1001))
        .await
        .unwrap();
    let kept = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept.result.provenance().configuration_version.get(), 1);
    let health = engine.runtime_health();
    assert_eq!(health.activation_revision.unwrap().get(), 3);
    assert!(health.ready);
    assert_eq!(health.phase, convex_engine::RuntimePhase::Ready);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_runtime_started_unselected_keeps_its_plan_after_a_restart_required_activation() {
    let setup = runtime::setup().await;
    let service = cases(&setup);
    let fixtures = fixtures(&setup);
    let engine = engine(&setup, service.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    let mut first = fixture::staged(1);
    first.document.process.max_concurrent_calcs = 6;
    service.stage(&first, &fixtures).await.unwrap();
    let mut command = fixture::activate(1, 0, "unselected-restart");
    command.mode = ReloadClass::RestartRequired;
    let outcome = engine
        .activate_configuration(&command, &fixtures)
        .await
        .unwrap();
    assert!(outcome.restart_required);
    // The runtime keeps pricing under its composed plan until the next start.
    engine
        .submit_update(runtime::event(2, 0, "99.25"), time(1000), time(1000))
        .await
        .unwrap();
    let kept = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept.result.status(), ResultStatus::Complete);
    let health = engine.runtime_health();
    assert_eq!(health.activation_revision, None);
    assert_ne!(health.phase, convex_engine::RuntimePhase::Failed);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn exact_reference_and_capability_failures_do_not_stage_invalid_content() {
    let setup = runtime::setup().await;
    let service = cases(&setup);
    let fixtures = fixtures(&setup);
    let mut missing = fixture::staged(1);
    missing.document.assembly.instruments[0].version = fixture::snapshot::version(999);
    let report = service
        .validate(&missing.document, &fixtures)
        .await
        .unwrap();
    assert!(!report.valid);
    assert!(report.issues.iter().any(|i| i.code == "INVALID_REFERENCE"));
    assert_eq!(
        service.stage(&missing, &fixtures).await.unwrap_err(),
        ConfigurationError::ValidationFailed
    );
    let mut unsupported = fixture::document(1);
    unsupported.process.shared_writers = true;
    let report = service.validate(&unsupported, &fixtures).await.unwrap();
    assert!(!report.valid);
    assert!(report
        .issues
        .iter()
        .any(|i| i.code == "UNSUPPORTED_STORAGE"));
}

#[tokio::test]
async fn committed_unknown_outcomes_reconcile_once_uncommitted_failure_and_false_receipts_fail_closed(
) {
    use std::sync::{atomic::Ordering, Arc};
    let setup = runtime::setup().await;
    let fixtures = fixtures(&setup);
    let document = fixture::document(1);
    let fault = Arc::new(fault::FaultRepository::new(
        setup.storage.configuration.clone(),
    ));
    let service = ConfigurationUseCases::new(
        fault.clone(),
        setup.storage.bonds.clone(),
        setup.storage.configs.clone(),
        setup.storage.curves.clone(),
        Arc::new(convex_ext_file::JsonFixingRepository::empty()),
        setup.snapshots.clone(),
        ConfigurationCapabilities {
            build_id: document.runtime.build_id,
            storage: document.process.storage,
            durable: true,
            atomic_activation: true,
            shared_writers: false,
            result_transport: document.process.result_transport,
            providers: document.providers,
            demo: false,
        },
    );
    fault.unknown_stage.store(true, Ordering::SeqCst);
    service.stage(&fixture::staged(1), &fixtures).await.unwrap();
    fault.unknown_activate.store(true, Ordering::SeqCst);
    let command = fixture::activate(1, 0, "first");
    let first = service.activate(&command, &fixtures).await.unwrap();
    assert_eq!(service.activate(&command, &fixtures).await.unwrap(), first);
    assert_eq!(
        service
            .history(&command.id, &Pagination::new(0, 100))
            .await
            .unwrap()
            .total,
        1
    );
    fault.corrupt_receipt.store(true, Ordering::SeqCst);
    assert_eq!(
        service.activate(&command, &fixtures).await.unwrap_err(),
        ConfigurationError::Conflict
    );
    fault.corrupt_receipt.store(false, Ordering::SeqCst);
    service.stage(&fixture::staged(2), &fixtures).await.unwrap();
    fault.fail_activate.store(true, Ordering::SeqCst);
    let error = service
        .activate(&fixture::activate(2, 1, "failed"), &fixtures)
        .await
        .unwrap_err();
    assert_eq!(error, ConfigurationError::Storage);
    assert!(!format!("{error:?} {error}").contains("PRIVATE_ADAPTER_MARKER"));
    assert_eq!(
        service
            .active(&command.id)
            .await
            .unwrap()
            .unwrap()
            .activation,
        first
    );
    assert!(service
        .repository()
        .activation_for_request(&RequestId::new("failed").unwrap())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn live_override_expiry_schedules_a_policy_refresh_without_a_quote_or_periodic_poll() {
    use std::sync::Arc;
    let setup = runtime::setup().await;
    let now = UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap();
    let (service, first, fixture_cases) = live_selection(&setup, now, 50).await;
    let expiry =
        UtcTimestamp::from_datetime(now.datetime() + chrono::Duration::seconds(2)).unwrap();
    let mut second = first.clone();
    second.document.pricing.version = 2;
    second.document.pricing.updated_at = now.datetime().timestamp_millis();
    second.document.assembly.configuration.version = fixture::snapshot::version(2);
    second.idempotency_key = RequestId::new("live-stage-second").unwrap();
    second.document.overrides = vec![PolicyOverride {
        id: SourceId::new("live-age-exception").unwrap(),
        scope: second.document.assembly.quotes[0].key.clone(),
        max_age_ms: 5000,
        actor: "fixture-operator".into(),
        reason: "Synthetic timed policy test".into(),
        created_at: now,
        expires_at: expiry,
        approved_by: "fixture-approver".into(),
        approved_at: now,
    }];
    service.stage(&second, &fixture_cases).await.unwrap();
    let mut command = fixture::activate(2, 1, "live-override");
    command.activated_at = now;
    service.activate(&command, &fixture_cases).await.unwrap();
    let source = Arc::new(subscription_source::SubscriptionSource::new());
    let engine = live_engine(&setup, service, &first.document, source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    let stamp = subscription_source::monotone_stamp();
    source.send(fixture::snapshot::event(1, stamp, stamp, "99.25"));
    let good = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(good.result.status(), ResultStatus::Complete);
    assert_eq!(good.live_valid_until, Some(expiry));
    let expired = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(expired.result.status(), ResultStatus::Failed);
    assert_eq!(expired.last_good, Some(good.result.clone()));
    assert!(!engine.runtime_health().ready);
    // The refresh ran at the expiry under the 50 ms base age, so the two-second-old quote is
    // stale. Live snapshots are not stored.
    assert!(expired.result.provenance().valuation_time >= expiry);
    assert!(setup
        .snapshots
        .get(&expired.result.provenance().snapshot_id)
        .await
        .unwrap()
        .is_none());
    engine.shutdown().await.unwrap();
    assert!(source
        .unsubscribed
        .load(std::sync::atomic::Ordering::SeqCst));
}

/// D56: a Live runtime keeps the selection it accepted until a refresh or a restart applies
/// another. A quote does not read the active configuration, so an activation committed
/// without the engine waits for the engine's refresh or the next start, either of which
/// re-resolves the pinned inputs.
#[tokio::test]
async fn live_runtime_applies_an_activation_at_refresh_not_at_the_next_quote() {
    use std::sync::Arc;
    let setup = runtime::setup().await;
    let now = UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap();
    let (service, first, fixture_cases) = live_selection(&setup, now, 600_000).await;
    let mut second = first.clone();
    second.document.pricing.version = 2;
    second.document.pricing.updated_at = now.datetime().timestamp_millis();
    second.document.assembly.configuration.version = fixture::snapshot::version(2);
    second.idempotency_key = RequestId::new("live-stage-repriced").unwrap();
    service.stage(&second, &fixture_cases).await.unwrap();
    let source = Arc::new(subscription_source::SubscriptionSource::new());
    let engine = live_engine(&setup, service.clone(), &first.document, source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    let quote = |sequence, value| {
        let stamp = subscription_source::monotone_stamp();
        source.send(fixture::snapshot::event(sequence, stamp, stamp, value));
    };
    quote(1, "99.25");
    let held = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.result.status(), ResultStatus::Complete);
    assert_eq!(held.result.provenance().configuration_version.get(), 1);
    // Committed without the engine: the next quote keeps the held selection.
    let mut command = fixture::activate(2, 1, "live-repriced");
    command.activated_at = now;
    service.activate(&command, &fixture_cases).await.unwrap();
    quote(2, "99.5");
    let kept = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept.result.status(), ResultStatus::Complete);
    assert_eq!(kept.result.provenance().configuration_version.get(), 1);
    assert_eq!(
        engine.runtime_health().activation_revision.unwrap().get(),
        1
    );
    // The engine's retry of the same activation refreshes the runtime, which values
    // under the new configuration version.
    let outcome = engine
        .activate_configuration(&command, &fixture_cases)
        .await
        .unwrap();
    assert!(!outcome.restart_required);
    let applied = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(applied.result.status(), ResultStatus::Complete);
    assert_eq!(applied.result.provenance().configuration_version.get(), 2);
    let health = engine.runtime_health();
    assert_eq!(health.activation_revision.unwrap().get(), 2);
    assert!(health.ready);
    engine.shutdown().await.unwrap();
    drop(engine);
    // Committed while the engine is stopped, as the offline CLI does: the restart reads the
    // active selection and refreshes to it before the first quote.
    let mut third = first.clone();
    third.document.pricing.version = 3;
    third.document.pricing.updated_at = now.datetime().timestamp_millis();
    third.document.assembly.configuration.version = fixture::snapshot::version(3);
    third.idempotency_key = RequestId::new("live-stage-restarted").unwrap();
    service.stage(&third, &fixture_cases).await.unwrap();
    let mut command = fixture::activate(3, 2, "live-restarted");
    command.activated_at = now;
    service.activate(&command, &fixture_cases).await.unwrap();
    let source = Arc::new(subscription_source::SubscriptionSource::new());
    let restarted = live_engine(&setup, service, &first.document, source.clone());
    restarted.start().await.unwrap();
    assert_eq!(
        restarted
            .runtime_health()
            .activation_revision
            .unwrap()
            .get(),
        3
    );
    let mut updates = restarted.subscribe_results();
    let stamp = subscription_source::monotone_stamp();
    source.send(fixture::snapshot::event(3, stamp, stamp, "99.75"));
    let restored = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.result.status(), ResultStatus::Complete);
    assert_eq!(restored.result.provenance().configuration_version.get(), 3);
    restarted.shutdown().await.unwrap();
}

/// A Live configuration of `convex_engine::MAX_RUNTIME_INSTRUMENTS` copies of the fixture
/// bond at `now`, with one fixture of an image and an expectation an instrument, and the
/// use cases that check it.
async fn wide_live(
    setup: &runtime::Setup,
    now: UtcTimestamp,
) -> (
    std::sync::Arc<ConfigurationUseCases>,
    StagedConfiguration,
    Vec<ConfigurationFixture>,
) {
    let mut staged = fixture::staged(1);
    staged.staged_at = now;
    let document = &mut staged.document;
    document.runtime.mode = RuntimeMode::Live;
    document.providers[0]
        .capabilities
        .push(ProviderCapability::LiveImages);
    document.assembly.as_of = now;
    document.assembly.known_at = now;
    document.assembly.quotes[0].max_age_ms = 600_000;
    document.assembly.curves[0].source_time = now;
    document.assembly.curves[0].recorded_at = now;
    document.assembly.curves[0].max_age_ms = 600_000;
    let mut wide = plan(document, vec![]);
    runtime::widen(setup, &mut wide, convex_engine::MAX_RUNTIME_INSTRUMENTS).await;
    document.assembly.instruments = wide.assembly.instruments;
    document.assembly.quotes = wide.assembly.quotes;

    let stamp = now.datetime().timestamp_millis();
    let mut case = fixtures(setup).remove(0);
    case.as_of = now;
    case.known_at = now;
    case.events = document
        .assembly
        .quotes
        .iter()
        .map(|quote| {
            let mut event = fixture::snapshot::event(1, stamp, stamp, "99.25");
            event.key = quote.key.clone();
            event
        })
        .collect();
    let expected = case.expected[0].clone();
    case.expected = document
        .assembly
        .instruments
        .iter()
        .map(|pin| ExpectedConfigurationMeasure {
            instrument_id: pin.instrument_id.clone(),
            ..expected.clone()
        })
        .collect();
    let doc = &staged.document;
    let service = std::sync::Arc::new(ConfigurationUseCases::new(
        setup.storage.configuration.clone(),
        setup.storage.bonds.clone(),
        setup.storage.configs.clone(),
        setup.storage.curves.clone(),
        std::sync::Arc::new(convex_ext_file::JsonFixingRepository::empty()),
        setup.snapshots.clone(),
        ConfigurationCapabilities {
            build_id: doc.runtime.build_id.clone(),
            storage: doc.process.storage.clone(),
            durable: true,
            atomic_activation: true,
            shared_writers: false,
            result_transport: doc.process.result_transport.clone(),
            providers: doc.providers.clone(),
            demo: false,
        },
    ));
    (service, staged, vec![case])
}

/// A Live engine for `document`'s plan that keeps no results, fed by `source`.
fn display_engine(
    setup: &runtime::Setup,
    service: std::sync::Arc<ConfigurationUseCases>,
    document: &ConfigurationDocument,
    source: std::sync::Arc<subscription_source::SubscriptionSource>,
) -> convex_engine::PricingEngine {
    convex_engine::PricingEngineBuilder::new()
        .with_storage(setup.storage.clone())
        .with_configuration_use_cases(service)
        .with_runtime(convex_engine::RuntimeServices {
            plan: plan(document, vec![]),
            snapshots: setup.snapshots.clone(),
            fixings: std::sync::Arc::new(convex_ext_file::JsonFixingRepository::empty()),
            store: setup.store.clone(),
            publisher: runtime::display_only(),
            input: convex_engine::RuntimeInput::Live(source),
        })
        .build()
        .unwrap()
}

/// D101: a Live configuration of 4,096 instruments passes its dry run, which values every
/// instrument, is staged and activated, and a Live runtime starts on it. As a Captured or
/// Replay configuration it is refused past 256 instruments.
#[tokio::test]
async fn a_live_configuration_of_4096_instruments_is_checked_staged_and_activated() {
    let setup = runtime::setup().await;
    let now = UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap();
    let (service, staged, cases) = wide_live(&setup, now).await;
    let document = &staged.document;
    assert_eq!(document.assembly.instruments.len(), 4096);
    document.validate().unwrap();
    for mode in [RuntimeMode::Captured, RuntimeMode::Replay] {
        let mut stored = document.clone();
        stored.runtime.mode = mode;
        assert!(stored.validate().is_err(), "{mode:?}");
    }
    let report = service.validate(document, &cases).await.unwrap();
    assert!(report.valid, "{:?}", report.issues);
    service.stage(&staged, &cases).await.unwrap();
    let mut command = fixture::activate(1, 0, "live-wide");
    command.activated_at = now;
    service.activate(&command, &cases).await.unwrap();

    let source = std::sync::Arc::new(subscription_source::SubscriptionSource::new());
    let engine = display_engine(&setup, service, document, source);
    engine.start().await.unwrap();
    engine.shutdown().await.unwrap();
}

/// Read `updates` until each of 4,096 instruments has a complete record of configuration
/// `version` or later and of generation `at_least` or later; the ticks they came in.
async fn every_result(
    updates: &mut tokio::sync::broadcast::Receiver<
        std::sync::Arc<convex_ports::runtime::PublishedValuation>,
    >,
    version: u64,
    at_least: u64,
) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    let mut ticks = std::collections::BTreeSet::new();
    tokio::time::timeout(Duration::from_secs(120), async {
        while seen.len() < 4096 {
            let record = updates.recv().await.unwrap();
            if record.result.status() == ResultStatus::Complete
                && record.result.provenance().configuration_version.get() >= version
                && record.generation.get() >= at_least
            {
                ticks.insert(record.generation.get());
                seen.insert(serde_json::to_string(&record.result.provenance().subject).unwrap());
            }
        }
    })
    .await
    .unwrap();
    ticks.len()
}

/// This process's resident memory, in MiB.
fn resident_mib() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .trim()
        .parse::<f64>()
        .unwrap()
        / 1024.0
}

/// P17b's measurement of a Live runtime of 4,096 instruments (`--ignored --nocapture`,
/// under the ci profile): the dry run; the first ticks after a start, which pin every
/// reference and value every bond; a tick in which every quote moved; one quote's tick;
/// the first tick after an activation, which values every bond again; and the memory.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement; run with --ignored --nocapture under the ci profile"]
async fn live_ticks_at_4096_instruments() {
    use std::{sync::Arc, time::Instant};
    let setup = runtime::setup().await;
    let now = UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap();
    let (service, first, cases) = wide_live(&setup, now).await;
    let started = Instant::now();
    let report = service.validate(&first.document, &cases).await.unwrap();
    assert!(report.valid);
    println!(
        "dry run of 4,096 instruments: {:.0} ms",
        started.elapsed().as_secs_f64() * 1e3
    );
    service.stage(&first, &cases).await.unwrap();
    let mut command = fixture::activate(1, 0, "live-wide");
    command.activated_at = now;
    service.activate(&command, &cases).await.unwrap();
    let mut second = first.clone();
    second.document.pricing.version = 2;
    second.document.pricing.updated_at = now.datetime().timestamp_millis();
    second.document.assembly.configuration.version = fixture::snapshot::version(2);
    second.idempotency_key = RequestId::new("live-wide-repriced").unwrap();
    service.stage(&second, &cases).await.unwrap();

    let source = Arc::new(subscription_source::SubscriptionSource::with_capacity(8192));
    let before = resident_mib();
    let engine = display_engine(&setup, service, &first.document, source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    let keys: Vec<_> = first
        .document
        .assembly
        .quotes
        .iter()
        .map(|q| q.key.clone())
        .collect();
    let mut sequence = 1;
    let send_all = |sequence: u64, value: &str| {
        for key in &keys {
            let stamp = subscription_source::monotone_stamp();
            let mut event = fixture::snapshot::event(sequence, stamp, stamp, value);
            event.key = key.clone();
            source.send(event);
        }
    };
    let started = Instant::now();
    send_all(sequence, "99.25");
    let ticks = every_result(&mut updates, 1, 0).await;
    println!(
        "first images to every result: {:.0} ms over {ticks} ticks",
        started.elapsed().as_secs_f64() * 1e3
    );
    let generation = engine.runtime_health().generation;
    sequence += 1;
    let started = Instant::now();
    send_all(sequence, "99.50");
    let ticks = every_result(&mut updates, 1, generation + 1).await;
    println!(
        "every quote moved, 4,096 sent at once: {:.0} ms over {ticks} ticks",
        started.elapsed().as_secs_f64() * 1e3
    );
    let mut single = Vec::new();
    for round in 0..40u64 {
        sequence += 1;
        let stamp = subscription_source::monotone_stamp();
        let price = if round % 2 == 0 { "99.30" } else { "99.25" };
        let mut event = fixture::snapshot::event(sequence, stamp, stamp, price);
        event.key = keys[2048].clone();
        let started = Instant::now();
        source.send(event);
        tokio::time::timeout(Duration::from_secs(5), updates.recv())
            .await
            .unwrap()
            .unwrap();
        single.push(started.elapsed().as_secs_f64() * 1e6);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    single.sort_by(f64::total_cmp);
    println!(
        "one quote to its record: median {:.0} µs",
        single[single.len() / 2]
    );

    let mut command = fixture::activate(2, 1, "live-wide-repriced");
    command.activated_at = now;
    let started = Instant::now();
    engine
        .activate_configuration(&command, &cases)
        .await
        .unwrap();
    let activated = started.elapsed();
    let ticks = every_result(&mut updates, 2, 0).await;
    println!(
        "activation (its dry run included): {:.0} ms; then every result under it: {:.0} ms \
         over {ticks} ticks",
        activated.as_secs_f64() * 1e3,
        (started.elapsed() - activated).as_secs_f64() * 1e3
    );

    let results = engine.runtime_results();
    let bytes: usize = results
        .iter()
        .map(|r| serde_json::to_vec(r).unwrap().len())
        .sum();
    println!(
        "4,096 results: {:.1} MB as JSON; resident memory {:.0} MiB before the engine, {:.0} MiB now",
        bytes as f64 / 1e6,
        before,
        resident_mib()
    );
    engine.shutdown().await.unwrap();
}

/// A Live selection staged and activated at `now` as revision 1, with the fixtures it was
/// checked against and the use cases that hold it.
async fn live_selection(
    setup: &runtime::Setup,
    now: UtcTimestamp,
    quote_max_age_ms: i64,
) -> (
    std::sync::Arc<ConfigurationUseCases>,
    StagedConfiguration,
    Vec<ConfigurationFixture>,
) {
    let mut first = fixture::staged(1);
    first.staged_at = now;
    first.document.runtime.mode = RuntimeMode::Live;
    first.document.providers[0]
        .capabilities
        .push(ProviderCapability::LiveImages);
    first.document.assembly.as_of = now;
    first.document.assembly.known_at = now;
    first.document.assembly.quotes[0].max_age_ms = quote_max_age_ms;
    first.document.assembly.curves[0].source_time = now;
    first.document.assembly.curves[0].recorded_at = now;
    first.document.assembly.curves[0].max_age_ms = 60_000;
    let mut fixture_cases = fixtures(setup);
    fixture_cases[0].as_of = now;
    fixture_cases[0].known_at = now;
    fixture_cases[0].events = vec![fixture::snapshot::event(
        1,
        now.datetime().timestamp_millis(),
        now.datetime().timestamp_millis(),
        "99.25",
    )];
    let doc = &first.document;
    let service = std::sync::Arc::new(ConfigurationUseCases::new(
        setup.storage.configuration.clone(),
        setup.storage.bonds.clone(),
        setup.storage.configs.clone(),
        setup.storage.curves.clone(),
        std::sync::Arc::new(convex_ext_file::JsonFixingRepository::empty()),
        setup.snapshots.clone(),
        ConfigurationCapabilities {
            build_id: doc.runtime.build_id.clone(),
            storage: doc.process.storage.clone(),
            durable: true,
            atomic_activation: true,
            shared_writers: false,
            result_transport: doc.process.result_transport.clone(),
            providers: doc.providers.clone(),
            demo: false,
        },
    ));
    service.stage(&first, &fixture_cases).await.unwrap();
    let mut command = fixture::activate(1, 0, "live-first");
    command.activated_at = now;
    service.activate(&command, &fixture_cases).await.unwrap();
    (service, first, fixture_cases)
}

/// A Live engine for `document`'s plan, fed by `source`.
fn live_engine(
    setup: &runtime::Setup,
    service: std::sync::Arc<ConfigurationUseCases>,
    document: &ConfigurationDocument,
    source: std::sync::Arc<subscription_source::SubscriptionSource>,
) -> convex_engine::PricingEngine {
    convex_engine::PricingEngineBuilder::new()
        .with_storage(setup.storage.clone())
        .with_configuration_use_cases(service)
        .with_runtime(convex_engine::RuntimeServices {
            plan: plan(document, vec![]),
            snapshots: setup.snapshots.clone(),
            fixings: std::sync::Arc::new(convex_ext_file::JsonFixingRepository::empty()),
            store: setup.store.clone(),
            publisher: runtime::archive(setup),
            input: convex_engine::RuntimeInput::Live(source),
        })
        .build()
        .unwrap()
}

/// A stored instrument: the fixture bond under `id`, committed at `at`.
async fn store_bond(setup: &runtime::Setup, id: &str, at: UtcTimestamp) {
    let mut bond = fixture::snapshot::bond();
    bond.instrument_id = convex_core::ids::InstrumentId::new(id);
    bond.issuer_type = convex_ports::reference_data::IssuerType::Sovereign;
    bond.cusip = Some("91282CBB6".into());
    bond.isin = None;
    setup
        .storage
        .bonds
        .commit(
            &convex_ports::reference_repository::ReferenceCommand {
                idempotency_key: RequestId::new(format!("store-{id}")).unwrap(),
                changes: vec![
                    convex_ports::reference_repository::ReferenceChange::Create {
                        bond: Box::new(bond),
                    },
                ],
                master_changes: vec![],
            },
            at,
        )
        .await
        .unwrap();
}

/// A Live runtime pins each bond's pricing rule with its references, once per activation.
/// A rule committed while it runs reaches it at the next activation, not the next quote.
/// A roll to a Treasury the runtime does not value changes its universe, which only a
/// restart applies.
#[tokio::test]
async fn a_live_runtime_reads_a_new_rule_at_its_next_activation_and_a_new_benchmark_at_restart() {
    use std::sync::Arc;
    let setup = runtime::setup().await;
    let now = UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap();
    let (service, first, fixture_cases) = live_selection(&setup, now, 600_000).await;
    let source = Arc::new(subscription_source::SubscriptionSource::new());
    let engine = live_engine(&setup, service.clone(), &first.document, source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    let quote = |sequence, value| {
        let stamp = subscription_source::monotone_stamp();
        source.send(fixture::snapshot::event(sequence, stamp, stamp, value));
    };
    let reads_rule = |record: &convex_ports::runtime::PublishedValuation| {
        record
            .result
            .provenance()
            .input_versions
            .iter()
            .any(|version| version.kind.as_str() == "pricing_rule")
    };
    let rule: convex_ports::reference_master::MasterChange =
        serde_json::from_value(serde_json::json!({
            "expected_version": null,
            "definition": {"kind": "pricing_rule", "definition": {
                "instrument_id": "BOND-001",
                "method": {"kind": "price"},
                "effective": {"from": "2025-01-01T00:00:00Z", "until": null},
                "provenance": {"source": "desk", "evidence_reference": "activation-test"},
            }},
        }))
        .unwrap();
    setup
        .storage
        .bonds
        .commit(
            &convex_ports::reference_repository::ReferenceCommand {
                idempotency_key: RequestId::new("price-rule").unwrap(),
                changes: vec![],
                master_changes: vec![rule],
            },
            now,
        )
        .await
        .unwrap();
    quote(1, "99.25");
    let held = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.result.status(), ResultStatus::Complete);
    assert!(!reads_rule(&held), "the activation pinned no rule");

    let mut second = first.clone();
    second.document.pricing.version = 2;
    second.document.pricing.updated_at = now.datetime().timestamp_millis();
    second.document.assembly.configuration.version = fixture::snapshot::version(2);
    second.idempotency_key = RequestId::new("live-stage-rule").unwrap();
    service.stage(&second, &fixture_cases).await.unwrap();
    let mut command = fixture::activate(2, 1, "live-rule");
    command.activated_at = now;
    engine
        .activate_configuration(&command, &fixture_cases)
        .await
        .unwrap();
    let applied = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(applied.result.status(), ResultStatus::Complete);
    assert!(reads_rule(&applied));

    // An on-the-run roll to a Treasury the runtime does not value.
    store_bond(&setup, "UST-10Y-NEW", now).await;
    let mut rolled = second.clone();
    rolled.document.pricing.version = 3;
    rolled.document.assembly.configuration.version = fixture::snapshot::version(3);
    rolled.idempotency_key = RequestId::new("live-stage-roll").unwrap();
    rolled.document.assembly.instruments.push(InstrumentPin {
        instrument_id: InstrumentId::new("UST-10Y-NEW").unwrap(),
        version: Version::new(1).unwrap(),
    });
    let mut new_quote = rolled.document.assembly.quotes[0].clone();
    new_quote.key.instrument_id = convex_core::ids::InstrumentId::new("UST-10Y-NEW");
    rolled.document.assembly.quotes.push(new_quote);
    let mut roll_cases = fixture_cases.clone();
    let mut image = roll_cases[0].events[0].clone();
    image.key.instrument_id = convex_core::ids::InstrumentId::new("UST-10Y-NEW");
    roll_cases[0].events.push(image);
    let mut expected = roll_cases[0].expected[0].clone();
    expected.instrument_id = InstrumentId::new("UST-10Y-NEW").unwrap();
    roll_cases[0].expected.push(expected);
    let report = service
        .validate(&rolled.document, &roll_cases)
        .await
        .unwrap();
    assert!(
        report
            .changes
            .iter()
            .any(|change| change.field == "assembly.instruments"
                && change.reload == ReloadClass::RestartRequired),
        "{:?}",
        report.changes
    );
    service.stage(&rolled, &roll_cases).await.unwrap();
    let mut command = fixture::activate(3, 2, "live-roll");
    command.activated_at = now;
    assert_eq!(
        engine
            .activate_configuration(&command, &roll_cases)
            .await
            .unwrap_err(),
        ConfigurationError::RestartRequired
    );
    engine.shutdown().await.unwrap();
}
