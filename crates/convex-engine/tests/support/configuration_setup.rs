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
            .with_market_data(runtime::market_data())
            .with_reference_data(Arc::new(create_empty_reference_data()))
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
