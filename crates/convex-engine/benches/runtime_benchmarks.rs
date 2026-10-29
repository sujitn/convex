//! P9c stage benchmarks on the shared fictional fixtures: one curve graph build,
//! snapshot verification and valuation, request-time pricing, and one Live tick.
//! P10a adds the Live quote rate and snapshot assembly over 1, 16 and 256 instruments;
//! P10b-i adds a 512-quote burst at 256, the batch conflation (D54) admits at that size.
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
use convex_engine::snapshot::{SnapshotAssembler, SnapshotValuator};
use convex_engine::{PricingEngine, PricingEngineBuilder, RuntimeInput, RuntimeServices};
use convex_ext_file::JsonFixingRepository;
use convex_ports::market_data::{QuoteSource, RawQuote, SourceType, StreamReceiver};
use convex_ports::quote_event::QuoteEvent;
use convex_ports::runtime::{PublishedValuation, ResultPublisher};
use convex_ports::snapshot::{MarketSnapshot, SnapshotRepository};
use convex_ports::TraitError;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
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
    let built = convex_engine::curve_replay::BuiltSnapshot::build(snapshot.clone()).unwrap();
    group.bench_function("value_built", |b| {
        b.iter(|| {
            SnapshotValuator::new(build.clone(), 1)
                .unwrap()
                .value_built(&built, &context, request.clone(), calculated)
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

/// The Live plan of `tests/runtime.rs`: one bond, one legacy curve record.
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
/// Live results are not archived: the server composes Live with a publisher that
/// only acknowledges (`DisplayOnlyPublisher`, D5a). This bench does the same.
struct DisplayOnly;
#[async_trait::async_trait]
impl ResultPublisher for DisplayOnly {
    async fn publish(&self, _: &PublishedValuation) -> Result<(), TraitError> {
        Ok(())
    }
}
fn live_engine(s: &Setup, plan: RuntimePlan, source: Arc<dyn QuoteSource>) -> PricingEngine {
    PricingEngineBuilder::new()
        .with_storage(s.storage.clone())
        .with_runtime(RuntimeServices {
            plan,
            snapshots: s.snapshots.clone(),
            fixings: Arc::new(JsonFixingRepository::empty()),
            store: s.store.clone(),
            publisher: Arc::new(DisplayOnly),
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
    let engine = live_engine(&s, live_plan(&s), source.clone());
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
                // The timer is created inside the runtime.
                let result = rt
                    .block_on(async {
                        tokio::time::timeout(Duration::from_secs(5), updates.recv()).await
                    })
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

/// Instrument counts for the P10a cases; 256 is `MAX_SNAPSHOT_INSTRUMENTS`.
const WIDTHS: [usize; 3] = [1, 16, 256];
/// Quotes sent per measured burst, well inside the 1,024-event queue.
const BURST: u64 = 64;
/// A burst at 256 instruments larger than the ~300 quotes one conflated tick must admit
/// for 10k quotes/s (D54), still inside the queue.
const SATURATING_BURST: u64 = 512;
/// One quote per tick at 256 instruments: the tick values one bond and keeps the rest
/// (D55), so it measures the per-tick cost that does not scale with the quotes.
const SINGLE: u64 = 1;

/// `setup()` widened to `width` bonds on one Live plan.
async fn wide_setup(width: usize) -> (Setup, RuntimePlan) {
    let s = setup().await;
    let mut plan = live_plan(&s);
    widen(&s, &mut plan, width).await;
    (s, plan)
}

/// A Live source shaped like the server quote ingress: one 1,024-event queue for every
/// key in the plan. The bench never offers more than the queue holds.
struct WideSource {
    sender: tokio::sync::broadcast::Sender<QuoteEvent>,
}
#[async_trait::async_trait]
impl QuoteSource for WideSource {
    fn source_type(&self) -> SourceType {
        SourceType::Streaming
    }
    async fn get_quote(
        &self,
        _: &convex_core::ids::InstrumentId,
    ) -> Result<Option<RawQuote>, TraitError> {
        Err(TraitError::Unsupported("canonical subscription bench only"))
    }
    async fn get_quotes(
        &self,
        _: &[convex_core::ids::InstrumentId],
    ) -> Result<Vec<RawQuote>, TraitError> {
        Err(TraitError::Unsupported("canonical subscription bench only"))
    }
    async fn subscribe(
        &self,
        _: &[convex_core::ids::InstrumentId],
    ) -> Result<StreamReceiver<QuoteEvent>, TraitError> {
        Ok(StreamReceiver::new(self.sender.subscribe()))
    }
    async fn unsubscribe(&self, _: &[convex_core::ids::InstrumentId]) -> Result<(), TraitError> {
        Ok(())
    }
}

/// Round-robin image quotes over the plan keys. Receipt stamps never step back, so a
/// wall-clock step cannot make the runtime refuse an event.
struct Quotes {
    keys: Vec<convex_core::ids::InstrumentId>,
    sequence: u64,
    stamp: i64,
}
impl Quotes {
    fn new(plan: &RuntimePlan) -> Self {
        Self {
            keys: plan
                .assembly
                .quotes
                .iter()
                .map(|q| q.key.instrument_id.clone())
                .collect(),
            sequence: 0,
            stamp: 0,
        }
    }
    fn next(&mut self) -> QuoteEvent {
        self.sequence += 1;
        self.stamp = self.stamp.max(chrono::Utc::now().timestamp_millis());
        let mut event = fixture::event(self.sequence, self.stamp, self.stamp, "99.25");
        event.key.instrument_id = self.keys[self.sequence as usize % self.keys.len()].clone();
        event
    }
}

/// Wait for a published result of `generation` or later. Display lag is skipped: only the
/// newest generation matters here.
fn wait_for(
    rt: &tokio::runtime::Runtime,
    updates: &mut tokio::sync::broadcast::Receiver<PublishedValuation>,
    generation: u64,
) -> PublishedValuation {
    use tokio::sync::broadcast::error::RecvError;
    rt.block_on(async {
        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                match updates.recv().await {
                    Ok(record) if record.generation.get() >= generation => return record,
                    Ok(_) | Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => panic!("the runtime stopped"),
                }
            }
        })
        .await
        .expect("a burst is valued within two minutes")
    })
}

/// Quotes/s a Live runtime of N instruments sustains: send a burst, then wait for the
/// result of its last quote.
fn bench_live_throughput(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("live_throughput");
    group.sample_size(10);
    let cases = WIDTHS
        .map(|width| (width, BURST))
        .into_iter()
        .chain([(256, SATURATING_BURST), (256, SINGLE)]);
    for (width, burst) in cases {
        let (s, plan) = rt.block_on(wide_setup(width));
        let mut quotes = Quotes::new(&plan);
        let (sender, _) = tokio::sync::broadcast::channel(1024);
        let source = Arc::new(WideSource { sender });
        let engine = live_engine(&s, plan, source.clone());
        rt.block_on(engine.start()).unwrap();
        let mut updates = engine.subscribe_results();
        let mut generation = engine.runtime_health().generation;
        // Every instrument needs its first image before its results are complete.
        for _ in 0..width {
            source.sender.send(quotes.next()).unwrap();
        }
        generation += width as u64;
        let warm = wait_for(&rt, &mut updates, generation);
        assert_eq!(warm.result.status(), ResultStatus::Complete);
        let id = if burst == BURST {
            BenchmarkId::from_parameter(width)
        } else {
            BenchmarkId::new(format!("burst{burst}"), width)
        };
        group.throughput(Throughput::Elements(burst));
        group.bench_function(id, |b| {
            b.iter_custom(|iterations| {
                let mut total = Duration::ZERO;
                for _ in 0..iterations {
                    let started = Instant::now();
                    for _ in 0..burst {
                        source.sender.send(quotes.next()).unwrap();
                    }
                    generation += burst;
                    let last = wait_for(&rt, &mut updates, generation);
                    total += started.elapsed();
                    assert_eq!(last.result.status(), ResultStatus::Complete);
                }
                total
            })
        });
        rt.block_on(engine.shutdown()).unwrap();
    }
    group.finish();
}

/// One snapshot over N instruments from their images through `resolve`, the Captured and
/// Replay path: configuration and reference reads from the store, quote ingestion and the
/// content hash, without publication. A Live tick does not take this path: it pins the
/// configuration and references once per activation (D56).
fn bench_assemble(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("snapshot/assemble");
    for width in WIDTHS {
        let (s, plan) = rt.block_on(wide_setup(width));
        let assembler = SnapshotAssembler::new(
            s.storage.bonds.clone(),
            s.storage.configs.clone(),
            Arc::new(JsonFixingRepository::empty()),
            s.snapshots.clone(),
        );
        let cutoff = plan.assembly.known_at.datetime().timestamp_millis() - 1_000;
        let mut quotes = Quotes::new(&plan);
        let events: Vec<_> = (0..width)
            .map(|_| {
                let mut event = quotes.next();
                (event.source_time_ms, event.receive_time_ms) = (cutoff, cutoff);
                event
            })
            .collect();
        let snapshot = rt
            .block_on(assembler.resolve(&plan.assembly, &events))
            .unwrap();
        assert_eq!(snapshot.content().references.len(), width);
        group.bench_function(BenchmarkId::from_parameter(width), |b| {
            b.iter(|| {
                rt.block_on(assembler.resolve(black_box(&plan.assembly), &events))
                    .unwrap()
            })
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_snapshot,
    bench_request_time,
    bench_live_tick,
    bench_live_throughput,
    bench_assemble
);
criterion_main!(benches);
