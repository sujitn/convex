//! # Convex Engine
//!
//! The reactive pricing engine for Convex.
//!
//! This crate provides:
//! - [`CalculationGraph`]: Dependency tracking and reactive recalculation
//! - [`PricingRouter`]: Model selection and batch pricing
//! - [`CurveBuilder`]: Curve construction from market data
//! - [`EtfPricer`]: ETF iNAV calculation from holdings
//! - [`PortfolioAnalyzer`]: Portfolio-level risk aggregation
//! - [`PricingEngine`]: Main engine orchestrating all components
//!
//! ## Architecture
//!
//! ```text
//! Market Data ─┬─> CurveBuilder ─> Curves
//!              │
//!              └─> CalculationGraph ─> PricingRouter ─┬─> BondQuotes
//!                                                     │
//!                                                     ├─> EtfPricer ─> ETF iNAV
//!                                                     │
//!                                                     └─> PortfolioAnalyzer ─> Risk
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! let engine = PricingEngineBuilder::new()
//!     .with_market_data(market_data_provider)
//!     .with_reference_data(ref_data_provider)
//!     .with_storage(storage_adapter)
//!     .with_config(config_source)
//!     .with_output(output_publisher)
//!     .build()?;
//!
//! engine.start().await?;
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod builder;
pub mod calc_graph;
pub mod curve_builder;
pub mod error;
pub mod etf_pricing;
pub mod portfolio_analytics;
pub mod pricing_router;

mod cache;
mod context;

// Re-exports
pub use builder::PricingEngineBuilder;
pub use calc_graph::{CalculationGraph, NodeId, NodeValue};
pub use curve_builder::{BuiltCurve, CurveBuilder};
pub use error::EngineError;
pub use etf_pricing::EtfPricer;
pub use portfolio_analytics::{Portfolio, PortfolioAnalyzer, Position};
pub use pricing_router::{BatchPricingResult, PricingRouter};

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::interval;
use tracing::{debug, info, warn};

use convex_traits::config::EngineConfig;
use convex_traits::market_data::MarketDataProvider;
use convex_traits::output::OutputPublisher;
use convex_traits::reference_data::ReferenceDataProvider;
use convex_traits::storage::StorageAdapter;

/// The main pricing engine.
pub struct PricingEngine {
    /// Engine configuration
    config: EngineConfig,

    /// Calculation graph
    calc_graph: Arc<CalculationGraph>,

    /// Curve builder
    curve_builder: Arc<CurveBuilder>,

    /// Pricing router
    pricing_router: Arc<PricingRouter>,

    /// ETF pricer
    etf_pricer: Arc<EtfPricer>,

    /// Portfolio analyzer
    portfolio_analyzer: Arc<PortfolioAnalyzer>,

    /// Market data provider
    #[allow(dead_code)]
    market_data: Arc<MarketDataProvider>,

    /// Reference data provider
    #[allow(dead_code)]
    reference_data: Arc<ReferenceDataProvider>,

    /// Storage adapter
    #[allow(dead_code)]
    storage: Arc<StorageAdapter>,

    /// Output publisher
    #[allow(dead_code)]
    output: Arc<OutputPublisher>,

    /// Shutdown signal sender
    shutdown_tx: broadcast::Sender<()>,
}

impl PricingEngine {
    /// Create a new pricing engine.
    pub fn new(
        config: EngineConfig,
        market_data: Arc<MarketDataProvider>,
        reference_data: Arc<ReferenceDataProvider>,
        storage: Arc<StorageAdapter>,
        output: Arc<OutputPublisher>,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);

        let calc_graph = Arc::new(CalculationGraph::new());
        let curve_builder = Arc::new(CurveBuilder::new(
            market_data.clone(),
            calc_graph.clone(),
        ));
        let pricing_router = Arc::new(PricingRouter::new());
        let etf_pricer = Arc::new(EtfPricer::new());
        let portfolio_analyzer = Arc::new(PortfolioAnalyzer::new());

        Self {
            config,
            calc_graph,
            curve_builder,
            pricing_router,
            etf_pricer,
            portfolio_analyzer,
            market_data,
            reference_data,
            storage,
            output,
            shutdown_tx,
        }
    }

    /// Start the pricing engine.
    pub async fn start(&self) -> Result<(), EngineError> {
        info!("Starting pricing engine: {}", self.config.name);

        // Start the calculation loop
        self.start_calculation_loop().await;

        info!("Pricing engine started");
        Ok(())
    }

    /// Start the main calculation loop.
    async fn start_calculation_loop(&self) {
        let calc_graph = self.calc_graph.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        // Get calculation interval from config (default 100ms)
        let calc_interval = Duration::from_millis(100);

        tokio::spawn(async move {
            let mut ticker = interval(calc_interval);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        // Get dirty nodes that need recalculation
                        let dirty_nodes = calc_graph.get_nodes_to_calculate();

                        if !dirty_nodes.is_empty() {
                            debug!("Processing {} dirty nodes", dirty_nodes.len());

                            for node_id in dirty_nodes {
                                // Process each node based on type
                                match &node_id {
                                    NodeId::Curve { curve_id } => {
                                        debug!("Rebuilding curve: {}", curve_id);
                                        // Curve building would happen here
                                    }
                                    NodeId::BondPrice { instrument_id } => {
                                        debug!("Repricing bond: {}", instrument_id);
                                        // Bond repricing would happen here
                                    }
                                    NodeId::EtfInav { etf_id } => {
                                        debug!("Recalculating ETF iNAV: {}", etf_id);
                                        // ETF iNAV calculation would happen here
                                    }
                                    NodeId::Portfolio { portfolio_id } => {
                                        debug!("Recalculating portfolio: {}", portfolio_id);
                                        // Portfolio analytics would happen here
                                    }
                                    _ => {
                                        // Other node types
                                    }
                                }

                                // Mark as clean (in real impl, after successful calculation)
                                calc_graph.update_cache(&node_id, NodeValue::Empty);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Calculation loop shutting down");
                        break;
                    }
                }
            }
        });
    }

    /// Process a single calculation cycle manually.
    ///
    /// This is useful for testing or on-demand calculations.
    pub fn process_dirty_nodes(&self) -> usize {
        let dirty_nodes = self.calc_graph.get_nodes_to_calculate();
        let count = dirty_nodes.len();

        for node_id in dirty_nodes {
            // Mark as processed
            self.calc_graph.update_cache(&node_id, NodeValue::Empty);
        }

        count
    }

    /// Shutdown the pricing engine.
    pub async fn shutdown(&self) -> Result<(), EngineError> {
        info!("Shutting down pricing engine");
        let _ = self.shutdown_tx.send(());
        info!("Pricing engine shutdown complete");
        Ok(())
    }

    /// Get the calculation graph.
    pub fn calc_graph(&self) -> &Arc<CalculationGraph> {
        &self.calc_graph
    }

    /// Get the curve builder.
    pub fn curve_builder(&self) -> &Arc<CurveBuilder> {
        &self.curve_builder
    }

    /// Get the pricing router.
    pub fn pricing_router(&self) -> &Arc<PricingRouter> {
        &self.pricing_router
    }

    /// Get the ETF pricer.
    pub fn etf_pricer(&self) -> &Arc<EtfPricer> {
        &self.etf_pricer
    }

    /// Get the portfolio analyzer.
    pub fn portfolio_analyzer(&self) -> &Arc<PortfolioAnalyzer> {
        &self.portfolio_analyzer
    }

    /// Get the engine configuration.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Get the reference data provider.
    pub fn reference_data(&self) -> &Arc<ReferenceDataProvider> {
        &self.reference_data
    }
}
