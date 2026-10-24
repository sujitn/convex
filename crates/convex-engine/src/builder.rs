//! Builder pattern for the pricing engine.

use std::sync::Arc;

use crate::ports::config::EngineConfig;
use crate::ports::storage::StorageAdapter;

use crate::error::EngineError;
use crate::PricingEngine;

/// Builder for constructing a [`PricingEngine`].
pub struct PricingEngineBuilder {
    config: Option<EngineConfig>,
    storage: Option<Arc<StorageAdapter>>,
    runtime: Option<crate::runtime::RuntimeServices>,
    configurations: Option<Arc<crate::configuration::ConfigurationUseCases>>,
    demo: bool,
}

impl PricingEngineBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            config: None,
            storage: None,
            runtime: None,
            configurations: None,
            demo: false,
        }
    }

    /// Set the engine configuration.
    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set the storage adapter.
    pub fn with_storage(mut self, storage: Arc<StorageAdapter>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Configure the sole event-driven pricing lifecycle and genuine I/O boundaries.
    pub fn with_runtime(mut self, services: crate::runtime::RuntimeServices) -> Self {
        self.runtime = Some(services);
        self
    }

    /// Enable audited application activation using capabilities of the actual adapter composition.
    pub fn with_configuration_use_cases(
        mut self,
        configurations: Arc<crate::configuration::ConfigurationUseCases>,
    ) -> Self {
        self.configurations = Some(configurations);
        self
    }

    /// Explicit idle demonstration mode. It performs no production pricing and is never ready.
    pub fn with_demo_mode(mut self) -> Self {
        self.demo = true;
        self
    }

    /// Build the pricing engine.
    pub fn build(self) -> Result<PricingEngine, EngineError> {
        let config = self.config.unwrap_or_default();

        let storage = self
            .storage
            .ok_or_else(|| EngineError::ConfigError("storage not configured".into()))?;

        if self.demo && self.runtime.is_some() {
            return Err(EngineError::ConfigError(
                "demo and configured runtime are mutually exclusive".into(),
            ));
        }
        if let Some(runtime) = &self.runtime {
            runtime.plan.validate()?;
        }
        if !(1..=64).contains(&config.max_concurrent_calcs) {
            return Err(EngineError::ConfigError(
                "max_concurrent_calcs requires 1..=64".into(),
            ));
        }
        let mut engine = PricingEngine::new(config, storage);
        engine.configurations = self.configurations;
        engine.runtime = crate::runtime::RuntimeController::new(
            self.runtime,
            self.demo,
            engine.config.max_concurrent_calcs,
        );
        Ok(engine)
    }
}

impl Default for PricingEngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}
