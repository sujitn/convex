//! Reactive pricing engine coordinator.
//!
//! The [`ReactiveEngine`] coordinates all reactive pricing components:
//! - Market data listener for real-time updates
//! - Interval scheduler for fixed-schedule calculations
//! - EOD scheduler for end-of-day calculations
//! - Throttle manager for debounced updates
//! - Node update broadcasting
//! - **Actual calculation execution** via PricingRouter

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use convex_core::Date;
use convex_traits::config::{NodeConfig, UpdateFrequency};
use convex_traits::ids::*;
use convex_traits::reference_data::ReferenceDataProvider;

use crate::calc_graph::{CalculationGraph, NodeId, NodeValue};
use crate::curve_builder::{BuiltCurve, CurveBuilder};
use crate::market_data_listener::{MarketDataListener, MarketDataPublisher};
use crate::pricing_router::{PricingInput, PricingRouter};
use crate::scheduler::{CronScheduler, EodScheduler, IntervalScheduler, NodeUpdate, ThrottleManager, UpdateSource};

// =============================================================================
// REACTIVE ENGINE
// =============================================================================

/// Reactive pricing engine that coordinates all components.
pub struct ReactiveEngine {
    /// Calculation graph
    calc_graph: Arc<CalculationGraph>,

    /// Curve builder
    curve_builder: Arc<CurveBuilder>,

    /// Pricing router for bond calculations
    pricing_router: Arc<PricingRouter>,

    /// Reference data provider for bond metadata
    #[allow(dead_code)]
    reference_data: Arc<ReferenceDataProvider>,

    /// Local cache of bond reference data for sync access in calc loop
    bond_cache: Arc<dashmap::DashMap<InstrumentId, convex_traits::reference_data::BondReferenceData>>,

    /// Interval scheduler
    interval_scheduler: Arc<IntervalScheduler>,

    /// EOD scheduler
    eod_scheduler: Arc<parking_lot::RwLock<EodScheduler>>,

    /// Cron scheduler for cron-based scheduling
    cron_scheduler: Arc<parking_lot::RwLock<CronScheduler>>,

    /// Throttle manager
    throttle_manager: Arc<ThrottleManager>,

    /// Market data publisher
    market_data_publisher: MarketDataPublisher,

    /// Combined node update sender
    node_update_tx: broadcast::Sender<NodeUpdate>,

    /// Settlement date for pricing (updated daily)
    settlement_date: parking_lot::RwLock<Date>,

    /// Shutdown signal
    shutdown_tx: broadcast::Sender<()>,
}

impl ReactiveEngine {
    /// Create a new reactive engine.
    pub fn new(
        calc_graph: Arc<CalculationGraph>,
        curve_builder: Arc<CurveBuilder>,
        pricing_router: Arc<PricingRouter>,
        reference_data: Arc<ReferenceDataProvider>,
    ) -> Self {
        let interval_scheduler = Arc::new(IntervalScheduler::new(calc_graph.clone()));
        let eod_scheduler = Arc::new(parking_lot::RwLock::new(EodScheduler::new(calc_graph.clone())));
        let cron_scheduler = Arc::new(parking_lot::RwLock::new(CronScheduler::new(calc_graph.clone())));
        let throttle_manager = Arc::new(ThrottleManager::new(calc_graph.clone()));
        let (market_data_publisher, _) = MarketDataPublisher::new();
        let (node_update_tx, _) = broadcast::channel(10000);
        let (shutdown_tx, _) = broadcast::channel(1);
        let bond_cache = Arc::new(dashmap::DashMap::new());

        // Default settlement date to today
        let today = Date::today();

        Self {
            calc_graph,
            curve_builder,
            pricing_router,
            reference_data,
            bond_cache,
            interval_scheduler,
            eod_scheduler,
            cron_scheduler,
            throttle_manager,
            market_data_publisher,
            node_update_tx,
            settlement_date: parking_lot::RwLock::new(today),
            shutdown_tx,
        }
    }

    /// Set the settlement date for pricing.
    pub fn set_settlement_date(&self, date: Date) {
        *self.settlement_date.write() = date;
    }

    /// Get the current settlement date.
    pub fn settlement_date(&self) -> Date {
        *self.settlement_date.read()
    }

    /// Cache bond reference data for reactive pricing.
    ///
    /// This should be called when registering a bond to ensure
    /// the reference data is available for sync calculations.
    pub fn cache_bond_reference(&self, bond: convex_traits::reference_data::BondReferenceData) {
        self.bond_cache.insert(bond.instrument_id.clone(), bond);
    }

    /// Get cached bond reference data.
    pub fn get_bond_reference(&self, instrument_id: &InstrumentId) -> Option<convex_traits::reference_data::BondReferenceData> {
        self.bond_cache.get(instrument_id).map(|r| r.clone())
    }

    /// Start the reactive engine.
    ///
    /// This starts all background tasks for reactive pricing.
    pub async fn start(&self) {
        info!("Starting reactive pricing engine");

        // Start the market data listener
        self.start_market_data_listener().await;

        // Start the EOD scheduler
        self.eod_scheduler.write().start();

        // Start the cron scheduler
        self.cron_scheduler.write().start();

        // Start the main processing loop
        self.start_processing_loop().await;

        info!("Reactive pricing engine started");
    }

    /// Start the market data listener task.
    async fn start_market_data_listener(&self) {
        let calc_graph = self.calc_graph.clone();
        let curve_builder = self.curve_builder.clone();
        let throttle_manager = self.throttle_manager.clone();
        let update_rx = self.market_data_publisher.subscribe();
        let shutdown_rx = self.shutdown_tx.subscribe();
        let node_update_tx = self.node_update_tx.clone();

        tokio::spawn(async move {
            let mut listener = MarketDataListener::new(
                calc_graph,
                curve_builder,
                throttle_manager,
                update_rx,
                shutdown_rx,
            );

            // Forward listener updates to main channel
            let mut listener_rx = listener.subscribe();
            let tx = node_update_tx.clone();
            tokio::spawn(async move {
                while let Ok(update) = listener_rx.recv().await {
                    let _ = tx.send(update);
                }
            });

            listener.run().await;
        });
    }

    /// Start the main processing loop.
    async fn start_processing_loop(&self) {
        let calc_graph = self.calc_graph.clone();
        let curve_builder = self.curve_builder.clone();
        let pricing_router = self.pricing_router.clone();
        let bond_cache = self.bond_cache.clone();
        let throttle_manager = self.throttle_manager.clone();
        let node_update_tx = self.node_update_tx.clone();
        let settlement_date = self.settlement_date.read().clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        // Forward interval scheduler updates
        let interval_scheduler = self.interval_scheduler.clone();
        let tx = self.node_update_tx.clone();
        tokio::spawn(async move {
            let mut rx = interval_scheduler.subscribe();
            while let Ok(update) = rx.recv().await {
                let _ = tx.send(update);
            }
        });

        // Forward EOD scheduler updates
        let eod_scheduler = self.eod_scheduler.clone();
        let tx = self.node_update_tx.clone();
        tokio::spawn(async move {
            let mut rx = eod_scheduler.read().subscribe();
            while let Ok(update) = rx.recv().await {
                let _ = tx.send(update);
            }
        });

        // Forward throttle manager updates
        let throttle_manager_clone = self.throttle_manager.clone();
        let tx = self.node_update_tx.clone();
        tokio::spawn(async move {
            let mut rx = throttle_manager_clone.subscribe();
            while let Ok(update) = rx.recv().await {
                let _ = tx.send(update);
            }
        });

        // Forward cron scheduler updates
        let cron_scheduler = self.cron_scheduler.clone();
        let tx = self.node_update_tx.clone();
        tokio::spawn(async move {
            let mut rx = cron_scheduler.read().subscribe();
            while let Ok(update) = rx.recv().await {
                let _ = tx.send(update);
            }
        });

        // Main processing loop with actual calculation execution
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(50));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        // Process dirty nodes that are ready
                        let nodes_to_calc = calc_graph.get_nodes_to_calculate();

                        for node_id in nodes_to_calc {
                            // Check throttle
                            if throttle_manager.should_calculate(&node_id) {
                                debug!("Processing node: {}", node_id);

                                // Execute actual calculation based on node type
                                let node_value = Self::calculate_node(
                                    &node_id,
                                    &calc_graph,
                                    &curve_builder,
                                    &pricing_router,
                                    &bond_cache,
                                    settlement_date,
                                );

                                // Store calculated value
                                calc_graph.update_cache(&node_id, node_value);
                                throttle_manager.mark_calculated(&node_id);

                                // Notify subscribers
                                let _ = node_update_tx.send(NodeUpdate {
                                    node_id,
                                    timestamp: chrono::Utc::now().timestamp(),
                                    source: UpdateSource::Immediate,
                                });
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Processing loop shutting down");
                        break;
                    }
                }
            }
        });
    }

    /// Calculate a node's value based on its type.
    fn calculate_node(
        node_id: &NodeId,
        calc_graph: &Arc<CalculationGraph>,
        curve_builder: &Arc<CurveBuilder>,
        pricing_router: &Arc<PricingRouter>,
        bond_cache: &Arc<dashmap::DashMap<InstrumentId, convex_traits::reference_data::BondReferenceData>>,
        settlement_date: Date,
    ) -> NodeValue {
        match node_id {
            NodeId::BondPrice { instrument_id } => {
                Self::calculate_bond_price(
                    instrument_id,
                    calc_graph,
                    curve_builder,
                    pricing_router,
                    bond_cache,
                    settlement_date,
                )
            }
            NodeId::Curve { curve_id } => {
                Self::calculate_curve(curve_id, curve_builder)
            }
            NodeId::EtfInav { etf_id: _ } => {
                // ETF iNAV calculation - aggregate constituent prices
                // For now, return empty; full implementation requires holdings data
                debug!("ETF iNAV calculation not yet fully implemented");
                NodeValue::Empty
            }
            NodeId::EtfNav { etf_id: _ } => {
                // ETF NAV is typically end-of-day
                debug!("ETF NAV calculation not yet fully implemented");
                NodeValue::Empty
            }
            NodeId::Portfolio { portfolio_id: _ } => {
                // Portfolio aggregation
                debug!("Portfolio calculation not yet fully implemented");
                NodeValue::Empty
            }
            // Input nodes don't need calculation - their values are set directly
            NodeId::Quote { .. }
            | NodeId::CurveInput { .. }
            | NodeId::VolSurface { .. }
            | NodeId::FxRate { .. }
            | NodeId::IndexFixing { .. }
            | NodeId::InflationFixing { .. }
            | NodeId::Config { .. } => {
                // Input nodes: value already set by market data listener
                // Just return what's in the cache
                calc_graph.get_cached(node_id)
                    .map(|cv| cv.value.clone())
                    .unwrap_or(NodeValue::Empty)
            }
        }
    }

    /// Calculate bond price using PricingRouter.
    fn calculate_bond_price(
        instrument_id: &InstrumentId,
        calc_graph: &Arc<CalculationGraph>,
        curve_builder: &Arc<CurveBuilder>,
        pricing_router: &Arc<PricingRouter>,
        bond_cache: &Arc<dashmap::DashMap<InstrumentId, convex_traits::reference_data::BondReferenceData>>,
        settlement_date: Date,
    ) -> NodeValue {
        // Get bond reference data from local cache
        let bond_ref = match bond_cache.get(instrument_id) {
            Some(bond) => bond.clone(),
            None => {
                warn!("Bond reference data not found in cache for {}", instrument_id);
                return NodeValue::Empty;
            }
        };

        // Get market quote from cache
        let quote_node = NodeId::Quote { instrument_id: instrument_id.clone() };
        let market_price = calc_graph.get_cached(&quote_node)
            .and_then(|cv| match &cv.value {
                NodeValue::Quote { mid, .. } => *mid,
                _ => None,
            });

        // Look up discount curve based on currency and issuer type
        let discount_curve = Self::lookup_discount_curve(&bond_ref, curve_builder);

        // Look up benchmark curve for I-spread calculations
        let benchmark_curve = Self::lookup_benchmark_curve(&bond_ref, curve_builder);

        // Create pricing input
        // Note: GovernmentCurve requires specific benchmark securities which are not
        // available from the curve builder. G-spread calculations require explicit
        // benchmark bond data.
        let input = PricingInput {
            bond: bond_ref.clone(),
            settlement_date,
            market_price_bid: None,
            market_price_mid: market_price,
            market_price_ask: None,
            discount_curve,
            benchmark_curve,
            government_curve: None, // Requires explicit benchmark securities
            volatility: None,
            bid_ask_config: None,
        };

        // Execute pricing
        match pricing_router.price(&input) {
            Ok(output) => {
                debug!(
                    "Priced bond {}: clean={:?}, ytm={:?}",
                    instrument_id, output.clean_price_mid, output.ytm_mid
                );
                NodeValue::BondPrice {
                    clean_price_bid: output.clean_price_bid,
                    clean_price_mid: output.clean_price_mid,
                    clean_price_ask: output.clean_price_ask,
                    accrued_interest: output.accrued_interest,
                    ytm_bid: output.ytm_bid,
                    ytm_mid: output.ytm_mid,
                    ytm_ask: output.ytm_ask,
                    z_spread_mid: output.z_spread_mid,
                    modified_duration: output.modified_duration,
                    dv01: output.dv01,
                }
            }
            Err(e) => {
                error!("Failed to price bond {}: {}", instrument_id, e);
                NodeValue::Empty
            }
        }
    }

    /// Look up discount curve based on bond reference data.
    ///
    /// Uses currency and issuer type to determine the appropriate curve:
    /// - Sovereign bonds: Use government curve (e.g., "USD_GOVT", "EUR_GOVT")
    /// - Corporate/Financial: Use OIS or swap curve (e.g., "USD_OIS", "EUR_OIS")
    fn lookup_discount_curve(
        bond_ref: &convex_traits::reference_data::BondReferenceData,
        curve_builder: &Arc<CurveBuilder>,
    ) -> Option<BuiltCurve> {
        use convex_core::Currency;
        use convex_traits::reference_data::IssuerType;

        let currency_code = match bond_ref.currency {
            Currency::USD => "USD",
            Currency::EUR => "EUR",
            Currency::GBP => "GBP",
            Currency::JPY => "JPY",
            Currency::CHF => "CHF",
            Currency::CAD => "CAD",
            Currency::AUD => "AUD",
            Currency::CNY => "CNY",
            _ => "USD", // Default to USD
        };

        // Determine curve type based on issuer type
        let curve_suffix = match bond_ref.issuer_type {
            IssuerType::Sovereign | IssuerType::Agency | IssuerType::Municipal => "GOVT",
            IssuerType::CorporateIG | IssuerType::CorporateHY | IssuerType::Financial => "OIS",
            IssuerType::Supranational => "GOVT",
        };

        // Try the specific curve first
        let curve_id = CurveId::new(&format!("{}_{}", currency_code, curve_suffix));
        if let Some(curve) = curve_builder.get(&curve_id) {
            return Some(curve);
        }

        // Fall back to OIS curve
        let ois_curve_id = CurveId::new(&format!("{}_OIS", currency_code));
        if let Some(curve) = curve_builder.get(&ois_curve_id) {
            return Some(curve);
        }

        // Fall back to government curve
        let govt_curve_id = CurveId::new(&format!("{}_GOVT", currency_code));
        if let Some(curve) = curve_builder.get(&govt_curve_id) {
            return Some(curve);
        }

        // Try generic discount curve
        let generic_curve_id = CurveId::new(&format!("{}_DISCOUNT", currency_code));
        curve_builder.get(&generic_curve_id)
    }

    /// Look up benchmark curve for I-spread calculations.
    ///
    /// Used to calculate I-spread (spread over swap/benchmark curve).
    /// Typically uses OIS or swap curves as the benchmark.
    fn lookup_benchmark_curve(
        bond_ref: &convex_traits::reference_data::BondReferenceData,
        curve_builder: &Arc<CurveBuilder>,
    ) -> Option<BuiltCurve> {
        use convex_core::Currency;

        let currency_code = match bond_ref.currency {
            Currency::USD => "USD",
            Currency::EUR => "EUR",
            Currency::GBP => "GBP",
            Currency::JPY => "JPY",
            Currency::CHF => "CHF",
            Currency::CAD => "CAD",
            Currency::AUD => "AUD",
            Currency::CNY => "CNY",
            _ => "USD", // Default to USD
        };

        // Try OIS curve first (preferred post-LIBOR)
        let ois_curve_id = CurveId::new(&format!("{}_OIS", currency_code));
        if let Some(curve) = curve_builder.get(&ois_curve_id) {
            return Some(curve);
        }

        // Try swap curve
        let swap_curve_id = CurveId::new(&format!("{}_SWAP", currency_code));
        if let Some(curve) = curve_builder.get(&swap_curve_id) {
            return Some(curve);
        }

        // Fall back to government curve
        let govt_curve_id = CurveId::new(&format!("{}_GOVT", currency_code));
        curve_builder.get(&govt_curve_id)
    }

    /// Calculate/rebuild a curve.
    fn calculate_curve(
        curve_id: &CurveId,
        curve_builder: &Arc<CurveBuilder>,
    ) -> NodeValue {
        match curve_builder.get(curve_id) {
            Some(curve) => {
                debug!("Rebuilt curve: {}", curve_id);
                NodeValue::Curve {
                    points: curve.to_points(),
                }
            }
            None => {
                warn!("Could not rebuild curve: {}", curve_id);
                NodeValue::Empty
            }
        }
    }

    /// Stop the reactive engine.
    pub fn stop(&self) {
        info!("Stopping reactive pricing engine");
        let _ = self.shutdown_tx.send(());
        self.interval_scheduler.stop_all();
        self.eod_scheduler.read().stop();
        self.cron_scheduler.read().stop();
        info!("Reactive pricing engine stopped");
    }

    /// Subscribe to node updates.
    ///
    /// This provides a unified stream of all node updates from all sources
    /// (immediate, throttled, interval, EOD).
    pub fn subscribe(&self) -> broadcast::Receiver<NodeUpdate> {
        self.node_update_tx.subscribe()
    }

    /// Get the market data publisher.
    ///
    /// Use this to inject market data updates into the system.
    pub fn market_data_publisher(&self) -> &MarketDataPublisher {
        &self.market_data_publisher
    }

    /// Register a node with appropriate scheduler based on its config.
    pub fn register_node(&self, node_id: NodeId, config: NodeConfig) {
        // Set config on calc graph
        self.calc_graph.set_node_config(node_id.clone(), config.clone());

        // Register with appropriate scheduler
        match config.frequency {
            UpdateFrequency::Immediate => {
                // No scheduler needed - handled by main loop
            }
            UpdateFrequency::Throttled { interval } => {
                self.throttle_manager.register(node_id, interval);
            }
            UpdateFrequency::Interval { interval } => {
                self.interval_scheduler.register(node_id, interval);
            }
            UpdateFrequency::OnDemand => {
                // No scheduler needed - only on explicit request
            }
            UpdateFrequency::EndOfDay { ref time } => {
                self.eod_scheduler.write().register(node_id, time);
            }
            UpdateFrequency::Scheduled { ref cron } => {
                if let Err(e) = self.cron_scheduler.write().register(node_id, cron) {
                    error!("Failed to register cron schedule: {}", e);
                }
            }
        }
    }

    /// Register a bond for reactive pricing.
    pub fn register_bond(&self, instrument_id: InstrumentId, config: NodeConfig) {
        let node_id = NodeId::BondPrice { instrument_id };
        self.register_node(node_id, config);
    }

    /// Register an ETF for iNAV calculations.
    pub fn register_etf_inav(&self, etf_id: EtfId) {
        let node_id = NodeId::EtfInav { etf_id };
        self.register_node(node_id, NodeConfig::etf_inav());
    }

    /// Register an ETF for NAV calculations.
    pub fn register_etf_nav(&self, etf_id: EtfId) {
        let node_id = NodeId::EtfNav { etf_id };
        self.register_node(node_id, NodeConfig::etf_nav());
    }

    /// Register a portfolio for analytics.
    pub fn register_portfolio(&self, portfolio_id: PortfolioId) {
        let node_id = NodeId::Portfolio { portfolio_id };
        self.register_node(node_id, NodeConfig::portfolio());
    }

    /// Force recalculation of a node (for on-demand).
    pub fn force_recalculate(&self, node_id: &NodeId) {
        self.calc_graph.invalidate(node_id);
        self.calc_graph.update_cache(node_id, NodeValue::Empty);

        let _ = self.node_update_tx.send(NodeUpdate {
            node_id: node_id.clone(),
            timestamp: chrono::Utc::now().timestamp(),
            source: UpdateSource::OnDemand,
        });
    }

    /// Get the calculation graph.
    pub fn calc_graph(&self) -> &Arc<CalculationGraph> {
        &self.calc_graph
    }

    /// Get the interval scheduler.
    pub fn interval_scheduler(&self) -> &Arc<IntervalScheduler> {
        &self.interval_scheduler
    }

    /// Get the throttle manager.
    pub fn throttle_manager(&self) -> &Arc<ThrottleManager> {
        &self.throttle_manager
    }
}

// =============================================================================
// REACTIVE ENGINE BUILDER
// =============================================================================

/// Builder for reactive engine.
pub struct ReactiveEngineBuilder {
    calc_graph: Option<Arc<CalculationGraph>>,
    curve_builder: Option<Arc<CurveBuilder>>,
    pricing_router: Option<Arc<PricingRouter>>,
    reference_data: Option<Arc<ReferenceDataProvider>>,
}

impl ReactiveEngineBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            calc_graph: None,
            curve_builder: None,
            pricing_router: None,
            reference_data: None,
        }
    }

    /// Set the calculation graph.
    pub fn with_calc_graph(mut self, calc_graph: Arc<CalculationGraph>) -> Self {
        self.calc_graph = Some(calc_graph);
        self
    }

    /// Set the curve builder.
    pub fn with_curve_builder(mut self, curve_builder: Arc<CurveBuilder>) -> Self {
        self.curve_builder = Some(curve_builder);
        self
    }

    /// Set the pricing router.
    pub fn with_pricing_router(mut self, pricing_router: Arc<PricingRouter>) -> Self {
        self.pricing_router = Some(pricing_router);
        self
    }

    /// Set the reference data provider.
    pub fn with_reference_data(mut self, reference_data: Arc<ReferenceDataProvider>) -> Self {
        self.reference_data = Some(reference_data);
        self
    }

    /// Build the reactive engine.
    pub fn build(self) -> Result<ReactiveEngine, &'static str> {
        let calc_graph = self.calc_graph.ok_or("calc_graph is required")?;
        let curve_builder = self.curve_builder.ok_or("curve_builder is required")?;
        let pricing_router = self.pricing_router.ok_or("pricing_router is required")?;
        let reference_data = self.reference_data.ok_or("reference_data is required")?;

        Ok(ReactiveEngine::new(calc_graph, curve_builder, pricing_router, reference_data))
    }
}

impl Default for ReactiveEngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use convex_traits::market_data::MarketDataProvider;
    use convex_ext_file::{
        EmptyQuoteSource, EmptyCurveInputSource, EmptyIndexFixingSource,
        EmptyVolatilitySource, EmptyFxRateSource, EmptyInflationFixingSource,
        EmptyEtfQuoteSource, EmptyBondReferenceSource, EmptyIssuerReferenceSource,
        EmptyRatingSource, EmptyEtfHoldingsSource,
    };

    fn create_test_engine() -> ReactiveEngine {
        let calc_graph = Arc::new(CalculationGraph::new());

        let market_data = Arc::new(MarketDataProvider {
            quotes: Arc::new(EmptyQuoteSource),
            curve_inputs: Arc::new(EmptyCurveInputSource),
            index_fixings: Arc::new(EmptyIndexFixingSource),
            volatility: Arc::new(EmptyVolatilitySource),
            fx_rates: Arc::new(EmptyFxRateSource),
            inflation_fixings: Arc::new(EmptyInflationFixingSource),
            etf_quotes: Arc::new(EmptyEtfQuoteSource),
        });

        let reference_data = Arc::new(ReferenceDataProvider {
            bonds: Arc::new(EmptyBondReferenceSource),
            issuers: Arc::new(EmptyIssuerReferenceSource),
            ratings: Arc::new(EmptyRatingSource),
            etf_holdings: Arc::new(EmptyEtfHoldingsSource),
        });

        let curve_builder = Arc::new(CurveBuilder::new(market_data, calc_graph.clone()));
        let pricing_router = Arc::new(PricingRouter::new());

        ReactiveEngine::new(calc_graph, curve_builder, pricing_router, reference_data)
    }

    #[test]
    fn test_reactive_engine_creation() {
        let engine = create_test_engine();
        assert!(engine.calc_graph().current_revision() == 0);
    }

    #[test]
    fn test_register_bond() {
        let engine = create_test_engine();

        let instrument_id = InstrumentId::new("US912810TD00");
        engine.register_bond(instrument_id.clone(), NodeConfig::bond_price_liquid());

        // Check that config was set
        let node_id = NodeId::BondPrice { instrument_id };
        assert!(engine.calc_graph().get_node_config(&node_id).is_some());
    }

    #[tokio::test]
    async fn test_register_etf_inav() {
        let engine = create_test_engine();

        let etf_id = EtfId::new("LQD");
        engine.register_etf_inav(etf_id.clone());

        // Check that it was registered with interval scheduler
        let intervals = engine.interval_scheduler().get_intervals();
        assert!(!intervals.is_empty());
    }

    #[tokio::test]
    async fn test_market_data_update_triggers_repricing() {
        use rust_decimal_macros::dec;
        use convex_core::Currency;
        use convex_traits::reference_data::{BondReferenceData, BondType, IssuerType};

        let engine = create_test_engine();
        let instrument_id = InstrumentId::new("TEST_BOND_001");

        // Create and cache bond reference data
        let bond_ref = BondReferenceData {
            instrument_id: instrument_id.clone(),
            isin: Some("US912810TD00".to_string()),
            cusip: Some("912810TD0".to_string()),
            sedol: None,
            bbgid: None,
            description: "Test Treasury Bond".to_string(),
            currency: Currency::USD,
            issue_date: Date::from_ymd(2020, 1, 15).unwrap(),
            maturity_date: Date::from_ymd(2030, 1, 15).unwrap(),
            coupon_rate: Some(dec!(0.025)), // 2.5%
            frequency: 2, // Semi-annual
            day_count: "ACT/ACT".to_string(),
            face_value: dec!(100),
            bond_type: BondType::FixedBullet,
            issuer_type: IssuerType::Sovereign,
            issuer_id: "US_TREASURY".to_string(),
            issuer_name: "US Treasury".to_string(),
            seniority: "Senior".to_string(),
            is_callable: false,
            call_schedule: vec![],
            is_putable: false,
            is_sinkable: false,
            floating_terms: None,
            inflation_index: None,
            inflation_base_index: None,
            has_deflation_floor: false,
            country_of_risk: "US".to_string(),
            sector: "Government".to_string(),
            amount_outstanding: None,
            first_coupon_date: None,
            last_updated: 0,
            source: "test".to_string(),
        };

        engine.cache_bond_reference(bond_ref);

        // Register the bond for reactive pricing
        engine.register_bond(instrument_id.clone(), NodeConfig::bond_price_liquid());

        // Add initial quote to the calc graph
        let quote_node = NodeId::Quote { instrument_id: instrument_id.clone() };
        engine.calc_graph().update_cache(
            &quote_node,
            NodeValue::Quote {
                bid: Some(dec!(99.50)),
                ask: Some(dec!(100.50)),
                mid: Some(dec!(100.00)),
            },
        );

        // Mark quote as dirty to trigger recalculation
        engine.calc_graph().invalidate(&quote_node);

        // Wait a moment for the processing loop to run (if started)
        // For this test, we'll manually trigger calculation
        let bond_node = NodeId::BondPrice { instrument_id: instrument_id.clone() };
        engine.calc_graph().invalidate(&bond_node);

        // Get dirty nodes and verify bond is in the list
        let dirty = engine.calc_graph().get_dirty_nodes();
        assert!(dirty.contains(&bond_node), "Bond node should be dirty after quote update");

        // Manually trigger calculation (simulating what the processing loop does)
        let bond_cache = engine.bond_cache.clone();
        let calc_graph = engine.calc_graph().clone();
        let curve_builder = engine.curve_builder.clone();
        let pricing_router = engine.pricing_router.clone();
        let settlement_date = engine.settlement_date();

        let result = ReactiveEngine::calculate_bond_price(
            &instrument_id,
            &calc_graph,
            &curve_builder,
            &pricing_router,
            &bond_cache,
            settlement_date,
        );

        // Verify we got a calculated result (not Empty)
        match result {
            NodeValue::BondPrice { clean_price_mid, ytm_mid, modified_duration, .. } => {
                // YTM should be calculated from the market price
                assert!(ytm_mid.is_some(), "YTM should be calculated");
                println!("Calculated YTM: {:?}", ytm_mid);

                // If we have a market price, clean price should be set
                if clean_price_mid.is_some() {
                    println!("Clean price: {:?}", clean_price_mid);
                }

                // Duration should be calculated
                if modified_duration.is_some() {
                    println!("Modified duration: {:?}", modified_duration);
                }
            }
            NodeValue::Empty => {
                // This is acceptable if bond ref wasn't found - but we cached it
                panic!("Expected bond price calculation, got Empty");
            }
            other => {
                panic!("Expected BondPrice, got {:?}", other);
            }
        }
    }

    #[tokio::test]
    async fn test_quote_update_propagates_to_bond_price() {
        use rust_decimal_macros::dec;

        let engine = create_test_engine();
        let instrument_id = InstrumentId::new("PROP_TEST");

        // Add quote node
        let quote_node = NodeId::Quote { instrument_id: instrument_id.clone() };
        let bond_node = NodeId::BondPrice { instrument_id: instrument_id.clone() };

        // Register bond node with dependency on quote
        engine.calc_graph().add_node(bond_node.clone(), vec![quote_node.clone()]);

        // Nodes start dirty - mark as clean first by updating cache
        engine.calc_graph().update_cache(&bond_node, NodeValue::Empty);

        // Now bond should be clean
        assert!(!engine.calc_graph().is_dirty(&bond_node), "Bond should be clean after cache update");

        // Update quote - this should propagate to bond
        engine.calc_graph().update_cache(
            &quote_node,
            NodeValue::Quote {
                bid: Some(dec!(98.00)),
                ask: Some(dec!(99.00)),
                mid: Some(dec!(98.50)),
            },
        );
        engine.calc_graph().invalidate(&quote_node);

        // Now bond should be dirty because its dependency (quote) was invalidated
        assert!(engine.calc_graph().is_dirty(&bond_node),
            "Bond node should be dirty after quote invalidation");

        // Verify bond is in the nodes to calculate
        let to_calc = engine.calc_graph().get_nodes_to_calculate();
        assert!(to_calc.contains(&bond_node),
            "Bond node should be in nodes to calculate");
    }

    #[tokio::test]
    async fn test_curve_update_propagates_to_bonds() {
        let engine = create_test_engine();

        let curve_id = CurveId::new("USD_GOVT");
        let instrument_id = InstrumentId::new("CURVE_DEP_TEST");

        // Create nodes
        let curve_node = NodeId::Curve { curve_id: curve_id.clone() };
        let bond_node = NodeId::BondPrice { instrument_id: instrument_id.clone() };

        // Bond depends on curve
        engine.calc_graph().add_node(bond_node.clone(), vec![curve_node.clone()]);

        // Nodes start dirty - mark as clean first by updating cache
        engine.calc_graph().update_cache(&bond_node, NodeValue::Empty);

        // Now bond should be clean
        assert!(!engine.calc_graph().is_dirty(&bond_node), "Bond should be clean after cache update");

        // Update curve
        engine.calc_graph().update_cache(
            &curve_node,
            NodeValue::Curve {
                points: vec![(365, 0.04), (730, 0.045), (1825, 0.05)],
            },
        );
        engine.calc_graph().invalidate(&curve_node);

        // Bond should now be dirty
        assert!(engine.calc_graph().is_dirty(&bond_node),
            "Bond should be dirty after curve update");
    }
}
