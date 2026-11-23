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

#[cfg(test)]
#[path = "../tests/support/products.rs"]
mod products;

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
pub mod level_pricing;
pub mod limits;
pub mod market_ingestion;
pub mod optional_valuation;
pub mod portfolio_analytics;
pub mod pricing_router;
pub mod request_valuation;
pub mod runtime;
pub mod snapshot;
pub mod telemetry;
pub mod workout_valuation;

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
    RuntimePlan, RuntimeResultsPage, RuntimeServices, MAX_RUNTIME_INSTRUMENTS,
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

/// The most curve sets one request context resolves. Each set is the roots a product
/// path reads from the configuration's roles: a bullet's discount and swap curves, a
/// callable's discount curve, and a note's discount and FRN reference curves.
const MAX_CURVE_SETS: usize = 3;

/// One curve set's resolution, kept by its context, failure included.
type CurveRead =
    Result<Arc<curve_resolution::ResolvedCurves>, curve_resolution::CurveResolutionError>;

/// The approved valuation context of one request: an activated configuration's exact
/// pricing version, at the request cutoff. Each product path reads the curves it needs
/// at first use, and the context keeps every set it resolved, a failed one included, so
/// a request resolves each set once. It also holds every curve record it read, and
/// a later set takes a held curve from it, so a request prices each curve on one
/// revision. The government role is not consumed here, as in snapshot valuation.
pub struct RequestContext {
    id: convex_core::contracts::SourceId,
    configuration_version: convex_core::contracts::Version,
    activation_revision: convex_core::contracts::Version,
    config: ports::storage::BondPricingConfig,
    client_tiers: Vec<ports::configuration::ClientTierWidth>,
    known_at: convex_core::contracts::UtcTimestamp,
    curves: Arc<dyn ports::storage::CurveStore>,
    /// Each curve set read, by its roots, in first-use order.
    reads: Vec<(Vec<convex_core::ids::CurveId>, CurveRead)>,
    /// Every stored curve record a set read, the revision each later set prices.
    held: Vec<ports::snapshot::SnapshotCurveRecipe>,
    /// Each benchmark set a bond's rule named, as read at `known_at`, failures included.
    benchmark_sets: level_pricing::BenchmarkSetReads,
}

impl RequestContext {
    /// The reference cutoff: the request's `known_at`, as of the same instant.
    pub fn lookup(&self) -> ports::reference_master::ReferenceLookup {
        ports::reference_master::ReferenceLookup {
            as_of: self.known_at,
            known_at: self.known_at,
        }
    }

    /// The approved pricing configuration.
    pub fn config(&self) -> &ports::storage::BondPricingConfig {
        &self.config
    }

    /// Revision of the activation that approved the configuration.
    pub fn activation_revision(&self) -> convex_core::contracts::Version {
        self.activation_revision
    }

    /// The approved configuration's id.
    pub fn configuration_id(&self) -> &convex_core::contracts::SourceId {
        &self.id
    }

    /// The approved configuration's version.
    pub fn configuration_version(&self) -> convex_core::contracts::Version {
        self.configuration_version
    }

    /// The factor the approved configuration sets on the width for `tier`, if it lists
    /// the tier.
    pub fn width_factor(
        &self,
        tier: &ports::rfq::ClientTier,
    ) -> Option<convex_core::types::WidthFactor> {
        self.client_tiers
            .iter()
            .find(|entry| entry.client_tier == *tier)
            .map(|entry| entry.width_factor)
    }

    /// Every (curve, version) pin the request read, roots and dependencies, each once,
    /// in first-use order. A set that failed to resolve priced nothing and lists nothing.
    pub fn pins(&self) -> Vec<(&str, convex_core::contracts::Version)> {
        let mut pins = Vec::new();
        for pin in self
            .reads
            .iter()
            .filter_map(|(_, read)| read.as_ref().ok())
            .flat_map(|resolved| resolved.pins())
        {
            if !pins.contains(&pin) {
                pins.push(pin);
            }
        }
        pins
    }

    /// The curves `roots` name, with their dependencies, at the cutoff. A set is
    /// resolved at its first read and kept, failure included. A curve an earlier set
    /// read is taken from the context, not the store.
    pub(crate) async fn read(&mut self, roots: &[convex_core::ids::CurveId]) -> CurveRead {
        if let Some((_, read)) = self.reads.iter().find(|(set, _)| set.as_slice() == roots) {
            return read.clone();
        }
        if self.reads.len() >= MAX_CURVE_SETS {
            return Err(ports::TraitError::CapacityExceeded.into());
        }
        let read = curve_resolution::resolve_held(
            self.curves.as_ref(),
            roots,
            self.known_at,
            &mut self.held,
        )
        .await
        .map(Arc::new);
        self.reads.push((roots.to_vec(), read.clone()));
        read
    }

    /// Curves for a bullet `bond`, which the configuration must cover: its discount
    /// curve and swap benchmark. Each curve must fit its role and the bond's currency.
    pub async fn curves_for(
        &mut self,
        bond: &ports::reference_data::BondReferenceData,
    ) -> Result<PricingCurves, curve_resolution::CurveResolutionError> {
        use curve_resolution::CurveResolutionError;
        if !self.config.applies_to.matches(bond) {
            return Err(CurveResolutionError::NotCovered(self.id.clone()));
        }
        let roles = &self.config.analytics_curves;
        let mut roots = vec![roles.discount_curve.clone()];
        roots.extend(roles.swap_curve.clone());
        let resolved = self.read(&roots).await?;
        let checked = |id: &convex_core::ids::CurveId, discount: bool| {
            let native = resolved.native(id)?;
            if !curve_resolution::pricing_role_matches(native.recipe(), discount, bond.currency) {
                return Err(CurveResolutionError::Role(id.clone()));
            }
            resolved.curve(id)
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
    /// Request-time valuation through the composed runtime; none without one.
    request: Option<request_valuation::RequestValuer>,
    /// The master keys of each committed reference command, for observers such as the
    /// limit monitor.
    commits: tokio::sync::broadcast::Sender<ReferenceCommitNotice>,
    /// Benchmark yields shared by request-time level quotes.
    benchmark_yield_cache: level_pricing::BenchmarkYieldCache,
}

/// The master keys one reference commit wrote. A transient notice: a receiver that lags
/// rereads what it watches.
pub type ReferenceCommitNotice = Arc<[ports::reference_master::MasterKey]>;

/// Notices a slow observer may fall behind by before it must reread.
const COMMIT_NOTICES: usize = 64;

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
            commits: tokio::sync::broadcast::channel(COMMIT_NOTICES).0,
            benchmark_yield_cache: Default::default(),
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

    /// Canonical persisted result states with original last-good timestamps. Copies every
    /// record; a reader of some instruments uses [`Self::runtime_results_of`], and a
    /// reader of all of them pages with [`Self::runtime_results_page`].
    pub fn runtime_results(&self) -> Vec<ports::runtime::PublishedValuation> {
        self.runtime.results()
    }

    /// The current records of the listed instruments, shared and in the runtime's order,
    /// all from one table. An instrument outside the plan, or not yet valued, has none.
    pub fn runtime_results_of<'a>(
        &self,
        instruments: impl IntoIterator<Item = &'a str>,
    ) -> Vec<Arc<ports::runtime::PublishedValuation>> {
        self.runtime.results_of(instruments)
    }

    /// At most `limit` (at least one) current records in instrument ID order, after the
    /// instrument `after`. Pages are read at different times, so a reader keeps the newer
    /// record by `(runtime_id, revision)`.
    pub fn runtime_results_page(&self, after: Option<&str>, limit: usize) -> RuntimeResultsPage {
        self.runtime.results_page(after, limit)
    }

    /// Settlement date of the composed runtime plan; every runtime result is valued for it.
    pub fn runtime_settlement_date(&self) -> Option<convex_core::contracts::BusinessDate> {
        self.runtime.settlement_date()
    }

    /// Transient display stream of shared records. Lag is explicit; resync through
    /// runtime_results_page or runtime_results_of.
    pub fn subscribe_results(
        &self,
    ) -> tokio::sync::broadcast::Receiver<Arc<ports::runtime::PublishedValuation>> {
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
    /// selection of `configuration` (or of the default). Only an activation approves a
    /// configuration; a staged version never prices. The configuration is the one
    /// active now, and the cutoff governs the curve reads, which each product path
    /// makes at first use.
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
        Ok(RequestContext {
            id: id.clone(),
            configuration_version: active.activation.command.configuration_version,
            activation_revision: active.activation.revision,
            config: active.staged.document.pricing,
            client_tiers: active.staged.document.client_tiers,
            known_at,
            curves: self.storage.curves.clone(),
            reads: Vec::new(),
            held: Vec::new(),
            benchmark_sets: level_pricing::BenchmarkSetReads::default(),
        })
    }

    /// Whether a composed runtime values callables and floating rate notes at request
    /// time.
    pub fn values_requests(&self) -> bool {
        self.request.is_some()
    }

    /// The stored reference that inline `bond` names, current at `known_at`, with its
    /// pricing rule. Callables and floating rate notes are valued on stored terms only,
    /// so inline terms must equal them.
    pub async fn stored_terms(
        &self,
        bond: &ports::reference_data::BondReferenceData,
        known_at: convex_core::contracts::UtcTimestamp,
    ) -> Result<
        ports::reference_master::ResolvedBondReference,
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
            Some(resolved) if resolved.record.bond.as_ref() == Some(bond) => Ok(resolved),
            _ => Err(request_valuation::RequestValuationError::UnstoredTerms),
        }
    }

    /// The pricing rule of the stored bond inline `bond` names, at `context`'s cutoff,
    /// when its stored terms then equal the inline ones; none otherwise, and the bond is
    /// quoted by price. The rule is read first, so a bond with none reads nothing more;
    /// only the terms are compared, so inline terms that differ never need the stored
    /// version's resources to resolve.
    pub async fn rule_for_inline_terms(
        &self,
        context: &RequestContext,
        bond: &ports::reference_data::BondReferenceData,
    ) -> Result<
        Option<ports::level_pricing::RequestPricingRule>,
        ports::reference_repository::ReferenceError,
    > {
        // An id no stored bond can have names no stored rule.
        if convex_core::contracts::InstrumentId::new(bond.instrument_id.as_str()).is_err() {
            return Ok(None);
        }
        let repository = &self.storage.bonds;
        let lookup = context.lookup();
        let Some(rule) = repository
            .pricing_rule_for_request(&bond.instrument_id, lookup)
            .await?
        else {
            return Ok(None);
        };
        let stored = repository.get_at(&bond.instrument_id, lookup).await?;
        Ok(stored
            .is_some_and(|stored| stored.bond.as_ref() == Some(bond))
            .then_some(rule))
    }

    /// Value a stored callable or floating rate note at request time: an
    /// unpublished one-instrument snapshot under `context` at `known_at`, with `mark` as
    /// a labelled `request` quote, valued through [`Self::evaluate_context`] as the stream
    /// values a bond. The mark is a clean price, or the level of a bond whose model values
    /// its rule's level itself: a callable's yield to worst on its workouts, or a floating
    /// rate note's discount margin. Without a mark every measure fails
    /// `MISSING_MARKET_DATA`. Nothing is stored.
    pub async fn value_request(
        &self,
        context: &mut RequestContext,
        reference: &ports::reference_master::ResolvedBondReference,
        settlement_date: convex_core::contracts::BusinessDate,
        known_at: convex_core::contracts::UtcTimestamp,
        mark: Option<level_pricing::QuotedInput>,
    ) -> Result<request_valuation::RequestValuation, request_valuation::RequestValuationError> {
        use request_valuation::RequestValuationError;
        let valuer = self
            .request
            .as_ref()
            .ok_or(RequestValuationError::NoRuntime)?;
        let (snapshot, valuation, curves) = valuer
            .prepare(context, reference, settlement_date, known_at, mark)
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
        let lookup = ports::reference_master::ReferenceLookup {
            as_of: time,
            known_at: time,
        };
        Ok(self
            .reference_at_for_pricing(id, lookup)
            .await?
            .map(|resolved| resolved.record))
    }

    /// A stored bond's reference at `lookup`, resolved for valuation: its terms, their
    /// resources and its pricing rule's pin.
    pub async fn reference_at_for_pricing(
        &self,
        id: &convex_core::ids::InstrumentId,
        lookup: ports::reference_master::ReferenceLookup,
    ) -> Result<
        Option<ports::reference_master::ResolvedBondReference>,
        ports::reference_repository::ReferenceError,
    > {
        self.storage.bonds.for_valuation(id, lookup).await
    }

    /// The rule a stored bond's reference was resolved with, `pin`, as a request reads
    /// it: with its width, read at the pinned version.
    pub async fn pinned_rule_for_request(
        &self,
        instrument_id: &convex_core::ids::InstrumentId,
        pin: Option<ports::level_pricing::ResolvedPricingRule>,
    ) -> Result<
        Option<ports::level_pricing::RequestPricingRule>,
        ports::reference_repository::ReferenceError,
    > {
        match pin {
            None => Ok(None),
            Some(pin) => self
                .storage
                .bonds
                .pinned_pricing_rule_for_request(instrument_id, &pin)
                .await
                .map(Some),
        }
    }

    /// A bond's rule at request time, for every route that quotes a stored bond: `rule`
    /// is its pin and width, from [`Self::pinned_rule_for_request`] or
    /// [`Self::rule_for_inline_terms`], and the set it names is read at `context`'s
    /// cutoff, with each security's pinned terms and Live clean price mid. The context
    /// keeps each set it read, so a batch reads one set once. A rule that no longer
    /// applies to `bond`, one revised since the rule was committed, is refused before
    /// anything is read.
    pub async fn rule_in_force(
        &self,
        context: &mut RequestContext,
        bond: &ports::reference_data::BondReferenceData,
        rule: Option<ports::level_pricing::RequestPricingRule>,
    ) -> Result<Option<level_pricing::PricingRuleInForce>, level_pricing::LevelError> {
        let Some(ports::level_pricing::RequestPricingRule { pin: rule, width }) = rule else {
            return Ok(None);
        };
        if !rule.applies_to(bond) {
            return Err(level_pricing::LevelError::Refused(
                ports::level_pricing::RULE_DOES_NOT_APPLY.into(),
            ));
        }
        let benchmark_set = match &rule.benchmark_set_id {
            None => None,
            Some(id) => Some(match context.benchmark_sets.get(id) {
                Some(read) => read?,
                None => {
                    let read = self.benchmark_set_quotes(id, context.lookup()).await;
                    context.benchmark_sets.keep(id.clone(), read)?
                }
            }),
        };
        Ok(Some(level_pricing::PricingRuleInForce {
            rule,
            width,
            benchmark_set,
        }))
    }

    /// [`Self::rule_in_force`] for a route that quotes mid only, such as the calculator:
    /// from `pin`, the rule a stored bond's reference was resolved with, and the set it
    /// names. The rule's width is not read.
    pub async fn rule_in_force_mid_only(
        &self,
        context: &mut RequestContext,
        bond: &ports::reference_data::BondReferenceData,
        pin: Option<ports::level_pricing::ResolvedPricingRule>,
    ) -> Result<Option<level_pricing::PricingRuleInForce>, level_pricing::LevelError> {
        let rule = pin.map(|pin| ports::level_pricing::RequestPricingRule { pin, width: None });
        self.rule_in_force(context, bond, rule).await
    }

    /// The level stage at request time under `in_force`, from [`Self::rule_in_force`],
    /// sharing each benchmark's solved yield with every request quoted to it. With no
    /// rule a price is taken as given.
    pub fn price_in_force(
        &self,
        bond: &ports::reference_data::BondReferenceData,
        in_force: Option<&level_pricing::PricingRuleInForce>,
        input: level_pricing::QuotedInput,
        settlement: convex_core::Date,
        discount: Option<&BuiltCurve>,
    ) -> Result<level_pricing::RulePricing, level_pricing::LevelError> {
        level_pricing::price_under_rule(
            bond,
            in_force,
            input,
            settlement,
            discount,
            &self.benchmark_yield_cache,
        )
    }

    /// [`Self::price_in_force`] for a trade of `size`: the rule's width for its size,
    /// scaled by its client tier's factor. A size beyond the schedule's last bucket, or a
    /// scaled width past its unit's widest, is refused.
    pub fn price_for_trade(
        &self,
        bond: &ports::reference_data::BondReferenceData,
        in_force: Option<&level_pricing::PricingRuleInForce>,
        input: level_pricing::QuotedInput,
        size: level_pricing::TradeSize,
        settlement: convex_core::Date,
        discount: Option<&BuiltCurve>,
    ) -> Result<level_pricing::RulePricing, level_pricing::LevelError> {
        level_pricing::price_sized(
            bond,
            in_force,
            input,
            Some(size),
            settlement,
            discount,
            &self.benchmark_yield_cache,
        )
    }

    /// The calculator's stage at request time: `level`, quoted by `method`, priced mid only
    /// against `set`, the set the bond's rule names as [`Self::rule_in_force`] read it,
    /// sharing each benchmark's solved yield as [`Self::price_in_force`] does.
    pub fn price_by_method(
        &self,
        bond: &ports::reference_data::BondReferenceData,
        set: Option<&level_pricing::BenchmarkSetQuotes>,
        method: &ports::level_pricing::PricingMethod,
        level: rust_decimal::Decimal,
        settlement: convex_core::Date,
        discount: Option<&BuiltCurve>,
    ) -> Result<level_pricing::RulePricing, level_pricing::LevelError> {
        level_pricing::price_by_method(
            bond,
            set,
            method,
            level,
            settlement,
            discount,
            &self.benchmark_yield_cache,
        )
    }

    /// The benchmark set `id` in force at `lookup`, each security's pinned terms and its
    /// Live clean price mid. A set not in force, a security not stored at its pinned
    /// version by the cutoff, or one without a usable Live price refuses by name.
    async fn benchmark_set_quotes(
        &self,
        id: &convex_core::contracts::ReferenceDataId,
        lookup: ports::reference_master::ReferenceLookup,
    ) -> Result<level_pricing::BenchmarkSetQuotes, level_pricing::LevelError> {
        use convex_core::contracts::{DataQuality, ValuationSubject};
        use level_pricing::{BenchmarkQuote, BenchmarkSetQuotes, LevelError};
        let repository = &self.storage.bonds;
        let set = repository
            .benchmark_set_for_valuation(id, lookup)
            .await?
            .ok_or_else(|| {
                LevelError::Refused(format!(
                    "benchmark set {} is not in force at the cutoff",
                    id.as_str()
                ))
            })?;
        // One read of the Live table, so every security is from the same tick.
        let live = self.runtime_results_of(
            set.securities
                .iter()
                .map(|security| security.instrument_id.as_str()),
        );
        let accepts_demo = self
            .configuration_use_cases()
            .is_some_and(|cases| cases.accepts(DataQuality::Demo));
        let mut quotes = Vec::with_capacity(set.securities.len());
        for security in set.securities {
            let key: convex_core::ids::InstrumentId = security.instrument_id.clone().into();
            let bond = repository
                .get_version(&key, security.version)
                .await?
                .filter(|r| r.recorded_at <= lookup.known_at)
                .and_then(|r| r.bond)
                .ok_or_else(|| {
                    level_pricing::benchmark_unavailable(
                        &security,
                        "its pinned version is not stored at the cutoff",
                    )
                })?;
            let record = live.iter().find(|record| {
                matches!(
                    &record.result.provenance().subject,
                    ValuationSubject::InstrumentVersion { instrument_id, .. }
                        if instrument_id == &security.instrument_id
                )
            });
            let live_price = level_pricing::live_clean_price_mid(
                record.map(|record| record.as_ref()),
                &security,
                lookup.known_at,
                accepts_demo,
            )?;
            quotes.push(BenchmarkQuote {
                security,
                bond,
                clean_price_mid: live_price.clean_price_mid,
                snapshot_id: live_price.snapshot_id,
            });
        }
        Ok(BenchmarkSetQuotes {
            benchmark_set_id: set.benchmark_set_id,
            version: set.version,
            quotes,
        })
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

    /// Commit a reference use case through the authoritative repository, then notify
    /// [`Self::subscribe_reference_commits`] of the master keys it wrote. A retried command
    /// notifies again.
    pub async fn commit_reference(
        &self,
        command: &ports::reference_repository::ReferenceCommand,
        recorded_at: convex_core::contracts::UtcTimestamp,
    ) -> Result<
        ports::reference_repository::ReferenceCommit,
        ports::reference_repository::ReferenceError,
    > {
        let receipt = self.storage.bonds.commit(command, recorded_at).await?;
        if !receipt.master_records.is_empty() {
            let keys: ReferenceCommitNotice = receipt
                .master_records
                .iter()
                .map(|record| record.definition.key())
                .collect();
            // No receiver is not an error: nothing observes commits.
            let _ = self.commits.send(keys);
        }
        Ok(receipt)
    }

    /// Master keys of each reference command committed through
    /// [`Self::commit_reference`] from now on. Lag is explicit: a lagging receiver rereads.
    pub fn subscribe_reference_commits(
        &self,
    ) -> tokio::sync::broadcast::Receiver<ReferenceCommitNotice> {
        self.commits.subscribe()
    }
}

/// Hexagonal port traits, re-exported from the [`convex_ports`] crate so that
/// `convex_engine::ports::*` paths keep working while adapters depend only on
/// the lightweight ports crate (not the whole engine).
pub use convex_ports as ports;
