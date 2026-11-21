//! Callables through the runtime: stored terms with explicit call mechanics, a stored curve
//! recipe and the configuration's HW1F assumption, valued by `convex::library`.
#[path = "support/configuration_setup.rs"]
mod configuration;
#[path = "support/products.rs"]
mod products;
#[path = "support/curve_replay.rs"]
mod recipes;
use configuration::{measure, runtime::*};
use convex_core::contracts::*;
use convex_engine::{
    level_pricing::QuotedInput, optional_valuation::*, snapshot::RUNTIME_MEASURES, RuntimeInput,
};
use convex_ports::{
    reference_master::CallStyle,
    reference_repository::{ReferenceChange, ReferenceCommand},
    snapshot::SnapshotRepository,
    storage::OptionModelAssumption,
};
use rust_decimal::Decimal;
use std::str::FromStr;

/// Version 2 of BOND-001 is a callable; configuration version 2 carries the HW1F assumption.
/// The plan requests the router measures and `oas_mid` together, over a stored recipe.
async fn callable_setup(style: CallStyle, assumption: bool) -> Setup {
    let mut setup = setup().await;
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("callable-terms").unwrap(),
                changes: vec![ReferenceChange::Update {
                    bond: Box::new(products::callable(fixture::bond(), style)),
                    expected_version: fixture::version(1),
                }],
                master_changes: vec![],
            },
            fixture::time("2025-06-01T00:00:00Z"),
        )
        .await
        .unwrap();
    let mut config = fixture::config();
    config.version = 2;
    config.option_model = assumption.then(|| OptionModelAssumption {
        mean_reversion: 0.03,
        volatility: 0.01,
        reason: "fictional fixture assumption".into(),
    });
    setup.storage.configs.save(&config).await.unwrap();
    let plan = &mut setup.plan;
    plan.assembly.instruments[0].version = fixture::version(2);
    plan.assembly.configuration.version = fixture::version(2);
    plan.assembly.curves.clear();
    plan.assembly.curve_recipes = vec![recipes::record(recipes::recipe())];
    plan.measures = RUNTIME_MEASURES
        .iter()
        .chain(["oas_mid"].iter())
        .map(|m| MeasureId::new(*m).unwrap())
        .collect();
    setup
}

/// [`callable_setup`] for a Bermudan under the HW1F assumption, quoted by an `oas` rule,
/// with the plan's quote also holding 50 bp in the rule's field, `mid_spread`, as a plan
/// compiled before the rule may.
async fn oas_setup() -> Setup {
    let mut setup = callable_setup(CallStyle::Bermudan, true).await;
    let plan = &mut setup.plan;
    products::also_quote_in(
        &mut plan.assembly.quotes[0],
        &mut plan.seed_events[0],
        convex_ports::quote_event::QuoteField::MidSpread,
        Decimal::from(50),
    );
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("callable-oas-rule").unwrap(),
                changes: vec![],
                master_changes: vec![products::pricing_rule(
                    "BOND-001",
                    serde_json::json!({"kind": "oas"}),
                )],
            },
            fixture::time("2025-06-02T00:00:00Z"),
        )
        .await
        .unwrap();
    setup
}

/// A started captured runtime over `setup`'s plan, with its discount recipe stored and
/// configuration version 2 activated.
async fn request_runtime(setup: &Setup) -> std::sync::Arc<convex_engine::PricingEngine> {
    configuration::request_runtime(setup, vec![recipes::record(recipes::recipe())]).await
}

#[tokio::test]
async fn a_stored_bermudan_callable_publishes_its_oas_and_effective_risk() {
    let setup = callable_setup(CallStyle::Bermudan, true).await;
    let engine = make_engine(
        &setup,
        archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let published = engine.runtime_results();
    engine.shutdown().await.unwrap();
    assert_eq!(published.len(), 1);
    let result = &published[0].result;
    assert_eq!(
        result.status(),
        ResultStatus::Complete,
        "{:?}",
        result.errors()
    );
    assert_eq!(
        result.provenance().model_version.as_str(),
        OPTIONAL_SNAPSHOT_MODEL
    );
    // The callable answers only the plan measures its model publishes.
    let names: Vec<_> = result.results().keys().map(|m| m.as_str()).collect();
    let mut expected = OPTIONAL_MEASURES.to_vec();
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
    assert!(matches!(value("oas_mid"), MeasureValue::ModelBps(_)));
    assert!(model("oas_mid").is_finite());
    assert!(model("effective_duration_mid") > 0.0 && model("pv01_mid") > 0.0);
    assert!((model("dirty_price_mid") - 99.25 - model("accrued_interest")).abs() < 1e-12);
    // The snapshot's own context for this instrument carries the same subset and model.
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
    assert_eq!(context.spec().model_id.as_str(), OPTIONAL_SNAPSHOT_MODEL);
    assert_eq!(context.spec().measures.len(), OPTIONAL_MEASURES.len());
    // A plan asking only for to-maturity yield has nothing this callable can answer.
    let mut yield_only = setup.plan.clone();
    yield_only.measures = vec![MeasureId::new("ytm_mid").unwrap()];
    assert!(yield_only
        .context(&snapshot, &InstrumentId::new("BOND-001").unwrap())
        .is_err());
}

#[tokio::test]
async fn callables_without_a_model_assumption_or_with_american_calls_publish_the_reason() {
    for (style, assumption, needle) in [
        (CallStyle::Bermudan, false, "option_model"),
        (CallStyle::American, true, "American"),
    ] {
        let setup = callable_setup(style, assumption).await;
        let engine = make_engine(
            &setup,
            archive(&setup),
            setup.plan.clone(),
            setup.store.clone(),
            RuntimeInput::Captured,
        );
        engine.start().await.unwrap();
        let published = engine.runtime_results();
        engine.shutdown().await.unwrap();
        let result = &published[0].result;
        assert_eq!(result.status(), ResultStatus::Failed);
        assert_eq!(result.errors().len(), OPTIONAL_MEASURES.len());
        for error in result.errors() {
            assert_eq!(error.code(), ErrorCode::UnsupportedConvention);
            assert!(error.message().contains(needle), "{}", error.message());
        }
    }
}

#[tokio::test]
async fn configuration_validation_reports_a_callable_no_requested_measure_applies_to() {
    let setup = callable_setup(CallStyle::Bermudan, true).await;
    let mut document = configuration::fixture::document(1);
    document.assembly.instruments[0].version = fixture::version(2);
    document.runtime.measures = vec![MeasureId::new("ytm_mid").unwrap()];
    let report = configuration::cases(&setup)
        .validate(&document, &configuration::fixtures(&setup))
        .await
        .unwrap();
    assert!(!report.valid);
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.code == "NO_APPLICABLE_MEASURE"),
        "{:?}",
        report.issues
    );
}

#[tokio::test]
async fn plans_accept_oas_and_still_refuse_measures_no_model_publishes() {
    let setup = callable_setup(CallStyle::Bermudan, true).await;
    setup.plan.validate().unwrap();
    let mut unknown = setup.plan.clone();
    unknown
        .measures
        .push(MeasureId::new("yield_to_worst_mid").unwrap());
    assert!(unknown.validate().is_err());
}

/// A request-time valuation of the stored callable, at the stream's cutoff and
/// settlement and with the stream's mark, gives the stream's values. Its one-instrument
/// snapshot reads only the discount curve and is not stored.
#[tokio::test]
async fn a_request_time_callable_valuation_matches_the_stream() {
    let setup = callable_setup(CallStyle::Bermudan, true).await;
    let engine = request_runtime(&setup).await;
    let stream = engine.runtime_results()[0].result.clone();
    let valued =
        configuration::value_at(&engine, &stream, setup.plan.settlement_date, Some("99.25"))
            .await
            .unwrap();
    engine.shutdown().await.unwrap();
    let result = valued.result.envelope();
    assert_eq!(
        result.status(),
        ResultStatus::Complete,
        "{:?}",
        result.errors()
    );
    assert_eq!(result.results(), stream.results());
    assert_eq!(
        valued.curves,
        vec![("USD-FIXTURE".to_owned(), fixture::version(7))]
    );
    assert!(setup
        .snapshots
        .get(&result.provenance().snapshot_id)
        .await
        .unwrap()
        .is_none());
}

/// A callable under an `oas` rule is valued from its price in Live, exactly as with no
/// rule, though its quote also holds a spread in the rule's field. At request time it is priced at an OAS: the stream's OAS gives the stream's
/// price back, with the same accrued, dirty price and risk, and the OAS as quoted; a
/// price gives the stream's values. An OAS out of range is refused before anything is
/// valued.
#[tokio::test]
async fn a_callable_under_an_oas_rule_is_valued_from_its_price_in_live_and_its_oas_at_request_time()
{
    let unruled = request_runtime(&callable_setup(CallStyle::Bermudan, true).await).await;
    let by_price = unruled.runtime_results()[0].result.clone();
    unruled.shutdown().await.unwrap();
    let setup = oas_setup().await;
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
        OPTIONAL_SNAPSHOT_MODEL
    );
    assert_eq!(stream.results(), by_price.results());

    let settlement = setup.plan.settlement_date;
    let at = |mark| configuration::value_quoted_at(&engine, &stream, settlement, Some(mark));
    let oas = measure(&stream, "oas_mid");
    let by_oas = at(QuotedInput::Level(Decimal::from_f64_retain(oas).unwrap()))
        .await
        .unwrap();
    let by_oas = by_oas.result.envelope();
    assert_eq!(
        by_oas.status(),
        ResultStatus::Complete,
        "{:?}",
        by_oas.errors()
    );
    assert_eq!(measure(by_oas, "oas_mid"), oas);
    assert!((measure(by_oas, "clean_price_mid") - 99.25).abs() < 1e-6);
    for name in [
        "dirty_price_mid",
        "accrued_interest",
        "effective_duration_mid",
        "effective_convexity_mid",
        "pv01_mid",
    ] {
        assert!(
            (measure(by_oas, name) - measure(&stream, name)).abs() < 1e-6,
            "{name}"
        );
    }
    let priced = at(QuotedInput::Price(Decimal::from_str("99.25").unwrap()))
        .await
        .unwrap();
    assert_eq!(priced.result.envelope().results(), stream.results());
    let refused = at(QuotedInput::Level(Decimal::from(1001)))
        .await
        .err()
        .unwrap();
    assert!(
        refused.to_string().contains("basis points of OAS"),
        "{refused}"
    );
    engine.shutdown().await.unwrap();
}

/// One request context prices each curve on one revision. A bullet's read takes the
/// discount curve at revision 7; revision 8, recorded by the same cutoff, is saved; the
/// callable then prices on revision 7, which the context holds.
#[tokio::test]
async fn a_request_context_prices_each_curve_on_one_revision() {
    let setup = callable_setup(CallStyle::Bermudan, true).await;
    let mut config = setup
        .storage
        .configs
        .get_version("CONFIG-RESTORE", 2)
        .await
        .unwrap()
        .unwrap();
    config.version = 3;
    config.analytics_curves.swap_curve = Some(convex_core::ids::CurveId::new("USD-SWAP"));
    setup.storage.configs.save(&config).await.unwrap();
    let discount = recipes::record(recipes::recipe());
    let mut swap = products::projection(recipes::recipe());
    swap["key"]["id"] = "USD-SWAP".into();
    for record in [discount.clone(), recipes::record(swap)] {
        setup.storage.curves.save_recipe(&record).await.unwrap();
    }
    let engine = make_engine(
        &setup,
        archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    configuration::activate_pricing(&setup, &config).await;
    let stream = engine.runtime_results()[0].result.clone();
    let known_at = stream.provenance().known_at;
    let ValuationSubject::InstrumentVersion { instrument_id, .. } = &stream.provenance().subject
    else {
        panic!("instrument valuation")
    };
    let reference = engine
        .reference_at_for_pricing(
            &instrument_id.clone().into(),
            convex_ports::reference_master::ReferenceLookup {
                as_of: known_at,
                known_at,
            },
        )
        .await
        .unwrap()
        .unwrap();

    let mut context = engine.request_context(None, known_at).await.unwrap();
    context.curves_for(&fixture::bond()).await.unwrap();
    let mut newer = discount;
    newer.version = fixture::version(8);
    newer.recipe["key"]["version"] = "supplied.recipe.v8".into();
    setup.storage.curves.save_recipe(&newer).await.unwrap();
    let mark = Some(convex_engine::level_pricing::QuotedInput::Price(
        Decimal::from_str("99.25").unwrap(),
    ));
    let valued = engine
        .value_request(
            &mut context,
            &reference,
            setup.plan.settlement_date,
            known_at,
            mark,
        )
        .await
        .unwrap();
    assert_eq!(
        valued.curves,
        vec![("USD-FIXTURE".to_owned(), fixture::version(7))]
    );
    assert_eq!(
        context.pins(),
        vec![
            ("USD-FIXTURE", fixture::version(7)),
            ("USD-SWAP", fixture::version(7))
        ]
    );
    // Revision 8 is recorded by the cutoff: a new context reads it.
    let mut fresh = engine.request_context(None, known_at).await.unwrap();
    fresh.curves_for(&fixture::bond()).await.unwrap();
    assert_eq!(fresh.pins()[0], ("USD-FIXTURE", fixture::version(8)));
    engine.shutdown().await.unwrap();
}
