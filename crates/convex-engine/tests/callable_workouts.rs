//! Callables quoted by price or by yield to worst, valued on their workouts by the
//! runtime: no option model and no curve. A request at the stream's cutoff gives the
//! stream's values, from a price or from a yield to worst.
#[path = "support/configuration_setup.rs"]
mod configuration;
#[path = "support/products.rs"]
mod products;
use configuration::runtime::*;
use convex_core::contracts::*;
use convex_engine::{
    level_pricing::QuotedInput,
    workout_valuation::{WORKOUT_MEASURES, WORKOUT_SNAPSHOT_MODEL},
    RuntimeInput,
};
use convex_ports::{
    quote_event::QuoteField,
    reference_master::CallStyle,
    reference_repository::{ReferenceChange, ReferenceCommand},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{json, Value};

/// Version 2 of BOND-001 is the fixture callable, quoted by `method` under a pricing rule.
/// The plan reads its rule's field, holding `mark`, and asks for the workout measures.
/// Configuration version 1 has no option model.
async fn workout_setup(method: Value, field: QuoteField, mark: Decimal) -> Setup {
    let mut setup = setup().await;
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("callable-on-workouts").unwrap(),
                changes: vec![ReferenceChange::Update {
                    bond: Box::new(products::callable(fixture::bond(), CallStyle::Bermudan)),
                    expected_version: fixture::version(1),
                }],
                master_changes: vec![products::pricing_rule(method)],
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
    plan.measures = WORKOUT_MEASURES
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

/// The one published result of a captured runtime over `setup`'s plan.
async fn stream(setup: &Setup) -> ResultEnvelope {
    let (engine, result) = started(setup).await;
    engine.shutdown().await.unwrap();
    result
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
        MeasureValue::ModelYears(v) => v.value().get(),
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
async fn a_callable_quoted_by_price_publishes_its_yield_to_worst_with_no_option_model() {
    let setup = workout_setup(json!({"kind": "price"}), QuoteField::MidPrice, dec!(99.25)).await;
    let result = stream(&setup).await;
    assert_complete(&result);
    assert_eq!(
        result.provenance().model_version.as_str(),
        WORKOUT_SNAPSHOT_MODEL
    );
    let names: Vec<_> = result.results().keys().map(|m| m.as_str()).collect();
    let mut expected = WORKOUT_MEASURES.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
    assert_eq!(clean(&result), dec!(99.25));
    assert!(
        (model(&result, "dirty_price_mid") - 99.25 - model(&result, "accrued_interest")).abs()
            < 1e-12
    );
    // Below every call price, the worst is maturity: 3 years and 101 days, ACT/ACT ISDA.
    assert!((model(&result, "years_to_worst_mid") - (3.0 + 101.0 / 365.0)).abs() < 1e-12);
    let ytw = model(&result, "ytw_mid");
    assert!((0.05..0.06).contains(&ytw), "{ytw}");
}

#[tokio::test]
async fn a_callable_quoted_by_yield_to_worst_is_priced_to_worst() {
    let setup = workout_setup(json!({"kind": "yield"}), QuoteField::MidYield, dec!(0.045)).await;
    let result = stream(&setup).await;
    assert_complete(&result);
    assert_eq!(model(&result, "ytw_mid"), 0.045);
    // At 4.5% the 5.375% bond is at a premium, and the first call, at 101 in January
    // 2027, is worst.
    assert!((model(&result, "years_to_worst_mid") - 101.0 / 365.0).abs() < 1e-12);
    let clean = clean(&result);
    assert!(clean > dec!(100) && clean < dec!(102), "{clean}");
    let dirty = model(&result, "dirty_price_mid");
    let accrued = model(&result, "accrued_interest");
    assert!(
        (dirty - accrued - rust_decimal::prelude::ToPrimitive::to_f64(&clean).unwrap()).abs()
            < 1e-9
    );
}

/// A request at the stream's cutoff, by the stream's yield to worst or by a price, gives
/// the stream's values.
#[tokio::test]
async fn a_request_values_a_callable_on_its_workouts_as_the_stream_does() {
    let setup = workout_setup(json!({"kind": "yield"}), QuoteField::MidYield, dec!(0.045)).await;
    let (engine, stream) = started(&setup).await;
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
        Some(QuotedInput::Level(dec!(0.045))),
    )
    .await
    .unwrap();
    let by_yield = by_yield.result.envelope();
    assert_complete(by_yield);
    assert_eq!(by_yield.results(), stream.results());
    // The price to worst, sent as a price, comes back to the same yield on the same path.
    let by_price = configuration::value_quoted_at(
        &engine,
        &stream,
        settlement,
        Some(QuotedInput::Price(clean(&stream))),
    )
    .await
    .unwrap();
    let by_price = by_price.result.envelope();
    assert_complete(by_price);
    assert!((model(by_price, "ytw_mid") - 0.045).abs() < 1e-12);
    assert_eq!(
        model(by_price, "years_to_worst_mid"),
        model(&stream, "years_to_worst_mid")
    );
    engine.shutdown().await.unwrap();
}
