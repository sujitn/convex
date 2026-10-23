//! Callables through the runtime: stored terms with explicit call mechanics, a stored curve
//! recipe and the configuration's HW1F assumption, valued by `convex::library`.
#[path = "support/configuration_setup.rs"]
mod configuration;
#[path = "support/curve_replay.rs"]
mod recipes;
use configuration::runtime::*;
use convex_core::contracts::*;
use convex_engine::{optional_valuation::*, snapshot::RUNTIME_MEASURES, RuntimeInput};
use convex_ports::{
    reference_data::{BondReferenceData, BondType, CallScheduleEntry},
    reference_master::{CallStyle, CallTerms},
    reference_repository::{ReferenceChange, ReferenceCommand},
    snapshot::SnapshotRepository,
    storage::OptionModelAssumption,
};
use rust_decimal::Decimal;
use std::str::FromStr;

fn callable(style: CallStyle) -> BondReferenceData {
    let mut bond = fixture::bond();
    bond.bond_type = BondType::FixedCallable;
    bond.is_callable = true;
    bond.call_schedule = [
        ("2027-01-15", "101"),
        ("2028-01-15", "100.5"),
        ("2029-01-15", "100"),
    ]
    .iter()
    .map(|(date, price)| CallScheduleEntry {
        call_date: convex_core::Date::parse(date).unwrap(),
        call_price: Decimal::from_str(price).unwrap(),
        is_make_whole: false,
    })
    .collect();
    bond.contractual_terms.as_mut().unwrap().call_terms = Some(CallTerms {
        style,
        notice_days: 30,
    });
    bond
}

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
                    bond: Box::new(callable(style)),
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
    // D29: the callable answers only the plan measures its model publishes.
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
    let model = |name: &str| match value(name) {
        MeasureValue::ModelPrice(v) => v.value().get(),
        MeasureValue::ModelBps(v) => v.value().get(),
        MeasureValue::ModelYears(v) => v.value().get(),
        MeasureValue::ModelYearsSquared(v) => v.value().get(),
        other => panic!("{name}: {other:?}"),
    };
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
        .push(MeasureId::new("g_spread_mid").unwrap());
    assert!(unknown.validate().is_err());
}
