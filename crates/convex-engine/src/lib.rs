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
pub mod market_data_listener;
pub mod portfolio_analytics;
pub mod pricing_router;
pub mod reactive;
pub mod scheduler;

mod cache;
mod context;

// Re-exports
pub use builder::PricingEngineBuilder;
pub use calc_graph::{
    CalculationGraph, NodeId, NodeValue,
    ShardConfig, ShardStrategy, ShardAssignment,
};
pub use curve_builder::{BuiltCurve, CurveBuilder};
pub use error::EngineError;
pub use etf_pricing::EtfPricer;
pub use market_data_listener::{
    MarketDataListener, MarketDataPublisher, MarketDataUpdate,
    QuoteUpdate, CurveUpdate, CurveInputUpdate, IndexFixingUpdate,
    InflationFixingUpdate, FxRateUpdate, VolSurfaceUpdate,
};
pub use portfolio_analytics::{Portfolio, PortfolioAnalyzer, Position};
pub use pricing_router::{BatchPricingResult, PricingRouter};
pub use reactive::{ReactiveEngine, ReactiveEngineBuilder};
pub use scheduler::{
    IntervalScheduler, EodScheduler, ThrottleManager,
    NodeUpdate, UpdateSource,
};

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

    // =========================================================================
    // REACTIVE MARKET DATA HANDLERS
    // =========================================================================

    /// Handle a quote update for a bond.
    ///
    /// This method is called when a new quote arrives for a bond.
    /// It invalidates the quote node in the calc graph, which propagates
    /// to dependent bond price nodes.
    pub fn on_quote_update(
        &self,
        instrument_id: &convex_traits::ids::InstrumentId,
        bid: Option<rust_decimal::Decimal>,
        ask: Option<rust_decimal::Decimal>,
    ) {
        let node_id = NodeId::Quote {
            instrument_id: instrument_id.clone(),
        };

        // Update quote in cache
        let mid = match (bid, ask) {
            (Some(b), Some(a)) => Some((b + a) / rust_decimal::Decimal::from(2)),
            (Some(b), None) => Some(b),
            (None, Some(a)) => Some(a),
            (None, None) => None,
        };

        self.calc_graph.update_cache(
            &node_id,
            NodeValue::Quote { bid, ask, mid },
        );

        // Invalidate quote node (propagates to bond price)
        self.calc_graph.invalidate(&node_id);

        debug!(
            "Quote update for {}: bid={:?} ask={:?}",
            instrument_id, bid, ask
        );
    }

    /// Handle a curve update.
    ///
    /// This method is called when a curve is rebuilt.
    /// It invalidates the curve node, which propagates to all bonds
    /// that depend on this curve.
    pub fn on_curve_update(&self, curve_id: &convex_traits::ids::CurveId, curve: &BuiltCurve) {
        let node_id = NodeId::Curve {
            curve_id: curve_id.clone(),
        };

        // Update curve in cache
        self.calc_graph.update_cache(
            &node_id,
            NodeValue::Curve {
                points: curve.to_points(),
            },
        );

        // Invalidate curve node (propagates to dependent bonds)
        self.calc_graph.invalidate(&node_id);

        debug!("Curve update for {}", curve_id);
    }

    /// Handle an index fixing update (for FRNs).
    ///
    /// This method is called when a floating rate index fixing arrives.
    /// It invalidates the index fixing node, which propagates to all FRNs
    /// that reference this index.
    pub fn on_index_fixing(
        &self,
        index: &convex_traits::ids::FloatingRateIndex,
        date: convex_core::Date,
        rate: rust_decimal::Decimal,
    ) {
        let node_id = NodeId::IndexFixing {
            index: index.clone(),
            date,
        };

        // Update fixing in cache
        self.calc_graph.update_cache(
            &node_id,
            NodeValue::IndexFixing { rate },
        );

        // Invalidate fixing node (propagates to FRNs)
        self.calc_graph.invalidate(&node_id);

        debug!("Index fixing for {} on {}: {}", index, date, rate);
    }

    /// Handle an inflation fixing update (for ILBs).
    ///
    /// This method is called when an inflation index fixing arrives.
    /// It invalidates the inflation fixing node, which propagates to all ILBs.
    pub fn on_inflation_fixing(
        &self,
        index: &convex_traits::ids::InflationIndex,
        month: convex_traits::ids::YearMonth,
        value: rust_decimal::Decimal,
    ) {
        let node_id = NodeId::InflationFixing {
            index: index.clone(),
            month,
        };

        // Update fixing in cache
        self.calc_graph.update_cache(
            &node_id,
            NodeValue::InflationFixing { value },
        );

        // Invalidate fixing node (propagates to ILBs)
        self.calc_graph.invalidate(&node_id);

        debug!("Inflation fixing for {} ({}): {}", index, month, value);
    }

    /// Handle an FX rate update.
    ///
    /// This method is called when an FX rate is updated.
    /// It invalidates the FX rate node, which propagates to cross-currency
    /// portfolio calculations.
    pub fn on_fx_rate_update(
        &self,
        pair: &convex_traits::ids::CurrencyPair,
        mid: rust_decimal::Decimal,
    ) {
        let node_id = NodeId::FxRate { pair: pair.clone() };

        // Update FX rate in cache
        self.calc_graph.update_cache(
            &node_id,
            NodeValue::FxRate { mid },
        );

        // Invalidate FX rate node
        self.calc_graph.invalidate(&node_id);

        debug!("FX rate update for {}: {}", pair, mid);
    }

    /// Register a bond for reactive pricing.
    ///
    /// This sets up the node in the calc graph with dependencies on
    /// the relevant curves and quote.
    pub fn register_bond(
        &self,
        instrument_id: convex_traits::ids::InstrumentId,
        discount_curve: convex_traits::ids::CurveId,
        benchmark_curve: Option<convex_traits::ids::CurveId>,
        config: convex_traits::config::NodeConfig,
    ) {
        let bond_node = NodeId::BondPrice {
            instrument_id: instrument_id.clone(),
        };

        // Build dependencies
        let mut deps = vec![
            NodeId::Curve {
                curve_id: discount_curve,
            },
            NodeId::Quote {
                instrument_id: instrument_id.clone(),
            },
        ];

        if let Some(bench_curve) = benchmark_curve {
            deps.push(NodeId::Curve {
                curve_id: bench_curve,
            });
        }

        // Add node with dependencies
        self.calc_graph.add_node(bond_node.clone(), deps);

        // Set config
        self.calc_graph.set_node_config(bond_node, config);
    }

    /// Register an ETF for iNAV calculations.
    pub fn register_etf_inav(&self, etf_id: convex_traits::ids::EtfId) {
        let node_id = NodeId::EtfInav {
            etf_id: etf_id.clone(),
        };

        // ETF iNAV depends on its constituent bond prices
        // In practice, we'd look up the holdings and add dependencies
        self.calc_graph.add_node(node_id.clone(), vec![]);

        // Use iNAV config (15 second interval)
        self.calc_graph
            .set_node_config(node_id, convex_traits::config::NodeConfig::etf_inav());
    }

    /// Register an ETF for NAV calculations.
    pub fn register_etf_nav(&self, etf_id: convex_traits::ids::EtfId) {
        let node_id = NodeId::EtfNav {
            etf_id: etf_id.clone(),
        };

        self.calc_graph.add_node(node_id.clone(), vec![]);

        // Use NAV config (end of day)
        self.calc_graph
            .set_node_config(node_id, convex_traits::config::NodeConfig::etf_nav());
    }

    /// Register a portfolio for analytics.
    pub fn register_portfolio(&self, portfolio_id: convex_traits::ids::PortfolioId) {
        let node_id = NodeId::Portfolio {
            portfolio_id: portfolio_id.clone(),
        };

        self.calc_graph.add_node(node_id.clone(), vec![]);

        // Use portfolio config (5 second throttle)
        self.calc_graph
            .set_node_config(node_id, convex_traits::config::NodeConfig::portfolio());
    }

    /// Create a reactive engine from this pricing engine.
    ///
    /// The reactive engine provides additional features like interval
    /// scheduling and market data listening.
    pub fn create_reactive_engine(&self) -> reactive::ReactiveEngine {
        reactive::ReactiveEngine::new(
            self.calc_graph.clone(),
            self.curve_builder.clone(),
            self.pricing_router.clone(),
            self.reference_data.clone(),
        )
    }
}
