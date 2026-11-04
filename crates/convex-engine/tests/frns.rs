//! Floating rate notes through the runtime: stored terms pinning a rate-index resource, its
//! published fixings in the durable store, and stored discount and projection recipes,
//! valued by `convex::library` at the observed clean mid.
#[path = "support/configuration_setup.rs"]
mod configuration;
#[path = "support/products.rs"]
mod products;
#[path = "support/curve_replay.rs"]
mod recipes;
use configuration::runtime::*;
use convex_core::{contracts::*, Date};
use convex_engine::{floating_valuation::*, snapshot::RUNTIME_MEASURES, RuntimeInput};
use convex_ports::{
    fixing::{FixingRepository, PublishedFixing},
    reference_repository::{ReferenceChange, ReferenceCommand},
    snapshot::SnapshotRepository,
};
use products::{convention, index, published};
use rust_decimal::Decimal;
use std::str::FromStr;

/// Version 2 of BOND-001 is the note; configuration version 2 names the projection curve.
/// The plan requests every router measure and the discount margin, over stored recipes.
async fn frn_setup(fixings: Vec<PublishedFixing>, projection_role: bool) -> Setup {
    let mut setup = setup().await;
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
    // D34: the note answers only the plan measures its model publishes.
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
    let model = |name: &str| match value(name) {
        MeasureValue::ModelPrice(v) => v.value().get(),
        MeasureValue::ModelBps(v) => v.value().get(),
        other => panic!("{name}: {other:?}"),
    };
    assert!(matches!(
        value("discount_margin_mid"),
        MeasureValue::ModelBps(_)
    ));
    assert!(model("discount_margin_mid").is_finite());
    assert!(model("accrued_interest") > 0.0);
    // The solve reprices the observed clean mid to its 1e-9 price tolerance.
    assert!((model("dirty_price_mid") - 99.25 - model("accrued_interest")).abs() < 1e-8);
    // D33: the snapshot holds exactly the due observations, from the current period's
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
    // D58: the note reads the fixings on its own index and fixing source, so a missing
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
        .push(MeasureId::new("g_spread_mid").unwrap());
    assert!(unknown.validate().is_err());
}

/// D57: a request-time valuation of the stored note at the stream's cutoff, settlement
/// and mark gives the stream's values, and with a due fixing missing from the store it
/// fails as the stream does.
#[tokio::test]
async fn a_request_time_note_valuation_matches_the_stream_and_fails_as_it_does() {
    let mut missing = published();
    missing.retain(|f| f.key.observation_date.date() != Date::parse("2026-09-15").unwrap());
    for (fixings, complete) in [(published(), true), (missing, false)] {
        let setup = frn_setup(fixings, true).await;
        let config = setup
            .storage
            .configs
            .get_version("CONFIG-RESTORE", 2)
            .await
            .unwrap()
            .unwrap();
        for recipe in [recipes::recipe(), products::projection(recipes::recipe())] {
            setup
                .storage
                .curves
                .save_recipe(&recipes::record(recipe))
                .await
                .unwrap();
        }
        let engine = make_engine(
            &setup,
            archive(&setup),
            setup.plan.clone(),
            setup.store.clone(),
            RuntimeInput::Captured,
        );
        engine.start().await.unwrap();
        // A runtime without configuration use cases starts only with nothing activated.
        configuration::activate_pricing(&setup, &config).await;
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
