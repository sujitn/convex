//! # Convex Engine
//!
//! The reactive pricing engine for Convex.
//!
//! This crate provides:
//! - [`CalculationGraph`]: Dependency tracking and reactive recalculation
//! - [`PricingRouter`]: Model selection and batch pricing
//! - [`curve_resolution`]: Stored curve recipes resolved at an explicit cutoff
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
//! CurveStore ──┬─> curve_resolution ─> Curves
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
pub mod calibration_replay;
pub mod core_workflow;
pub mod curve_builder;
pub mod curve_replay;
pub mod curve_resolution;
pub mod error;
pub mod etf_pricing;
pub mod floating_valuation;
pub mod market_ingestion;
pub mod optional_valuation;
pub mod portfolio_analytics;
pub mod pricing_router;
pub mod request_valuation;
pub mod runtime;
pub mod snapshot;
pub mod telemetry;

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
pub use curve_builder::BuiltCurve;
pub use error::EngineError;
pub use etf_pricing::EtfPricer;
pub use portfolio_analytics::{
    request_bond_quote, runtime_bond_quote, Portfolio, PortfolioAnalyzer, Position,
};
pub use pricing_router::{BatchPricingResult, PricingRouter};
pub use runtime::{
    AcceptedUpdate, RuntimeError, RuntimeHealth, RuntimeInput, RuntimeMode, RuntimePhase,
    RuntimePlan, RuntimeServices,
};

use std::sync::Arc;

use crate::ports::config::EngineConfig;
use crate::ports::storage::StorageAdapter;

/// Curves one bond prices on under a request's valuation context.
#[derive(Debug, Clone)]
pub struct PricingCurves {
    /// Configured discount curve.
    pub discount: BuiltCurve,
    /// Configured swap benchmark, when the configuration names one.
    pub benchmark: Option<BuiltCurve>,
}

/// The approved valuation context of one request: an activated configuration's exact
/// pricing version and the curves its roles name, resolved once at the request cutoff.
/// Government and FRN reference roles are not consumed here, as in snapshot valuation.
pub struct RequestContext {
    id: convex_core::contracts::SourceId,
    activation_revision: convex_core::contracts::Version,
    config: ports::storage::BondPricingConfig,
    resolved: curve_resolution::ResolvedCurves,
}

impl RequestContext {
    /// The approved pricing configuration.
    pub fn config(&self) -> &ports::storage::BondPricingConfig {
        &self.config
    }

    /// Revision of the activation that approved the configuration.
    pub fn activation_revision(&self) -> convex_core::contracts::Version {
        self.activation_revision
    }

    /// Every consumed (curve, version) pin, roots and dependencies.
    pub fn pins(&self) -> impl Iterator<Item = (&str, convex_core::contracts::Version)> {
        self.resolved.pins()
    }

    /// Curves for `bond`, which the configuration must cover; each curve must fit
    /// its role and the bond's currency.
    pub fn curves_for(
        &self,
        bond: &ports::reference_data::BondReferenceData,
    ) -> Result<PricingCurves, curve_resolution::CurveResolutionError> {
        use curve_resolution::CurveResolutionError;
        if !self.config.applies_to.matches(bond) {
            return Err(CurveResolutionError::NotCovered(self.id.clone()));
        }
        let checked = |id: &convex_core::ids::CurveId, discount: bool| {
            let native = self.resolved.native(id)?;
            if !curve_resolution::pricing_role_matches(native.recipe(), discount, bond.currency) {
                return Err(CurveResolutionError::Role(id.clone()));
            }
            self.resolved.curve(id)
        };
        let roles = &self.config.analytics_curves;
        Ok(PricingCurves {
            discount: checked(&roles.discount_curve, true)?,
            benchmark: roles
                .swap_curve
                .as_ref()
                .map(|id| checked(id, false))
                .transpose()?,
        })
    }
}

/// The main pricing engine.
pub struct PricingEngine {
    /// Engine configuration
    config: EngineConfig,

    /// Calculation graph
    calc_graph: Arc<CalculationGraph>,

    /// Pricing router
    pricing_router: Arc<PricingRouter>,

    /// ETF pricer
    etf_pricer: Arc<EtfPricer>,

    /// Portfolio analyzer
    portfolio_analyzer: Arc<PortfolioAnalyzer>,

    /// Storage adapter, including the authoritative reference repository.
    storage: Arc<StorageAdapter>,

    /// Sole lifecycle and accepted-work owner.
    runtime: runtime::RuntimeController,
    configurations: Option<Arc<configuration::ConfigurationUseCases>>,
    /// Request-time valuation through the composed runtime (D57); none without one.
    request: Option<request_valuation::RequestValuer>,
}

impl PricingEngine {
    /// Create a new pricing engine.
    pub fn new(config: EngineConfig, storage: Arc<StorageAdapter>) -> Self {
        let concurrency = config.max_concurrent_calcs;
        let calc_graph = Arc::new(CalculationGraph::new());
        let pricing_router = Arc::new(PricingRouter::new());
        let etf_pricer = Arc::new(EtfPricer::new());
        let portfolio_analyzer = Arc::new(PortfolioAnalyzer::new());

        Self {
            config,
            calc_graph,
            pricing_router,
            etf_pricer,
            portfolio_analyzer,
            storage,
            runtime: runtime::RuntimeController::new(None, false, concurrency),
            configurations: None,
            request: None,
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

    /// Validate/warm then atomically activate. A running runtime applies only a hot
    /// selection; a restart-required one stays committed and applies at the next start.
    /// A committed selection remains committed if runtime application is delayed or unavailable.
    pub async fn activate_configuration(
        &self,
        command: &ports::configuration::ActivationCommand,
        fixtures: &[configuration::ConfigurationFixture],
    ) -> Result<configuration::ConfigurationActivationOutcome, configuration::ConfigurationError>
    {
        self.apply_activation(self.composed_configurations()?, command, fixtures)
            .await
    }

    /// As [`Self::activate_configuration`], validating under the capabilities the next start
    /// composes for the staged document.
    pub async fn activate_configuration_with(
        &self,
        capabilities: configuration::ConfigurationCapabilities,
        command: &ports::configuration::ActivationCommand,
        fixtures: &[configuration::ConfigurationFixture],
    ) -> Result<configuration::ConfigurationActivationOutcome, configuration::ConfigurationError>
    {
        let cases = self
            .composed_configurations()?
            .with_capabilities(capabilities);
        self.apply_activation(&cases, command, fixtures).await
    }

    fn composed_configurations(
        &self,
    ) -> Result<&configuration::ConfigurationUseCases, configuration::ConfigurationError> {
        self.configurations
            .as_deref()
            .ok_or(configuration::ConfigurationError::Invalid(
                "configuration use cases are not composed",
            ))
    }

    async fn apply_activation(
        &self,
        cases: &configuration::ConfigurationUseCases,
        command: &ports::configuration::ActivationCommand,
        fixtures: &[configuration::ConfigurationFixture],
    ) -> Result<configuration::ConfigurationActivationOutcome, configuration::ConfigurationError>
    {
        let running = !matches!(
            self.runtime_health().phase,
            RuntimePhase::New | RuntimePhase::Stopped
        );
        let hot = command.mode == ports::configuration::ReloadClass::Hot;
        let activation = cases.activate(command, fixtures).await?;
        // Applied only when the runtime accepted this exact revision; a running runtime keeps
        // its selection when the active one needs a restart.
        let applied = if running && hot {
            self.runtime.refresh_configuration(&activation).await.ok()
        } else {
            None
        }
        .filter(|r| r.activation_revision == Some(activation.revision));
        // The commit stands either way; a runtime that did not accept it applies it at the
        // next start.
        Ok(configuration::ConfigurationActivationOutcome {
            restart_required: applied.is_none(),
            activation,
            accepted_generation: applied.as_ref().map(|r| r.generation),
            accepted_activation_revision: applied.and_then(|r| r.activation_revision),
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

    /// Settlement date of the composed runtime plan; every runtime result is valued for it.
    pub fn runtime_settlement_date(&self) -> Option<convex_core::contracts::BusinessDate> {
        self.runtime.settlement_date()
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

    /// The single durable curve owner.
    pub fn curve_store(&self) -> &Arc<dyn ports::storage::CurveStore> {
        &self.storage.curves
    }

    /// The configuration a request prices under when it names none: the one the
    /// runtime runs. A server without a runtime has no default.
    pub fn default_valuation_configuration(&self) -> Option<&convex_core::contracts::SourceId> {
        self.runtime.configuration_id()
    }

    /// The approved valuation context of a request at `known_at`: the active
    /// selection of `configuration` (or of the default) and the curves its roles
    /// name, current at the cutoff. Only an activation approves a configuration;
    /// a staged version never prices. The configuration is the one active now, and
    /// the cutoff governs the curve reads.
    pub async fn request_context(
        &self,
        configuration: Option<&convex_core::contracts::SourceId>,
        known_at: convex_core::contracts::UtcTimestamp,
    ) -> Result<RequestContext, curve_resolution::CurveResolutionError> {
        let started = std::time::Instant::now();
        let context = self.resolve_request_context(configuration, known_at).await;
        telemetry::observe(telemetry::REQUEST_CONTEXT_SECONDS, started);
        context
    }

    async fn resolve_request_context(
        &self,
        configuration: Option<&convex_core::contracts::SourceId>,
        known_at: convex_core::contracts::UtcTimestamp,
    ) -> Result<RequestContext, curve_resolution::CurveResolutionError> {
        use curve_resolution::CurveResolutionError;
        let id = configuration
            .or(self.default_valuation_configuration())
            .ok_or(CurveResolutionError::NoConfiguration)?;
        let active = self
            .storage
            .configuration
            .active(id)
            .await?
            .ok_or_else(|| CurveResolutionError::NotActivated(id.clone()))?;
        let config = active.staged.document.pricing;
        let roles = &config.analytics_curves;
        let mut roots = vec![roles.discount_curve.clone()];
        roots.extend(roles.swap_curve.clone());
        let resolved =
            curve_resolution::resolve_curves(self.storage.curves.as_ref(), &roots, known_at)
                .await?;
        Ok(RequestContext {
            id: id.clone(),
            activation_revision: active.activation.revision,
            config,
            resolved,
        })
    }

    /// Whether a composed runtime values callables and floating rate notes at request
    /// time (D57, D65).
    pub fn values_requests(&self) -> bool {
        self.request.is_some()
    }

    /// The stored terms that inline `bond` names, current at `known_at`. Callables and
    /// floating rate notes are valued on stored terms only, so inline terms must equal
    /// them (D62).
    pub async fn stored_terms(
        &self,
        bond: &ports::reference_data::BondReferenceData,
        known_at: convex_core::contracts::UtcTimestamp,
    ) -> Result<
        ports::reference_repository::ReferenceRecord,
        request_valuation::RequestValuationError,
    > {
        let lookup = ports::reference_master::ReferenceLookup {
            as_of: known_at,
            known_at,
        };
        match self
            .reference_at_for_pricing(&bond.instrument_id, lookup)
            .await?
        {
            Some(record) if record.bond.as_ref() == Some(bond) => Ok(record),
            _ => Err(request_valuation::RequestValuationError::UnstoredTerms),
        }
    }

    /// Value a stored callable or floating rate note at request time (D57): an
    /// unpublished one-instrument snapshot under `context` at `known_at`, with `mark` as
    /// a labelled `request` quote, valued through [`Self::evaluate_context`] as the stream
    /// values a bond. Without a mark every measure fails `MISSING_MARKET_DATA`. Nothing
    /// is stored.
    pub async fn value_request(
        &self,
        context: &RequestContext,
        reference: &ports::reference_repository::ReferenceRecord,
        settlement_date: convex_core::contracts::BusinessDate,
        known_at: convex_core::contracts::UtcTimestamp,
        mark: Option<rust_decimal::Decimal>,
    ) -> Result<request_valuation::RequestValuation, request_valuation::RequestValuationError> {
        use request_valuation::RequestValuationError;
        let valuer = self
            .request
            .as_ref()
            .ok_or(RequestValuationError::NoRuntime)?;
        let (snapshot, valuation, curves) = valuer
            .prepare(
                &self.storage,
                context,
                reference,
                settlement_date,
                known_at,
                mark,
            )
            .await?;
        let invalid = |_| ports::TraitError::InvalidInput("invalid request identity");
        // As the stream names its valuations, by context.
        let request =
            convex_core::contracts::RequestId::new(format!("request-{}", valuation.cache_key()))
                .map_err(invalid)?;
        let calculated = convex_core::contracts::UtcTimestamp::from_datetime(chrono::Utc::now())
            .map_err(invalid)?
            .max(known_at);
        let result = self
            .evaluate_context(snapshot, valuation, request, calculated)
            .await?;
        Ok(request_valuation::RequestValuation { result, curves })
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

    /// Commit a reference use case through the authoritative repository.
    pub async fn commit_reference(
        &self,
        command: &ports::reference_repository::ReferenceCommand,
        recorded_at: convex_core::contracts::UtcTimestamp,
    ) -> Result<
        ports::reference_repository::ReferenceCommit,
        ports::reference_repository::ReferenceError,
    > {
        self.storage.bonds.commit(command, recorded_at).await
    }
}

/// Hexagonal port traits, re-exported from the [`convex_ports`] crate so that
/// `convex_engine::ports::*` paths keep working while adapters depend only on
/// the lightweight ports crate (not the whole engine).
pub use convex_ports as ports;
