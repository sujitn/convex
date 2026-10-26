//! P9c stage benchmarks on the shared fictional fixtures: one curve graph build,
//! snapshot verification and valuation, request-time pricing, and one Live tick.
//!
//! Run with: cargo bench -p convex-engine --bench runtime_benchmarks --profile ci
//! Report medians only (`tools/REPLAY-MEASUREMENTS.md`); CI sets no timing threshold.

#[path = "../tests/support/configuration_setup.rs"]
mod configuration;
#[path = "../tests/support/curve_replay.rs"]
mod recipes;
#[path = "../tests/support/subscription_source.rs"]
mod subscription_source;

use configuration::runtime::*;
use convex_core::contracts::*;
use convex_core::Date;
use convex_engine::context::ValuationContext;
use convex_engine::pricing_router::PricingInput;
use convex_engine::runtime::{RuntimeMode, RuntimePlan};
use convex_engine::snapshot::SnapshotValuator;
use convex_engine::{PricingEngine, PricingEngineBuilder, RuntimeInput, RuntimeServices};
use convex_ext_file::JsonFixingRepository;
use convex_ports::snapshot::{MarketSnapshot, SnapshotRepository};
use criterion::{criterion_group, criterion_main, Criterion};
use rust_decimal_macros::dec;
use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};
use subscription_source::SubscriptionSource;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// The snapshot a Captured runtime stored for one bond priced from one stored recipe.
async fn recipe_snapshot() -> (Setup, Arc<MarketSnapshot>, ValuationContext) {
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
    let result = engine.runtime_results().remove(0).result;
    assert_eq!(result.status(), ResultStatus::Complete);
    engine.shutdown().await.unwrap();
    let snapshot = setup
        .snapshots
        .get(&result.provenance().snapshot_id)
        .await
        .unwrap()
        .unwrap();
    let context = setup
        .plan
        .context(&snapshot, &InstrumentId::new("BOND-001").unwrap())
        .unwrap();
    (setup, Arc::new(snapshot), context)
}

fn bench_snapshot(c: &mut Criterion) {
    let rt = runtime();
    let (setup, snapshot, context) = rt.block_on(recipe_snapshot());
    let build = setup.plan.build_id.clone();
    let request = RequestId::new("benchmark").unwrap();
    let calculated = snapshot.content().known_at.max(snapshot.content().as_of);
    let mut group = c.benchmark_group("snapshot");
    group.bench_function("curve_build", |b| {
        b.iter(|| {
            convex_engine::curve_replay::rebuild_curve_recipes(black_box(
                &snapshot.content().curve_recipes,
            ))
            .unwrap()
        })
    });
    group.bench_function("verify", |b| {
        b.iter(|| black_box(&snapshot).verify().unwrap())
    });
    group.bench_function("value_uncached", |b| {
        b.iter(|| {
            SnapshotValuator::new(build.clone(), 1)
                .unwrap()
                .value(&snapshot, &context, request.clone(), calculated)
                .unwrap()
        })
    });
    let cached = SnapshotValuator::new(build.clone(), 1).unwrap();
    cached
        .value(&snapshot, &context, request.clone(), calculated)
        .unwrap();
    group.bench_function("value_cached", |b| {
        b.iter(|| {
            cached
                .value(&snapshot, &context, request.clone(), calculated)
                .unwrap()
        })
    });
    group.finish();
}

/// The fixture configuration activated over the stored recipe its discount role names.
async fn request_time_engine() -> (Setup, Arc<PricingEngine>, SourceId) {
    let setup = setup().await;
    setup
        .storage
        .curves
        .save_recipe(&recipes::record(recipes::recipe()))
        .await
        .unwrap();
    let service = configuration::cases(&setup);
    let fixtures = configuration::fixtures(&setup);
    service
        .stage(&configuration::fixture::staged(1), &fixtures)
        .await
        .unwrap();
    service
        .activate(
            &configuration::fixture::activate(1, 0, "benchmark"),
            &fixtures,
        )
        .await
        .unwrap();
    // The runtime is never started; request-time pricing reads only storage.
    let engine = make_engine(
        &setup,
        archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    (setup, engine, SourceId::new("CONFIG-RESTORE").unwrap())
}

fn bench_request_time(c: &mut Criterion) {
    let rt = runtime();
    let (_setup, engine, id) = rt.block_on(request_time_engine());
    let bond = fixture::bond();
    let settlement = Date::from_ymd(2026, 10, 6).unwrap();
    let mut group = c.benchmark_group("request_time");
    group.bench_function("request_context", |b| {
        b.iter(|| {
            rt.block_on(engine.request_context(Some(&id), time(0)))
                .unwrap()
        })
    });
    group.bench_function("context_curves_price", |b| {
        b.iter(|| {
            let context = rt
                .block_on(engine.request_context(Some(&id), time(0)))
                .unwrap();
            let curves = context.curves_for(&bond).unwrap();
            let input = PricingInput::with_mid_price(
                bond.clone(),
                settlement,
                Some(dec!(99.25)),
                Some(curves.discount),
                curves.benchmark,
                None,
                None,
            );
            engine.pricing_router().price(&input).unwrap()
        })
    });
    group.finish();
}

/// The Live plan and engine of `tests/runtime.rs`: one bond, one legacy curve record.
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
fn live_engine(s: &Setup, source: Arc<SubscriptionSource>) -> PricingEngine {
    PricingEngineBuilder::new()
        .with_storage(s.storage.clone())
        .with_runtime(RuntimeServices {
            plan: live_plan(s),
            snapshots: s.snapshots.clone(),
            fixings: Arc::new(JsonFixingRepository::empty()),
            store: s.store.clone(),
            publisher: archive(s),
            input: RuntimeInput::Live(source),
        })
        .build()
        .unwrap()
}

/// One quote tick, from the source until its published result arrives.
fn bench_live_tick(c: &mut Criterion) {
    let rt = runtime();
    let s = rt.block_on(setup());
    let source = Arc::new(SubscriptionSource::new());
    let engine = live_engine(&s, source.clone());
    rt.block_on(engine.start()).unwrap();
    let mut updates = engine.subscribe_results();
    let mut sequence = 0u64;
    c.bench_function("live_tick", |b| {
        b.iter_custom(|iterations| {
            let mut total = Duration::ZERO;
            for _ in 0..iterations {
                sequence += 1;
                let stamp = chrono::Utc::now().timestamp_millis();
                let started = Instant::now();
                source.send(fixture::event(sequence, stamp, stamp, "99.25"));
                let result = rt
                    .block_on(tokio::time::timeout(Duration::from_secs(5), updates.recv()))
                    .expect("a Live tick publishes within five seconds")
                    .unwrap();
                total += started.elapsed();
                assert_eq!(result.result.status(), ResultStatus::Complete);
            }
            total
        })
    });
    rt.block_on(engine.shutdown()).unwrap();
}

criterion_group!(benches, bench_snapshot, bench_request_time, bench_live_tick);
criterion_main!(benches);
