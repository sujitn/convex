//! A fixed-coupon perpetual quoted by price or by yield to worst, valued by the runtime on
//! each call through its horizon and to perpetuity: no option model and no curve. A
//! request at the stream's cutoff gives the stream's values. Its G-spread reads the
//! benchmark set at the worst call. A perpetual the library does not admit is refused
//! when committed.
#[path = "support/configuration_setup.rs"]
mod configuration;
#[path = "support/level_rules.rs"]
mod level_rules;
#[path = "support/products.rs"]
mod products;
use configuration::runtime::*;
use convex_analytics::workflow::PerpetualWorkout;
use convex_core::contracts::*;
use convex_engine::{
    level_pricing::{perpetual_g_spread_under_rule, BenchmarkYieldCache, QuotedInput},
    perpetual_valuation::{
        PerpetualValuation, PERPETUAL_MEASURES, PERPETUAL_NOT_ADMITTED, PERPETUAL_SNAPSHOT_MODEL,
    },
    pricing_router::{PricingInput, PricingRouter},
    RuntimeInput,
};
use convex_ports::{level_pricing::TenorBasis, reference_data::BondReferenceData};
use convex_ports::{
    quote_event::QuoteField,
    reference_repository::{ReferenceChange, ReferenceCommand, ReferenceError},
};
use level_rules::{government_note, rule_in_force, BENCHMARK_CLEAN_PRICE};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{json, Value};

/// Version 2 of BOND-001 is the fixture perpetual, quoted by `method` under a pricing
/// rule, on a calendar and convention pack of its own. The plan reads its rule's field,
/// holding `mark`, and asks for the perpetual measures.
async fn perpetual_setup(method: Value, field: QuoteField, mark: Decimal) -> Setup {
    let mut setup = setup().await;
    let bond = products::perpetual(fixture::bond());
    // The issuer is stored already.
    let mut master_changes = fixture::reference_terms::resources(&bond).split_off(1);
    master_changes.push(products::pricing_rule("BOND-001", method));
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("perpetual-on-workouts").unwrap(),
                changes: vec![ReferenceChange::Update {
                    bond: Box::new(bond),
                    expected_version: fixture::version(1),
                }],
                master_changes,
            },
            fixture::time("2025-06-01T00:00:00Z"),
        )
        .await
        .unwrap();
    let plan = &mut setup.plan;
    plan.assembly.instruments[0].version = fixture::version(2);
    products::quote_in(
        &mut plan.assembly.quotes[0],
        &mut plan.seed_events[0],
        field,
        mark,
    );
    plan.measures = PERPETUAL_MEASURES
        .iter()
        .map(|m| MeasureId::new(*m).unwrap())
        .collect();
    setup
}

/// A started captured runtime over `setup`'s plan and its one published result.
async fn started(setup: &Setup) -> (std::sync::Arc<convex_engine::PricingEngine>, ResultEnvelope) {
    let engine = make_engine(
        setup,
        archive(setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let published = engine.runtime_results();
    assert_eq!(published.len(), 1);
    let result = published[0].result.clone();
    (engine, result)
}

fn assert_complete(result: &ResultEnvelope) {
    assert_eq!(
        result.status(),
        ResultStatus::Complete,
        "{:?}",
        result.errors()
    );
}

fn model(result: &ResultEnvelope, name: &str) -> f64 {
    match &result.results()[&MeasureId::new(name).unwrap()].value {
        MeasureValue::ModelPrice(v) => v.value().get(),
        MeasureValue::ModelRate(v) => v.value().get(),
        other => panic!("{name}: {other:?}"),
    }
}
fn clean(result: &ResultEnvelope) -> Decimal {
    let MeasureValue::Price(clean) =
        &result.results()[&MeasureId::new("clean_price_mid").unwrap()].value
    else {
        panic!("a clean price")
    };
    clean.value().decimal()
}

#[tokio::test]
async fn a_perpetual_quoted_by_price_publishes_its_yield_to_worst_with_no_option_model() {
    let setup = perpetual_setup(json!({"kind": "price"}), QuoteField::MidPrice, dec!(92.5)).await;
    let (engine, result) = started(&setup).await;
    engine.shutdown().await.unwrap();
    assert_complete(&result);
    assert_eq!(
        result.provenance().model_version.as_str(),
        PERPETUAL_SNAPSHOT_MODEL
    );
    let names: Vec<_> = result.results().keys().map(|m| m.as_str()).collect();
    let mut expected = PERPETUAL_MEASURES.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
    assert_eq!(clean(&result), dec!(92.5));
    assert!(
        (model(&result, "dirty_price_mid") - 92.5 - model(&result, "accrued_interest")).abs()
            < 1e-12
    );
    // Below par the worst is the perpetuity: above its 6.5% coupon, below every call's.
    let ytw = model(&result, "ytw_mid");
    assert!((0.065..0.075).contains(&ytw), "{ytw}");
}

/// A request at the stream's cutoff, by the stream's yield to worst or by a price, gives
/// the stream's values.
#[tokio::test]
async fn a_request_values_a_perpetual_as_the_stream_does() {
    let setup = perpetual_setup(json!({"kind": "yield"}), QuoteField::MidYield, dec!(0.055)).await;
    let (engine, stream) = started(&setup).await;
    assert_complete(&stream);
    assert_eq!(model(&stream, "ytw_mid"), 0.055);
    // At 5.5% the 6.5% perpetual is at a premium, and the first call is worst: its price
    // is between par and the perpetuity's.
    let price = clean(&stream);
    assert!(price > dec!(100) && price < dec!(106), "{price}");
    let config = setup
        .storage
        .configs
        .get_version("CONFIG-RESTORE", 1)
        .await
        .unwrap()
        .unwrap();
    configuration::activate_pricing(&setup, &config).await;
    let settlement = setup.plan.settlement_date;
    let by_yield = configuration::value_quoted_at(
        &engine,
        &stream,
        settlement,
        Some(QuotedInput::Level(dec!(0.055))),
    )
    .await
    .unwrap();
    let by_yield = by_yield.result.envelope();
    assert_complete(by_yield);
    assert_eq!(by_yield.results(), stream.results());
    let by_price = configuration::value_quoted_at(
        &engine,
        &stream,
        settlement,
        Some(QuotedInput::Price(price)),
    )
    .await
    .unwrap();
    let by_price = by_price.result.envelope();
    assert_complete(by_price);
    assert!((model(by_price, "ytw_mid") - 0.055).abs() < 1e-9);
    engine.shutdown().await.unwrap();
}

/// The engine admits a perpetual only as the library builds it: month end on a semiannual
/// 30/360 US grid counts February short, so it is refused, and nothing is committed.
#[tokio::test]
async fn a_perpetual_the_library_refuses_is_not_committed() {
    let setup = setup().await;
    let engine = make_engine(
        &setup,
        archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    let bond = products::month_end_perpetual(fixture::bond());
    let command = |bond| ReferenceCommand {
        idempotency_key: RequestId::new("month-end-perpetual").unwrap(),
        changes: vec![ReferenceChange::Update {
            bond: Box::new(bond),
            expected_version: fixture::version(1),
        }],
        master_changes: fixture::reference_terms::resources(&products::perpetual(fixture::bond()))
            .split_off(1),
    };
    let refused = engine
        .commit_reference(&command(bond), fixture::time("2025-06-01T00:00:00Z"))
        .await
        .err()
        .unwrap();
    assert!(matches!(
        refused,
        ReferenceError::InvalidInput(PERPETUAL_NOT_ADMITTED)
    ));
    let stored = setup
        .storage
        .bonds
        .get(&convex_core::ids::InstrumentId::new("BOND-001"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.version, fixture::version(1));
}

/// The G-spread at the worst call is the yield to worst over the set's yield interpolated
/// at the call date, in whole basis points as the library reports it. A call outside the
/// set reads its nearest end, and the perpetuity its longest security.
#[test]
fn the_g_spread_reads_the_set_at_the_worst_call() {
    let bond = products::perpetual(fixture::bond());
    let settlement = convex_core::Date::parse("2026-10-06").unwrap();
    let notes = [
        (
            government_note("UST-2Y", "2028-01-15", "0.035", "91282CAA9"),
            "2Y",
        ),
        (
            government_note("UST-10Y", "2035-01-15", "0.04125", "91282CBB6"),
            "10Y",
        ),
    ];
    let note_yield = |note: &BondReferenceData| {
        let input = PricingInput::with_mid_price(
            note.clone(),
            settlement,
            Some(BENCHMARK_CLEAN_PRICE),
            None,
            None,
            None,
            None,
        );
        let quote = PricingRouter::new().price(&input).unwrap();
        quote.ytm_mid.unwrap().to_f64().unwrap()
    };
    let years = |date: convex_core::Date| settlement.days_between(&date) as f64 / 365.0;
    let maturity = |note: &BondReferenceData| years(note.maturity_date.unwrap());
    let (short, long) = (&notes[0].0, &notes[1].0);
    let at = |time: f64| {
        note_yield(short)
            + (note_yield(long) - note_yield(short)) * (time - maturity(short))
                / (maturity(long) - maturity(short))
    };
    let mut in_force = rule_in_force(json!({"kind": "yield"}), &notes);
    in_force.rule.tenor_basis = TenorBasis::Workout;
    let to = |worst: PerpetualWorkout| PerpetualValuation {
        clean_price: dec!(104.25),
        dirty_price: 104.5,
        accrued: 0.25,
        yield_to_worst: 0.0526,
        worst,
    };
    let g_spread = |worst| {
        perpetual_g_spread_under_rule(
            &bond,
            Some(&in_force),
            &to(worst),
            settlement,
            &BenchmarkYieldCache::default(),
        )
        .unwrap()
    };
    let first_call = convex_core::Date::parse("2030-01-15").unwrap();
    let spread = g_spread(PerpetualWorkout::Call { date: first_call }).unwrap();
    let expected = ((0.0526 - at(years(first_call))) * 10_000.0).round();
    assert_eq!(spread.to_f64().unwrap(), expected);
    // The perpetuity, and a call beyond the 10-year, read the 10-year: the spread over the
    // long bond. A call before the 2-year reads the 2-year.
    let over = |benchmark: f64| ((0.0526 - benchmark) * 10_000.0).round();
    let beyond = PerpetualWorkout::Call {
        date: convex_core::Date::parse("2036-01-15").unwrap(),
    };
    for worst in [PerpetualWorkout::Perpetuity, beyond] {
        let spread = g_spread(worst).unwrap();
        assert_eq!(
            spread.to_f64().unwrap(),
            over(note_yield(long)),
            "{worst:?}"
        );
    }
    let before = PerpetualWorkout::Call {
        date: convex_core::Date::parse("2027-07-15").unwrap(),
    };
    let spread = g_spread(before).unwrap();
    assert_eq!(spread.to_f64().unwrap(), over(note_yield(short)));
    // A rule naming no set publishes none.
    in_force.benchmark_set = None;
    assert!(perpetual_g_spread_under_rule(
        &bond,
        Some(&in_force),
        &to(PerpetualWorkout::Perpetuity),
        settlement,
        &BenchmarkYieldCache::default(),
    )
    .is_none());
}
