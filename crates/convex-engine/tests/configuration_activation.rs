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
    let before = std::fs::read_dir(setup.dir.path().join("snapshots"))
        .unwrap()
        .count();
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
    assert_eq!(
        std::fs::read_dir(setup.dir.path().join("snapshots"))
            .unwrap()
            .count(),
        before
    );
    let mut wrong = fixtures.clone();
    wrong[0].expected[0].value = FiniteF64::new(0.0).unwrap();
    let mismatch = service
        .validate(&fixture::document(1), &wrong)
        .await
        .unwrap();
    assert!(!mismatch.valid);
    assert!(mismatch.issues.iter().any(|i| i.code == "FIXTURE_MISMATCH"));
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
            .activate(&fixture::activate(1, 0, "first"), &fixtures, false)
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
            .activate(&fixture::activate(2, 1, "second"), &fixtures, true)
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
        .activate(&fixture::activate(1, 0, "first"), &fixtures, false)
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
    assert_eq!(restarted.runtime_results(), [expired]);
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn cvx010_ac3_rollback_revalidates_content_and_records_new_activation_and_runtime_version() {
    let setup = runtime::setup().await;
    let service = cases(&setup);
    let fixtures = fixtures(&setup);
    service.stage(&fixture::staged(1), &fixtures).await.unwrap();
    let first = service
        .activate(&fixture::activate(1, 0, "first"), &fixtures, false)
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
    let mut command = fixture::activate(3, 3, "restart");
    command.mode = ReloadClass::RestartRequired;
    assert_eq!(
        engine
            .activate_configuration(&command, &fixtures)
            .await
            .unwrap_err(),
        ConfigurationError::RestartRequired
    );
    command.mode = ReloadClass::Hot;
    assert_eq!(
        engine
            .activate_configuration(&command, &fixtures)
            .await
            .unwrap_err(),
        ConfigurationError::RestartRequired
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
        3
    );
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
    let first = service.activate(&command, &fixtures, false).await.unwrap();
    assert_eq!(
        service.activate(&command, &fixtures, false).await.unwrap(),
        first
    );
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
        service
            .activate(&command, &fixtures, false)
            .await
            .unwrap_err(),
        ConfigurationError::Conflict
    );
    fault.corrupt_receipt.store(false, Ordering::SeqCst);
    service.stage(&fixture::staged(2), &fixtures).await.unwrap();
    fault.fail_activate.store(true, Ordering::SeqCst);
    let error = service
        .activate(&fixture::activate(2, 1, "failed"), &fixtures, true)
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
    use convex_engine::{PricingEngineBuilder, RuntimeInput, RuntimeServices};
    use convex_ext_file::*;
    use std::sync::Arc;
    let setup = runtime::setup().await;
    let now = UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap();
    let mut first = fixture::staged(1);
    first.staged_at = now;
    first.document.runtime.mode = RuntimeMode::Live;
    first.document.providers[0]
        .capabilities
        .push(ProviderCapability::LiveImages);
    first.document.assembly.as_of = now;
    first.document.assembly.known_at = now;
    first.document.assembly.quotes[0].max_age_ms = 50;
    first.document.assembly.curves[0].source_time = now;
    first.document.assembly.curves[0].recorded_at = now;
    first.document.assembly.curves[0].max_age_ms = 60_000;
    let mut fixture_cases = fixtures(&setup);
    fixture_cases[0].as_of = now;
    fixture_cases[0].known_at = now;
    fixture_cases[0].events = vec![fixture::snapshot::event(
        1,
        now.datetime().timestamp_millis(),
        now.datetime().timestamp_millis(),
        "99.25",
    )];
    let doc = &first.document;
    let service = Arc::new(ConfigurationUseCases::new(
        setup.storage.configuration.clone(),
        setup.storage.bonds.clone(),
        setup.storage.configs.clone(),
        Arc::new(JsonFixingRepository::empty()),
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
    service
        .activate(&command, &fixture_cases, false)
        .await
        .unwrap();
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
    service
        .activate(&command, &fixture_cases, false)
        .await
        .unwrap();
    let source = Arc::new(subscription_source::SubscriptionSource::new());
    let mut market = runtime::market_data();
    Arc::get_mut(&mut market).unwrap().quotes = source.clone();
    let engine = PricingEngineBuilder::new()
        .with_market_data(market)
        .with_reference_data(Arc::new(create_empty_reference_data()))
        .with_storage(setup.storage.clone())
        .with_configuration_use_cases(service)
        .with_runtime(RuntimeServices {
            plan: plan(&first.document, vec![]),
            snapshots: setup.snapshots.clone(),
            fixings: Arc::new(JsonFixingRepository::empty()),
            store: setup.store.clone(),
            publisher: runtime::archive(&setup),
            input: RuntimeInput::Live,
        })
        .build()
        .unwrap();
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    let stamp = chrono::Utc::now().timestamp_millis();
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
    assert_eq!(expired.last_good, Some(good.result));
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
