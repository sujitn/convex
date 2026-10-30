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
/// Stage `config` in a fixture document and activate it, as an operator approves a
/// configuration, so request-time pricing resolves it (P9a).
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
/// Value the instrument of a stream result at request time (D57), at its cutoff and
/// `settlement`, with `mark`, under the runtime's configuration.
pub async fn value_at(
    engine: &PricingEngine,
    stream: &ResultEnvelope,
    settlement: BusinessDate,
    mark: Option<&str>,
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
    let mark = mark.map(|mid| mid.parse().unwrap());
    engine
        .value_request(&mut context, &reference, settlement, known_at, mark)
        .await
}
