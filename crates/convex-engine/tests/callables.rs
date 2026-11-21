//! Callables through the runtime: stored terms with explicit call mechanics, a stored curve
//! recipe and the configuration's HW1F assumption, valued by `convex::library`.
#[path = "support/configuration_setup.rs"]
mod configuration;
#[path = "support/products.rs"]
mod products;
#[path = "support/curve_replay.rs"]
mod recipes;
#[path = "support/subscription_source.rs"]
mod subscription_source;
use configuration::{measure, runtime::*};
use convex_core::contracts::*;
use convex_engine::{
    level_pricing::QuotedInput,
    optional_valuation::*,
    snapshot::{SnapshotValuator, RUNTIME_MEASURES},
    RuntimeInput,
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
    commit_rule(&setup, serde_json::json!({"kind": "oas"})).await;
    setup
}

/// BOND-001 quoted by `method` from 2025-06-02.
async fn commit_rule(setup: &Setup, method: serde_json::Value) {
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("callable-rule").unwrap(),
                changes: vec![],
                master_changes: vec![products::pricing_rule("BOND-001", method)],
            },
            fixture::time("2025-06-02T00:00:00Z"),
        )
        .await
        .unwrap();
}

/// The records a captured runtime over `setup`'s plan publishes at its start.
async fn captured_results(setup: &Setup) -> Vec<convex_ports::runtime::PublishedValuation> {
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

/// A started captured runtime over `setup`'s plan, with its discount recipe stored and
/// configuration version 2 activated.
async fn request_runtime(setup: &Setup) -> std::sync::Arc<convex_engine::PricingEngine> {
    configuration::request_runtime(setup, vec![recipes::record(recipes::recipe())]).await
}

#[tokio::test]
async fn a_stored_bermudan_callable_publishes_its_oas_and_effective_risk() {
    let setup = callable_setup(CallStyle::Bermudan, true).await;
    let published = captured_results(&setup).await;
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
        let published = captured_results(&setup).await;
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

/// A callable under an `oas` rule is valued from its OAS in Live, though its quote also
/// holds a price: the stream publishes the OAS as quoted and the model's price at it. At
/// request time the same OAS gives the stream's values, and the stream's price gives the
/// OAS back, with the same accrued, dirty price and risk. An OAS out of range is refused
/// before anything is valued.
#[tokio::test]
async fn a_callable_under_an_oas_rule_is_valued_from_its_oas_in_live_and_at_request_time() {
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
    assert_eq!(measure(&stream, "oas_mid"), 50.0);
    let clean = measure(&stream, "clean_price_mid");
    assert!((clean - 99.25).abs() > 1e-3, "{clean}");

    let settlement = setup.plan.settlement_date;
    let at = |mark| configuration::value_quoted_at(&engine, &stream, settlement, Some(mark));
    let by_oas = at(QuotedInput::Level(Decimal::from(50))).await.unwrap();
    assert_eq!(by_oas.result.envelope().results(), stream.results());
    let priced = at(QuotedInput::Price(Decimal::from_f64_retain(clean).unwrap()))
        .await
        .unwrap();
    let priced = priced.result.envelope();
    assert_eq!(
        priced.status(),
        ResultStatus::Complete,
        "{:?}",
        priced.errors()
    );
    assert!((measure(priced, "oas_mid") - 50.0).abs() < 1e-4);
    for name in [
        "dirty_price_mid",
        "accrued_interest",
        "effective_duration_mid",
        "effective_convexity_mid",
        "pv01_mid",
    ] {
        assert!(
            (measure(priced, name) - measure(&stream, name)).abs() < 1e-6,
            "{name}"
        );
    }
    let refused = at(QuotedInput::Level(Decimal::from(5001)))
        .await
        .err()
        .unwrap();
    assert!(
        refused.to_string().contains("basis points of OAS"),
        "{refused}"
    );
    engine.shutdown().await.unwrap();
}

/// A Live plan reads a callable under an `oas` rule by its OAS alone, as a compile reads
/// its rule's field, so a feed that sends only its price leaves it unvalued: each measure
/// fails for the missing OAS.
#[tokio::test]
async fn a_callable_under_an_oas_rule_whose_feed_sends_only_a_price_fails_for_its_oas() {
    let mut setup = callable_setup(CallStyle::Bermudan, true).await;
    commit_rule(&setup, serde_json::json!({"kind": "oas"})).await;
    setup.plan.assembly.quotes[0].fields = vec![convex_ports::quote_event::QuoteField::MidSpread];
    let published = captured_results(&setup).await;
    let result = &published[0].result;
    assert_eq!(result.status(), ResultStatus::Failed);
    assert_eq!(result.errors().len(), OPTIONAL_MEASURES.len());
    assert!(
        result
            .errors()
            .iter()
            .all(|error| error.code() == ErrorCode::MissingMarketData),
        "{:?}",
        result.errors()
    );
}

/// A callable priced beyond the OAS solver's bracket, as a distressed one is, publishes
/// its price, dirty price and accrued, and its OAS and effective risk fail, saying why.
#[tokio::test]
async fn a_callable_priced_beyond_the_oas_bracket_keeps_its_price_measures() {
    let mut setup = callable_setup(CallStyle::Bermudan, true).await;
    let plan = &mut setup.plan;
    products::quote_in(
        &mut plan.assembly.quotes[0],
        &mut plan.seed_events[0],
        convex_ports::quote_event::QuoteField::MidPrice,
        Decimal::from(15),
    );
    let published = captured_results(&setup).await;
    let result = &published[0].result;
    assert_eq!(
        result.status(),
        ResultStatus::Partial,
        "{:?}",
        result.errors()
    );
    assert_eq!(measure(result, "clean_price_mid"), 15.0);
    let accrued = measure(result, "accrued_interest");
    assert!((measure(result, "dirty_price_mid") - 15.0 - accrued).abs() < 1e-12);
    let failed: Vec<_> = result
        .errors()
        .iter()
        .map(|error| error.field().unwrap())
        .collect();
    assert_eq!(
        failed,
        [
            "effective_convexity_mid",
            "effective_duration_mid",
            "oas_mid",
            "pv01_mid"
        ]
    );
    assert!(
        result
            .errors()
            .iter()
            .all(|error| error.code() == ErrorCode::SolverLimit
                && error.message().contains("bracket"))
    );
}

/// A request may quote a callable the HW1F model values by an OAS whatever its rule: with
/// none, or under a `price` rule that values it on its workouts, it is valued on its trees
/// at that OAS, alike. Under an `oas` rule the OAS is the rule's level, and the snapshot is
/// the one a level makes. A callable the model does not value is refused.
#[tokio::test]
async fn a_request_values_a_callable_at_an_oas_whatever_its_rule() {
    use convex_engine::workout_valuation::WORKOUT_SNAPSHOT_MODEL;
    let at_oas = |setup: Setup, mark: fn(Decimal) -> QuotedInput| async move {
        let engine = request_runtime(&setup).await;
        let stream = engine.runtime_results()[0].result.clone();
        let valued = configuration::value_quoted_at(
            &engine,
            &stream,
            setup.plan.settlement_date,
            Some(mark(Decimal::from(50))),
        )
        .await;
        engine.shutdown().await.unwrap();
        (stream, valued)
    };
    let (_, unruled) = at_oas(
        callable_setup(CallStyle::Bermudan, true).await,
        QuotedInput::Oas,
    )
    .await;
    let unruled = unruled.unwrap().result;
    let unruled = unruled.envelope();
    assert_eq!(
        unruled.status(),
        ResultStatus::Complete,
        "{:?}",
        unruled.errors()
    );
    assert_eq!(
        unruled.provenance().model_version.as_str(),
        OPTIONAL_SNAPSHOT_MODEL
    );
    assert_eq!(measure(unruled, "oas_mid"), 50.0);

    let priced = callable_setup(CallStyle::Bermudan, true).await;
    commit_rule(&priced, serde_json::json!({"kind": "price"})).await;
    let (stream, on_trees) = at_oas(priced, QuotedInput::Oas).await;
    assert_eq!(
        stream.provenance().model_version.as_str(),
        WORKOUT_SNAPSHOT_MODEL
    );
    let on_trees = on_trees.unwrap().result;
    assert_eq!(
        on_trees.envelope().provenance().model_version.as_str(),
        OPTIONAL_SNAPSHOT_MODEL
    );
    assert_eq!(on_trees.envelope().results(), unruled.results());

    let (_, by_rule) = at_oas(oas_setup().await, QuotedInput::Level).await;
    let (_, by_request) = at_oas(oas_setup().await, QuotedInput::Oas).await;
    let (by_rule, by_request) = (by_rule.unwrap().result, by_request.unwrap().result);
    assert_eq!(
        by_request.envelope().provenance().snapshot_id,
        by_rule.envelope().provenance().snapshot_id
    );
    assert_eq!(by_request.envelope().results(), unruled.results());

    let (_, refused) = at_oas(
        callable_setup(CallStyle::American, true).await,
        QuotedInput::Oas,
    )
    .await;
    let refused = refused.err().unwrap();
    assert!(
        refused
            .to_string()
            .contains("an OAS quotes only a callable"),
        "{refused}"
    );
}

/// A runtime reads each quote by its bond's rule. A callable with no rule whose quote holds
/// only a spread is not valued from it as an OAS, and a plan naming a quoting method of its
/// own is refused, as every runtime validates its plan before it starts.
#[tokio::test]
async fn a_runtime_never_reads_a_spread_as_an_oas_without_an_oas_rule() {
    use convex_ports::{level_pricing::PricingMethod, quote_event::QuoteField};
    let mut setup = callable_setup(CallStyle::Bermudan, true).await;
    let plan = &mut setup.plan;
    products::quote_in(
        &mut plan.assembly.quotes[0],
        &mut plan.seed_events[0],
        QuoteField::MidSpread,
        Decimal::from(50),
    );
    let published = captured_results(&setup).await;
    let result = &published[0].result;
    assert_eq!(result.status(), ResultStatus::Failed);
    assert!(
        result
            .errors()
            .iter()
            .all(|error| error.code() == ErrorCode::InvalidInput),
        "{:?}",
        result.errors()
    );
    setup.plan.assembly.quotes[0].quoted_by = Some(PricingMethod::Oas {});
    let refused = setup.plan.validate().err().unwrap();
    assert!(refused.to_string().contains("quoting method"), "{refused}");
}

/// A supplied snapshot's quoting method is checked where it is valued: an OAS for a callable
/// the HW1F model values is read from its quote, and one for any other callable is refused.
#[tokio::test]
async fn a_supplied_snapshot_quoted_by_oas_is_valued_only_for_a_callable_the_model_values() {
    use convex_engine::context::{ValuationContext, ValuationContextSpec};
    use convex_ports::{level_pricing::PricingMethod, quote_event::QuoteField, snapshot::*};
    let mut setup = callable_setup(CallStyle::Bermudan, true).await;
    let plan = &mut setup.plan;
    products::quote_in(
        &mut plan.assembly.quotes[0],
        &mut plan.seed_events[0],
        QuoteField::MidSpread,
        Decimal::from(50),
    );
    let published = captured_results(&setup).await;
    let provenance = published[0].result.provenance();
    let stored = setup
        .snapshots
        .get(&provenance.snapshot_id)
        .await
        .unwrap()
        .unwrap();
    let context = setup
        .plan
        .context(&stored, &InstrumentId::new("BOND-001").unwrap())
        .unwrap();
    let value = |content: SnapshotContent| {
        let snapshot = MarketSnapshot::new(content).unwrap();
        let context = ValuationContext::new(ValuationContextSpec {
            snapshot_id: snapshot.id().clone(),
            ..context.spec().clone()
        })
        .unwrap();
        SnapshotValuator::new(context.spec().build_id.clone(), 1)
            .unwrap()
            .value(
                &snapshot,
                &context,
                RequestId::new("supplied").unwrap(),
                provenance.calculated_at,
            )
    };
    let mut content = stored.content().clone();
    content.quotes[0].make_mut().requirement.quoted_by = Some(PricingMethod::Oas {});
    let by_oas = value(content.clone()).unwrap();
    let by_oas = by_oas.envelope();
    assert_eq!(
        by_oas.status(),
        ResultStatus::Complete,
        "{:?}",
        by_oas.errors()
    );
    assert_eq!(measure(by_oas, "oas_mid"), 50.0);
    let bond = content.references.make_mut()[0]
        .record
        .bond
        .as_mut()
        .unwrap();
    let terms = bond.contractual_terms.as_mut().unwrap();
    terms.call_terms.as_mut().unwrap().style = CallStyle::American;
    let refused = value(content).err().unwrap();
    assert!(refused.to_string().contains("quoting method"), "{refused}");
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

/// [`callable_setup`] for nine Bermudans under the HW1F assumption: BOND-001 and eight
/// copies, `BOND-W001` to `BOND-W008`, with `rules` committed. Only BOND-001, BOND-W007
/// and BOND-W008 are quoted.
async fn nine_callables(rules: Vec<convex_ports::reference_master::MasterChange>) -> Setup {
    let mut setup = callable_setup(CallStyle::Bermudan, true).await;
    let mut plan = setup.plan.clone();
    widen(&setup, &mut plan, 9).await;
    let mut copies = Vec::new();
    for pin in &mut plan.assembly.instruments[1..] {
        let id = pin.instrument_id.clone().into();
        let stored = setup.storage.bonds.get(&id).await.unwrap().unwrap();
        copies.push(ReferenceChange::Update {
            bond: Box::new(products::callable(
                stored.bond.unwrap(),
                CallStyle::Bermudan,
            )),
            expected_version: stored.version,
        });
        pin.version = fixture::version(2);
    }
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("nine-callables").unwrap(),
                changes: copies,
                master_changes: rules,
            },
            fixture::time("2025-06-01T00:00:00Z"),
        )
        .await
        .unwrap();
    let mut quoted = plan.seed_events[0].clone();
    quoted.key.instrument_id = convex_core::ids::InstrumentId::new("BOND-W007");
    plan.seed_events.push(quoted);
    setup.plan = plan;
    setup
}

/// A plan that holds more callables on HW1F trees than the bound, as a rule or revision
/// committed after its configuration was checked can make it, values the first eight by
/// instrument id. The eighth is valued, and the ninth fails each measure, saying why,
/// though it has no quote, and so does the snapshot valuation of it alone. The runtime
/// runs.
#[tokio::test]
async fn a_ninth_callable_on_hw1f_trees_fails_each_measure_and_the_eighth_is_valued() {
    let setup = nine_callables(vec![]).await;
    let published = captured_results(&setup).await;
    assert_eq!(published.len(), 9);
    let beyond = |error: &StructuredError| error.message() == BEYOND_CALLABLE_BOUND;
    let refused = |errors: &[StructuredError]| {
        errors.len() == OPTIONAL_MEASURES.len()
            && errors
                .iter()
                .all(|error| beyond(error) && error.code() == ErrorCode::SolverLimit)
    };
    for record in &published {
        let result = &record.result;
        let ValuationSubject::InstrumentVersion { instrument_id, .. } =
            &result.provenance().subject
        else {
            panic!("instrument valuation")
        };
        match instrument_id.as_str() {
            "BOND-001" | "BOND-W007" => assert_eq!(
                result.status(),
                ResultStatus::Complete,
                "{:?}",
                result.errors()
            ),
            "BOND-W008" => {
                assert_eq!(result.status(), ResultStatus::Failed);
                assert_ne!(result.quality(), DataQuality::Valid);
                assert!(refused(result.errors()), "{:?}", result.errors());
                let snapshot = setup
                    .snapshots
                    .get(&result.provenance().snapshot_id)
                    .await
                    .unwrap()
                    .unwrap();
                let context = setup.plan.context(&snapshot, instrument_id).unwrap();
                let alone = SnapshotValuator::new(context.spec().build_id.clone(), 1)
                    .unwrap()
                    .value(
                        &snapshot,
                        &context,
                        RequestId::new("ninth-alone").unwrap(),
                        result.provenance().calculated_at,
                    )
                    .unwrap();
                assert!(refused(alone.envelope().errors()));
            }
            // Unquoted, so failed, but not for the bound.
            _ => assert!(
                !result.errors().is_empty() && !result.errors().iter().any(beyond),
                "{instrument_id:?}: {:?}",
                result.errors()
            ),
        }
    }
}

/// A Live runtime restarted from the checkpoint an activation saves, pending at a cutoff
/// after BOND-W008's price rule ended, so with nine callables on HW1F trees, starts. A
/// quote on BOND-W008 then publishes its refusal through a Live tick.
#[tokio::test]
async fn a_live_restart_after_a_ninth_callable_moved_onto_hw1f_trees_runs() {
    let rule = products::pricing_rule("BOND-W008", serde_json::json!({"kind": "price"}));
    let setup = nine_callables(vec![rule.clone()]).await;
    let now = UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap();
    let mut plan = setup.plan.clone();
    plan.mode = convex_engine::RuntimeMode::Live;
    plan.seed_events.clear();
    plan.assembly.as_of = now;
    plan.assembly.known_at = now;
    for quote in &mut plan.assembly.quotes {
        quote.max_age_ms = 600_000;
    }
    for recipe in &mut plan.assembly.curve_recipes {
        recipe.source_time = now;
        recipe.recorded_at = now;
        recipe.max_age_ms = 600_000;
    }
    let live_engine = |source| {
        make_engine(
            &setup,
            display_only(),
            plan.clone(),
            setup.store.clone(),
            RuntimeInput::Live(source),
        )
    };
    let engine = live_engine(std::sync::Arc::new(
        subscription_source::SubscriptionSource::new(),
    ));
    engine.start().await.unwrap();
    engine.shutdown().await.unwrap();

    let ended = UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap();
    let mut end = rule;
    end.expected_version = Some(fixture::version(1));
    let convex_ports::reference_master::MasterDefinition::PricingRule(definition) =
        &mut end.definition
    else {
        panic!("a pricing rule")
    };
    definition.effective.until = Some(ended);
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("end-price-rule").unwrap(),
                changes: vec![],
                master_changes: vec![end],
            },
            ended,
        )
        .await
        .unwrap();
    use convex_ports::runtime::RuntimeStore;
    let mut saved = setup.store.load(&plan.runtime_id).await.unwrap().unwrap();
    let expected = saved.revision;
    let after = UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap();
    saved.revision += 1;
    saved.as_of = after;
    saved.known_at = after;
    saved.pending = true;
    setup.store.checkpoint(expected, &saved).await.unwrap();

    let source = std::sync::Arc::new(subscription_source::SubscriptionSource::new());
    let restarted = live_engine(source.clone());
    restarted.start().await.unwrap();
    let mut updates = restarted.subscribe_results();
    let stamp = subscription_source::monotone_stamp();
    let mut quote = fixture::event(1, stamp, stamp, "99.25");
    quote.key.instrument_id = convex_core::ids::InstrumentId::new("BOND-W008");
    source.send(quote);
    let refused = tokio::time::timeout(std::time::Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    restarted.shutdown().await.unwrap();
    assert_eq!(
        refused.result.provenance().model_version.as_str(),
        OPTIONAL_SNAPSHOT_MODEL
    );
    assert_eq!(refused.result.errors().len(), OPTIONAL_MEASURES.len());
    assert!(
        refused
            .result
            .errors()
            .iter()
            .all(|error| error.message() == BEYOND_CALLABLE_BOUND),
        "{:?}",
        refused.result.errors()
    );
}
