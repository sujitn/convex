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
pub mod config_resolver;
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
    CalculationGraph, NodeId, NodeValue, ShardAssignment, ShardConfig, ShardStrategy,
};
pub use config_resolver::ConfigResolver;
pub use curve_builder::{BuiltCurve, CurveBuilder};
pub use error::EngineError;
pub use etf_pricing::EtfPricer;
pub use market_data_listener::{
    CurveInputUpdate, CurveUpdate, FxRateUpdate, IndexFixingUpdate, InflationFixingUpdate,
    MarketDataListener, MarketDataPublisher, MarketDataUpdate, QuoteUpdate, VolSurfaceUpdate,
};
pub use portfolio_analytics::{Portfolio, PortfolioAnalyzer, Position};
pub use pricing_router::{BatchPricingResult, PricingInput, PricingRouter};
pub use reactive::{ReactiveEngine, ReactiveEngineBuilder};
pub use scheduler::{EodScheduler, IntervalScheduler, NodeUpdate, ThrottleManager, UpdateSource};

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::interval;
use tracing::{debug, info, warn};

use convex_core::Date;
use convex_traits::config::EngineConfig;
use convex_traits::market_data::{CompositeQuote, MarketDataProvider};
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

    /// Config resolver
    config_resolver: Arc<ConfigResolver>,

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
        let curve_builder = Arc::new(CurveBuilder::new(market_data.clone(), calc_graph.clone()));
        let pricing_router = Arc::new(PricingRouter::new());
        let config_resolver = Arc::new(ConfigResolver::new(storage.configs.clone()));
        let etf_pricer = Arc::new(EtfPricer::new());
        let portfolio_analyzer = Arc::new(PortfolioAnalyzer::new());

        Self {
            config,
            calc_graph,
            curve_builder,
            pricing_router,
            config_resolver,
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

        // Initialize config resolver
        if let Err(e) = self.config_resolver.init().await {
            warn!("Failed to initialize config resolver: {}", e);
        }

        // Start the calculation loop
        self.start_calculation_loop().await;

        info!("Pricing engine started");
        Ok(())
    }

    /// Reprice a bond reactively.
    ///
    /// This method is called when inputs for a bond change.
    /// It:
    /// 1. Fetches bond reference data
    /// 2. Resolves valuation context (config)
    /// 3. Gathers market data inputs (quotes, curves)
    /// 4. Prices the bond
    /// 5. Publishes the result
    pub async fn reprice_bond(&self, instrument_id: &convex_traits::ids::InstrumentId) -> Result<(), EngineError> {
        debug!("Repricing bond: {}", instrument_id);

        // 1. Fetch bond reference data
        let bond = match self.reference_data.bonds.get_by_id(instrument_id).await {
            Ok(Some(b)) => b,
            Ok(None) => {
                warn!("Bond not found for repricing: {}", instrument_id);
                return Ok(());
            }
            Err(e) => {
                warn!("Failed to fetch bond {}: {}", instrument_id, e);
                return Ok(());
            }
        };

        // 2. Resolve valuation context
        let config = self.config_resolver.resolve(&bond).await;

        // 3. Gather inputs
        // Get market price (quote) from CalcGraph (which acts as cache)
        let quote_node = crate::calc_graph::NodeId::Quote { instrument_id: instrument_id.clone() };
        let market_quote = if let Some(val) = self.calc_graph.get_cached(&quote_node) {
            match val.value {
                crate::calc_graph::NodeValue::Quote(quote) => Some(quote),
                _ => None,
            }
        } else {
            None
        };

        // Determine curves to use based on config
        let discount_curve_id = config.as_ref().map(|c| c.analytics_curves.discount_curve.clone());
        let discount_curve = if let Some(curve_id) = discount_curve_id {
             self.curve_builder.get(&curve_id)
        } else {
            None
        };

        // 4. Construct pricing input
        let settlement_date = Date::today();

        // Use composite quote fields
        let (bid, mid, ask) = if let Some(q) = &market_quote {
            // Calculate mid if missing
            let mid = q.mid_price.or_else(|| {
                match (q.bid_price, q.ask_price) {
                    (Some(b), Some(a)) => Some((b + a) / rust_decimal::Decimal::from(2)),
                    (Some(b), None) => Some(b),
                    (None, Some(a)) => Some(a),
                    _ => None,
                }
            });
            (q.bid_price, mid, q.ask_price)
        } else {
            (None, None, None)
        };

        let input = PricingInput {
            bond,
            settlement_date,
            market_price_bid: bid,
            market_price_mid: mid,
            market_price_ask: ask,
            discount_curve,
            benchmark_curve: None, // TODO: resolve from config
            government_curve: None, // TODO: resolve from config
            volatility: None, // TODO: resolve from config
            bid_ask_config: config.and_then(|c| c.bid_ask_spread),
            composite_quote: market_quote, // Pass full composite quote
        };

        // 5. Price
        let quote = self.pricing_router.price(&input)?;

        // 6. Publish
        if let Err(e) = self.output.quotes.publish(&quote).await {
            warn!("Failed to publish quote for {}: {}", instrument_id, e);
        }

        Ok(())
    }

    /// Start the main calculation loop.
    async fn start_calculation_loop(&self) {
        let calc_graph = self.calc_graph.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        // Get calculation interval from config (default 100ms)
        let calc_interval = Duration::from_millis(100);

        let engine_for_worker = self.clone_for_worker();

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
                                        // Call reprice_bond
                                        if let Err(e) = engine_for_worker.reprice_bond(instrument_id).await {
                                            warn!("Error repricing bond {}: {}", instrument_id, e);
                                        }
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

    /// Internal helper to clone necessary components for the worker task.
    /// This effectively creates a lightweight copy of the engine sharing the same state.
    fn clone_for_worker(&self) -> Self {
        Self {
            config: self.config.clone(),
            calc_graph: self.calc_graph.clone(),
            curve_builder: self.curve_builder.clone(),
            pricing_router: self.pricing_router.clone(),
            config_resolver: self.config_resolver.clone(),
            etf_pricer: self.etf_pricer.clone(),
            portfolio_analyzer: self.portfolio_analyzer.clone(),
            market_data: self.market_data.clone(),
            reference_data: self.reference_data.clone(),
            storage: self.storage.clone(),
            output: self.output.clone(),
            shutdown_tx: self.shutdown_tx.clone(),
        }
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

    /// Get the config resolver.
    pub fn config_resolver(&self) -> &Arc<ConfigResolver> {
        &self.config_resolver
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

    /// Get the storage adapter.
    pub fn storage(&self) -> &Arc<StorageAdapter> {
        &self.storage
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
        quote: CompositeQuote,
    ) {
        let node_id = NodeId::Quote {
            instrument_id: instrument_id.clone(),
        };

        // Fill in mid price if missing but bid/ask present
        let mut quote = quote;
        if quote.mid_price.is_none() {
             match (quote.bid_price, quote.ask_price) {
                (Some(b), Some(a)) => quote.mid_price = Some((b + a) / rust_decimal::Decimal::from(2)),
                (Some(b), None) => quote.mid_price = Some(b),
                (None, Some(a)) => quote.mid_price = Some(a),
                _ => {},
            }
        }

        self.calc_graph
            .update_cache(&node_id, NodeValue::Quote(quote.clone()));

        // Invalidate quote node (propagates to bond price)
        self.calc_graph.invalidate(&node_id);

        debug!(
            "Quote update for {}: mid={:?}",
            instrument_id, quote.mid_price
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
        self.calc_graph
            .update_cache(&node_id, NodeValue::IndexFixing { rate });

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
        self.calc_graph
            .update_cache(&node_id, NodeValue::InflationFixing { value });

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
        self.calc_graph
            .update_cache(&node_id, NodeValue::FxRate { mid });

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
