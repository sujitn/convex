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
        assert_ne!(ctx.cache_key(), changed.cache_key());
    }
    let mut reordered = ctx.spec().clone();
    reordered.measures.reverse();
    assert_eq!(
        ValuationContext::new(reordered).unwrap().cache_key(),
        ctx.cache_key()
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

/// D51: the cache keeps one snapshot's contexts. A result for another snapshot releases them,
/// so a full cache refuses only within one snapshot.
#[test]
fn a_result_for_another_snapshot_releases_the_retained_snapshot() {
    let snapshot = fixture::snapshot();
    let ctx = context(&snapshot);
    let result = calculate(&snapshot, &ctx);
    let cache = ValuationCache::new(1).unwrap();
    for index in 0..300 {
        let mut spec = ctx.spec().clone();
        spec.snapshot_id = SnapshotId::new(format!("snapshot-{index}")).unwrap();
        let envelope = result.envelope();
        let mut provenance = envelope.provenance().clone();
        provenance.snapshot_id = spec.snapshot_id.clone();
        let moved = SnapshotValuationResult::new(
            ValuationContext::new(spec).unwrap(),
            ResultEnvelope::new(
                envelope.request_id().clone(),
                envelope.status(),
                envelope.quality(),
                envelope.results().clone(),
                envelope.errors().to_vec(),
                provenance,
            )
            .unwrap(),
        )
        .unwrap();
        cache.insert(moved.clone()).unwrap();
        assert_eq!(cache.len(), 1);
        assert!(cache.get(moved.context()).unwrap().is_some());
    }
    assert!(cache.get(&ctx).unwrap().is_none());
    cache.insert(result).unwrap();
    assert_eq!(cache.len(), 1);
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

/// G32: a context derives its key once, when it is made, from the bytes the key always
/// hashed: the context's own serialization, which carries no key. A context read back
/// derives the same key.
#[test]
fn a_context_key_is_the_digest_of_its_serialization() {
    let ctx = context(&fixture::snapshot());
    let bytes = serde_json::to_vec(&ctx).unwrap();
    assert_eq!(bytes, serde_json::to_vec(ctx.spec()).unwrap());
    assert_eq!(
        ctx.cache_key(),
        content_digest(b"convex.valuation-context.v1 ", &bytes)
    );
    let read: ValuationContext = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(read, ctx);
    assert_eq!(read.cache_key(), ctx.cache_key());
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

/// D58: a valuation is judged only on the curves it reads. A fixed bond's spread
/// valuation reads its discount and swap curves, never the government or FRN reference
/// curve, so a stale curve in those roles leaves it valid until it is also the swap curve.
#[test]
fn a_stale_curve_counts_only_for_a_valuation_that_reads_it() {
    for (swap, quality) in [(false, DataQuality::Valid), (true, DataQuality::Stale)] {
        let mut content = fixture::content();
        let mut other = content.curves[0].clone();
        other.curve_id = CurveId::new("USD-OTHER").unwrap();
        other.source_time = fixture::time("2026-10-02T14:59:00Z");
        other.recorded_at = other.source_time;
        content.required_curves.push(other.curve_id.clone());
        content.curves.push(other);
        let id = convex_core::ids::CurveId::new("USD-OTHER");
        let roles = &mut content.configuration.analytics_curves;
        roles.government_curve = Some(id.clone());
        roles.frn_reference_curve = Some(id.clone());
        if swap {
            roles.swap_curve = Some(id);
        }
        let snapshot = MarketSnapshot::new(content).unwrap();
        assert_eq!(snapshot.quality().quality, DataQuality::Stale);
        let mut spec = context(&snapshot).spec().clone();
        spec.measures.push(MeasureId::new("z_spread_mid").unwrap());
        let result = calculate(&snapshot, &ValuationContext::new(spec).unwrap());
        let envelope = result.envelope();
        assert_eq!(
            envelope.status(),
            ResultStatus::Complete,
            "{:?}",
            envelope.errors()
        );
        assert_eq!(envelope.quality(), quality, "swap curve stale: {swap}");
    }
}

#[test]
fn risk_measures_publish_the_router_quote_in_explicit_units() {
    use convex_engine::pricing_router::{PricingInput, PricingRouter};
    // The fixture curve also serves the swap role, so I-spread and ASW are defined.
    let mut content = fixture::content();
    let curves = &mut content.configuration.analytics_curves;
    curves.swap_curve = Some(curves.discount_curve.clone());
    let snapshot = MarketSnapshot::new(content).unwrap();
    let mut spec = context(&snapshot).spec().clone();
    spec.measures = RUNTIME_MEASURES
        .iter()
        .map(|m| MeasureId::new(*m).unwrap())
        .collect();
    let ctx = ValuationContext::new(spec).unwrap();
    let result = calculate(&snapshot, &ctx);
    let envelope = result.envelope();
    assert_eq!(
        envelope.status(),
        ResultStatus::Complete,
        "{:?}",
        envelope.errors()
    );
    assert_eq!(envelope.results().len(), RUNTIME_MEASURES.len());
    // The oracle is the router itself, on the inputs the valuator reads from the snapshot.
    let content = snapshot.content();
    let curve = || Some(convex_engine::BuiltCurve::from_snapshot(&content.curves[0]).unwrap());
    let quote = PricingRouter { bump_bps: 10.0 }
        .price_at(
            &PricingInput {
                bond: content.references[0].record.bond.clone().unwrap(),
                settlement_date: ctx.spec().settlement_date.date(),
                market_price_bid: None,
                market_price_mid: Some(ExactDecimal::parse("99.25").unwrap().decimal()),
                market_price_ask: None,
                discount_curve: curve(),
                benchmark_curve: curve(),
                government_curve: None,
                volatility: None,
                bid_ask_config: content.configuration.bid_ask_spread.clone(),
            },
            fixture::time("2026-10-04T10:00:00Z")
                .datetime()
                .timestamp_millis(),
        )
        .unwrap();
    let model = |value: Option<rust_decimal::Decimal>| {
        ExactDecimal::new(value.unwrap()).to_model().unwrap()
    };
    let years = |value| MeasureValue::ModelYears(ModelYears::new(model(value)));
    let years_squared =
        |value| MeasureValue::ModelYearsSquared(ModelYearsSquared::new(model(value)));
    let price = |value| MeasureValue::ModelPrice(ModelPricePer100::new(model(value)));
    let bps = |value| MeasureValue::ModelBps(ModelBasisPoints::new(model(value)));
    let krd = |label: &str| {
        quote
            .key_rate_durations
            .as_ref()
            .unwrap()
            .iter()
            .find(|(tenor, _)| tenor == label)
            .map(|(_, value)| *value)
    };
    let expected = [
        ("modified_duration_mid", years(quote.modified_duration)),
        ("convexity_mid", years_squared(quote.convexity)),
        ("dv01_mid", price(quote.dv01)),
        ("z_spread_mid", bps(quote.z_spread_mid)),
        ("i_spread_mid", bps(quote.i_spread_mid)),
        ("asw_mid", bps(quote.asw_mid)),
        ("macaulay_duration_mid", years(quote.macaulay_duration)),
        ("effective_duration_mid", years(quote.effective_duration)),
        (
            "effective_convexity_mid",
            years_squared(quote.effective_convexity),
        ),
        ("spread_duration_mid", years(quote.spread_duration)),
        ("pv01_mid", price(quote.pv01)),
        ("cs01_mid", price(quote.cs01)),
        ("krd_3m_mid", years(krd("3M"))),
        ("krd_6m_mid", years(krd("6M"))),
        ("krd_1y_mid", years(krd("1Y"))),
        ("krd_2y_mid", years(krd("2Y"))),
        ("krd_3y_mid", years(krd("3Y"))),
        ("krd_5y_mid", years(krd("5Y"))),
        ("krd_7y_mid", years(krd("7Y"))),
        ("krd_10y_mid", years(krd("10Y"))),
        ("krd_20y_mid", years(krd("20Y"))),
        ("krd_30y_mid", years(krd("30Y"))),
    ];
    for (measure, value) in &expected {
        assert_eq!(
            &envelope.results()[&MeasureId::new(*measure).unwrap()].value,
            value,
            "{measure}"
        );
    }
    // A long bond's PV falls as zero rates and its Z-spread rise.
    let positive =
        |measure: &str| match &envelope.results()[&MeasureId::new(measure).unwrap()].value {
            MeasureValue::ModelPrice(value) => value.value().get() > 0.0,
            other => panic!("{measure}: {other:?}"),
        };
    assert!(positive("pv01_mid") && positive("cs01_mid"));
    let wire = serde_json::to_value(envelope).unwrap();
    for (measure, value) in &expected {
        let unit = match value {
            MeasureValue::ModelYears(_) => "years",
            MeasureValue::ModelYearsSquared(_) => "years_squared",
            MeasureValue::ModelPrice(_) => "price_per_100",
            MeasureValue::ModelBps(_) => "bp",
            other => panic!("{other:?}"),
        };
        assert_eq!(wire["results"][measure]["value"]["unit"], unit, "{wire}");
    }
    let back: ResultEnvelope = serde_json::from_value(wire).unwrap();
    assert_eq!(&back, envelope);
}

/// A plan that asks for no spread or curve-risk measure is priced without curves. Its
/// values equal those of a valuation that also asks for curve measures, which the test
/// above ties to the full router quote (bid, mid and ask over the configured curves).
#[test]
fn yield_measures_priced_without_curves_equal_those_priced_with_them() {
    let snapshot = fixture::snapshot();
    let mut spec = context(&snapshot).spec().clone();
    spec.measures = [
        "clean_price_mid",
        "dirty_price_mid",
        "accrued_interest",
        "ytm_mid",
        "modified_duration_mid",
        "convexity_mid",
        "dv01_mid",
        "macaulay_duration_mid",
    ]
    .iter()
    .map(|m| MeasureId::new(*m).unwrap())
    .collect();
    let lean = ValuationContext::new(spec.clone()).unwrap();
    spec.measures = RUNTIME_MEASURES
        .iter()
        .map(|m| MeasureId::new(*m).unwrap())
        .collect();
    let with_curves = calculate(&snapshot, &ValuationContext::new(spec).unwrap());
    let lean_result = calculate(&snapshot, &lean);
    assert_eq!(
        lean_result.envelope().status(),
        ResultStatus::Complete,
        "{:?}",
        lean_result.envelope().errors()
    );
    for measure in &lean.spec().measures {
        assert_eq!(
            lean_result.envelope().results()[measure].value,
            with_curves.envelope().results()[measure].value,
            "{measure:?}"
        );
    }
}

#[test]
fn a_snapshot_built_once_values_as_a_build_per_valuation_does() {
    let snapshot = std::sync::Arc::new(fixture::snapshot());
    let context = context(&snapshot);
    let built = convex_engine::curve_replay::BuiltSnapshot::build(snapshot.clone()).unwrap();
    let request = RequestId::new("replay-result").unwrap();
    let at = fixture::time("2026-10-04T10:00:00Z");
    let shared = valuator()
        .value_built(&built, &context, request.clone(), at)
        .unwrap();
    let separate = valuator().value(&snapshot, &context, request, at).unwrap();
    assert_eq!(shared.envelope().status(), ResultStatus::Complete);
    assert_eq!(shared, separate);
}

#[test]
fn a_zero_prices_its_yield_measures_and_keeps_the_router_reason_for_curve_measures() {
    // An unadjusted zero; its calendar/convention resources are rebuilt from its own terms.
    let mut bond = fixture::bond();
    bond.bond_type = convex_ports::reference_data::BondType::ZeroCoupon;
    bond.coupon_rate = None;
    bond.contractual_terms
        .as_mut()
        .unwrap()
        .settings
        .business_day_convention = convex_core::calendars::BusinessDayConvention::Unadjusted;
    let resources: Vec<_> = fixture::reference_terms::resources(&bond)
        .into_iter()
        .map(|change| convex_ports::reference_master::MasterRecord {
            schema_version: SchemaVersion::V2,
            version: fixture::version(1),
            recorded_at: fixture::time("2025-01-01T00:00:00Z"),
            definition: change.definition,
        })
        .collect();
    let mut content = fixture::content();
    let reference = &mut content.references.make_mut()[0];
    reference.record.bond = Some(bond);
    reference.issuer = resources[0].clone();
    reference.calendar = resources[1].clone();
    reference.convention = resources[2].clone();
    let curves = &mut content.configuration.analytics_curves;
    curves.swap_curve = Some(curves.discount_curve.clone());
    let snapshot = MarketSnapshot::new(content).unwrap();
    let mut spec = context(&snapshot).spec().clone();
    spec.measures = RUNTIME_MEASURES
        .iter()
        .map(|m| MeasureId::new(*m).unwrap())
        .collect();
    let ctx = ValuationContext::new(spec).unwrap();
    let envelope = calculate(&snapshot, &ctx).envelope().clone();
    assert_eq!(envelope.status(), ResultStatus::Partial);
    for measure in [
        "clean_price_mid",
        "dirty_price_mid",
        "accrued_interest",
        "ytm_mid",
        "modified_duration_mid",
        "macaulay_duration_mid",
        "convexity_mid",
        "dv01_mid",
    ] {
        assert!(
            envelope
                .results()
                .contains_key(&MeasureId::new(measure).unwrap()),
            "{measure}: {:?}",
            envelope.errors()
        );
    }
    // Every spread and curve-risk measure carries the router's own code, never a generic one.
    assert_eq!(
        envelope.results().len() + envelope.errors().len(),
        RUNTIME_MEASURES.len()
    );
    for error in envelope.errors() {
        assert_eq!(error.code(), ErrorCode::UnsupportedConvention, "{error:?}");
    }
}
