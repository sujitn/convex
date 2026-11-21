#![allow(dead_code)]
#[path = "runtime_setup.rs"]
pub mod runtime;
use convex_core::contracts::*;
use convex_engine::{
    configuration::*, PricingEngine, PricingEngineBuilder, RuntimeInput, RuntimeServices,
};
use convex_ext_file::*;
pub use runtime::configuration_fixture as fixture;
pub use runtime::{time, Setup};
use std::sync::Arc;

pub fn cases(setup: &Setup) -> Arc<ConfigurationUseCases> {
    let document = fixture::document(1);
    Arc::new(ConfigurationUseCases::new(
        setup.storage.configuration.clone(),
        setup.storage.bonds.clone(),
        setup.storage.configs.clone(),
        setup.storage.curves.clone(),
        Arc::new(JsonFixingRepository::empty()),
        setup.snapshots.clone(),
        ConfigurationCapabilities {
            build_id: document.runtime.build_id,
            storage: document.process.storage,
            durable: true,
            atomic_activation: true,
            shared_writers: false,
            result_transport: document.process.result_transport,
            providers: document.providers,
            demo: false,
        },
    ))
}
pub fn fixtures(setup: &Setup) -> Vec<ConfigurationFixture> {
    vec![ConfigurationFixture {
        id: SourceId::new("bond-mid-regression").unwrap(),
        as_of: time(0),
        known_at: time(0),
        events: setup.plan.seed_events.clone(),
        expected: vec![ExpectedConfigurationMeasure {
            instrument_id: InstrumentId::new("BOND-001").unwrap(),
            measure: MeasureId::new("clean_price_mid").unwrap(),
            value: FiniteF64::new(99.25).unwrap(),
            tolerance: FiniteF64::new(0.0).unwrap(),
        }],
    }]
}
pub fn engine(setup: &Setup, cases: Arc<ConfigurationUseCases>) -> Arc<PricingEngine> {
    Arc::new(
        PricingEngineBuilder::new()
            .with_storage(setup.storage.clone())
            .with_configuration_use_cases(cases)
            .with_runtime(RuntimeServices {
                plan: setup.plan.clone(),
                snapshots: setup.snapshots.clone(),
                fixings: Arc::new(JsonFixingRepository::empty()),
                store: setup.store.clone(),
                publisher: runtime::archive(setup),
                input: RuntimeInput::Captured,
            })
            .build()
            .unwrap(),
    )
}
/// A started captured runtime over `setup`'s plan, with `recipes` stored and
/// configuration version 2 activated, so it also values requests.
pub async fn request_runtime(
    setup: &Setup,
    recipes: Vec<convex_ports::snapshot::SnapshotCurveRecipe>,
) -> Arc<PricingEngine> {
    for recipe in &recipes {
        setup.storage.curves.save_recipe(recipe).await.unwrap();
    }
    let config = setup
        .storage
        .configs
        .get_version("CONFIG-RESTORE", 2)
        .await
        .unwrap()
        .unwrap();
    let engine = runtime::make_engine(
        setup,
        runtime::archive(setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    // A runtime without configuration use cases starts only with nothing activated.
    activate_pricing(setup, &config).await;
    engine
}
/// A published measure's value as a float.
pub fn measure(result: &ResultEnvelope, name: &str) -> f64 {
    match &result.results()[&MeasureId::new(name).unwrap()].value {
        MeasureValue::Price(v) => v.value().decimal().try_into().unwrap(),
        MeasureValue::ModelPrice(v) => v.value().get(),
        MeasureValue::ModelBps(v) => v.value().get(),
        MeasureValue::ModelRate(v) => v.value().get(),
        MeasureValue::ModelYears(v) => v.value().get(),
        MeasureValue::ModelYearsSquared(v) => v.value().get(),
        other => panic!("{name}: {other:?}"),
    }
}
/// Stage `config` in a fixture document and activate it, as an operator approves a
/// configuration, so request-time pricing resolves it.
pub async fn activate_pricing(setup: &Setup, config: &convex_ports::storage::BondPricingConfig) {
    let mut staged = fixture::staged(config.version);
    staged.document.pricing = config.clone();
    staged.document.assembly.configuration.version = Version::new(config.version).unwrap();
    setup.storage.configuration.stage(&staged).await.unwrap();
    setup
        .storage
        .configuration
        .activate(&fixture::activate(config.version, 0, "request-time"))
        .await
        .unwrap();
}
/// Value the instrument of a stream result at request time, at its cutoff and
/// `settlement`, with `mark` as a clean price, under the runtime's configuration.
pub async fn value_at(
    engine: &PricingEngine,
    stream: &ResultEnvelope,
    settlement: BusinessDate,
    mark: Option<&str>,
) -> Result<
    convex_engine::request_valuation::RequestValuation,
    convex_engine::request_valuation::RequestValuationError,
> {
    let mark =
        mark.map(|mid| convex_engine::level_pricing::QuotedInput::Price(mid.parse().unwrap()));
    value_quoted_at(engine, stream, settlement, mark).await
}
/// [`value_at`] with `mark` as quoted: a clean price or a level.
pub async fn value_quoted_at(
    engine: &PricingEngine,
    stream: &ResultEnvelope,
    settlement: BusinessDate,
    mark: Option<convex_engine::level_pricing::QuotedInput>,
) -> Result<
    convex_engine::request_valuation::RequestValuation,
    convex_engine::request_valuation::RequestValuationError,
> {
    let known_at = stream.provenance().known_at;
    let ValuationSubject::InstrumentVersion { instrument_id, .. } = &stream.provenance().subject
    else {
        panic!("instrument valuation")
    };
    let mut context = engine.request_context(None, known_at).await.unwrap();
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
    engine
        .value_request(&mut context, &reference, settlement, known_at, mark)
        .await
}
