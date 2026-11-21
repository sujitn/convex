//! Floating rate notes through the runtime: stored terms pinning a rate-index resource, its
//! published fixings in the durable store, and stored discount and projection recipes,
//! valued by `convex::library` at the observed clean mid, or at the discount margin a
//! note's rule quotes it by.
#[path = "support/configuration_setup.rs"]
mod configuration;
#[path = "support/products.rs"]
mod products;
#[path = "support/curve_replay.rs"]
mod recipes;
use configuration::{measure, runtime::*};
use convex_core::{contracts::*, Date};
use convex_engine::{
    floating_valuation::*, level_pricing::QuotedInput, snapshot::RUNTIME_MEASURES, RuntimeInput,
};
use convex_ports::{
    fixing::{FixingRepository, PublishedFixing},
    quote_event::QuoteField,
    reference_repository::{ReferenceChange, ReferenceCommand},
    snapshot::SnapshotRepository,
};
use products::{convention, index, published};
use rust_decimal::Decimal;
use std::str::FromStr;

/// Version 2 of BOND-001 is the note; configuration version 2 names the projection curve.
/// The plan requests every router measure and the discount margin, over stored recipes.
async fn frn_setup(fixings: Vec<PublishedFixing>, projection_role: bool) -> Setup {
    frn_setup_from(setup().await, fixings, projection_role).await
}

/// [`frn_setup`] over `setup`, whose BOND-001 is still the fixed bullet.
async fn frn_setup_from(
    mut setup: Setup,
    fixings: Vec<PublishedFixing>,
    projection_role: bool,
) -> Setup {
    let bond = products::frn(fixture::bond());
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("frn-terms").unwrap(),
                changes: vec![ReferenceChange::Update {
                    bond: Box::new(bond.clone()),
                    expected_version: fixture::version(1),
                }],
                master_changes: vec![convention(&bond), index(&bond)],
            },
            fixture::time("2025-06-01T00:00:00Z"),
        )
        .await
        .unwrap();
    if !fixings.is_empty() {
        setup.fixings.publish(&fixings).await.unwrap();
    }
    let mut config = fixture::config();
    config.version = 2;
    config.analytics_curves.frn_reference_curve =
        projection_role.then(|| convex_core::ids::CurveId::new("USD-SOFR-PROJECTION"));
    setup.storage.configs.save(&config).await.unwrap();
    let plan = &mut setup.plan;
    plan.assembly.instruments[0].version = fixture::version(2);
    plan.assembly.configuration.version = fixture::version(2);
    plan.assembly.curves.clear();
    plan.assembly.curve_recipes = vec![
        recipes::record(recipes::recipe()),
        recipes::record(products::projection(recipes::recipe())),
    ];
    plan.assembly
        .required_curves
        .push(CurveId::new("USD-SOFR-PROJECTION").unwrap());
    plan.measures = RUNTIME_MEASURES
        .iter()
        .chain(["discount_margin_mid"].iter())
        .map(|m| MeasureId::new(*m).unwrap())
        .collect();
    setup
}
/// [`frn_setup`] with every fixing, the note quoted by its discount margin: a
/// `discount_margin` rule, and the plan reading the rule's field, `mid_spread`, holding
/// `margin`.
async fn margin_setup(margin: Decimal) -> Setup {
    let mut setup = frn_setup(published(), true).await;
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("frn-margin-rule").unwrap(),
                changes: vec![],
                master_changes: vec![products::pricing_rule(
                    "BOND-001",
                    serde_json::json!({"kind": "discount_margin"}),
                )],
            },
            fixture::time("2025-06-02T00:00:00Z"),
        )
        .await
        .unwrap();
    let plan = &mut setup.plan;
    products::quote_in(
        &mut plan.assembly.quotes[0],
        &mut plan.seed_events[0],
        QuoteField::MidSpread,
        margin,
    );
    setup
}

/// A started captured runtime over `setup`'s plan, with its discount and projection
/// recipes stored and configuration version 2 activated.
async fn request_runtime(setup: &Setup) -> std::sync::Arc<convex_engine::PricingEngine> {
    let recipes = [recipes::recipe(), products::projection(recipes::recipe())];
    configuration::request_runtime(setup, recipes.map(recipes::record).to_vec()).await
}

async fn run(setup: &Setup) -> Vec<convex_ports::runtime::PublishedValuation> {
    let engine = make_engine(
        setup,
        archive(setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let published = engine.runtime_results();
    engine.shutdown().await.unwrap();
    published
}

#[tokio::test]
async fn a_stored_sofr_note_publishes_its_discount_margin_from_derived_fixings() {
    let setup = frn_setup(published(), true).await;
    let outcome = run(&setup).await;
    assert_eq!(outcome.len(), 1);
    let result = &outcome[0].result;
    assert_eq!(
        result.status(),
        ResultStatus::Complete,
        "{:?}",
        result.errors()
    );
    assert_eq!(
        result.provenance().model_version.as_str(),
        FLOATING_SNAPSHOT_MODEL
    );
    // The note answers only the plan measures its model publishes.
    let names: Vec<_> = result.results().keys().map(|m| m.as_str()).collect();
    let mut expected = FLOATING_MEASURES.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
    let value = |name: &str| {
        result.results()[&MeasureId::new(name).unwrap()]
            .value
            .clone()
    };
    let MeasureValue::Price(clean) = value("clean_price_mid") else {
        panic!("observed clean mid")
    };
    assert_eq!(clean.value().decimal(), Decimal::from_str("99.25").unwrap());
    let model = |name: &str| measure(result, name);
    assert!(matches!(
        value("discount_margin_mid"),
        MeasureValue::ModelBps(_)
    ));
    assert!(model("discount_margin_mid").is_finite());
    assert!(model("accrued_interest") > 0.0);
    // The solve reprices the observed clean mid to its 1e-9 price tolerance.
    assert!((model("dirty_price_mid") - 99.25 - model("accrued_interest")).abs() < 1e-8);
    // The snapshot holds exactly the due observations, from the current period's
    // lookback to Thursday, each found in the store; Friday's own fixing is forecast.
    let snapshot = setup
        .snapshots
        .get(&result.provenance().snapshot_id)
        .await
        .unwrap()
        .unwrap();
    let fixings = &snapshot.content().fixings;
    assert!(fixings.iter().all(|f| f.fixing.is_some()));
    assert_eq!(
        fixings.first().unwrap().key.observation_date.date(),
        Date::parse("2026-07-13").unwrap()
    );
    assert_eq!(
        fixings.last().unwrap().key.observation_date.date(),
        Date::parse("2026-10-01").unwrap()
    );
    // The note reads the fixings on its own index and fixing source, so a missing
    // fixing on another index fails the snapshot but not the note.
    let mut content = snapshot.content().clone();
    let mut other = content.fixings[0].key.clone();
    other.index_id = IndexId::new("EUR-ESTR").unwrap();
    content
        .fixings
        .push(convex_ports::snapshot::SnapshotFixing {
            key: other,
            fixing: None,
        });
    let widened = convex_ports::snapshot::MarketSnapshot::new(content).unwrap();
    assert_eq!(widened.quality().quality, DataQuality::Invalid);
    // It reads its discount and reference curves.
    let roles = &widened.content().configuration.analytics_curves;
    let read = [
        roles.discount_curve.as_str(),
        roles.frn_reference_curve.as_ref().unwrap().as_str(),
    ];
    let inputs = widened.inputs(fixture::bond().instrument_id.as_str(), &read);
    assert_eq!(inputs.fixings().len(), fixings.len());
    assert_eq!(
        inputs
            .curves()
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        read
    );
    assert_eq!(widened.quality_of(&inputs).quality, DataQuality::Valid);
    assert!(result
        .provenance()
        .input_versions
        .iter()
        .any(|v| v.kind.as_str() == "rate_index" && v.id.as_str() == "usd-sofr"));
}

#[tokio::test]
async fn a_missing_due_fixing_or_projection_curve_publishes_the_reason() {
    // A due fixing absent from the store stays explicit, so the snapshot is incomplete.
    let mut fixings = published();
    fixings.retain(|f| f.key.observation_date.date() != Date::parse("2026-09-15").unwrap());
    let setup = frn_setup(fixings, true).await;
    let outcome = run(&setup).await;
    let result = &outcome[0].result;
    assert_eq!(result.status(), ResultStatus::Failed);
    let snapshot = setup
        .snapshots
        .get(&result.provenance().snapshot_id)
        .await
        .unwrap()
        .unwrap();
    assert!(snapshot.content().fixings.iter().any(|f| f.fixing.is_none()
        && f.key.observation_date.date() == Date::parse("2026-09-15").unwrap()));
    // Without the configuration's projection curve, every measure says so.
    let setup = frn_setup(published(), false).await;
    let outcome = run(&setup).await;
    let result = &outcome[0].result;
    assert_eq!(result.status(), ResultStatus::Failed);
    assert_eq!(result.errors().len(), FLOATING_MEASURES.len());
    for error in result.errors() {
        assert_eq!(error.code(), ErrorCode::UnsupportedConvention);
        assert!(
            error.message().contains("frn_reference_curve"),
            "{}",
            error.message()
        );
    }
}
#[tokio::test]
async fn plans_accept_the_discount_margin_and_still_refuse_measures_no_model_publishes() {
    let setup = frn_setup(published(), true).await;
    setup.plan.validate().unwrap();
    let mut unknown = setup.plan.clone();
    unknown
        .measures
        .push(MeasureId::new("yield_to_worst_mid").unwrap());
    assert!(unknown.validate().is_err());
}

/// A request-time valuation of the stored note at the stream's cutoff, settlement
/// and mark gives the stream's values, and with a due fixing missing from the store it
/// fails as the stream does.
#[tokio::test]
async fn a_request_time_note_valuation_matches_the_stream_and_fails_as_it_does() {
    let mut missing = published();
    missing.retain(|f| f.key.observation_date.date() != Date::parse("2026-09-15").unwrap());
    for (fixings, complete) in [(published(), true), (missing, false)] {
        let setup = frn_setup(fixings, true).await;
        let engine = request_runtime(&setup).await;
        let stream = engine.runtime_results()[0].result.clone();
        let valued =
            configuration::value_at(&engine, &stream, setup.plan.settlement_date, Some("99.25"))
                .await
                .unwrap();
        engine.shutdown().await.unwrap();
        let result = valued.result.envelope();
        assert_eq!(result.status() == ResultStatus::Complete, complete);
        assert_eq!(result.status(), stream.status());
        assert_eq!(result.results(), stream.results());
        assert_eq!(result.errors(), stream.errors());
        assert_eq!(valued.curves.len(), 2, "{:?}", valued.curves);
    }
}

/// A note quoted by its discount margin is priced at it in Live: the margin solved from a
/// clean mid gives that mid back, with the same accrued and dirty price, and the margin
/// as quoted. A request at the stream's cutoff gives the stream's values from the margin,
/// and the margin back from the price. A margin out of range is refused by a request and
/// fails every measure in Live.
#[tokio::test]
async fn a_note_quoted_by_its_discount_margin_is_priced_at_it_in_live_and_at_request_time() {
    let by_price = run(&frn_setup(published(), true).await).await[0]
        .result
        .clone();
    let solved = measure(&by_price, "discount_margin_mid");
    let level = Decimal::from_f64_retain(solved).unwrap();
    let setup = margin_setup(level).await;
    let engine = request_runtime(&setup).await;
    let stream = engine.runtime_results()[0].result.clone();
    assert_eq!(
        stream.status(),
        ResultStatus::Complete,
        "{:?}",
        stream.errors()
    );
    assert_eq!(
        stream.provenance().model_version.as_str(),
        FLOATING_SNAPSHOT_MODEL
    );
    assert!((measure(&stream, "clean_price_mid") - 99.25).abs() < 1e-9);
    for name in ["dirty_price_mid", "accrued_interest"] {
        assert!(
            (measure(&stream, name) - measure(&by_price, name)).abs() < 1e-9,
            "{name}"
        );
    }
    assert_eq!(measure(&stream, "discount_margin_mid"), solved);

    let settlement = setup.plan.settlement_date;
    let at = |mark| configuration::value_quoted_at(&engine, &stream, settlement, Some(mark));
    let by_margin = at(QuotedInput::Level(level)).await.unwrap();
    assert_eq!(by_margin.result.envelope().results(), stream.results());
    let priced = at(QuotedInput::Price(Decimal::from_str("99.25").unwrap()))
        .await
        .unwrap();
    let priced = priced.result.envelope();
    assert!((measure(priced, "discount_margin_mid") - solved).abs() < 1e-9);
    // A request's margin out of range is refused before anything is valued.
    let refused = at(QuotedInput::Level(Decimal::from(6000)))
        .await
        .err()
        .unwrap();
    assert!(
        refused.to_string().contains("basis points of margin"),
        "{refused}"
    );
    engine.shutdown().await.unwrap();

    // A Live margin out of range fails every measure, saying why.
    let outcome = run(&margin_setup(Decimal::from(6000)).await).await;
    let result = &outcome[0].result;
    assert_eq!(result.status(), ResultStatus::Failed);
    assert_eq!(result.errors().len(), FLOATING_MEASURES.len());
    assert!(result.errors().iter().all(|error| {
        error.code() == ErrorCode::InvalidInput
            && error.message().contains("basis points of margin")
    }));
}

/// A note with no rule is quoted by price: a quote holding only a spread is not read as
/// its margin, and the note is not valued.
#[tokio::test]
async fn a_note_with_no_rule_is_not_valued_from_a_spread() {
    let mut setup = frn_setup(published(), true).await;
    let plan = &mut setup.plan;
    products::quote_in(
        &mut plan.assembly.quotes[0],
        &mut plan.seed_events[0],
        QuoteField::MidSpread,
        Decimal::from(51),
    );
    let outcome = run(&setup).await;
    let result = &outcome[0].result;
    assert_eq!(result.status(), ResultStatus::Failed);
    assert_eq!(result.errors().len(), FLOATING_MEASURES.len());
    assert!(result
        .errors()
        .iter()
        .all(|error| error.code() == ErrorCode::InvalidInput));
}

/// A rule committed while BOND-001 was a fixed bullet, naming a benchmark set, does not
/// price it once its terms are a floating rate note: each measure is refused, saying why.
#[tokio::test]
async fn a_rule_the_note_no_longer_takes_refuses_its_measures() {
    let setup = setup().await;
    let mut treasury = fixture::bond();
    treasury.instrument_id = convex_core::ids::InstrumentId::new("UST-10Y");
    treasury.issuer_type = convex_ports::reference_data::IssuerType::Sovereign;
    treasury.cusip = Some("91282CBB6".into());
    treasury.isin = None;
    let effective = serde_json::json!({"from": "2025-01-01T00:00:00Z", "until": null});
    let provenance = serde_json::json!({"source": "desk", "evidence_reference": "frn-test"});
    let master = |kind: &str, definition: serde_json::Value| {
        serde_json::from_value(serde_json::json!({
            "expected_version": null,
            "definition": {"kind": kind, "definition": definition},
        }))
        .unwrap()
    };
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("bullet-rule").unwrap(),
                changes: vec![ReferenceChange::Create {
                    bond: Box::new(treasury),
                }],
                master_changes: vec![
                    master(
                        "benchmark_set",
                        serde_json::json!({
                            "benchmark_set_id": "UST-10",
                            "currency": "USD",
                            "securities": [{"tenor": "10Y", "instrument_id": "UST-10Y", "version": 1}],
                            "effective": effective,
                            "provenance": provenance,
                        }),
                    ),
                    master(
                        "pricing_rule",
                        serde_json::json!({
                            "instrument_id": "BOND-001",
                            "method": {"kind": "price"},
                            "benchmark_set_id": "UST-10",
                            "effective": effective,
                            "provenance": provenance,
                        }),
                    ),
                ],
            },
            fixture::time("2025-03-01T00:00:00Z"),
        )
        .await
        .unwrap();
    let setup = frn_setup_from(setup, published(), true).await;
    let outcome = run(&setup).await;
    let result = &outcome[0].result;
    assert_eq!(result.status(), ResultStatus::Failed);
    assert!(result.errors().iter().all(|error| {
        error.code() == ErrorCode::InvalidInput && error.message().contains("only a fixed bullet")
    }));
}
