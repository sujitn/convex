//! Builder pattern for the pricing engine.

use std::sync::Arc;

use convex_traits::config::EngineConfig;
use convex_traits::market_data::MarketDataProvider;
use convex_traits::output::OutputPublisher;
use convex_traits::reference_data::ReferenceDataProvider;
use convex_traits::storage::StorageAdapter;

use crate::error::EngineError;
use crate::PricingEngine;

/// Builder for constructing a [`PricingEngine`].
pub struct PricingEngineBuilder {
    config: Option<EngineConfig>,
    market_data: Option<Arc<MarketDataProvider>>,
    reference_data: Option<Arc<ReferenceDataProvider>>,
    storage: Option<Arc<StorageAdapter>>,
    output: Option<Arc<OutputPublisher>>,
}

impl PricingEngineBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            config: None,
            market_data: None,
            reference_data: None,
            storage: None,
            output: None,
        }
    }

    /// Set the engine configuration.
    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set the market data provider.
    pub fn with_market_data(mut self, provider: Arc<MarketDataProvider>) -> Self {
        self.market_data = Some(provider);
        self
    }

    /// Set the reference data provider.
    pub fn with_reference_data(mut self, provider: Arc<ReferenceDataProvider>) -> Self {
        self.reference_data = Some(provider);
        self
    }

    /// Set the storage adapter.
    pub fn with_storage(mut self, storage: Arc<StorageAdapter>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Set the output publisher.
    pub fn with_output(mut self, output: Arc<OutputPublisher>) -> Self {
        self.output = Some(output);
        self
    }

    /// Build the pricing engine.
    pub fn build(self) -> Result<PricingEngine, EngineError> {
        let config = self.config.unwrap_or_default();

        let market_data = self
            .market_data
            .ok_or_else(|| EngineError::ConfigError("market_data not configured".into()))?;

        let reference_data = self
            .reference_data
            .ok_or_else(|| EngineError::ConfigError("reference_data not configured".into()))?;

        let storage = self
            .storage
            .ok_or_else(|| EngineError::ConfigError("storage not configured".into()))?;

        let output = self
            .output
            .ok_or_else(|| EngineError::ConfigError("output not configured".into()))?;

        Ok(PricingEngine::new(
            config,
            market_data,
            reference_data,
            storage,
            output,
        ))
    }
}

impl Default for PricingEngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}
