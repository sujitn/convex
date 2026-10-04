//! Reactive pricing engine coordinator.
//!
//! The [`ReactiveEngine`] coordinates all reactive pricing components:
//! - Market data listener for real-time updates
//! - Interval scheduler for fixed-schedule calculations
//! - EOD scheduler for end-of-day calculations
//! - Throttle manager for debounced updates
//! - Node update broadcasting
//! - **Actual calculation execution** via PricingRouter

#[cfg(test)]
use crate::reference_terms;

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::ports::config::{NodeConfig, UpdateFrequency};
use crate::ports::reference_repository::ReferenceRepository;
use convex_core::ids::*;
use convex_core::Date;

use crate::calc_graph::{
    CalculationFailure, CalculationGraph, CalculationTicket, CommitOutcome, GraphError, NodeId,
    NodeRole, NodeSpec, NodeValue,
};
use crate::curve_builder::CurveBuilder;
use crate::market_data_listener::{MarketDataListener, MarketDataPublisher};
use crate::pricing_router::{PricingInput, PricingRouter};
use crate::scheduler::{
    CronScheduler, EodScheduler, IntervalScheduler, NodeUpdate, ThrottleManager, UpdateSource,
};
use convex_core::contracts::ErrorCode;

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

    /// Authoritative reference repository shared with API mutations.
    references: Arc<dyn ReferenceRepository>,

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
    settlement_date: Arc<parking_lot::RwLock<Date>>,

    /// Shutdown signal
    shutdown_tx: broadcast::Sender<()>,
}

impl ReactiveEngine {
    /// Create a new reactive engine.
    pub fn new(
        calc_graph: Arc<CalculationGraph>,
        curve_builder: Arc<CurveBuilder>,
        pricing_router: Arc<PricingRouter>,
        references: Arc<dyn ReferenceRepository>,
    ) -> Self {
        let interval_scheduler = Arc::new(IntervalScheduler::new(calc_graph.clone()));
        let eod_scheduler = Arc::new(parking_lot::RwLock::new(EodScheduler::new(
            calc_graph.clone(),
        )));
        let cron_scheduler = Arc::new(parking_lot::RwLock::new(CronScheduler::new(
            calc_graph.clone(),
        )));
        let throttle_manager = Arc::new(ThrottleManager::new(calc_graph.clone()));
        let (market_data_publisher, _) = MarketDataPublisher::new();
        let (node_update_tx, _) = broadcast::channel(10000);
        let (shutdown_tx, _) = broadcast::channel(1);

        // Default settlement date to today
        let today = Date::today();

        Self {
            calc_graph,
            curve_builder,
            pricing_router,
            references,
            interval_scheduler,
            eod_scheduler,
            cron_scheduler,
            throttle_manager,
            market_data_publisher,
            node_update_tx,
            settlement_date: Arc::new(parking_lot::RwLock::new(today)),
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
        let throttle_manager = self.throttle_manager.clone();
        let update_rx = self.market_data_publisher.subscribe();
        let shutdown_rx = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut listener =
                MarketDataListener::new(calc_graph, throttle_manager, update_rx, shutdown_rx);

            listener.run().await;
        });
    }

    /// Start the main processing loop.
    async fn start_processing_loop(&self) {
        let calc_graph = self.calc_graph.clone();
        let curve_builder = self.curve_builder.clone();
        let pricing_router = self.pricing_router.clone();
        let references = self.references.clone();
        let throttle_manager = self.throttle_manager.clone();
        let node_update_tx = self.node_update_tx.clone();
        let settlement_date = self.settlement_date.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        // Main processing loop with actual calculation execution
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(50));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let ready = match calc_graph.get_nodes_to_calculate() {
                            Ok(nodes) => nodes,
                            Err(error) => { error!("Calculation frontier failed: {error}"); continue; }
                        };
                        for node_id in ready {
                            if !throttle_manager.should_calculate(&node_id) { continue; }
                            let ticket = match calc_graph.begin_calculation(&node_id) {
                                Ok(ticket) => ticket,
                                Err(GraphError::NotReady | GraphError::Suspended | GraphError::CapacityExceeded | GraphError::ContextRequired) => continue,
                                Err(error) => { warn!("Calculation admission failed: {error}"); continue; }
                            };
                            let captured_settlement = *settlement_date.read();
                            let result = match tokio::time::timeout_at(
                                tokio::time::Instant::from_std(ticket.deadline()),
                                Self::calculate_node(&ticket, &curve_builder, &pricing_router, &references, captured_settlement),
                            ).await {
                                Ok(result) => result,
                                Err(_) => Err(CalculationFailure::fixed(ErrorCode::DeadlineExceeded, "reference/pricing attempt exceeded its deadline")),
                            };
                            match calc_graph.commit_result(&ticket, result) {
                                Ok(CommitOutcome::Committed { completion }) => {
                                    throttle_manager.mark_calculated(&node_id);
                                    let _ = node_update_tx.send(NodeUpdate {
                                        node_id, timestamp: chrono::Utc::now().timestamp(), source: UpdateSource::Immediate, completion: Some(completion),
                                    });
                                }
                                Ok(CommitOutcome::Discarded) => debug!("Discarded obsolete calculation: {node_id}"),
                                Err(error) => {
                                    calc_graph.cancel_attempt(&ticket);
                                    error!("Calculation commit rejected: {error}");
                                }
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

    /// Execute outside the graph lock, using only the captured dependency values.
    async fn calculate_node(
        ticket: &CalculationTicket,
        curve_builder: &Arc<CurveBuilder>,
        pricing_router: &Arc<PricingRouter>,
        references: &Arc<dyn ReferenceRepository>,
        settlement_date: Date,
    ) -> Result<NodeValue, CalculationFailure> {
        match ticket.node_id() {
            NodeId::BondPrice { instrument_id } => {
                Self::calculate_bond_price(
                    instrument_id,
                    ticket,
                    curve_builder,
                    pricing_router,
                    references,
                    settlement_date,
                )
                .await
            }
            NodeId::Curve { .. } => Err(CalculationFailure::fixed(
                ErrorCode::UnsupportedConvention,
                "graph curve calibration requires a captured recipe and input pipeline",
            )),
            _ => Err(CalculationFailure::fixed(
                ErrorCode::UnsupportedProduct,
                "this node calculation is not implemented",
            )),
        }
    }

    /// Compatibility observed-mark pricing. Full immutable reference/configuration adoption is CVX-005.
    async fn calculate_bond_price(
        instrument_id: &InstrumentId,
        ticket: &CalculationTicket,
        curve_builder: &Arc<CurveBuilder>,
        pricing_router: &Arc<PricingRouter>,
        references: &Arc<dyn ReferenceRepository>,
        settlement_date: Date,
    ) -> Result<NodeValue, CalculationFailure> {
        if ticket.snapshot().is_some() {
            return Err(CalculationFailure::fixed(
                ErrorCode::UnsupportedConvention,
                "snapshot work requires the supplied-context SnapshotValuator",
            ));
        }
        let quote_node = NodeId::Quote {
            instrument_id: instrument_id.clone(),
        };
        let (bid, market_price, ask) = match ticket.input(&quote_node).map(|input| &input.value) {
            Some(NodeValue::Quote { bid, mid, ask }) => (*bid, *mid, *ask),
            _ => {
                return Err(CalculationFailure::fixed(
                    ErrorCode::MissingMarketData,
                    "declared market quote is missing",
                ))
            }
        };
        if market_price.is_none() && bid.is_none() && ask.is_none() {
            return Err(CalculationFailure::fixed(
                ErrorCode::MissingMarketData,
                "market quote contains no price",
            ));
        }
        // Freeze declared builder objects before reference I/O. Do not look up undeclared/default curves.
        let mut curves = Vec::new();
        for input in ticket.inputs() {
            if let NodeId::Curve { curve_id } = &input.node_id {
                let curve = curve_builder.get(curve_id).ok_or_else(|| {
                    CalculationFailure::fixed(
                        ErrorCode::MissingMarketData,
                        "declared curve is unavailable",
                    )
                })?;
                if !matches!(&input.value.value, NodeValue::Curve { points } if *points == curve.to_points())
                {
                    return Err(CalculationFailure::fixed(
                        ErrorCode::VersionConflict,
                        "curve builder differs from captured graph input",
                    ));
                }
                curves.push(curve);
            }
        }
        if curves.len() > 2 {
            return Err(CalculationFailure::fixed(
                ErrorCode::UnsupportedConvention,
                "observed-mark pricing accepts explicit discount and benchmark curves only",
            ));
        }
        let discount_curve = curves.first().cloned();
        let benchmark_curve = curves.get(1).cloned();
        let time = convex_core::contracts::UtcTimestamp::from_datetime(chrono::Utc::now())
            .map_err(|_| {
                CalculationFailure::fixed(ErrorCode::InvalidInput, "invalid valuation time")
            })?;
        let reference = references
            .for_valuation(
                instrument_id,
                crate::ports::reference_master::ReferenceLookup {
                    as_of: time,
                    known_at: time,
                },
            )
            .await
            .map_err(|_| {
                CalculationFailure::fixed(
                    ErrorCode::Unavailable,
                    "reference repository unavailable",
                )
            })?;
        let bond_ref = reference
            .and_then(|resolved| resolved.record.bond)
            .ok_or_else(|| {
                CalculationFailure::fixed(
                    ErrorCode::MissingReference,
                    "bond reference missing or deleted",
                )
            })?;

        // Create pricing input
        // Note: GovernmentCurve requires specific benchmark securities which are not
        // available from the curve builder. G-spread calculations require explicit
        // benchmark bond data.
        let input = PricingInput {
            bond: bond_ref.clone(),
            settlement_date,
            market_price_bid: bid,
            market_price_mid: market_price,
            market_price_ask: ask,
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
                Ok(NodeValue::BondPrice {
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
                })
            }
            Err(e) => {
                error!("Failed to price bond {}: {}", instrument_id, e);
                Err(CalculationFailure::fixed(
                    ErrorCode::InvalidInput,
                    "bond pricing failed; see router diagnostics",
                ))
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
    pub fn register_node(&self, node_id: NodeId, config: NodeConfig) -> Result<u64, GraphError> {
        let node = self.calc_graph.node_spec(&node_id).unwrap_or_else(|| {
            let role = match node_id {
                NodeId::BondPrice { .. }
                | NodeId::Curve { .. }
                | NodeId::EtfInav { .. }
                | NodeId::EtfNav { .. }
                | NodeId::Portfolio { .. } => NodeRole::Calculation,
                _ => NodeRole::Input,
            };
            NodeSpec {
                id: node_id.clone(),
                role,
                dependencies: vec![],
                config: config.clone(),
            }
        });
        let mut nodes = vec![node.with_config(config.clone())];
        if let NodeId::BondPrice { instrument_id } = &node_id {
            let quote = NodeId::Quote {
                instrument_id: instrument_id.clone(),
            };
            if !nodes[0].dependencies.contains(&quote) {
                nodes[0].dependencies.push(quote.clone());
            }
            if self.calc_graph.node_spec(&quote).is_none() {
                nodes.push(NodeSpec::input(quote));
            }
        }
        let generation = self.calc_graph.upsert_nodes(nodes)?;

        // Policy replacement removes prior timer registrations before installing the validated policy.
        self.interval_scheduler.unregister(&node_id);
        self.throttle_manager.unregister(&node_id);
        self.eod_scheduler.read().unregister(&node_id);
        self.cron_scheduler.read().unregister(&node_id);
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
        Ok(generation)
    }

    /// Register a bond for reactive pricing.
    pub fn register_bond(
        &self,
        instrument_id: InstrumentId,
        config: NodeConfig,
    ) -> Result<u64, GraphError> {
        let node_id = NodeId::BondPrice { instrument_id };
        self.register_node(node_id, config)
    }

    /// Register an ETF for iNAV calculations.
    pub fn register_etf_inav(&self, etf_id: EtfId) -> Result<u64, GraphError> {
        let node_id = NodeId::EtfInav { etf_id };
        self.register_node(node_id, NodeConfig::etf_inav())
    }

    /// Register an ETF for NAV calculations.
    pub fn register_etf_nav(&self, etf_id: EtfId) -> Result<u64, GraphError> {
        let node_id = NodeId::EtfNav { etf_id };
        self.register_node(node_id, NodeConfig::etf_nav())
    }

    /// Register a portfolio for analytics.
    pub fn register_portfolio(&self, portfolio_id: PortfolioId) -> Result<u64, GraphError> {
        let node_id = NodeId::Portfolio { portfolio_id };
        self.register_node(node_id, NodeConfig::portfolio())
    }

    /// Force recalculation of a node (for on-demand).
    pub fn force_recalculate(&self, node_id: &NodeId) -> Result<u64, GraphError> {
        self.calc_graph.request_calculation(node_id)
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
    references: Option<Arc<dyn ReferenceRepository>>,
}

impl ReactiveEngineBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            calc_graph: None,
            curve_builder: None,
            pricing_router: None,
            references: None,
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
    pub fn with_reference_repository(mut self, references: Arc<dyn ReferenceRepository>) -> Self {
        self.references = Some(references);
        self
    }

    /// Build the reactive engine.
    pub fn build(self) -> Result<ReactiveEngine, &'static str> {
        let calc_graph = self.calc_graph.ok_or("calc_graph is required")?;
        let curve_builder = self.curve_builder.ok_or("curve_builder is required")?;
        let pricing_router = self.pricing_router.ok_or("pricing_router is required")?;
        let references = self.references.ok_or("reference repository is required")?;

        Ok(ReactiveEngine::new(
            calc_graph,
            curve_builder,
            pricing_router,
            references,
        ))
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
    use crate::ports::market_data::MarketDataProvider;
    use convex_ext_file::{
        EmptyCurveInputSource, EmptyEtfQuoteSource, EmptyFxRateSource, EmptyIndexFixingSource,
        EmptyInflationFixingSource, EmptyQuoteSource, EmptyVolatilitySource,
    };
    use std::sync::Arc;

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

        let references = Arc::new(convex_ext_file::MemoryReferenceRepository::default());

        let curve_builder = Arc::new(CurveBuilder::new(market_data, calc_graph.clone()));
        let pricing_router = Arc::new(PricingRouter::new());

        ReactiveEngine::new(calc_graph, curve_builder, pricing_router, references)
    }

    #[tokio::test]
    async fn completion_stream_reports_committed_failure_and_on_demand_revision_without_fake_success(
    ) {
        let engine = create_test_engine();
        let node = NodeId::Portfolio {
            portfolio_id: PortfolioId::new("UNSUPPORTED"),
        };
        engine
            .register_node(node.clone(), NodeConfig::default())
            .unwrap();
        let mut updates = engine.subscribe();
        engine.start_processing_loop().await;
        let first = tokio::time::timeout(Duration::from_secs(2), updates.recv())
            .await
            .unwrap()
            .unwrap();
        let first = first.completion.unwrap();
        assert_eq!(first.status, convex_core::contracts::ResultStatus::Failed);
        assert_eq!(first.quality, convex_core::contracts::DataQuality::Invalid);
        assert_eq!(first.failure.unwrap().code, ErrorCode::UnsupportedProduct);
        assert_eq!(
            engine.calc_graph.inspect_node(&node).unwrap().revision,
            first.revision
        );
        assert!(engine.calc_graph.get_cached(&node).is_none());
        let generation = engine.force_recalculate(&node).unwrap();
        let second = tokio::time::timeout(Duration::from_secs(2), updates.recv())
            .await
            .unwrap()
            .unwrap()
            .completion
            .unwrap();
        assert_eq!(second.generation, generation);
        assert!(second.revision > first.revision);
        assert_eq!(second.status, convex_core::contracts::ResultStatus::Failed);
        engine.stop();
    }

    #[tokio::test]
    async fn actual_router_uses_captured_quote_after_async_yield_and_obsolete_result_is_discarded()
    {
        use rust_decimal_macros::dec;
        let engine = create_test_engine();
        let bond = serde_json::from_str::<crate::ports::reference_data::BondReferenceData>(
            include_str!("../../convex-ports/tests/fixtures/reference_bond.json"),
        )
        .unwrap();
        engine
            .references
            .commit(
                &crate::ports::reference_repository::ReferenceCommand {
                    idempotency_key: convex_core::contracts::RequestId::new("captured-router-test")
                        .unwrap(),
                    master_changes: reference_terms::resources(&bond),
                    changes: vec![
                        crate::ports::reference_repository::ReferenceChange::Create {
                            bond: Box::new(bond.clone()),
                        },
                    ],
                },
                convex_core::contracts::UtcTimestamp::parse("2025-01-01T00:00:00Z").unwrap(),
            )
            .await
            .unwrap();
        engine
            .register_bond(bond.instrument_id.clone(), NodeConfig::bond_price_liquid())
            .unwrap();
        let quote = NodeId::Quote {
            instrument_id: bond.instrument_id.clone(),
        };
        let node = NodeId::BondPrice {
            instrument_id: bond.instrument_id.clone(),
        };
        engine
            .calc_graph
            .publish_input(
                &quote,
                NodeValue::Quote {
                    bid: None,
                    ask: None,
                    mid: Some(dec!(99.25)),
                },
            )
            .unwrap();
        let ticket = engine.calc_graph.begin_calculation(&node).unwrap();
        let captured = Arc::new(tokio::sync::Barrier::new(2));
        let finish = Arc::new(tokio::sync::Barrier::new(2));
        let worker = {
            let graph = engine.calc_graph.clone();
            let curves = engine.curve_builder.clone();
            let router = engine.pricing_router.clone();
            let references = engine.references.clone();
            let captured = captured.clone();
            let finish = finish.clone();
            tokio::spawn(async move {
                captured.wait().await;
                finish.wait().await;
                let value = ReactiveEngine::calculate_node(
                    &ticket,
                    &curves,
                    &router,
                    &references,
                    Date::from_ymd(2026, 10, 6).unwrap(),
                )
                .await
                .unwrap();
                assert!(
                    matches!(&value, NodeValue::BondPrice { clean_price_mid: Some(price), .. } if *price == dec!(99.25))
                );
                graph.commit_result(&ticket, Ok(value)).unwrap()
            })
        };
        captured.wait().await;
        engine
            .calc_graph
            .publish_input(
                &quote,
                NodeValue::Quote {
                    bid: None,
                    ask: None,
                    mid: Some(dec!(100.25)),
                },
            )
            .unwrap();
        finish.wait().await;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), worker)
                .await
                .unwrap()
                .unwrap(),
            CommitOutcome::Discarded
        );
        assert!(engine.calc_graph.is_dirty(&node));
        assert!(engine.calc_graph.get_cached(&node).is_none());
        let current = engine.calc_graph.begin_calculation(&node).unwrap();
        let value = ReactiveEngine::calculate_node(
            &current,
            &engine.curve_builder,
            &engine.pricing_router,
            &engine.references,
            Date::from_ymd(2026, 10, 6).unwrap(),
        )
        .await
        .unwrap();
        assert!(
            matches!(&value, NodeValue::BondPrice { clean_price_mid: Some(price), .. } if *price == dec!(100.25))
        );
        engine
            .calc_graph
            .commit_result(&current, Ok(value))
            .unwrap();
    }

    #[test]
    fn test_reactive_engine_creation() {
        let engine = create_test_engine();
        assert!(engine.calc_graph().current_revision() == 0);
    }

    #[tokio::test]
    async fn timer_does_not_publish_a_suspended_or_expired_quote_as_empty_success() {
        let engine = create_test_engine();
        let instrument_id = InstrumentId::new("GAPPED-BOND");
        engine
            .register_bond(instrument_id.clone(), NodeConfig::bond_price_liquid())
            .unwrap();
        let quote = NodeId::Quote {
            instrument_id: instrument_id.clone(),
        };
        let bond = NodeId::BondPrice { instrument_id };
        engine
            .calc_graph
            .publish_input_until(
                &quote,
                NodeValue::Quote {
                    bid: None,
                    ask: None,
                    mid: Some(rust_decimal::Decimal::from(100)),
                },
                Some(std::time::Instant::now() - Duration::from_millis(1)),
            )
            .unwrap();
        engine.calc_graph.invalidate(&quote).unwrap();
        let mut updates = engine.subscribe();
        engine.start_processing_loop().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(150), updates.recv())
                .await
                .is_err()
        );
        assert!(engine.calc_graph.get_cached(&bond).is_none());
        engine.calc_graph.evict_cached(&quote).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(150), updates.recv())
                .await
                .is_err()
        );
        assert!(engine.calc_graph.get_cached(&bond).is_none());
        engine.stop();
    }

    #[tokio::test]
    async fn shared_transport_loss_blocks_scheduled_auxiliary_and_bond_publication() {
        let engine = create_test_engine();
        let quote = NodeId::Quote {
            instrument_id: "LOSS".into(),
        };
        engine
            .register_bond("LOSS".into(), NodeConfig::bond_price_liquid())
            .unwrap();
        engine.calc_graph.suspend_market_delivery().unwrap();
        // A quote image can be assembled while other dependencies still require full reload.
        engine
            .calc_graph
            .publish_input(
                &quote,
                NodeValue::Quote {
                    bid: None,
                    ask: None,
                    mid: Some(rust_decimal::Decimal::from(100)),
                },
            )
            .unwrap();
        assert!(engine.calc_graph.market_delivery_suspended());
        let curve = NodeId::Curve {
            curve_id: "USD.TEST".into(),
        };
        engine.calc_graph.add_node(curve.clone(), vec![]).unwrap();
        let mut updates = engine.subscribe();
        engine.start_processing_loop().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(150), updates.recv())
                .await
                .is_err()
        );
        assert!(engine.calc_graph.get_cached(&curve).is_none());
        assert!(engine
            .calc_graph
            .get_cached(&NodeId::BondPrice {
                instrument_id: "LOSS".into()
            })
            .is_none());
        engine.stop();
    }

    #[test]
    fn test_register_bond() {
        let engine = create_test_engine();

        let instrument_id = InstrumentId::new("US912810TD00");
        engine
            .register_bond(instrument_id.clone(), NodeConfig::bond_price_liquid())
            .unwrap();

        // Check that config was set
        let node_id = NodeId::BondPrice { instrument_id };
        assert!(engine.calc_graph().get_node_config(&node_id).is_some());
    }

    #[tokio::test]
    async fn test_register_etf_inav() {
        let engine = create_test_engine();

        let etf_id = EtfId::new("LQD");
        engine.register_etf_inav(etf_id.clone()).unwrap();

        // Check that it was registered with interval scheduler
        let intervals = engine.interval_scheduler().get_intervals();
        assert!(!intervals.is_empty());
    }

    #[tokio::test]
    async fn test_market_data_update_triggers_repricing() {
        use crate::ports::reference_data::{BondReferenceData, BondType, IssuerType};
        use convex_core::Currency;
        use rust_decimal_macros::dec;

        let engine = create_test_engine();
        let instrument_id = InstrumentId::new("TEST_BOND_001");

        // Commit bond reference data to the shared repository
        let bond_ref = reference_terms::complete(BondReferenceData {
            contractual_terms: None,
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
            frequency: 2,                   // Semi-annual
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
        });

        engine
            .references
            .commit(
                &crate::ports::reference_repository::ReferenceCommand {
                    master_changes: reference_terms::resources(&bond_ref),
                    idempotency_key: convex_core::contracts::RequestId::new("reactive-test")
                        .unwrap(),
                    changes: vec![
                        crate::ports::reference_repository::ReferenceChange::Create {
                            bond: Box::new(bond_ref),
                        },
                    ],
                },
                convex_core::contracts::UtcTimestamp::parse("2024-01-15T00:00:00Z").unwrap(),
            )
            .await
            .unwrap();

        // Register the bond for reactive pricing
        engine
            .register_bond(instrument_id.clone(), NodeConfig::bond_price_liquid())
            .unwrap();

        // Add initial quote to the calc graph
        let quote_node = NodeId::Quote {
            instrument_id: instrument_id.clone(),
        };
        engine
            .calc_graph()
            .publish_input(
                &quote_node,
                NodeValue::Quote {
                    bid: Some(dec!(99.50)),
                    ask: Some(dec!(100.50)),
                    mid: Some(dec!(100.00)),
                },
            )
            .unwrap();

        // Mark quote as dirty to trigger recalculation

        // Wait a moment for the processing loop to run (if started)
        // For this test, we'll manually trigger calculation
        let bond_node = NodeId::BondPrice {
            instrument_id: instrument_id.clone(),
        };
        engine.calc_graph().invalidate(&bond_node).unwrap();

        // Get dirty nodes and verify bond is in the list
        let dirty = engine.calc_graph().get_dirty_nodes();
        assert!(
            dirty.contains(&bond_node),
            "Bond node should be dirty after quote update"
        );

        // Manually trigger calculation (simulating what the processing loop does)
        let references = engine.references.clone();
        let calc_graph = engine.calc_graph().clone();
        let curve_builder = engine.curve_builder.clone();
        let pricing_router = engine.pricing_router.clone();
        let settlement_date = engine.settlement_date();

        let result = ReactiveEngine::calculate_bond_price(
            &instrument_id,
            &calc_graph.begin_calculation(&bond_node).unwrap(),
            &curve_builder,
            &pricing_router,
            &references,
            settlement_date,
        )
        .await
        .unwrap();

        // Verify actual calculated measures
        match result {
            NodeValue::BondPrice {
                clean_price_mid,
                ytm_mid,
                modified_duration,
                ..
            } => {
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
        let quote_node = NodeId::Quote {
            instrument_id: instrument_id.clone(),
        };
        let bond_node = NodeId::BondPrice {
            instrument_id: instrument_id.clone(),
        };

        engine
            .register_bond(instrument_id, NodeConfig::bond_price_liquid())
            .unwrap();
        engine
            .calc_graph()
            .publish_input(
                &quote_node,
                NodeValue::Quote {
                    bid: Some(dec!(98.00)),
                    ask: Some(dec!(99.00)),
                    mid: Some(dec!(98.50)),
                },
            )
            .unwrap();
        assert!(engine.calc_graph().is_dirty(&bond_node));
        assert!(engine
            .calc_graph()
            .get_nodes_to_calculate()
            .unwrap()
            .contains(&bond_node));
    }

    #[tokio::test]
    async fn test_curve_update_propagates_to_bonds() {
        let engine = create_test_engine();

        let curve_id = CurveId::new("USD_GOVT");
        let instrument_id = InstrumentId::new("CURVE_DEP_TEST");

        // Create nodes
        let curve_node = NodeId::Curve {
            curve_id: curve_id.clone(),
        };
        let bond_node = NodeId::BondPrice {
            instrument_id: instrument_id.clone(),
        };

        engine
            .calc_graph()
            .upsert_nodes(vec![
                NodeSpec::input(curve_node.clone()),
                NodeSpec::calculation(bond_node.clone(), vec![curve_node.clone()]),
            ])
            .unwrap();
        engine
            .calc_graph()
            .publish_input(
                &curve_node,
                NodeValue::Curve {
                    points: vec![(365, 0.04), (730, 0.045), (1825, 0.05)],
                },
            )
            .unwrap();
        assert!(engine.calc_graph().is_dirty(&bond_node));
        assert!(engine
            .calc_graph()
            .get_nodes_to_calculate()
            .unwrap()
            .contains(&bond_node));
    }
}
