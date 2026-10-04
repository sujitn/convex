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
//! - [`snapshot::SnapshotAssembler`]: Exact immutable input capture and publication
//! - [`snapshot::SnapshotValuator`]: Supplied-context valuation with bounded complete-key caching
//!
//! Snapshot use cases create no scheduling loop. The graph admits supplied snapshot/context tickets;
//! the event-driven runtime owns authoritative snapshot valuation and publication. `SNAPSHOTS.md` and `GRAPH.md` document scope and migration.
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
//!     .with_config(engine_config)
//!     .with_runtime(runtime_services)
//!     .build()?;
//!
//! engine.start().await?;
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

#[cfg(test)]
#[path = "../../convex-ports/tests/support/reference_terms.rs"]
mod reference_terms;

pub mod builder;
pub mod calc_graph;
pub mod curve_builder;
pub mod error;
pub mod etf_pricing;
pub mod market_data_listener;
pub mod market_ingestion;
pub mod portfolio_analytics;
pub mod pricing_router;
pub mod runtime;
pub mod scheduler;
pub mod snapshot;

pub mod cache;
pub mod configuration;
pub mod context;

// Re-exports
pub use builder::PricingEngineBuilder;
pub use calc_graph::{
    CalculationCompletion, CalculationFailure, CalculationGraph, CalculationTicket, CommitOutcome,
    GraphError, GraphLimits, NodeId, NodePhase, NodeRole, NodeSpec, NodeValue, ShardAssignment,
    ShardConfig, ShardStrategy,
};
pub use curve_builder::{BuiltCurve, CurveBuilder};
pub use error::EngineError;
pub use etf_pricing::EtfPricer;
pub use market_data_listener::{
    CurveInputUpdate, CurveUpdate, FxRateUpdate, IndexFixingUpdate, InflationFixingUpdate,
    IngestionStatus, MarketDataListener, MarketDataPublisher, MarketDataUpdate, VolSurfaceUpdate,
};
pub use portfolio_analytics::{Portfolio, PortfolioAnalyzer, Position};
pub use pricing_router::{BatchPricingResult, PricingRouter};
pub use runtime::{
    AcceptedUpdate, RuntimeError, RuntimeHealth, RuntimeInput, RuntimeMode, RuntimePhase,
    RuntimePlan, RuntimeServices,
};
pub use scheduler::{EodScheduler, IntervalScheduler, NodeUpdate, ThrottleManager, UpdateSource};

use std::sync::Arc;
use tracing::{debug, warn};

use crate::ports::config::EngineConfig;
use crate::ports::market_data::MarketDataProvider;
use crate::ports::reference_data::ReferenceDataProvider;
use crate::ports::storage::StorageAdapter;

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
    market_data: Arc<MarketDataProvider>,

    /// Auxiliary issuer, rating and ETF sources; bond terms come from storage.
    reference_data: Arc<ReferenceDataProvider>,

    /// Storage adapter, including the authoritative reference repository.
    storage: Arc<StorageAdapter>,

    /// Sole lifecycle and accepted-work owner.
    runtime: runtime::RuntimeController,
    configurations: Option<Arc<configuration::ConfigurationUseCases>>,
}

impl PricingEngine {
    /// Create a new pricing engine.
    pub fn new(
        config: EngineConfig,
        market_data: Arc<MarketDataProvider>,
        reference_data: Arc<ReferenceDataProvider>,
        storage: Arc<StorageAdapter>,
    ) -> Self {
        let concurrency = config.max_concurrent_calcs;
        let calc_graph = Arc::new(CalculationGraph::new());
        let curve_builder = Arc::new(CurveBuilder::new(market_data.clone(), calc_graph.clone()));
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
            runtime: runtime::RuntimeController::new(None, false, concurrency),
            configurations: None,
        }
    }

    /// Start once, restoring exact dependencies and accepted work before readiness.
    pub async fn start(&self) -> Result<(), EngineError> {
        self.runtime.start(self).await
    }

    /// Close admission, drain accepted work, and join all owned tasks and workers.
    pub async fn shutdown(&self) -> Result<(), EngineError> {
        self.runtime.shutdown().await
    }

    /// Separate lifecycle/dependency/data status; never execution permission.
    pub fn runtime_health(&self) -> RuntimeHealth {
        self.runtime.health()
    }

    /// Shared configuration validation, staging and inspection use cases, when explicitly composed.
    pub fn configuration_use_cases(&self) -> Option<&Arc<configuration::ConfigurationUseCases>> {
        self.configurations.as_ref()
    }

    /// Validate/warm then atomically activate; running runtimes accept only reloadable settings.
    /// A committed selection remains committed if runtime application is delayed or unavailable.
    pub async fn activate_configuration(
        &self,
        command: &ports::configuration::ActivationCommand,
        fixtures: &[configuration::ConfigurationFixture],
    ) -> Result<configuration::ConfigurationActivationOutcome, configuration::ConfigurationError>
    {
        let cases =
            self.configurations
                .as_ref()
                .ok_or(configuration::ConfigurationError::Invalid(
                    "configuration use cases are not composed",
                ))?;
        let running = !matches!(
            self.runtime_health().phase,
            RuntimePhase::New | RuntimePhase::Stopped
        );
        let activation = cases.activate(command, fixtures, running).await?;
        let applied = if running {
            self.runtime.refresh_configuration(&activation).await.ok()
        } else {
            None
        };
        Ok(configuration::ConfigurationActivationOutcome {
            activation,
            accepted_generation: applied.as_ref().map(|r| r.generation),
            accepted_activation_revision: applied.and_then(|r| r.activation_revision),
            restart_required: !running,
        })
    }

    /// Observe joined shutdown so transport sessions can close promptly.
    pub fn subscribe_shutdown(&self) -> tokio::sync::watch::Receiver<bool> {
        self.runtime.subscribe_shutdown()
    }

    /// Canonical persisted result states with original last-good timestamps.
    pub fn runtime_results(&self) -> Vec<ports::runtime::PublishedValuation> {
        self.runtime.results()
    }

    /// Transient display stream. Lag is explicit; resync through runtime_results.
    pub fn subscribe_results(
        &self,
    ) -> tokio::sync::broadcast::Receiver<ports::runtime::PublishedValuation> {
        self.runtime.subscribe()
    }

    /// Admit a captured update only after its checkpoint is durable; overload fails explicitly.
    pub async fn submit_update(
        &self,
        event: ports::quote_event::QuoteEvent,
        as_of: convex_core::contracts::UtcTimestamp,
        known_at: convex_core::contracts::UtcTimestamp,
    ) -> Result<AcceptedUpdate, RuntimeError> {
        self.runtime.submit(event, as_of, known_at).await
    }

    /// Run a complete supplied context on the bounded interactive CPU budget; shutdown joins it.
    pub async fn evaluate_context(
        &self,
        snapshot: ports::snapshot::MarketSnapshot,
        context: context::ValuationContext,
        request: convex_core::contracts::RequestId,
        calculated_at: convex_core::contracts::UtcTimestamp,
    ) -> Result<snapshot::SnapshotValuationResult, ports::TraitError> {
        if snapshot.content().configuration_capture.is_some() {
            let cases = self
                .configurations
                .as_ref()
                .ok_or(ports::TraitError::Unsupported(
                    "supplied activation requires configuration composition",
                ))?;
            cases.verify_snapshot_selection(&snapshot).await.map_err(|_| ports::TraitError::Conflict("snapshot activation or effective policies differ from retained configuration"))?;
        }
        self.runtime
            .evaluate_context(snapshot, context, request, calculated_at)
            .await
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

    /// Get auxiliary issuer, rating and ETF sources.
    pub fn reference_data(&self) -> &Arc<ReferenceDataProvider> {
        &self.reference_data
    }

    /// The sole authoritative source for reference reads and mutations.
    pub fn reference_repository(
        &self,
    ) -> &Arc<dyn ports::reference_repository::ReferenceRepository> {
        &self.storage.bonds
    }

    /// Read full reference terms and the exact version consumed by a pricing request.
    pub async fn reference_for_pricing(
        &self,
        id: &convex_core::ids::InstrumentId,
    ) -> Result<
        Option<ports::reference_repository::ReferenceRecord>,
        ports::reference_repository::ReferenceError,
    > {
        let time = convex_core::contracts::UtcTimestamp::from_datetime(chrono::Utc::now())
            .map_err(|_| ports::reference_repository::ReferenceError::Unavailable)?;
        self.reference_at_for_pricing(
            id,
            ports::reference_master::ReferenceLookup {
                as_of: time,
                known_at: time,
            },
        )
        .await
    }

    /// Validate a supplied economic/known-at reference context for deterministic application use.
    pub async fn reference_at_for_pricing(
        &self,
        id: &convex_core::ids::InstrumentId,
        lookup: ports::reference_master::ReferenceLookup,
    ) -> Result<
        Option<ports::reference_repository::ReferenceRecord>,
        ports::reference_repository::ReferenceError,
    > {
        Ok(self
            .storage
            .bonds
            .for_valuation(id, lookup)
            .await?
            .map(|resolved| resolved.record))
    }

    /// Validate inline terms and their exact resource pins before service pricing.
    pub async fn validate_inline_reference(
        &self,
        bond: &ports::reference_data::BondReferenceData,
    ) -> Result<(), ports::reference_repository::ReferenceError> {
        let time = convex_core::contracts::UtcTimestamp::from_datetime(chrono::Utc::now())
            .map_err(|_| ports::reference_repository::ReferenceError::Unavailable)?;
        self.storage
            .bonds
            .resources_for_valuation(
                bond,
                ports::reference_master::ReferenceLookup {
                    as_of: time,
                    known_at: time,
                },
            )
            .await?;
        Ok(())
    }

    /// Commit a reference use case before invalidating dependent pricing work.
    pub async fn commit_reference(
        &self,
        command: &ports::reference_repository::ReferenceCommand,
        recorded_at: convex_core::contracts::UtcTimestamp,
    ) -> Result<
        ports::reference_repository::ReferenceCommit,
        ports::reference_repository::ReferenceError,
    > {
        let result = self.storage.bonds.commit(command, recorded_at).await?;
        for record in &result.records {
            let node = NodeId::BondPrice {
                instrument_id: record.instrument_id.clone(),
            };
            if self.runtime_health().mode.is_none() && self.calc_graph.node_spec(&node).is_some() {
                if let Err(error) = self.calc_graph.invalidate(&node) {
                    warn!("Reference invalidation failed: {error}");
                }
            }
        }
        Ok(result)
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
        instrument_id: &convex_core::ids::InstrumentId,
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

        if let Err(error) = self
            .calc_graph
            .publish_input(&node_id, NodeValue::Quote { bid, ask, mid })
        {
            warn!("Quote input publication failed: {error}");
        }

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
    pub fn on_curve_update(&self, curve_id: &convex_core::ids::CurveId, curve: &BuiltCurve) {
        let node_id = NodeId::Curve {
            curve_id: curve_id.clone(),
        };

        // Update curve in cache
        if let Err(error) = self.calc_graph.publish_input(
            &node_id,
            NodeValue::Curve {
                points: curve.to_points(),
            },
        ) {
            warn!("Curve input publication failed: {error}");
        }

        debug!("Curve update for {}", curve_id);
    }

    /// Handle an index fixing update (for FRNs).
    ///
    /// This method is called when a floating rate index fixing arrives.
    /// It invalidates the index fixing node, which propagates to all FRNs
    /// that reference this index.
    pub fn on_index_fixing(
        &self,
        index: &convex_core::ids::FloatingRateIndex,
        date: convex_core::Date,
        rate: rust_decimal::Decimal,
    ) {
        let node_id = NodeId::IndexFixing {
            index: index.clone(),
            date,
        };

        if let Err(error) = self
            .calc_graph
            .publish_input(&node_id, NodeValue::IndexFixing { rate })
        {
            warn!("Input publication failed: {error}");
        }

        debug!("Index fixing for {} on {}: {}", index, date, rate);
    }

    /// Handle an inflation fixing update (for ILBs).
    ///
    /// This method is called when an inflation index fixing arrives.
    /// It invalidates the inflation fixing node, which propagates to all ILBs.
    pub fn on_inflation_fixing(
        &self,
        index: &convex_core::ids::InflationIndex,
        month: convex_core::ids::YearMonth,
        value: rust_decimal::Decimal,
    ) {
        let node_id = NodeId::InflationFixing {
            index: index.clone(),
            month,
        };

        if let Err(error) = self
            .calc_graph
            .publish_input(&node_id, NodeValue::InflationFixing { value })
        {
            warn!("Input publication failed: {error}");
        }

        debug!("Inflation fixing for {} ({}): {}", index, month, value);
    }

    /// Handle an FX rate update.
    ///
    /// This method is called when an FX rate is updated.
    /// It invalidates the FX rate node, which propagates to cross-currency
    /// portfolio calculations.
    pub fn on_fx_rate_update(
        &self,
        pair: &convex_core::ids::CurrencyPair,
        mid: rust_decimal::Decimal,
    ) {
        let node_id = NodeId::FxRate { pair: pair.clone() };

        if let Err(error) = self
            .calc_graph
            .publish_input(&node_id, NodeValue::FxRate { mid })
        {
            warn!("Input publication failed: {error}");
        }

        debug!("FX rate update for {}: {}", pair, mid);
    }

    /// Register a bond for reactive pricing.
    ///
    /// This sets up the node in the calc graph with dependencies on
    /// the relevant curves and quote.
    pub fn register_bond(
        &self,
        instrument_id: convex_core::ids::InstrumentId,
        discount_curve: convex_core::ids::CurveId,
        benchmark_curve: Option<convex_core::ids::CurveId>,
        config: crate::ports::config::NodeConfig,
    ) -> Result<u64, GraphError> {
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

        // Include only missing roots; preserve existing curve/calculation definitions.
        let mut nodes: Vec<_> = deps
            .iter()
            .filter(|id| self.calc_graph.node_spec(id).is_none())
            .cloned()
            .map(NodeSpec::input)
            .collect();
        let mut roots = std::collections::HashSet::new();
        nodes.retain(|node| roots.insert(node.id.clone()));
        let mut seen = std::collections::HashSet::new();
        deps.retain(|id| seen.insert(id.clone()));
        nodes.push(NodeSpec::calculation(bond_node, deps).with_config(config));
        self.calc_graph.upsert_nodes(nodes)
    }

    /// Register a currently unsupported ETF aggregation with explicit scheduling policy.
    pub fn register_etf_inav(&self, etf_id: convex_core::ids::EtfId) -> Result<u64, GraphError> {
        self.calc_graph.upsert_nodes(vec![NodeSpec::calculation(
            NodeId::EtfInav { etf_id },
            vec![],
        )
        .with_config(crate::ports::config::NodeConfig::etf_inav())])
    }
    /// Register end-of-day ETF work; mathematical holdings integration remains explicit.
    pub fn register_etf_nav(&self, etf_id: convex_core::ids::EtfId) -> Result<u64, GraphError> {
        self.calc_graph.upsert_nodes(vec![NodeSpec::calculation(
            NodeId::EtfNav { etf_id },
            vec![],
        )
        .with_config(crate::ports::config::NodeConfig::etf_nav())])
    }
    /// Register portfolio work; missing aggregation is never cached as a numerical result.
    pub fn register_portfolio(
        &self,
        portfolio_id: convex_core::ids::PortfolioId,
    ) -> Result<u64, GraphError> {
        self.calc_graph.upsert_nodes(vec![NodeSpec::calculation(
            NodeId::Portfolio { portfolio_id },
            vec![],
        )
        .with_config(crate::ports::config::NodeConfig::portfolio())])
    }
}

/// Hexagonal port traits, re-exported from the [`convex_ports`] crate so that
/// `convex_engine::ports::*` paths keep working while adapters depend only on
/// the lightweight ports crate (not the whole engine).
pub use convex_ports as ports;
