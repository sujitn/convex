//! A corporate quoted by a level in a Live runtime. Its benchmark Treasuries are valued
//! beside it, so their quotes are in every snapshot it is priced from. A tick on a
//! benchmark reprices exactly the bonds quoted to it, a stale benchmark makes only those
//! bonds stale, and a desk's level is judged by its own age.
#[path = "support/level_rules.rs"]
mod level_rules;
#[path = "support/subscription_source.rs"]
mod subscription_source;
#[path = "support/runtime_setup.rs"]
mod support;
use convex_core::contracts::*;
use convex_engine::level_pricing::{
    price_under_rule, BenchmarkQuote, BenchmarkSetQuotes, BenchmarkYieldCache, PricingRuleInForce,
    QuotedInput,
};
use convex_engine::*;
use convex_ports::{
    configuration::InstrumentPin,
    level_pricing::BenchmarkSecurity,
    quote_event::{
        FieldUpdate, QuoteChange, QuoteEvent, QuoteEventKind, QuoteField, QuoteQuality, QuoteValue,
    },
    reference_data::BondReferenceData,
    reference_master::{MasterChange, MasterDefinition},
    reference_repository::{ReferenceChange, ReferenceCommand},
    runtime::PublishedValuation,
};
use level_rules::government_note;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use subscription_source::{monotone_stamp, SubscriptionSource};
use support::*;
use tokio::sync::broadcast::Receiver;

const CORPORATE: &str = "BOND-001";
const PRICED_CORPORATE: &str = "BOND-W001";
const TWO_YEAR: &str = "UST-2Y";
const TEN_YEAR: &str = "UST-10Y";
/// The corporate's spread over the 10-year, in basis points.
const SPREAD_BPS: Decimal = dec!(125);

fn notes() -> [BondReferenceData; 2] {
    [
        government_note(TWO_YEAR, "2028-01-15", "0.035", "91282CAA9"),
        government_note(TEN_YEAR, "2035-01-15", "0.04125", "91282CBB6"),
    ]
}

fn spread_over_ten_year() -> Value {
    json!({"kind": "benchmark_spread", "tenor": "10Y"})
}

fn master(kind: &str, definition: Value) -> MasterChange {
    serde_json::from_value(json!({
        "expected_version": null,
        "definition": {"kind": kind, "definition": definition},
    }))
    .unwrap()
}

/// The `UST-OTR` set of the 2- and 10-year notes, and the corporate's rule quoting it by
/// `method` against it.
fn rule_and_set(method: Value) -> Vec<MasterChange> {
    let effective = json!({"from": "2025-01-01T00:00:00Z", "until": null});
    let provenance = json!({"source": "desk", "evidence_reference": "live-level-test"});
    vec![
        master(
            "benchmark_set",
            json!({
                "benchmark_set_id": "UST-OTR",
                "currency": "USD",
                "securities": [
                    {"tenor": "2Y", "instrument_id": TWO_YEAR, "version": 1},
                    {"tenor": "10Y", "instrument_id": TEN_YEAR, "version": 1},
                ],
                "effective": effective,
                "provenance": provenance,
            }),
        ),
        master(
            "pricing_rule",
            json!({
                "instrument_id": CORPORATE,
                "method": method,
                "benchmark_set_id": "UST-OTR",
                "effective": effective,
                "provenance": provenance,
            }),
        ),
    ]
}

/// [`desk_quoting`] with the corporate quoted as a spread over the 10-year.
async fn desk(pinned: &[&str]) -> (Setup, RuntimePlan) {
    desk_quoting(spread_over_ten_year(), &[QuoteField::MidSpread], pinned).await
}

/// A Live plan over the fixture corporate, quoted by `method` and read from `fields`, a
/// second corporate quoted by price, and the notes `pinned` names, with the notes, the
/// set and the rule stored.
async fn desk_quoting(
    method: Value,
    fields: &[QuoteField],
    pinned: &[&str],
) -> (Setup, RuntimePlan) {
    let s = setup().await;
    let at = fixture::time("2025-06-01T00:00:00Z");
    s.storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("live-level-notes").unwrap(),
                changes: notes()
                    .into_iter()
                    .map(|bond| ReferenceChange::Create {
                        bond: Box::new(bond),
                    })
                    .collect(),
                master_changes: rule_and_set(method),
            },
            at,
        )
        .await
        .unwrap();

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
    plan.measures.push(MeasureId::new("g_spread_mid").unwrap());
    widen(&s, &mut plan, 2).await;
    for id in pinned {
        plan.assembly.instruments.push(InstrumentPin {
            instrument_id: InstrumentId::new(*id).unwrap(),
            version: fixture::version(1),
        });
        let mut quote = plan.assembly.quotes[0].clone();
        quote.key.instrument_id = convex_core::ids::InstrumentId::new(*id);
        plan.assembly.quotes.push(quote);
    }
    plan.assembly.quotes[0].fields = fields.to_vec();
    (s, plan)
}

fn engine(s: &Setup, plan: RuntimePlan, source: Arc<SubscriptionSource>) -> Arc<PricingEngine> {
    make_engine(
        s,
        display_only(),
        plan,
        s.store.clone(),
        RuntimeInput::Live(source),
    )
}

/// An image of one field of `id`'s quote from the fixture feed, observed `age_ms` ago.
fn image(
    id: &str,
    field: QuoteField,
    value: Decimal,
    quality: QuoteQuality,
    age_ms: i64,
) -> QuoteEvent {
    let stamp = monotone_stamp();
    let mut event = fixture::event(1, stamp - age_ms, stamp, "100");
    event.key.instrument_id = convex_core::ids::InstrumentId::new(id);
    event.sequence = u64::try_from(stamp).unwrap();
    event.quality = quality;
    event.event = QuoteEventKind::Image {
        fields: vec![QuoteChange {
            field,
            update: FieldUpdate::Set(QuoteValue {
                value,
                unit: field.unit(),
            }),
        }],
    };
    event
}

fn price(id: &str, value: Decimal) -> QuoteEvent {
    image(id, QuoteField::MidPrice, value, QuoteQuality::Live, 0)
}

fn spread(value: Decimal) -> QuoteEvent {
    image(
        CORPORATE,
        QuoteField::MidSpread,
        value,
        QuoteQuality::Live,
        0,
    )
}

/// Every bond's first image: the corporate's `level`, the second corporate's price and
/// each note's.
fn open_desk(source: &SubscriptionSource, level: QuoteEvent, notes: &[(&str, Decimal)]) {
    source.send(level);
    source.send(price(PRICED_CORPORATE, dec!(99.00)));
    for (id, value) in notes {
        source.send(price(id, *value));
    }
}

fn instrument_of(record: &PublishedValuation) -> &str {
    match &record.result.provenance().subject {
        ValuationSubject::InstrumentVersion { instrument_id, .. } => instrument_id.as_str(),
        other => panic!("not an instrument: {other:?}"),
    }
}

/// The records of the next tick, by instrument: each record published until the
/// generation changes or the runtime goes quiet.
async fn tick(
    updates: &mut Receiver<Arc<PublishedValuation>>,
) -> BTreeMap<String, Arc<PublishedValuation>> {
    let first = tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    let generation = first.generation;
    let mut records = BTreeMap::from([(instrument_of(&first).to_owned(), first)]);
    while let Ok(Ok(record)) =
        tokio::time::timeout(Duration::from_millis(300), updates.recv()).await
    {
        assert_eq!(record.generation, generation, "one tick at a time");
        records.insert(instrument_of(&record).to_owned(), record);
    }
    records
}

fn measure(record: &PublishedValuation, name: &str) -> Option<Decimal> {
    record
        .result
        .results()
        .iter()
        .find(|(measure, _)| measure.as_str() == name)
        .map(|(_, result)| match &result.value {
            MeasureValue::Price(price) => price.value().decimal(),
            MeasureValue::ModelBps(bps) => Decimal::from_f64_retain(bps.value().get()).unwrap(),
            MeasureValue::ModelRate(rate) => Decimal::from_f64_retain(rate.value().get()).unwrap(),
            other => panic!("unexpected measure value {other:?}"),
        })
}

fn assert_valued(record: &PublishedValuation) {
    assert_eq!(
        record.result.status(),
        ResultStatus::Complete,
        "{}: {:?}",
        instrument_of(record),
        record.result.errors()
    );
    assert_eq!(record.result.quality(), DataQuality::Valid);
}

/// The clean price the level stage gives the corporate at `spread_bps` over the 10-year,
/// from the notes' prices as the request-time path reads them.
fn staged_clean_price(plan: &RuntimePlan, two_year: Decimal, ten_year: Decimal) -> Decimal {
    let MasterDefinition::PricingRule(rule) =
        rule_and_set(spread_over_ten_year()).remove(1).definition
    else {
        unreachable!("the second change is the rule")
    };
    let quotes = notes()
        .into_iter()
        .zip([("2Y", two_year), ("10Y", ten_year)])
        .map(|(bond, (tenor, clean_price_mid))| BenchmarkQuote {
            security: BenchmarkSecurity {
                tenor: tenor.into(),
                instrument_id: InstrumentId::new(bond.instrument_id.as_str()).unwrap(),
                version: fixture::version(1),
            },
            bond,
            clean_price_mid,
            snapshot_id: SnapshotId::new("sha256:fixture".to_owned()).unwrap(),
        })
        .collect();
    let in_force = PricingRuleInForce {
        rule: convex_ports::level_pricing::ResolvedPricingRule {
            version: fixture::version(1),
            recorded_at: rule.effective.from,
            method: rule.method,
            benchmark_set_id: rule.benchmark_set_id,
        },
        width: None,
        benchmark_set: Some(Arc::new(BenchmarkSetQuotes {
            benchmark_set_id: ReferenceDataId::new("UST-OTR").unwrap(),
            version: fixture::version(1),
            quotes,
        })),
    };
    price_under_rule(
        &fixture::bond(),
        Some(&in_force),
        QuotedInput::Level(SPREAD_BPS),
        plan.settlement_date.date(),
        None,
        &BenchmarkYieldCache::default(),
    )
    .unwrap()
    .clean_price_mid
}

/// A tick on a benchmark reprices exactly the bonds quoted to it, at the price the level
/// stage gives from the snapshot's quotes, which is the price a request-time quote gives
/// from the same prices. A restart builds the index of dependents again.
#[tokio::test]
async fn a_benchmark_tick_reprices_exactly_the_bonds_quoted_to_it() {
    let (s, plan) = desk(&[TWO_YEAR, TEN_YEAR]).await;
    for restart in [false, true] {
        let source = Arc::new(SubscriptionSource::new());
        let engine = engine(&s, plan.clone(), source.clone());
        engine.start().await.unwrap();
        let mut updates = engine.subscribe_results();
        open_desk(
            &source,
            spread(SPREAD_BPS),
            &[(TWO_YEAR, dec!(99.50)), (TEN_YEAR, dec!(98.25))],
        );
        let first = tick(&mut updates).await;
        assert_eq!(first.len(), 4, "{restart}: {:?}", first.keys());
        first.values().for_each(|record| assert_valued(record));
        let corporate = &first[CORPORATE];
        assert_eq!(
            measure(corporate, "clean_price_mid"),
            Some(staged_clean_price(&plan, dec!(99.50), dec!(98.25)))
        );
        // The G-spread is asked only of the bond priced against a set it matures within.
        assert!(measure(corporate, "g_spread_mid").is_some());
        assert!(measure(&first[PRICED_CORPORATE], "g_spread_mid").is_none());
        let versions = corporate.result.provenance().input_versions.clone();
        for (kind, id) in [("pricing_rule", CORPORATE), ("benchmark_set", "UST-OTR")] {
            assert!(
                versions
                    .iter()
                    .any(|v| v.kind.as_str() == kind && v.id.as_str() == id),
                "{kind} {id}: {versions:?}"
            );
        }

        source.send(price(TEN_YEAR, dec!(98.50)));
        let ten_year = tick(&mut updates).await;
        assert_eq!(
            ten_year.keys().collect::<Vec<_>>(),
            [CORPORATE, TEN_YEAR],
            "{restart}"
        );
        assert_eq!(
            measure(&ten_year[CORPORATE], "clean_price_mid"),
            Some(staged_clean_price(&plan, dec!(99.50), dec!(98.50)))
        );
        source.send(price(PRICED_CORPORATE, dec!(99.10)));
        assert_eq!(
            tick(&mut updates).await.keys().collect::<Vec<_>>(),
            [PRICED_CORPORATE]
        );
        // The 2-year moves the corporate's G-spread, not its price.
        source.send(price(TWO_YEAR, dec!(99.40)));
        let two_year = tick(&mut updates).await;
        assert_eq!(two_year.keys().collect::<Vec<_>>(), [CORPORATE, TWO_YEAR]);
        assert_eq!(
            measure(&two_year[CORPORATE], "clean_price_mid"),
            measure(&ten_year[CORPORATE], "clean_price_mid")
        );
        assert_ne!(
            measure(&two_year[CORPORATE], "g_spread_mid"),
            measure(&ten_year[CORPORATE], "g_spread_mid")
        );
        assert!(engine.runtime_health().ready);
        engine.shutdown().await.unwrap();
    }
}

/// When a benchmark's quote ages out, the bonds quoted to it are stale with it, in the
/// same tick, and every other bond keeps its record.
#[tokio::test]
async fn a_stale_benchmark_makes_only_the_bonds_quoted_to_it_stale() {
    let (s, mut plan) = desk(&[TWO_YEAR, TEN_YEAR]).await;
    let ten_year = plan
        .assembly
        .quotes
        .iter_mut()
        .find(|quote| quote.key.instrument_id.as_str() == TEN_YEAR)
        .unwrap();
    ten_year.max_age_ms = 3_000;
    let source = Arc::new(SubscriptionSource::new());
    let engine = engine(&s, plan, source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    open_desk(
        &source,
        spread(SPREAD_BPS),
        &[(TWO_YEAR, dec!(99.50)), (TEN_YEAR, dec!(98.25))],
    );
    let first = tick(&mut updates).await;
    first.values().for_each(|record| assert_valued(record));

    let aged = tick(&mut updates).await;
    assert_eq!(aged.keys().collect::<Vec<_>>(), [CORPORATE, TEN_YEAR]);
    for record in aged.values() {
        assert_eq!(record.result.status(), ResultStatus::Failed);
        assert_eq!(record.result.quality(), DataQuality::Stale);
        assert!(record
            .result
            .errors()
            .iter()
            .all(|e| e.code() == ErrorCode::StaleInput));
    }
    for id in [PRICED_CORPORATE, TWO_YEAR] {
        let current = engine.runtime_results_of([id]);
        assert!(Arc::ptr_eq(&current[0], &first[id]), "{id}");
    }
    assert_eq!(engine.runtime_health().failed_instruments, 2);
    engine.shutdown().await.unwrap();
}

/// A desk's level is the desk's own mark: a manual spread an hour old prices the bond,
/// although the Treasuries ticked a second ago and the skew policy is half a second. A
/// manual price, or a manual quote on a benchmark, is still indicative.
#[tokio::test]
async fn a_desk_level_is_judged_by_its_own_age() {
    let (s, mut plan) = desk(&[TWO_YEAR, TEN_YEAR]).await;
    plan.assembly.max_source_skew_ms = 500;
    plan.assembly.quotes[0].max_age_ms = 86_400_000;
    let source = Arc::new(SubscriptionSource::new());
    let engine = engine(&s, plan, source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    let hour = 3_600_000;
    source.send(image(
        CORPORATE,
        QuoteField::MidSpread,
        SPREAD_BPS,
        QuoteQuality::Manual,
        hour,
    ));
    source.send(image(
        PRICED_CORPORATE,
        QuoteField::MidPrice,
        dec!(99.00),
        QuoteQuality::Manual,
        0,
    ));
    source.send(price(TWO_YEAR, dec!(99.50)));
    source.send(price(TEN_YEAR, dec!(98.25)));
    let first = tick(&mut updates).await;
    for id in [CORPORATE, TWO_YEAR, TEN_YEAR] {
        assert_valued(&first[id]);
    }
    assert_eq!(
        first[PRICED_CORPORATE].result.quality(),
        DataQuality::Indicative
    );

    source.send(image(
        TWO_YEAR,
        QuoteField::MidPrice,
        dec!(99.40),
        QuoteQuality::Manual,
        0,
    ));
    let manual = tick(&mut updates).await;
    assert_eq!(manual.keys().collect::<Vec<_>>(), [CORPORATE, TWO_YEAR]);
    for record in manual.values() {
        assert_eq!(record.result.quality(), DataQuality::Indicative);
    }
    engine.shutdown().await.unwrap();
}

/// A rule whose set holds a Treasury the runtime does not value fails its own bond, named
/// by the benchmark's tenor; every other bond is valued and the runtime stays ready.
#[tokio::test]
async fn a_rule_the_runtime_cannot_price_fails_only_its_bond() {
    let (s, plan) = desk(&[TEN_YEAR]).await;
    let source = Arc::new(SubscriptionSource::new());
    let engine = engine(&s, plan, source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    open_desk(&source, spread(SPREAD_BPS), &[(TEN_YEAR, dec!(98.25))]);
    let first = tick(&mut updates).await;
    let corporate = &first[CORPORATE];
    assert_eq!(corporate.result.status(), ResultStatus::Failed);
    let error = &corporate.result.errors()[0];
    assert_eq!(error.code(), ErrorCode::MissingMarketData);
    assert!(
        error.message().contains("the 2Y benchmark"),
        "{}",
        error.message()
    );
    assert!(!error.message().contains(TWO_YEAR));
    for id in [PRICED_CORPORATE, TEN_YEAR] {
        assert_valued(&first[id]);
    }
    let health = engine.runtime_health();
    assert!(health.ready && health.failed_instruments == 1, "{health:?}");
    engine.shutdown().await.unwrap();
}

/// A runtime that values another version of a benchmark than its set pins fails the bonds
/// quoted to it, naming the benchmark by tenor; the Treasury itself is valued.
#[tokio::test]
async fn a_benchmark_valued_at_another_version_fails_the_bonds_quoted_to_it() {
    let (s, mut plan) = desk(&[TWO_YEAR, TEN_YEAR]).await;
    let mut reissued = notes()[1].clone();
    reissued.description = format!("{} (reopened)", reissued.description);
    s.storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("live-level-reopen").unwrap(),
                changes: vec![ReferenceChange::Update {
                    bond: Box::new(reissued),
                    expected_version: fixture::version(1),
                }],
                master_changes: vec![],
            },
            fixture::time("2025-07-01T00:00:00Z"),
        )
        .await
        .unwrap();
    plan.assembly
        .instruments
        .iter_mut()
        .find(|pin| pin.instrument_id.as_str() == TEN_YEAR)
        .unwrap()
        .version = fixture::version(2);
    let source = Arc::new(SubscriptionSource::new());
    let engine = engine(&s, plan, source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    open_desk(
        &source,
        spread(SPREAD_BPS),
        &[(TWO_YEAR, dec!(99.50)), (TEN_YEAR, dec!(98.25))],
    );
    let first = tick(&mut updates).await;
    let error = &first[CORPORATE].result.errors()[0];
    assert!(
        error.message().contains("the 10Y benchmark") && error.message().contains("pinned version"),
        "{}",
        error.message()
    );
    for id in [PRICED_CORPORATE, TWO_YEAR, TEN_YEAR] {
        assert_valued(&first[id]);
    }
    engine.shutdown().await.unwrap();
}

/// A yield level comes back as the bond's yield, and a Z-spread level prices over the
/// configuration's discount curve, each read from its rule's field.
#[tokio::test]
async fn a_yield_or_z_spread_level_prices_from_its_own_field() {
    for (method, field, level) in [
        (json!({"kind": "yield"}), QuoteField::MidYield, dec!(0.0612)),
        (
            json!({"kind": "z_spread"}),
            QuoteField::MidSpread,
            dec!(150),
        ),
    ] {
        let (s, plan) = desk_quoting(method, &[field], &[TWO_YEAR, TEN_YEAR]).await;
        let source = Arc::new(SubscriptionSource::new());
        let engine = engine(&s, plan, source.clone());
        engine.start().await.unwrap();
        let mut updates = engine.subscribe_results();
        open_desk(
            &source,
            image(CORPORATE, field, level, QuoteQuality::Live, 0),
            &[(TWO_YEAR, dec!(99.50)), (TEN_YEAR, dec!(98.25))],
        );
        let first = tick(&mut updates).await;
        first.values().for_each(|record| assert_valued(record));
        if field == QuoteField::MidYield {
            let ytm = measure(&first[CORPORATE], "ytm_mid").unwrap();
            assert!((ytm - level).abs() < dec!(0.000000001), "{ytm}");
        }
        engine.shutdown().await.unwrap();
    }
}

/// A Z-spread level reads the discount curve, so a stale curve makes the bond quoted by
/// it stale; a bond quoted by price that asks for no curve measure is still valued.
#[tokio::test]
async fn a_stale_curve_fails_a_bond_quoted_by_z_spread() {
    let (s, mut plan) = desk_quoting(
        json!({"kind": "z_spread"}),
        &[QuoteField::MidSpread],
        &[TWO_YEAR, TEN_YEAR],
    )
    .await;
    let stale =
        UtcTimestamp::from_datetime(chrono::Utc::now() - chrono::Duration::hours(1)).unwrap();
    plan.assembly.curves[0].source_time = stale;
    plan.assembly.curves[0].recorded_at = stale;
    plan.assembly.curves[0].max_age_ms = 1_000;
    let source = Arc::new(SubscriptionSource::new());
    let engine = engine(&s, plan, source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    open_desk(
        &source,
        spread(dec!(150)),
        &[(TWO_YEAR, dec!(99.50)), (TEN_YEAR, dec!(98.25))],
    );
    let first = tick(&mut updates).await;
    assert_eq!(first[CORPORATE].result.quality(), DataQuality::Stale);
    assert_eq!(first[CORPORATE].result.status(), ResultStatus::Failed);
    for id in [PRICED_CORPORATE, TWO_YEAR, TEN_YEAR] {
        assert_valued(&first[id]);
    }
    engine.shutdown().await.unwrap();
}

/// A bond quoted by a level whose quote declares only a price fails, saying so; the
/// runtime and every other bond are valued.
#[tokio::test]
async fn a_level_quoted_bond_without_its_field_fails_alone() {
    let (s, plan) = desk_quoting(
        spread_over_ten_year(),
        &[QuoteField::MidPrice],
        &[TWO_YEAR, TEN_YEAR],
    )
    .await;
    let source = Arc::new(SubscriptionSource::new());
    let engine = engine(&s, plan, source.clone());
    engine.start().await.unwrap();
    let mut updates = engine.subscribe_results();
    open_desk(
        &source,
        price(CORPORATE, dec!(99.00)),
        &[(TWO_YEAR, dec!(99.50)), (TEN_YEAR, dec!(98.25))],
    );
    let first = tick(&mut updates).await;
    let corporate = &first[CORPORATE];
    assert_eq!(corporate.result.status(), ResultStatus::Failed);
    assert_eq!(corporate.result.errors()[0].code(), ErrorCode::InvalidInput);
    for id in [PRICED_CORPORATE, TWO_YEAR, TEN_YEAR] {
        assert_valued(&first[id]);
    }
    assert!(engine.runtime_health().ready);
    engine.shutdown().await.unwrap();
}
