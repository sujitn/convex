#[path = "../../convex-ports/tests/support/snapshot_fixtures.rs"]
mod fixture;

use convex_core::{contracts::*, Currency};
use convex_engine::{cache::ValuationCache, context::*, snapshot::*};
use convex_ext_file::{FileSnapshotRepository, JsonFixingRepository};
use convex_ports::{quote_event::*, reference_repository::*, snapshot::*, storage::*};
use std::sync::Arc;

struct Setup {
    _directory: tempfile::TempDir,
    storage: StorageAdapter,
    snapshots: Arc<FileSnapshotRepository>,
    assembler: SnapshotAssembler,
    request: SnapshotAssembly,
    events: Vec<QuoteEvent>,
}
async fn setup(fixings: Vec<convex_ports::fixing::PublishedFixing>) -> Setup {
    let directory = tempfile::tempdir().unwrap();
    let storage =
        convex_ext_redb::create_redb_storage(directory.path().join("inputs.redb")).unwrap();
    let bond = fixture::bond();
    storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("snapshot-seed").unwrap(),
                changes: vec![ReferenceChange::Create {
                    bond: Box::new(bond.clone()),
                }],
                master_changes: fixture::reference_terms::resources(&bond),
            },
            fixture::time("2025-01-01T00:00:00Z"),
        )
        .await
        .unwrap();
    storage.configs.save(&fixture::config()).await.unwrap();
    let path = directory.path().join("fixings.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({"schema_version":1,"fixings":fixings})).unwrap(),
    )
    .unwrap();
    let fixings = Arc::new(JsonFixingRepository::new(&path).unwrap());
    let snapshots =
        Arc::new(FileSnapshotRepository::new(directory.path().join("snapshots")).unwrap());
    let assembler = SnapshotAssembler::new(
        storage.bonds.clone(),
        storage.configs.clone(),
        fixings,
        snapshots.clone(),
    );
    let time = fixture::time("2026-10-02T15:00:00Z");
    let request = SnapshotAssembly {
        as_of: time,
        known_at: time,
        generation: fixture::version(1),
        configuration: ConfigurationPin {
            id: SourceId::new(fixture::config().config_id).unwrap(),
            version: fixture::version(1),
        },
        configuration_capture: None,
        instruments: vec![InstrumentPin {
            instrument_id: InstrumentId::new(bond.instrument_id.as_str()).unwrap(),
            version: fixture::version(1),
        }],
        quotes: vec![fixture::quote_requirement()],
        fixings: vec![],
        required_curves: vec![fixture::curve().curve_id],
        curves: vec![fixture::curve()],
        curve_recipes: vec![],
        calibration: None,
        max_source_skew_ms: 1000,
    };
    let events = vec![fixture::event(
        1,
        time.datetime().timestamp_millis(),
        time.datetime().timestamp_millis(),
        "99.25",
    )];
    Setup {
        _directory: directory,
        storage,
        snapshots,
        assembler,
        request,
        events,
    }
}
fn context(snapshot: &MarketSnapshot) -> ValuationContext {
    ValuationContext::new(ValuationContextSpec {
        snapshot_id: snapshot.id().clone(),
        generation: snapshot.content().generation,
        subject: ValuationSubject::InstrumentVersion {
            instrument_id: InstrumentId::new("BOND-001").unwrap(),
            version: snapshot.content().references[0].record.version,
        },
        valuation_time: snapshot.content().as_of,
        known_at: snapshot.content().known_at,
        settlement_date: BusinessDate::parse("2026-10-06").unwrap(),
        configuration: ConfigurationPin {
            id: SourceId::new(&snapshot.content().configuration.config_id).unwrap(),
            version: fixture::version(snapshot.content().configuration.version),
        },
        model_id: ModelId::new(BOND_SNAPSHOT_MODEL).unwrap(),
        build_id: ModelId::new("fixture-build-008").unwrap(),
        reporting_currency: Currency::USD,
        fixing_known_at: snapshot.content().known_at,
        scenario: ValuationScenario::Base,
        numerical_settings: NumericalSettings {
            policy_id: ModelId::new(BOND_NUMERICAL_POLICY).unwrap(),
            bump_bps: FiniteF64::new(10.0).unwrap(),
        },
        measures: [
            "clean_price_mid",
            "dirty_price_mid",
            "accrued_interest",
            "ytm_mid",
        ]
        .into_iter()
        .map(|m| MeasureId::new(m).unwrap())
        .collect(),
    })
    .unwrap()
}
fn valuator() -> SnapshotValuator {
    SnapshotValuator::new(ModelId::new("fixture-build-008").unwrap(), 64).unwrap()
}
fn calculate(snapshot: &MarketSnapshot, context: &ValuationContext) -> SnapshotValuationResult {
    valuator()
        .value(
            snapshot,
            context,
            RequestId::new("replay-result").unwrap(),
            fixture::time("2026-10-04T10:00:00Z"),
        )
        .unwrap()
}

#[tokio::test]
async fn cvx008_ac1_replay_from_reopened_inputs_reproduces_results_and_retains_cache_lineage() {
    let setup = setup(vec![]).await;
    let snapshot = setup
        .assembler
        .assemble(&setup.request, &setup.events)
        .await
        .unwrap();
    let replay = setup.snapshots.get(snapshot.id()).await.unwrap().unwrap();
    let ctx = context(&snapshot);
    let first = calculate(&snapshot, &ctx);
    let second = calculate(
        &replay,
        &serde_json::from_slice(&serde_json::to_vec(&ctx).unwrap()).unwrap(),
    );
    assert_eq!(first, second);
    assert_eq!(first.envelope().status(), ResultStatus::Complete);
    assert_eq!(first.envelope().results().len(), 4);
    let service = valuator();
    let first = service
        .value(
            &snapshot,
            &ctx,
            RequestId::new("original-request").unwrap(),
            fixture::time("2026-10-04T10:00:00Z"),
        )
        .unwrap();
    let cached = service
        .value(
            &snapshot,
            &ctx,
            RequestId::new("next-request").unwrap(),
            fixture::time("2026-10-04T11:00:00Z"),
        )
        .unwrap();
    assert_eq!(first.envelope().results(), cached.envelope().results());
    assert_eq!(
        first.envelope().provenance(),
        cached.envelope().provenance()
    );
    assert_eq!(cached.envelope().request_id().as_str(), "next-request");
    // A current repository correction does not change an already-published replay.
    let mut corrected = fixture::bond();
    corrected.coupon_rate = Some(rust_decimal::Decimal::new(6, 2));
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("snapshot-correction").unwrap(),
                changes: vec![ReferenceChange::Update {
                    bond: Box::new(corrected),
                    expected_version: fixture::version(1),
                }],
                master_changes: vec![],
            },
            fixture::time("2026-10-02T14:00:00Z"),
        )
        .await
        .unwrap();
    assert_eq!(calculate(&replay, &ctx), second);
    let mut request = setup.request.clone();
    request.instruments[0].version = fixture::version(2);
    request.generation = fixture::version(2);
    let changed = setup
        .assembler
        .assemble(&request, &setup.events)
        .await
        .unwrap();
    assert_ne!(changed.id(), snapshot.id());
    let changed_result = calculate(&changed, &context(&changed));
    assert_ne!(first.cache_key(), changed_result.cache_key());
    assert_ne!(
        first
            .envelope()
            .results()
            .get(&MeasureId::new("ytm_mid").unwrap()),
        changed_result
            .envelope()
            .results()
            .get(&MeasureId::new("ytm_mid").unwrap())
    );
}

#[test]
fn cvx008_ac1_every_context_dimension_is_a_cache_compatibility_input() {
    let snapshot = fixture::snapshot();
    let ctx = context(&snapshot);
    let cache = ValuationCache::new(1).unwrap();
    let result = calculate(&snapshot, &ctx);
    cache.insert(result.clone()).unwrap();
    for dimension in 0..14 {
        let mut spec = ctx.spec().clone();
        match dimension {
            0 => {
                spec.subject = ValuationSubject::InstrumentVersion {
                    instrument_id: InstrumentId::new("BOND-001").unwrap(),
                    version: fixture::version(2),
                }
            }
            1 => spec.model_id = ModelId::new("changed-model").unwrap(),
            2 => spec.settlement_date = BusinessDate::parse("2026-10-07").unwrap(),
            3 => {
                spec.measures.pop();
            }
            4 => spec.build_id = ModelId::new("changed-build").unwrap(),
            5 => spec.configuration.version = fixture::version(2),
            6 => spec.generation = fixture::version(2),
            7 => {
                spec.scenario = ValuationScenario::Named {
                    id: ModelId::new("rates-up").unwrap(),
                    version: fixture::version(1),
                }
            }
            8 => spec.numerical_settings.bump_bps = FiniteF64::new(5.0).unwrap(),
            9 => spec.fixing_known_at = fixture::time("2026-10-02T14:00:00Z"),
            10 => spec.reporting_currency = Currency::EUR,
            11 => spec.valuation_time = fixture::time("2026-10-02T14:00:00Z"),
            12 => spec.known_at = fixture::time("2026-10-02T14:00:00Z"),
            _ => spec.snapshot_id = SnapshotId::new("another-snapshot").unwrap(),
        }
        let changed = ValuationContext::new(spec).unwrap();
        assert!(
            cache.get(&changed).unwrap().is_none(),
            "dimension {dimension}"
        );
        assert_ne!(ctx.cache_key().unwrap(), changed.cache_key().unwrap());
    }
    let mut reordered = ctx.spec().clone();
    reordered.measures.reverse();
    assert_eq!(
        ValuationContext::new(reordered)
            .unwrap()
            .cache_key()
            .unwrap(),
        ctx.cache_key().unwrap()
    );
    assert!(cache.insert(result).is_ok());
    let mut changed = ctx.spec().clone();
    changed.settlement_date = BusinessDate::parse("2026-10-07").unwrap();
    assert!(cache
        .insert(calculate(
            &snapshot,
            &ValuationContext::new(changed).unwrap()
        ))
        .is_err());
}

#[tokio::test]
async fn cvx008_ac2_assembly_excludes_later_published_and_later_recorded_fixings() {
    let initial = fixture::fixing(1, "2026-10-02T14:00:00Z", "2026-10-02T14:00:01Z", "-0.001");
    let later = fixture::fixing(2, "2026-10-02T14:30:00Z", "2026-10-03T09:00:00Z", "0.05");
    let setup = setup(vec![later.clone(), initial.clone()]).await;
    let mut request = setup.request.clone();
    request.fixings = vec![fixture::fixing_key()];
    let snapshot = setup
        .assembler
        .assemble(&request, &setup.events)
        .await
        .unwrap();
    assert_eq!(snapshot.content().fixings[0].fixing, Some(initial));
    request.known_at = later.recorded_at;
    let restatement = setup
        .assembler
        .assemble(&request, &setup.events)
        .await
        .unwrap();
    assert_eq!(restatement.content().fixings[0].fixing, Some(later));
    assert_ne!(restatement.id(), snapshot.id());
    assert_eq!(
        setup.snapshots.get(snapshot.id()).await.unwrap(),
        Some(snapshot)
    );
    let late_publication =
        fixture::fixing(1, "2026-10-03T09:00:00Z", "2026-10-03T09:00:01Z", "0.05");
    let missing = setup_fixture_late(late_publication).await;
    assert!(missing.content().fixings[0].fixing.is_none());
    assert!(missing
        .quality()
        .issues
        .iter()
        .any(|issue| issue.reason == SnapshotIssueKind::MissingFixing));
}
async fn setup_fixture_late(fixing: convex_ports::fixing::PublishedFixing) -> MarketSnapshot {
    let setup = setup(vec![fixing]).await;
    let mut request = setup.request.clone();
    request.fixings = vec![fixture::fixing_key()];
    setup
        .assembler
        .assemble(&request, &setup.events)
        .await
        .unwrap()
}

#[tokio::test]
async fn cvx008_ac3_partial_gap_clear_and_recovered_feeds_have_explicit_execution_quality() {
    let setup = setup(vec![]).await;
    let base = setup
        .assembler
        .assemble(&setup.request, &setup.events)
        .await
        .unwrap();
    assert!(base
        .require_execution_quality_at(base.content().as_of)
        .is_ok());
    let missing = setup.assembler.assemble(&setup.request, &[]).await.unwrap();
    assert!(!missing.quality().complete);
    assert!(missing
        .require_execution_quality_at(missing.content().as_of)
        .is_err());
    assert_eq!(
        calculate(&missing, &context(&missing)).envelope().status(),
        ResultStatus::Failed
    );
    let mut events = setup.events.clone();
    let mut delta = events[0].clone();
    delta.sequence = 3;
    delta.event = QuoteEventKind::Delta { fields: vec![] };
    events.push(delta);
    let gap = setup
        .assembler
        .assemble(&setup.request, &events)
        .await
        .unwrap();
    assert!(gap
        .quality()
        .issues
        .iter()
        .any(|issue| issue.reason == SnapshotIssueKind::Gap));
    assert_eq!(
        gap.content().quotes[0]
            .delivery_watermark
            .as_ref()
            .unwrap()
            .sequence,
        3
    );
    assert_eq!(
        gap.content().quotes[0]
            .image
            .as_ref()
            .unwrap()
            .watermark
            .sequence,
        1
    );
    assert!(gap
        .require_execution_quality_at(gap.content().as_of)
        .is_err());
    let display = calculate(&gap, &context(&gap));
    assert_eq!(display.envelope().quality(), DataQuality::Invalid);
    assert_eq!(
        display.context().spec().generation,
        setup.request.generation
    );
    let mut refresh = events[0].clone();
    refresh.sequence = 4;
    events.push(refresh);
    let recovered = setup
        .assembler
        .assemble(&setup.request, &events)
        .await
        .unwrap();
    assert!(recovered
        .require_execution_quality_at(recovered.content().as_of)
        .is_ok());
    let mut clear = events[0].clone();
    clear.sequence = 5;
    clear.event = QuoteEventKind::Delta {
        fields: vec![QuoteChange {
            field: QuoteField::MidPrice,
            update: FieldUpdate::Clear,
        }],
    };
    events.push(clear);
    let partial = setup
        .assembler
        .assemble(&setup.request, &events)
        .await
        .unwrap();
    assert!(!partial.quality().complete);
    assert!(partial
        .require_execution_quality_at(partial.content().as_of)
        .is_err());
    assert!(partial.content().quotes[0]
        .image
        .as_ref()
        .unwrap()
        .fields
        .is_empty());
}

#[tokio::test]
async fn economic_and_nanosecond_information_cutoffs_do_not_use_current_observations() {
    let setup = setup(vec![]).await;
    let mut events = setup.events.clone();
    let time = setup.request.as_of.datetime().timestamp_millis();
    events.push(fixture::event(2, time, time + 1, "98.25"));
    let mut request = setup.request.clone();
    request.known_at = fixture::time("2026-10-02T15:00:00.000999999Z");
    let old = setup.assembler.assemble(&request, &events).await.unwrap();
    assert_eq!(
        old.content().quotes[0]
            .image
            .as_ref()
            .unwrap()
            .watermark
            .sequence,
        1
    );
    request.known_at = fixture::time("2026-10-02T15:00:00.001Z");
    let received = setup.assembler.assemble(&request, &events).await.unwrap();
    assert_eq!(
        received.content().quotes[0]
            .image
            .as_ref()
            .unwrap()
            .watermark
            .sequence,
        2
    );
    events.push(fixture::event(3, time + 2, time + 2, "97.25"));
    request.known_at = fixture::time("2026-10-02T15:00:00.002Z");
    let economic = setup.assembler.assemble(&request, &events).await.unwrap();
    assert_eq!(
        economic.content().quotes[0]
            .image
            .as_ref()
            .unwrap()
            .watermark
            .sequence,
        2
    );
}

#[test]
fn supplied_curve_reconstruction_preserves_fractional_pillars_dates_and_policies() {
    let mut record = fixture::curve();
    record.points[0].tenor_years = FiniteF64::new(1.123456789).unwrap();
    record.points[0].continuous_zero = FiniteF64::new(-0.01).unwrap();
    // Nonnegative MonotoneConvex cannot represent this signed-rate input.
    // Reconstruction must reject its original policy rather than relabel it.
    assert!(convex_engine::BuiltCurve::from_snapshot(&record).is_err());
    record.interpolation = ZeroInterpolation::Linear;
    let curve = convex_engine::BuiltCurve::from_snapshot(&record).unwrap();
    assert_eq!(curve.reference_date, record.reference_date.date());
    assert_eq!(curve.points[0], (1.123456789, -0.01));
    assert_eq!(
        curve.built_at,
        record.recorded_at.datetime().timestamp_millis()
    );
    let replay = convex_engine::BuiltCurve::from_snapshot(
        &serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap(),
    )
    .unwrap();
    for tenor in [0.01, 1.123456789, 3.5, 12.0] {
        assert_eq!(
            curve.interpolate_rate(tenor).to_bits(),
            replay.interpolate_rate(tenor).to_bits()
        );
    }
    record.points[1].tenor_years = record.points[0].tenor_years;
    assert!(convex_engine::BuiltCurve::from_snapshot(&record).is_err());
}

#[test]
fn results_account_for_each_measure_and_cannot_claim_another_context_or_generation() {
    let snapshot = fixture::snapshot();
    let ctx = context(&snapshot);
    let result = calculate(&snapshot, &ctx);
    for field in ["cache_key", "generation", "build_id"] {
        let mut wire = serde_json::to_value(&result).unwrap();
        match field {
            "cache_key" => wire[field] = "invalid".into(),
            "generation" => wire["context"][field] = 2.into(),
            _ => wire["context"][field] = "another-build".into(),
        }
        assert!(serde_json::from_value::<SnapshotValuationResult>(wire).is_err());
    }
    let mut spec = ctx.spec().clone();
    spec.measures
        .push(MeasureId::new("unsupported_risk").unwrap());
    let extended = ValuationContext::new(spec).unwrap();
    assert!(SnapshotValuationResult::new(extended.clone(), result.envelope().clone()).is_err());
    let partial = calculate(&snapshot, &extended);
    assert_eq!(partial.envelope().status(), ResultStatus::Partial);
    assert!(partial
        .envelope()
        .errors()
        .iter()
        .any(|error| error.field() == Some("unsupported_risk")));
    let mut wrong_generation = ctx.spec().clone();
    wrong_generation.generation = fixture::version(2);
    assert!(ValuationContext::new(wrong_generation)
        .unwrap()
        .validate_snapshot(&snapshot)
        .is_err());
}

#[test]
fn concurrent_identical_contexts_keep_one_retained_result_and_original_calculation_time() {
    let snapshot = Arc::new(fixture::snapshot());
    let context = Arc::new(context(&snapshot));
    let service = Arc::new(valuator());
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|index| {
            let snapshot = snapshot.clone();
            let context = context.clone();
            let service = service.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                service
                    .value(
                        &snapshot,
                        &context,
                        RequestId::new(format!("concurrent-{index}")).unwrap(),
                        fixture::time(&format!("2026-10-04T10:00:0{index}Z")),
                    )
                    .unwrap()
            })
        })
        .collect();
    let results: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(service.cache_len(), 1);
    for result in &results {
        assert_eq!(
            result.envelope().provenance(),
            results[0].envelope().provenance()
        );
        assert_eq!(result.envelope().results(), results[0].envelope().results());
    }
    service.clear_cache();
    assert_eq!(service.cache_len(), 0);
}

#[tokio::test]
async fn selected_sources_exceeding_skew_policy_are_ineligible_even_with_fresh_complete_fields() {
    let setup = setup(vec![]).await;
    let mut second = fixture::bond();
    second.instrument_id = "BOND-002".into();
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("second-instrument").unwrap(),
                changes: vec![ReferenceChange::Create {
                    bond: Box::new(second),
                }],
                master_changes: vec![],
            },
            fixture::time("2025-01-02T00:00:00Z"),
        )
        .await
        .unwrap();
    let mut request = setup.request.clone();
    request.instruments.push(InstrumentPin {
        instrument_id: InstrumentId::new("BOND-002").unwrap(),
        version: fixture::version(1),
    });
    let mut requirement = fixture::quote_requirement();
    requirement.key.instrument_id = "BOND-002".into();
    requirement.key.source = "feed-b".into();
    request.quotes.push(requirement.clone());
    request.max_source_skew_ms = 5;
    let mut event = setup.events[0].clone();
    event.key = requirement.key;
    event.source_time_ms -= 10;
    let mut events = setup.events.clone();
    events.push(event);
    let snapshot = setup.assembler.assemble(&request, &events).await.unwrap();
    assert!(snapshot.quality().complete);
    assert!(snapshot
        .quality()
        .issues
        .iter()
        .any(|issue| issue.reason == SnapshotIssueKind::SourceSkew));
    assert!(snapshot
        .require_execution_quality_at(snapshot.content().as_of)
        .is_err());
}
