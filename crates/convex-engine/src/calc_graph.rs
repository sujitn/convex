//! Bounded process-local calculation DAG and generation-fenced result commits.
//!
//! Validate candidates before activation; capture immutable inputs before work; commit
//! only the still-current attempt. Priority applies only to dependency-ready nodes.
//! Calculations and I/O never execute under the graph state lock. See `GRAPH.md`.
//!
//! ```
//! use convex_engine::{CalculationGraph, CalculationFailure, CommitOutcome, NodeId};
//! use convex_core::contracts::ErrorCode;
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let graph = CalculationGraph::new();
//! let node = NodeId::Curve { curve_id: "CALIBRATION".into() };
//! graph.add_node(node.clone(), vec![])?;
//! let ticket = graph.begin_calculation(&node)?;
//! let outcome = graph.commit_result(&ticket, Err(CalculationFailure::fixed(
//!     ErrorCode::MissingMarketData, "required calibration inputs are missing",
//! )))?;
//! assert!(matches!(outcome, CommitOutcome::Committed { .. }));
//! assert!(graph.get_cached(&node).is_none());
//! assert_eq!(graph.inspect_node(&node).unwrap().failure.unwrap().code, ErrorCode::MissingMarketData);
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::context::ValuationContext;
use crate::ports::config::{NodeConfig, UpdateFrequency};
use crate::snapshot::SnapshotValuationResult;
use convex_core::contracts::{DataQuality, ErrorCode, ResultStatus, Version};
use convex_core::ids::*;
use convex_core::Date;
use convex_ports::snapshot::MarketSnapshot;

mod dag;
mod state;
use dag::Dag;
use state::{GraphState, NodeState};

/// Node identifier in the calculation graph.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum NodeId {
    /// Market quote for a bond (input node)
    Quote {
        /// Instrument ID
        instrument_id: InstrumentId,
    },

    /// Curve input point (input node)
    CurveInput {
        /// Curve ID
        curve_id: CurveId,
        /// Instrument identifier
        instrument: String,
    },

    /// Built curve (depends on CurveInput nodes)
    Curve {
        /// Curve ID
        curve_id: CurveId,
    },

    /// Volatility surface (input node)
    VolSurface {
        /// Surface ID
        surface_id: VolSurfaceId,
    },

    /// FX rate (input node)
    FxRate {
        /// Currency pair
        pair: CurrencyPair,
    },

    /// Floating rate index fixing (input node for FRNs)
    IndexFixing {
        /// Rate index
        index: FloatingRateIndex,
        /// Fixing date
        date: Date,
    },

    /// Inflation index fixing (input node for ILBs)
    InflationFixing {
        /// Inflation index
        index: InflationIndex,
        /// Reference month
        month: YearMonth,
    },

    /// Pricing config (input node)
    Config {
        /// Config ID
        config_id: String,
    },

    /// Bond price calculation (depends on Curve, Quote, Config, etc.)
    BondPrice {
        /// Instrument ID
        instrument_id: InstrumentId,
    },

    /// ETF iNAV (depends on BondPrice nodes)
    EtfInav {
        /// ETF ID
        etf_id: EtfId,
    },

    /// ETF NAV (end-of-day)
    EtfNav {
        /// ETF ID
        etf_id: EtfId,
    },

    /// Portfolio aggregate (depends on BondPrice nodes)
    Portfolio {
        /// Portfolio ID
        portfolio_id: PortfolioId,
    },
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeId::Quote { instrument_id } => write!(f, "Quote({})", instrument_id),
            NodeId::CurveInput {
                curve_id,
                instrument,
            } => {
                write!(f, "CurveInput({}.{})", curve_id, instrument)
            }
            NodeId::Curve { curve_id } => write!(f, "Curve({})", curve_id),
            NodeId::VolSurface { surface_id } => write!(f, "VolSurface({})", surface_id),
            NodeId::FxRate { pair } => write!(f, "FxRate({})", pair),
            NodeId::IndexFixing { index, date } => write!(f, "IndexFixing({}, {})", index, date),
            NodeId::InflationFixing { index, month } => {
                write!(f, "InflationFixing({}, {})", index, month)
            }
            NodeId::Config { config_id } => write!(f, "Config({})", config_id),
            NodeId::BondPrice { instrument_id } => write!(f, "BondPrice({})", instrument_id),
            NodeId::EtfInav { etf_id } => write!(f, "EtfInav({})", etf_id),
            NodeId::EtfNav { etf_id } => write!(f, "EtfNav({})", etf_id),
            NodeId::Portfolio { portfolio_id } => write!(f, "Portfolio({})", portfolio_id),
        }
    }
}

/// Node value (result of calculation).
#[derive(Debug, Clone)]
pub enum NodeValue {
    /// Quote value
    Quote {
        /// Bid price
        bid: Option<Decimal>,
        /// Ask price
        ask: Option<Decimal>,
        /// Mid price
        mid: Option<Decimal>,
    },

    /// Curve value (zero rates at various tenors)
    Curve {
        /// Curve points (tenor days -> zero rate)
        points: Vec<(u32, f64)>,
    },

    /// Vol surface value
    VolSurface {
        /// ATM vol by expiry (years -> vol)
        atm_vols: Vec<(f64, Decimal)>,
    },

    /// FX rate
    FxRate {
        /// Mid rate
        mid: Decimal,
    },

    /// Index fixing
    IndexFixing {
        /// Rate value
        rate: Decimal,
    },

    /// Inflation fixing
    InflationFixing {
        /// Index value
        value: Decimal,
    },

    /// Bond price calculation result
    BondPrice {
        /// Clean price (bid)
        clean_price_bid: Option<Decimal>,
        /// Clean price (mid)
        clean_price_mid: Option<Decimal>,
        /// Clean price (ask)
        clean_price_ask: Option<Decimal>,
        /// Accrued interest
        accrued_interest: Option<Decimal>,
        /// YTM (bid)
        ytm_bid: Option<Decimal>,
        /// YTM (mid)
        ytm_mid: Option<Decimal>,
        /// YTM (ask)
        ytm_ask: Option<Decimal>,
        /// Z-spread (from mid price)
        z_spread_mid: Option<Decimal>,
        /// Modified duration (from mid price)
        modified_duration: Option<Decimal>,
        /// DV01 (from mid price)
        dv01: Option<Decimal>,
    },

    /// ETF iNAV
    EtfInav {
        /// iNAV value
        inav: Decimal,
        /// Coverage
        coverage: Decimal,
    },

    /// ETF NAV
    EtfNav {
        /// NAV value
        nav: Decimal,
    },

    /// Portfolio value
    Portfolio {
        /// Market value
        market_value: Decimal,
        /// Duration
        duration: Decimal,
    },

    /// Rate for one explicitly supplied curve calibration input.
    CurveInput {
        /// Decimal rate, not a price or a basis-point quote.
        rate: Decimal,
    },

    /// Explicit version of a resolved configuration input.
    Config {
        /// Positive immutable configuration revision.
        version: Version,
    },

    /// Checked full-context valuation over an immutable snapshot.
    Valuation(Arc<SnapshotValuationResult>),
}

// =============================================================================
// SHARDING TYPES
// =============================================================================

/// Strategy for assigning nodes to shards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShardStrategy {
    /// Assign by hash of instrument ID (default).
    #[default]
    HashBased,
    /// Assign by currency.
    ByCurrency,
    /// Assign by issuer type.
    ByIssuerType,
    /// Manual assignment via configuration.
    Manual,
}

/// Assignment specification for manual/explicit sharding.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShardAssignment {
    /// Currencies this shard handles.
    pub currencies: Option<Vec<String>>,
    /// Issuer types this shard handles.
    pub issuer_types: Option<Vec<String>>,
    /// Explicit instrument IDs this shard handles.
    pub instrument_ids: Option<Vec<String>>,
}

/// Configuration for sharded calculation graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardConfig {
    /// This shard's ID (0-indexed).
    pub shard_id: u32,
    /// Total number of shards.
    pub total_shards: u32,
    /// Sharding strategy.
    pub strategy: ShardStrategy,
    /// Explicit assignment (for ByCurrency, ByIssuerType, Manual).
    pub assignment: Option<ShardAssignment>,
}

impl Default for ShardConfig {
    fn default() -> Self {
        Self {
            shard_id: 0,
            total_shards: 1,
            strategy: ShardStrategy::HashBased,
            assignment: None,
        }
    }
}

impl ShardConfig {
    /// Create a new shard config for a single-shard deployment.
    pub fn single_shard() -> Self {
        Self::default()
    }

    /// Create a new shard config for hash-based sharding.
    pub fn hash_based(shard_id: u32, total_shards: u32) -> Self {
        Self {
            shard_id,
            total_shards,
            strategy: ShardStrategy::HashBased,
            assignment: None,
        }
    }

    /// Create a shard config for currency-based sharding.
    pub fn by_currency(shard_id: u32, total_shards: u32, currencies: Vec<String>) -> Self {
        Self {
            shard_id,
            total_shards,
            strategy: ShardStrategy::ByCurrency,
            assignment: Some(ShardAssignment {
                currencies: Some(currencies),
                issuer_types: None,
                instrument_ids: None,
            }),
        }
    }

    /// Check if this is a single-shard configuration.
    pub fn is_single_shard(&self) -> bool {
        self.total_shards == 1
    }

    /// Compute which shard owns a given string key (for hash-based).
    pub fn shard_for_key(&self, key: &str) -> u32 {
        if self.total_shards == 1 {
            return 0;
        }
        Self::hash_string(key) % self.total_shards
    }

    /// Check if this shard owns a given key (for hash-based).
    pub fn owns_key(&self, key: &str) -> bool {
        if self.total_shards == 1 {
            return true;
        }
        self.shard_for_key(key) == self.shard_id
    }

    /// Check if this shard owns a given currency (for ByCurrency strategy).
    pub fn owns_currency(&self, currency: &str) -> bool {
        if self.total_shards == 1 {
            return true;
        }
        match &self.assignment {
            Some(assignment) => assignment
                .currencies
                .as_ref()
                .map(|cs| cs.iter().any(|c| c.eq_ignore_ascii_case(currency)))
                .unwrap_or(false),
            None => {
                // Fall back to hash-based
                self.owns_key(currency)
            }
        }
    }

    /// Simple string hash function (DJB2 variant).
    fn hash_string(s: &str) -> u32 {
        let mut hash: u32 = 5381;
        for byte in s.bytes() {
            hash = hash.wrapping_mul(33).wrapping_add(byte as u32);
        }
        hash
    }
}

/// Explicit node responsibility; input nodes are supplied, never calculated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    /// Published external input with no graph dependencies.
    Input,
    /// Calculation over captured dependencies or a complete supplied snapshot.
    Calculation,
}

/// One node in a candidate graph. Edges point to prerequisite nodes.
#[derive(Debug, Clone)]
pub struct NodeSpec {
    /// Stable node identity.
    pub id: NodeId,
    /// Explicit input/calculation responsibility.
    pub role: NodeRole,
    /// Unique prerequisite identities; all must exist in the candidate.
    pub dependencies: Vec<NodeId>,
    /// Frequency, priority and display publication policy.
    pub config: NodeConfig,
}
impl NodeSpec {
    /// An externally published root input.
    pub fn input(id: NodeId) -> Self {
        Self {
            id,
            role: NodeRole::Input,
            dependencies: vec![],
            config: NodeConfig::default(),
        }
    }
    /// A calculated node with explicit prerequisites.
    pub fn calculation(id: NodeId, dependencies: Vec<NodeId>) -> Self {
        Self {
            id,
            role: NodeRole::Calculation,
            dependencies,
            config: NodeConfig::default(),
        }
    }
    /// Supply the actual update policy; priority cannot bypass a dependency.
    pub fn with_config(mut self, config: NodeConfig) -> Self {
        self.config = config;
        self
    }
}

/// Administrative graph and work bounds. Lower limits are supported, never higher ceilings.
#[derive(Debug, Clone, Copy)]
pub struct GraphLimits {
    /// Maximum active nodes (hard ceiling 100,000).
    pub max_nodes: usize,
    /// Maximum directed edges (hard ceiling 500,000).
    pub max_edges: usize,
    /// Maximum number of nodes on a dependency path (hard ceiling 4,096).
    pub max_depth: usize,
    /// Maximum direct dependents per node (hard ceiling 10,000).
    pub max_fanout: usize,
    /// Maximum queued ready candidates per admission pass (hard ceiling 1,024).
    pub max_ready: usize,
    /// Maximum simultaneous claimed calculations (hard ceiling 64).
    pub max_in_flight: usize,
    /// Monotonic attempt lifetime (hard ceiling 300 seconds).
    pub max_job_lifetime: Duration,
}
impl Default for GraphLimits {
    fn default() -> Self {
        Self {
            max_nodes: 100_000,
            max_edges: 500_000,
            max_depth: 4_096,
            max_fanout: 10_000,
            max_ready: 1_024,
            max_in_flight: 64,
            max_job_lifetime: Duration::from_secs(30),
        }
    }
}
impl GraphLimits {
    fn validate(self) -> Result<(), GraphError> {
        if self.max_nodes == 0
            || self.max_nodes > 100_000
            || self.max_edges > 500_000
            || self.max_depth == 0
            || self.max_depth > 4_096
            || self.max_fanout == 0
            || self.max_fanout > 10_000
            || self.max_ready == 0
            || self.max_ready > 1_024
            || self.max_in_flight == 0
            || self.max_in_flight > 64
            || self.max_job_lifetime.is_zero()
            || self.max_job_lifetime > Duration::from_secs(300)
        {
            return Err(GraphError::InvalidLimits);
        }
        Ok(())
    }
}

/// Checked graph/admission errors, never an invented financial result.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GraphError {
    /// Invalid or above-ceiling resource policy.
    #[error("invalid graph limits")]
    InvalidLimits,
    /// Candidate exceeds an explicit configured bound.
    #[error("graph exceeds {0} limit")]
    Limit(&'static str),
    /// Repeated identity in one candidate/update.
    #[error("duplicate node: {0}")]
    DuplicateNode(NodeId),
    /// Unknown prerequisite or target identity.
    #[error("unknown node: {0}")]
    UnknownNode(NodeId),
    /// A repeated edge would distort indegree/reverse edges.
    #[error("duplicate dependency on node: {0}")]
    DuplicateDependency(NodeId),
    /// No DAG ordering exists, including a self-edge.
    #[error("calculation graph contains a cycle")]
    Cycle,
    /// Input has dependencies, unsupported role, invalid config or invalid value.
    #[error("invalid graph node/value: {0}")]
    InvalidNode(&'static str),
    /// Current generation/node is not ready, already claimed, or terminal.
    #[error("calculation is not ready")]
    NotReady,
    /// Work capacity is already claimed.
    #[error("calculation capacity exhausted")]
    CapacityExceeded,
    /// Counter cannot wrap and admit an old ticket.
    #[error("graph counter exhausted")]
    CounterExhausted,
    /// Captured graph/context/snapshot differs from the requested values.
    #[error("graph snapshot/context conflict")]
    ContextConflict,
    /// Snapshot pricing must supply its complete CVX-008 context.
    #[error("complete valuation context required")]
    ContextRequired,
    /// Transport loss suspends automatic work until full reload.
    #[error("market delivery suspended")]
    Suspended,
}

/// Bounded structured calculation failure, optionally rooted in a dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalculationFailure {
    /// Canonical stable error code.
    pub code: ErrorCode,
    /// Bounded explanatory detail.
    pub message: String,
    /// Failed/missing prerequisite, when applicable.
    pub dependency: Option<NodeId>,
}
impl CalculationFailure {
    /// Validate supplied detail without truncating or replacing the error.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Result<Self, GraphError> {
        let message = message.into();
        if message.is_empty() || message.len() > 1024 {
            return Err(GraphError::InvalidNode(
                "failure detail must be 1..1024 bytes",
            ));
        }
        Ok(Self {
            code,
            message,
            dependency: None,
        })
    }
    /// A fixed bounded diagnostic useful for model/adapter failure branches.
    pub fn fixed(code: ErrorCode, message: &'static str) -> Self {
        // Static callers use short literals; commit still validates the diagnostic.
        Self {
            code,
            message: message.into(),
            dependency: None,
        }
    }
}

/// Committed result plus exact graph lineage. A result revision is distinct from input generation.
#[derive(Debug, Clone)]
pub struct CachedValue {
    /// Actual value, never an Empty placeholder.
    pub value: NodeValue,
    /// Monotonically increasing committed result revision.
    pub revision: u64,
    /// Input generation captured before this calculation/publication.
    pub generation: u64,
    /// Original monotonic completion time; stale inspection never refreshes it.
    pub calculated_at: Instant,
    /// Earliest consumed input freshness deadline.
    pub valid_until: Option<Instant>,
    /// Exact immutable snapshot, when this graph was activated with one.
    pub snapshot_id: Option<convex_core::contracts::SnapshotId>,
    /// Pinned snapshot configuration, when present.
    pub configuration: Option<crate::context::ConfigurationPin>,
    /// Exact dependency result revisions and captured generations.
    pub dependencies: Vec<DependencyVersion>,
    /// Numerical result completeness (quality remains independent).
    pub status: ResultStatus,
    /// Original result data quality, never inferred from completion alone.
    pub quality: DataQuality,
}

/// Revision of an actual consumed prerequisite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyVersion {
    /// Stable prerequisite identity.
    pub node_id: NodeId,
    /// Captured prerequisite generation.
    pub generation: u64,
    /// Captured committed result revision.
    pub revision: u64,
}

/// Captured immutable prerequisite value; calculations must use these values.
#[derive(Debug, Clone)]
pub struct CapturedInput {
    /// Stable prerequisite identity.
    pub node_id: NodeId,
    /// Read-only committed value, including its original lineage/deadline.
    pub value: Arc<CachedValue>,
}

/// Opaque, single-attempt fence captured atomically before work starts.
#[derive(Debug)]
pub struct CalculationTicket {
    owner: Arc<()>,
    node_id: NodeId,
    generation: u64,
    epoch: u64,
    attempt: u64,
    deadline: Instant,
    inputs: Vec<CapturedInput>,
    snapshot: Option<Arc<MarketSnapshot>>,
    context: Option<ValuationContext>,
    configuration: Option<crate::context::ConfigurationPin>,
}
impl CalculationTicket {
    /// Target identity.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }
    /// Target input generation, captured before calculation.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Immutable graph activation epoch.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    /// Monotonic job deadline; expired work cannot publish.
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    /// Exact committed prerequisite values, not later mutable graph reads.
    pub fn inputs(&self) -> &[CapturedInput] {
        &self.inputs
    }
    /// Find an explicitly declared prerequisite.
    pub fn input(&self, id: &NodeId) -> Option<&CachedValue> {
        self.inputs
            .iter()
            .find(|input| &input.node_id == id)
            .map(|input| input.value.as_ref())
    }
    /// Entire immutable information set, when supplied at activation.
    pub fn snapshot(&self) -> Option<&Arc<MarketSnapshot>> {
        self.snapshot.as_ref()
    }
    /// Complete supplied context for snapshot bond valuation.
    pub fn context(&self) -> Option<&ValuationContext> {
        self.context.as_ref()
    }
    /// Exact snapshot configuration carried independently of result success/failure.
    pub fn configuration(&self) -> Option<&crate::context::ConfigurationPin> {
        self.configuration.as_ref()
    }
}

/// Captured lineage and committed numerical/status outcome for one completion notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalculationCompletion {
    /// Target input generation captured before work.
    pub generation: u64,
    /// Monotonic result/status revision assigned atomically at commit.
    pub revision: u64,
    /// Numerical measure completeness; Failed has no fresh numeric result.
    pub status: ResultStatus,
    /// Original data quality, independent of numerical completeness.
    pub quality: DataQuality,
    /// Structured primary failure; full snapshot errors remain in NodeView.failed_valuation.
    pub failure: Option<CalculationFailure>,
    /// Frozen information set; absent on the explicitly unconverted compatibility path.
    pub snapshot_id: Option<convex_core::contracts::SnapshotId>,
    /// Exact frozen configuration, when supplied.
    pub configuration: Option<crate::context::ConfigurationPin>,
}
impl CalculationCompletion {
    fn new(
        ticket: &CalculationTicket,
        revision: u64,
        status: ResultStatus,
        quality: DataQuality,
        failure: Option<CalculationFailure>,
    ) -> Self {
        Self {
            generation: ticket.generation,
            revision,
            status,
            quality,
            failure,
            snapshot_id: ticket
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.id().clone()),
            configuration: ticket.configuration.clone(),
        }
    }
}
/// Commit distinguishes an accepted explicit outcome from discarded obsolete work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    /// Numerical/status outcome and lineage assigned atomically while committing.
    Committed {
        /// Actual completion; never reconstructed by reading later mutable node state.
        completion: CalculationCompletion,
    },
    /// Epoch, generation, attempt, dependency or deadline no longer matches.
    Discarded,
}

/// Observable node state. A stale retained value is never Fresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodePhase {
    /// Dirty calculation awaiting admission.
    Pending,
    /// Input needs external publication.
    WaitingInput,
    /// Calculation attempt in flight.
    Running,
    /// Current nonexpired complete/valid result.
    Fresh,
    /// Current result has incomplete measures or nonvalid data quality.
    Degraded,
    /// Current explicit calculation failure.
    Failed,
    /// A prerequisite or snapshot prevents calculation/use.
    Blocked,
    /// Result/input deadline has expired.
    Stale,
}

/// Explicit inspection/display path for state and retained last-good values.
#[derive(Debug, Clone)]
pub struct NodeView {
    /// Current target generation.
    pub generation: u64,
    /// Latest committed result or failure revision (zero before any commit).
    pub revision: u64,
    /// Current availability/work state.
    pub phase: NodePhase,
    /// Structured own/dependency failure, when available.
    pub failure: Option<CalculationFailure>,
    /// Last valid complete result, preserving its original generation/time.
    pub last_good: Option<Arc<CachedValue>>,
    /// Complete failed snapshot envelope, preserving every requested-measure diagnostic and context.
    pub failed_valuation: Option<Arc<SnapshotValuationResult>>,
    /// Explicit display quality of the retained result.
    pub retained_quality: Option<DataQuality>,
}

/// One bounded process-local DAG. Local locking is not distributed ownership/fencing.
pub struct CalculationGraph {
    state: Mutex<GraphState>,
    owner: Arc<()>,
    limits: GraphLimits,
    shard_config: ShardConfig,
}
impl CalculationGraph {
    /// Empty graph with bounded default policy.
    pub fn new() -> Self {
        Self::construct(GraphLimits::default(), ShardConfig::single_shard())
    }
    fn construct(limits: GraphLimits, shard_config: ShardConfig) -> Self {
        Self {
            state: Mutex::new(GraphState::empty()),
            owner: Arc::new(()),
            limits,
            shard_config,
        }
    }
    /// Empty graph with a validated tighter resource policy.
    pub fn with_limits(limits: GraphLimits) -> Result<Self, GraphError> {
        limits.validate()?;
        Ok(Self::construct(limits, ShardConfig::single_shard()))
    }
    /// Existing local partition hints; these do not establish distributed ownership.
    pub fn with_sharding(shard_config: ShardConfig) -> Result<Self, GraphError> {
        if shard_config.total_shards == 0 || shard_config.shard_id >= shard_config.total_shards {
            return Err(GraphError::InvalidNode("invalid shard coordinates"));
        }
        Ok(Self::construct(GraphLimits::default(), shard_config))
    }
    /// Actual enforced bounds.
    pub fn limits(&self) -> GraphLimits {
        self.limits
    }
    /// Validated partition hint.
    pub fn shard_config(&self) -> &ShardConfig {
        &self.shard_config
    }
    /// Local shard number.
    pub fn shard_id(&self) -> u32 {
        self.shard_config.shard_id
    }
    /// Number of configured partitions.
    pub fn total_shards(&self) -> u32 {
        self.shard_config.total_shards
    }
    /// Whether local partition hints have multiple partitions.
    pub fn is_sharded(&self) -> bool {
        !self.shard_config.is_single_shard()
    }
    /// Hash-based node ownership hint; not an execution fence or cross-host graph.
    pub fn owns_node(&self, node: &NodeId) -> bool {
        self.shard_for_node(node) == self.shard_id()
    }
    /// Partition number for a stable node key.
    pub fn shard_for_node(&self, node: &NodeId) -> u32 {
        self.shard_config.shard_for_key(&node_shard_key(node))
    }

    /// Validate then atomically replace the full graph. Rejected candidates cannot change active state.
    pub fn activate(&self, nodes: Vec<NodeSpec>) -> Result<u64, GraphError> {
        let dag = Dag::validate(nodes, self.limits)?;
        let mut state = self.state.lock();
        if !dag.nodes.is_empty() && (state.snapshot.is_some() || state.snapshot_pending) {
            return Err(GraphError::ContextConflict);
        }
        let generation = next(state.generation)?;
        self.replace(&mut state, dag, generation, None)
    }
    /// Activate one verified immutable information set; generations must advance, never rewind.
    pub fn activate_snapshot(
        &self,
        nodes: Vec<NodeSpec>,
        snapshot: Arc<MarketSnapshot>,
    ) -> Result<u64, GraphError> {
        let built = crate::curve_replay::BuiltSnapshot::build(snapshot)
            .map_err(|_| GraphError::ContextConflict)?;
        self.activate_built(nodes, &built)
    }
    /// Activate a snapshot whose curves were built once and are shared by its valuations.
    pub fn activate_built(
        &self,
        nodes: Vec<NodeSpec>,
        built: &crate::curve_replay::BuiltSnapshot,
    ) -> Result<u64, GraphError> {
        let dag = Dag::validate(nodes, self.limits)?;
        let snapshot = built.snapshot();
        let generation = snapshot.content().generation.get();
        let mut state = self.state.lock();
        if generation <= state.generation {
            return Err(GraphError::ContextConflict);
        }
        self.replace(&mut state, dag, generation, Some(snapshot.clone()))
    }
    /// Bind a later built snapshot to the active graph and reset only `revalue`, closed
    /// over its connected components (D55). Every other node keeps its committed value,
    /// failure and generation, stays readable and is not calculated again; its inputs
    /// are unchanged in the new snapshot, which the caller has established. A component
    /// is reset whole because capture reads only dependencies committed under the bound
    /// snapshot. A kept node must be settled (committed, no attempt in flight), and a
    /// refused call changes nothing.
    pub fn rebind_built(
        &self,
        built: &crate::curve_replay::BuiltSnapshot,
        revalue: &[NodeId],
    ) -> Result<u64, GraphError> {
        let snapshot = built.snapshot();
        let generation = snapshot.content().generation.get();
        let mut state = self.state.lock();
        if generation <= state.generation || state.snapshot.is_none() {
            return Err(GraphError::ContextConflict);
        }
        let roots = revalue
            .iter()
            .map(|id| {
                state
                    .dag
                    .indices
                    .get(id)
                    .copied()
                    .ok_or_else(|| GraphError::UnknownNode(id.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let reset = state.dag.component(&roots);
        if state
            .nodes
            .iter()
            .zip(&reset)
            .any(|(node, &reset)| !reset && (node.dirty || node.attempt.is_some()))
        {
            return Err(GraphError::ContextConflict);
        }
        let epoch = next(state.epoch)?;
        for (node, _) in state
            .nodes
            .iter_mut()
            .zip(reset)
            .filter(|(_, reset)| *reset)
        {
            *node = NodeState {
                generation,
                dirty: true,
                last_good: node.last_good.take(),
                ..NodeState::default()
            };
        }
        state.generation = generation;
        state.epoch = epoch;
        state.snapshot = Some(snapshot.clone());
        Ok(generation)
    }
    fn replace(
        &self,
        state: &mut GraphState,
        dag: Dag,
        generation: u64,
        snapshot: Option<Arc<MarketSnapshot>>,
    ) -> Result<u64, GraphError> {
        let epoch = next(state.epoch)?;
        let mut nodes = Vec::with_capacity(dag.nodes.len());
        for node in &dag.nodes {
            let last_good = state
                .dag
                .indices
                .get(&node.id)
                .and_then(|&i| state.nodes[i].last_good.clone());
            nodes.push(NodeState {
                generation,
                dirty: true,
                last_good,
                ..NodeState::default()
            });
        }
        state.dag = dag;
        state.nodes = nodes;
        state.generation = generation;
        state.epoch = epoch;
        let snapshot_pending =
            snapshot.is_none() && (state.snapshot.is_some() || state.snapshot_pending);
        state.snapshot = snapshot;
        state.snapshot_pending = snapshot_pending;
        Ok(generation)
    }
    /// Apply an administrative batch atomically. Reverse edges are rebuilt and deduplicated by validation.
    pub fn upsert_nodes(&self, changes: Vec<NodeSpec>) -> Result<u64, GraphError> {
        if changes.len() > self.limits.max_nodes {
            return Err(GraphError::Limit("nodes"));
        }
        let mut seen = HashSet::new();
        for node in &changes {
            if !seen.insert(node.id.clone()) {
                return Err(GraphError::DuplicateNode(node.id.clone()));
            }
        }
        let mut state = self.state.lock();
        if state.snapshot.is_some() || state.snapshot_pending {
            return Err(GraphError::ContextConflict);
        }
        let mut candidate = state.dag.nodes.clone();
        for node in changes {
            if let Some(&i) = state.dag.indices.get(&node.id) {
                candidate[i] = node;
            } else {
                candidate.push(node);
            }
        }
        let dag = Dag::validate(candidate, self.limits)?;
        self.merge(&mut state, dag, &seen)
    }
    fn merge(
        &self,
        state: &mut GraphState,
        dag: Dag,
        changed: &HashSet<NodeId>,
    ) -> Result<u64, GraphError> {
        let generation = next(state.generation)?;
        let epoch = next(state.epoch)?;
        let roots: Vec<_> = changed
            .iter()
            .filter_map(|id| dag.indices.get(id).copied())
            .collect();
        let affected: HashSet<_> = dag.affected(&roots).into_iter().collect();
        let mut previous: HashMap<_, _> = state
            .dag
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .zip(std::mem::take(&mut state.nodes))
            .collect();
        let nodes = dag
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let mut current = previous.remove(&node.id).unwrap_or_default();
                if affected.contains(&i) || current.attempt.is_some() {
                    current.invalidate(generation, false);
                }
                current
            })
            .collect();
        state.dag = dag;
        state.nodes = nodes;
        state.generation = generation;
        state.epoch = epoch;
        Ok(generation)
    }
    /// Add/replace a calculation node. Register every prerequisite explicitly first or in one batch.
    pub fn add_node(&self, id: NodeId, dependencies: Vec<NodeId>) -> Result<u64, GraphError> {
        self.upsert_nodes(vec![NodeSpec::calculation(id, dependencies)])
    }
    /// Update policy for an existing node; in-flight work is invalidated by the administrative update.
    pub fn set_node_config(&self, id: NodeId, config: NodeConfig) -> Result<u64, GraphError> {
        let mut state = self.state.lock();
        if state.snapshot.is_some() || state.snapshot_pending {
            return Err(GraphError::ContextConflict);
        }
        let &i = state
            .dag
            .indices
            .get(&id)
            .ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        let mut candidate = state.dag.nodes.clone();
        candidate[i].config = config;
        let dag = Dag::validate(candidate, self.limits)?;
        self.merge(&mut state, dag, &HashSet::from([id]))
    }
    /// Current node definition, without mutable graph access.
    pub fn node_spec(&self, id: &NodeId) -> Option<NodeSpec> {
        let state = self.state.lock();
        state
            .dag
            .indices
            .get(id)
            .map(|&i| state.dag.nodes[i].clone())
    }
    /// Explicit scheduling policy for an existing node.
    pub fn get_node_config(&self, id: &NodeId) -> Option<NodeConfig> {
        self.node_spec(id).map(|node| node.config)
    }
    /// Invalidate a known node and its entire dependent closure once, even if already dirty.
    pub fn invalidate(&self, id: &NodeId) -> Result<u64, GraphError> {
        self.invalidate_inner(id, false)
    }
    /// Explicit retry/timer/on-demand trigger overrides frequency for the target only.
    pub fn request_calculation(&self, id: &NodeId) -> Result<u64, GraphError> {
        self.invalidate_inner(id, true)
    }
    fn invalidate_inner(&self, id: &NodeId, forced: bool) -> Result<u64, GraphError> {
        let mut state = self.state.lock();
        let &i = state
            .dag
            .indices
            .get(id)
            .ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        state.invalidate(i, forced)
    }
    /// External input publication. It atomically invalidates dependents and commits the supplied root.
    /// Unknown roots are admitted only as explicit inputs under the same DAG limits.
    pub fn publish_input(&self, id: &NodeId, value: NodeValue) -> Result<u64, GraphError> {
        self.publish_input_until(id, value, None)
    }
    /// Publish an input with its original monotonic freshness deadline (never extend it on cache hit).
    pub fn publish_input_until(
        &self,
        id: &NodeId,
        value: NodeValue,
        valid_until: Option<Instant>,
    ) -> Result<u64, GraphError> {
        validate_value(id, &value)?;
        if !can_be_input(id) {
            return Err(GraphError::InvalidNode(
                "calculated outputs require a captured ticket",
            ));
        }
        let now = Instant::now();
        let mut state = self.state.lock();
        if state.snapshot.is_some() || state.snapshot_pending {
            return Err(GraphError::ContextConflict);
        }
        let revision = next(state.result_revision)?;
        next(state.generation)?;
        if state
            .dag
            .indices
            .get(id)
            .is_some_and(|&i| state.dag.nodes[i].role != NodeRole::Input)
        {
            return Err(GraphError::InvalidNode(
                "publication target is not an input",
            ));
        }
        if !state.dag.indices.contains_key(id) {
            let mut candidate = state.dag.nodes.clone();
            candidate.push(NodeSpec::input(id.clone()));
            let dag = Dag::validate(candidate, self.limits)?;
            let epoch = next(state.epoch)?;
            state.dag = dag;
            state.nodes.push(NodeState::default());
            state.epoch = epoch;
            // A structural epoch cannot leave prior attempts occupying work capacity.
            for node in &mut state.nodes {
                node.attempt = None;
            }
        }
        let i = state.dag.indices[id];

        let generation = state.invalidate(i, false)?;
        let policy_deadline = state.dag.nodes[i]
            .config
            .stale_threshold
            .and_then(|duration| now.checked_add(duration));
        let valid_until = earliest(valid_until, policy_deadline);
        state.result_revision = revision;
        let cached = Arc::new(CachedValue {
            value,
            revision,
            generation,
            calculated_at: now,
            valid_until,
            snapshot_id: None,
            configuration: None,
            dependencies: vec![],
            status: ResultStatus::Complete,
            quality: DataQuality::Valid,
        });
        let root = &mut state.nodes[i];
        root.cached = Some(cached.clone());
        root.last_good = Some(cached);
        root.revision = revision;
        root.dirty = false;
        root.last_calc = Some(now);
        Ok(revision)
    }
    /// Remove a lost input and fail its dependent closure, retaining last-good display values as stale.
    pub fn evict_cached(&self, id: &NodeId) -> Result<(), GraphError> {
        let mut state = self.state.lock();
        let &i = state
            .dag
            .indices
            .get(id)
            .ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        state
            .result_revision
            .checked_add(state.dag.affected(&[i]).len() as u64)
            .ok_or(GraphError::CounterExhausted)?;
        state.invalidate(i, false)?;
        state.fail(
            i,
            CalculationFailure::fixed(
                ErrorCode::MissingMarketData,
                "input unavailable; full recovery required",
            ),
        )?;
        Ok(())
    }
    /// Unreconciled shared transport loss cancels every attempt and prevents new admission.
    pub fn suspend_market_delivery(&self) -> Result<(), GraphError> {
        let mut state = self.state.lock();
        let generation = state.advance_generation()?;
        state.suspended = true;
        for node in &mut state.nodes {
            node.invalidate(generation, false);
        }
        Ok(())
    }
    /// Whether shared delivery still needs a full engine reload.
    pub fn market_delivery_suspended(&self) -> bool {
        self.state.lock().suspended
    }
    /// Pending work, including waiting/blocked inputs; a terminal failure is inspected separately.
    pub fn get_dirty_nodes(&self) -> Vec<NodeId> {
        let state = self.state.lock();
        state
            .dag
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| state.nodes[*i].dirty)
            .map(|(_, node)| node.id.clone())
            .collect()
    }
    /// Whether this generation still needs work/input.
    pub fn is_dirty(&self, id: &NodeId) -> bool {
        let state = self.state.lock();
        state
            .dag
            .indices
            .get(id)
            .is_some_and(|&i| state.nodes[i].dirty)
    }
    /// Return only dependency-ready nodes, bounded by queue policy. Claiming rechecks all conditions.
    pub fn get_nodes_to_calculate(&self) -> Result<Vec<NodeId>, GraphError> {
        let mut state = self.state.lock();
        let now = Instant::now();
        state.maintain(now)?;
        Ok(state
            .ready(now, self.limits.max_ready)
            .into_iter()
            .map(|i| state.dag.nodes[i].id.clone())
            .collect())
    }
    /// Atomically claim bounded ready work; parallel callers cannot claim the same attempt.
    pub fn claim_ready(&self, max: usize) -> Result<Vec<CalculationTicket>, GraphError> {
        if max == 0 || max > self.limits.max_ready {
            return Err(GraphError::Limit("ready batch"));
        }
        let mut state = self.state.lock();
        let now = Instant::now();
        state.maintain(now)?;
        if state.suspended {
            return Err(GraphError::Suspended);
        }
        let in_flight = state
            .nodes
            .iter()
            .filter(|node| node.attempt.is_some())
            .count();
        let count = max.min(self.limits.max_in_flight.saturating_sub(in_flight));
        let ready: Vec<_> = state
            .ready(now, self.limits.max_ready)
            .into_iter()
            .filter(|&i| {
                !(state.snapshot.is_some()
                    && matches!(state.dag.nodes[i].id, NodeId::BondPrice { .. }))
            })
            .take(count)
            .collect();
        state
            .next_attempt
            .checked_add(ready.len() as u64)
            .ok_or(GraphError::CounterExhausted)?;
        let mut tickets = Vec::with_capacity(count);
        for i in ready {
            tickets.push(self.capture(&mut state, i, now, None)?);
        }
        Ok(tickets)
    }
    /// Claim an explicitly requested ready calculation, without bypassing prerequisites or capacity.
    pub fn begin_calculation(&self, id: &NodeId) -> Result<CalculationTicket, GraphError> {
        self.begin(id, None)
    }
    /// Claim snapshot pricing using the complete supplied immutable context.
    pub fn begin_valuation(
        &self,
        id: &NodeId,
        context: ValuationContext,
    ) -> Result<CalculationTicket, GraphError> {
        self.begin(id, Some(context))
    }
    fn begin(
        &self,
        id: &NodeId,
        context: Option<ValuationContext>,
    ) -> Result<CalculationTicket, GraphError> {
        let mut state = self.state.lock();
        let now = Instant::now();
        state.maintain(now)?;
        let &i = state
            .dag
            .indices
            .get(id)
            .ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        self.capture(&mut state, i, now, context)
    }
    fn capture(
        &self,
        state: &mut GraphState,
        i: usize,
        now: Instant,
        context: Option<ValuationContext>,
    ) -> Result<CalculationTicket, GraphError> {
        if state.suspended {
            return Err(GraphError::Suspended);
        }
        if state.snapshot_pending
            || state.dag.nodes[i].role != NodeRole::Calculation
            || !state.nodes[i].dirty
            || state.nodes[i].attempt.is_some()
        {
            return Err(GraphError::NotReady);
        }
        if state
            .nodes
            .iter()
            .filter(|node| node.attempt.is_some())
            .count()
            >= self.limits.max_in_flight
        {
            return Err(GraphError::CapacityExceeded);
        }
        if let Some(snapshot) = &state.snapshot {
            if matches!(state.dag.nodes[i].id, NodeId::BondPrice { .. }) && context.is_none() {
                return Err(GraphError::ContextRequired);
            }
            if let Some(context) = &context {
                context
                    .validate_snapshot(snapshot)
                    .map_err(|_| GraphError::ContextConflict)?;
                if context.spec().generation.get() != state.nodes[i].generation
                    || !context_subject_matches(&state.dag.nodes[i].id, context)
                {
                    return Err(GraphError::ContextConflict);
                }
            }
        } else if context.is_some() {
            return Err(GraphError::ContextConflict);
        }
        let mut inputs = Vec::with_capacity(state.dag.dependencies[i].len());
        for &d in &state.dag.dependencies[i] {
            let value = state.nodes[d].current(now).ok_or(GraphError::NotReady)?;
            if value.status != ResultStatus::Complete
                || value.quality != DataQuality::Valid
                || value.snapshot_id.as_ref()
                    != state.snapshot.as_ref().map(|snapshot| snapshot.id())
            {
                return Err(GraphError::NotReady);
            }
            inputs.push(CapturedInput {
                node_id: state.dag.nodes[d].id.clone(),
                value: value.clone(),
            });
        }
        let configuration = state
            .snapshot
            .as_ref()
            .map(|snapshot| {
                Ok(crate::context::ConfigurationPin {
                    id: convex_core::contracts::SourceId::new(
                        &snapshot.content().configuration.config_id,
                    )
                    .map_err(|_| GraphError::ContextConflict)?,
                    version: Version::new(snapshot.content().configuration.version)
                        .map_err(|_| GraphError::ContextConflict)?,
                })
            })
            .transpose()?;
        let deadline = now
            .checked_add(self.limits.max_job_lifetime)
            .ok_or(GraphError::InvalidLimits)?;
        let attempt = next(state.next_attempt)?;
        state.next_attempt = attempt;
        state.nodes[i].attempt = Some((attempt, deadline));
        Ok(CalculationTicket {
            owner: self.owner.clone(),
            node_id: state.dag.nodes[i].id.clone(),
            generation: state.nodes[i].generation,
            epoch: state.epoch,
            attempt,
            deadline,
            inputs,
            snapshot: state.snapshot.clone(),
            context,
            configuration,
        })
    }
    /// Commit only the still-current target/attempt/dependencies. Late success and late failure are discarded alike.
    pub fn commit_result(
        &self,
        ticket: &CalculationTicket,
        result: Result<NodeValue, CalculationFailure>,
    ) -> Result<CommitOutcome, GraphError> {
        let mut state = self.state.lock();
        let now = Instant::now();
        if !Arc::ptr_eq(&ticket.owner, &self.owner)
            || state.epoch != ticket.epoch
            || state.suspended
            || state.snapshot_pending
        {
            return Ok(CommitOutcome::Discarded);
        }
        let Some(&i) = state.dag.indices.get(&ticket.node_id) else {
            return Ok(CommitOutcome::Discarded);
        };
        if state.nodes[i].generation != ticket.generation
            || state.nodes[i].attempt.map(|(attempt, _)| attempt) != Some(ticket.attempt)
        {
            return Ok(CommitOutcome::Discarded);
        }
        if now > ticket.deadline {
            state.fail(
                i,
                CalculationFailure::fixed(
                    ErrorCode::DeadlineExceeded,
                    "calculation completed after its job deadline",
                ),
            )?;
            return Ok(CommitOutcome::Discarded);
        }
        for input in &ticket.inputs {
            let Some(&d) = state.dag.indices.get(&input.node_id) else {
                return Ok(CommitOutcome::Discarded);
            };
            if state.nodes[d].current(now).is_none_or(|current| {
                current.revision != input.value.revision
                    || current.generation != input.value.generation
            }) {
                // Freshness expiry cannot strand a current attempt or be relabelled fresh.
                state.fail(
                    i,
                    CalculationFailure::fixed(
                        ErrorCode::StaleInput,
                        "captured prerequisite is no longer current/fresh",
                    ),
                )?;
                return Ok(CommitOutcome::Discarded);
            }
        }
        let mut failed_valuation = None;
        let result = match result {
            Ok(value) => {
                validate_value(&ticket.node_id, &value)?;
                validate_ticket_value(ticket, &value)?;
                match &value {
                    NodeValue::Valuation(result)
                        if result.envelope().status() == ResultStatus::Failed =>
                    {
                        failed_valuation = Some(result.clone());
                        let error = result
                            .envelope()
                            .errors()
                            .first()
                            .ok_or(GraphError::InvalidNode("failed valuation has no error"))?;
                        Err(CalculationFailure::new(error.code(), error.message())?)
                    }
                    _ => Ok(value),
                }
            }
            Err(failure) => {
                CalculationFailure::new(failure.code, &failure.message)?;
                Err(failure)
            }
        };
        match result {
            Err(failure) => {
                let completion_failure = failure.clone();
                let revision = state.fail(i, failure)?;
                state.nodes[i].failed_valuation = failed_valuation;
                Ok(CommitOutcome::Committed {
                    completion: CalculationCompletion::new(
                        ticket,
                        revision,
                        ResultStatus::Failed,
                        DataQuality::Invalid,
                        Some(completion_failure),
                    ),
                })
            }
            Ok(value) => {
                let revision = next(state.result_revision)?;
                let policy_deadline = state.dag.nodes[i]
                    .config
                    .stale_threshold
                    .and_then(|duration| now.checked_add(duration));
                let valid_until = earliest(
                    ticket
                        .inputs
                        .iter()
                        .filter_map(|input| input.value.valid_until)
                        .min(),
                    policy_deadline,
                );
                let (status, quality) = match &value {
                    NodeValue::Valuation(result) => {
                        (result.envelope().status(), result.envelope().quality())
                    }
                    // A curve node takes its own curve's quality. Any other node reads no
                    // snapshot input directly, and capture took each dependency Complete
                    // and Valid, so one stale bond never stops shared nodes (D58).
                    _ => (
                        ResultStatus::Complete,
                        match (&ticket.snapshot, &state.dag.nodes[i].id) {
                            (Some(snapshot), NodeId::Curve { curve_id }) => {
                                snapshot.curve_quality(curve_id.as_str())
                            }
                            _ => DataQuality::Valid,
                        },
                    ),
                };
                let cached = Arc::new(CachedValue {
                    value,
                    revision,
                    generation: ticket.generation,
                    calculated_at: now,
                    valid_until,
                    snapshot_id: ticket
                        .snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.id().clone()),
                    configuration: ticket.configuration.clone(),
                    dependencies: ticket
                        .inputs
                        .iter()
                        .map(|input| DependencyVersion {
                            node_id: input.node_id.clone(),
                            generation: input.value.generation,
                            revision: input.value.revision,
                        })
                        .collect(),
                    status,
                    quality,
                });
                state.result_revision = revision;
                let node = &mut state.nodes[i];
                node.revision = revision;
                node.dirty = false;
                node.forced = false;
                node.attempt = None;
                node.failure = None;
                node.blocked = false;
                node.failed_valuation = None;
                node.last_calc = Some(now);
                if status == ResultStatus::Complete && quality == DataQuality::Valid {
                    node.last_good = Some(cached.clone());
                }
                node.cached = Some(cached);
                Ok(CommitOutcome::Committed {
                    completion: CalculationCompletion::new(ticket, revision, status, quality, None),
                })
            }
        }
    }
    /// Cancel a current attempt without clearing dirty work; stale tickets cannot cancel newer work.
    pub fn cancel_attempt(&self, ticket: &CalculationTicket) -> bool {
        let mut state = self.state.lock();
        if !Arc::ptr_eq(&ticket.owner, &self.owner) || state.epoch != ticket.epoch {
            return false;
        }
        let Some(&i) = state.dag.indices.get(&ticket.node_id) else {
            return false;
        };
        if state.nodes[i].generation != ticket.generation
            || state.nodes[i].attempt.map(|(attempt, _)| attempt) != Some(ticket.attempt)
        {
            return false;
        }
        state.nodes[i].attempt = None;
        true
    }
    /// Current result only. Dirty, failed, blocked, expired or suspended values are never returned as fresh.
    pub fn get_cached(&self, id: &NodeId) -> Option<Arc<CachedValue>> {
        let state = self.state.lock();
        if state.suspended || state.snapshot_pending {
            return None;
        }
        state
            .dag
            .indices
            .get(id)
            .and_then(|&i| state.nodes[i].current(Instant::now()).cloned())
    }
    /// Explicit state and stale display value, preserving the last good calculation timestamp.
    pub fn inspect_node(&self, id: &NodeId) -> Option<NodeView> {
        let state = self.state.lock();
        let &i = state.dag.indices.get(id)?;
        let node = &state.nodes[i];
        let now = Instant::now();
        let mut failure = node.failure.clone();
        let phase = if state.suspended || state.snapshot_pending {
            NodePhase::Blocked
        } else if node
            .failure
            .as_ref()
            .is_some_and(|f| f.code == ErrorCode::StaleInput)
        {
            NodePhase::Stale
        } else if node.blocked {
            NodePhase::Blocked
        } else if node.failure.is_some() {
            NodePhase::Failed
        } else if node.attempt.is_some() {
            NodePhase::Running
        } else if let Some(current) = node.current(now) {
            if current.status == ResultStatus::Complete && current.quality == DataQuality::Valid {
                NodePhase::Fresh
            } else {
                NodePhase::Degraded
            }
        } else if node.cached.as_ref().is_some_and(|v| {
            v.generation == node.generation && v.valid_until.is_some_and(|deadline| now > deadline)
        }) {
            failure = Some(CalculationFailure::fixed(
                ErrorCode::StaleInput,
                "input/result freshness deadline expired",
            ));
            NodePhase::Stale
        } else if state.dag.nodes[i].role == NodeRole::Input {
            NodePhase::WaitingInput
        } else if let Some(&d) = state.dag.dependencies[i].iter().find(|&&d| {
            state.nodes[d].current(now).is_none_or(|v| {
                v.status != ResultStatus::Complete || v.quality != DataQuality::Valid
            })
        }) {
            failure = state.nodes[d].failure.clone().or_else(|| {
                Some(CalculationFailure::fixed(
                    ErrorCode::MissingMarketData,
                    "prerequisite has no usable complete result",
                ))
            });
            if let Some(failure) = &mut failure {
                failure.dependency = Some(state.dag.nodes[d].id.clone());
            }
            NodePhase::Blocked
        } else if node
            .cached
            .as_ref()
            .is_some_and(|v| v.valid_until.is_some_and(|deadline| now > deadline))
        {
            NodePhase::Stale
        } else {
            NodePhase::Pending
        };
        let retained_quality = node.last_good.as_ref().map(|_| {
            if phase == NodePhase::Fresh
                && node.current(now).is_some_and(|current| {
                    node.last_good
                        .as_ref()
                        .is_some_and(|last| current.revision == last.revision)
                })
            {
                DataQuality::Valid
            } else {
                DataQuality::Stale
            }
        });
        Some(NodeView {
            generation: node.generation,
            revision: node.revision,
            phase,
            failure,
            last_good: node.last_good.clone(),
            failed_valuation: node.failed_valuation.clone(),
            retained_quality,
        })
    }
    /// Prerequisite identities from the active validated DAG.
    pub fn get_dependencies(&self, id: &NodeId) -> Vec<NodeId> {
        self.node_spec(id)
            .map(|node| node.dependencies)
            .unwrap_or_default()
    }
    /// Unique current reverse edges, rebuilt on every administrative activation.
    pub fn get_dependents(&self, id: &NodeId) -> Vec<NodeId> {
        let state = self.state.lock();
        state
            .dag
            .indices
            .get(id)
            .map(|&i| {
                state.dag.dependents[i]
                    .iter()
                    .map(|&d| state.dag.nodes[d].id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
    /// Monotonic input generation (separate from committed result/status revisions).
    pub fn current_revision(&self) -> u64 {
        self.state.lock().generation
    }
    /// Current structural activation epoch.
    pub fn current_epoch(&self) -> u64 {
        self.state.lock().epoch
    }
    /// Active bounded node count.
    pub fn node_count(&self) -> usize {
        self.state.lock().nodes.len()
    }
    /// Claimed work count; abandoned attempts expire on the next admission pass.
    pub fn in_flight_count(&self) -> usize {
        self.state
            .lock()
            .nodes
            .iter()
            .filter(|node| node.attempt.is_some())
            .count()
    }
    /// Clear through a new epoch/generation. Counters and transport suspension never reset.
    pub fn clear(&self) -> Result<u64, GraphError> {
        self.activate(vec![])
    }
}
impl Default for CalculationGraph {
    fn default() -> Self {
        Self::new()
    }
}

fn next(value: u64) -> Result<u64, GraphError> {
    value.checked_add(1).ok_or(GraphError::CounterExhausted)
}
fn earliest(left: Option<Instant>, right: Option<Instant>) -> Option<Instant> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    }
}
fn frequency_ready(frequency: &UpdateFrequency, state: &NodeState, now: Instant) -> bool {
    state.forced
        || match frequency {
            UpdateFrequency::Immediate => true,
            UpdateFrequency::Throttled { interval } => state
                .last_calc
                .is_none_or(|time| now.saturating_duration_since(time) >= *interval),
            _ => false,
        }
}
fn can_be_input(id: &NodeId) -> bool {
    !matches!(
        id,
        NodeId::BondPrice { .. }
            | NodeId::EtfInav { .. }
            | NodeId::EtfNav { .. }
            | NodeId::Portfolio { .. }
    )
}
fn validate_spec(node: &NodeSpec) -> Result<(), GraphError> {
    if node.id.to_string().len() > 1024
        || (node.role == NodeRole::Input
            && (!node.dependencies.is_empty() || !can_be_input(&node.id)))
    {
        return Err(GraphError::InvalidNode(
            "invalid input role, identity or dependencies",
        ));
    }
    if node
        .config
        .stale_threshold
        .is_some_and(|duration| duration.is_zero() || duration > Duration::from_secs(86_400))
    {
        return Err(GraphError::InvalidNode("invalid node staleness threshold"));
    }
    match &node.config.frequency {
        UpdateFrequency::Throttled { interval } | UpdateFrequency::Interval { interval }
            if interval.is_zero() || *interval > Duration::from_secs(86_400) =>
        {
            Err(GraphError::InvalidNode("invalid update interval"))
        }
        UpdateFrequency::Scheduled { cron }
            if cron.is_empty() || cron.len() > 256 || cron.parse::<cron::Schedule>().is_err() =>
        {
            Err(GraphError::InvalidNode("invalid schedule length"))
        }
        UpdateFrequency::EndOfDay { time }
            if time.is_empty()
                || time.len() > 64
                || chrono::NaiveTime::parse_from_str(time, "%H:%M:%S").is_err() =>
        {
            Err(GraphError::InvalidNode("invalid end-of-day time length"))
        }
        _ => Ok(()),
    }
}
fn validate_value(id: &NodeId, value: &NodeValue) -> Result<(), GraphError> {
    let valid = match (id, value) {
        (NodeId::Quote { .. }, NodeValue::Quote { bid, ask, mid }) => {
            bid.is_some() || ask.is_some() || mid.is_some()
        }
        (NodeId::CurveInput { .. }, NodeValue::CurveInput { .. }) => true,
        (NodeId::Curve { .. }, NodeValue::Curve { points }) => {
            !points.is_empty()
                && points.len() <= 4096
                && points
                    .iter()
                    .all(|(tenor, rate)| *tenor > 0 && rate.is_finite())
                && points.windows(2).all(|p| p[0].0 < p[1].0)
        }
        (NodeId::VolSurface { .. }, NodeValue::VolSurface { atm_vols }) => {
            !atm_vols.is_empty()
                && atm_vols.len() <= 4096
                && atm_vols.iter().all(|(expiry, vol)| {
                    expiry.is_finite() && *expiry > 0.0 && *vol >= Decimal::ZERO
                })
                && atm_vols.windows(2).all(|p| p[0].0 < p[1].0)
        }
        (NodeId::FxRate { .. }, NodeValue::FxRate { mid }) => *mid > Decimal::ZERO,
        (NodeId::IndexFixing { .. }, NodeValue::IndexFixing { .. }) => true,
        (NodeId::InflationFixing { .. }, NodeValue::InflationFixing { value }) => {
            *value > Decimal::ZERO
        }
        (NodeId::Config { .. }, NodeValue::Config { .. }) => true,
        (
            NodeId::BondPrice { .. },
            NodeValue::BondPrice {
                clean_price_bid,
                clean_price_mid,
                clean_price_ask,
                ..
            },
        ) => clean_price_bid.is_some() || clean_price_mid.is_some() || clean_price_ask.is_some(),
        (NodeId::BondPrice { .. }, NodeValue::Valuation(result)) => result.validate().is_ok(),
        (NodeId::EtfInav { .. }, NodeValue::EtfInav { coverage, .. }) => {
            *coverage >= Decimal::ZERO && *coverage <= Decimal::ONE
        }
        (NodeId::EtfNav { .. }, NodeValue::EtfNav { .. })
        | (NodeId::Portfolio { .. }, NodeValue::Portfolio { .. }) => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(GraphError::InvalidNode(
            "value is absent, invalid or has the wrong node type",
        ))
    }
}
fn context_subject_matches(id: &NodeId, context: &ValuationContext) -> bool {
    matches!((id, &context.spec().subject), (NodeId::BondPrice { instrument_id: node }, convex_core::contracts::ValuationSubject::InstrumentVersion { instrument_id, .. }) if node.as_str() == instrument_id.as_str())
}
fn validate_ticket_value(ticket: &CalculationTicket, value: &NodeValue) -> Result<(), GraphError> {
    match (ticket.context.as_ref(), value) {
        (Some(context), NodeValue::Valuation(result)) if context == result.context() => Ok(()),
        (None, NodeValue::Valuation(_)) | (Some(_), _) => Err(GraphError::ContextConflict),
        _ => Ok(()),
    }
}
fn node_shard_key(node: &NodeId) -> String {
    match node {
        NodeId::Quote { instrument_id } | NodeId::BondPrice { instrument_id } => {
            instrument_id.to_string()
        }
        NodeId::Curve { curve_id } | NodeId::CurveInput { curve_id, .. } => curve_id.to_string(),
        NodeId::VolSurface { surface_id } => surface_id.to_string(),
        NodeId::FxRate { pair } => pair.to_string(),
        NodeId::IndexFixing { index, .. } => index.to_string(),
        NodeId::InflationFixing { index, .. } => index.to_string(),
        NodeId::Config { config_id } => config_id.clone(),
        NodeId::EtfInav { etf_id } | NodeId::EtfNav { etf_id } => etf_id.to_string(),
        NodeId::Portfolio { portfolio_id } => portfolio_id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_node_and_invalidate() {
        let graph = CalculationGraph::new();
        let curve = NodeId::Curve {
            curve_id: "USD.SOFR".into(),
        };
        let bond = NodeId::BondPrice {
            instrument_id: "US912810TD00".into(),
        };
        graph
            .upsert_nodes(vec![
                NodeSpec::input(curve.clone()),
                NodeSpec::calculation(bond.clone(), vec![curve.clone()]),
            ])
            .unwrap();
        assert!(graph.is_dirty(&curve));
        assert!(graph.is_dirty(&bond));
        graph
            .publish_input(
                &curve,
                NodeValue::Curve {
                    points: vec![(365, 0.04), (730, 0.04)],
                },
            )
            .unwrap();
        let ticket = graph.begin_calculation(&bond).unwrap();
        graph
            .commit_result(
                &ticket,
                Ok(NodeValue::BondPrice {
                    clean_price_bid: None,
                    clean_price_mid: Some(Decimal::from(99)),
                    clean_price_ask: None,
                    accrued_interest: None,
                    ytm_bid: None,
                    ytm_mid: None,
                    ytm_ask: None,
                    z_spread_mid: None,
                    modified_duration: None,
                    dv01: None,
                }),
            )
            .unwrap();
        assert!(!graph.is_dirty(&curve));
        assert!(!graph.is_dirty(&bond));
        graph.invalidate(&curve).unwrap();
        assert!(graph.is_dirty(&curve));
        assert!(graph.is_dirty(&bond));
        assert!(graph.get_cached(&curve).is_none());
        assert!(graph.get_cached(&bond).is_none());
    }

    #[test]
    fn counter_exhaustion_rejects_activation_input_and_batch_without_partial_mutation() {
        let graph = CalculationGraph::new();
        let a = NodeId::Curve {
            curve_id: "A".into(),
        };
        let b = NodeId::Curve {
            curve_id: "B".into(),
        };
        graph
            .activate(vec![
                NodeSpec::calculation(a.clone(), vec![]),
                NodeSpec::calculation(b, vec![]),
            ])
            .unwrap();
        graph.state.lock().next_attempt = u64::MAX - 1;
        assert!(matches!(
            graph.claim_ready(2),
            Err(GraphError::CounterExhausted)
        ));
        assert_eq!(graph.in_flight_count(), 0);
        assert_eq!(graph.get_dirty_nodes().len(), 2);
        let epoch = graph.current_epoch();
        graph.state.lock().generation = u64::MAX;
        assert_eq!(graph.clear().unwrap_err(), GraphError::CounterExhausted);
        assert_eq!(graph.current_epoch(), epoch);
        assert_eq!(graph.node_count(), 2);
        let new_input = NodeId::Curve {
            curve_id: "NEW".into(),
        };
        assert_eq!(
            graph
                .publish_input(
                    &new_input,
                    NodeValue::Curve {
                        points: vec![(365, 0.04)]
                    }
                )
                .unwrap_err(),
            GraphError::CounterExhausted
        );
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.current_epoch(), epoch);
        assert!(graph.node_spec(&new_input).is_none());
    }

    #[test]
    fn failed_status_revision_exhaustion_does_not_clear_current_work() {
        let graph = CalculationGraph::new();
        let a = NodeId::Curve {
            curve_id: "A".into(),
        };
        let b = NodeId::Curve {
            curve_id: "B".into(),
        };
        graph
            .activate(vec![
                NodeSpec::calculation(a.clone(), vec![]),
                NodeSpec::calculation(b.clone(), vec![a.clone()]),
            ])
            .unwrap();
        let ticket = graph.begin_calculation(&a).unwrap();
        graph.state.lock().result_revision = u64::MAX - 1;
        assert_eq!(
            graph
                .commit_result(
                    &ticket,
                    Err(CalculationFailure::fixed(
                        ErrorCode::CalibrationFailed,
                        "fixture failure"
                    ))
                )
                .unwrap_err(),
            GraphError::CounterExhausted
        );
        assert!(graph.is_dirty(&a));
        assert!(graph.is_dirty(&b));
        assert_eq!(graph.inspect_node(&a).unwrap().phase, NodePhase::Running);
        assert!(graph.get_cached(&a).is_none());
    }

    // ============ Sharding Tests ============

    #[test]
    fn test_single_shard_owns_everything() {
        let graph = CalculationGraph::new();

        // Single shard should own all nodes
        assert!(!graph.is_sharded());
        assert_eq!(graph.shard_id(), 0);
        assert_eq!(graph.total_shards(), 1);

        let bond_node = NodeId::BondPrice {
            instrument_id: InstrumentId::new("US912810TD00"),
        };
        assert!(graph.owns_node(&bond_node));
    }

    #[test]
    fn test_shard_config_hash_based() {
        let config = ShardConfig::hash_based(0, 4);

        assert!(!config.is_single_shard());

        // Same key always maps to same shard
        let shard1 = config.shard_for_key("US912810TD00");
        let shard2 = config.shard_for_key("US912810TD00");
        assert_eq!(shard1, shard2);

        // Keys should be distributed across shards
        let mut shard_counts = [0; 4];
        for i in 0..100 {
            let key = format!("INSTRUMENT_{}", i);
            let shard = config.shard_for_key(&key);
            shard_counts[shard as usize] += 1;
        }

        // Each shard should get some keys (rough distribution check)
        for count in shard_counts.iter() {
            assert!(*count > 0, "Shard should have at least one key");
        }
    }

    #[test]
    fn test_sharded_graph_owns_subset() {
        // Create 4 shards
        let shard_configs: Vec<_> = (0..4).map(|i| ShardConfig::hash_based(i, 4)).collect();

        // Test that exactly one shard owns each node
        let test_ids = [
            "US912810TD00",
            "US912810TE00",
            "US912810TF00",
            "XS123456789",
            "DE000ABC123",
        ];

        for id in test_ids.iter() {
            let _node = NodeId::BondPrice {
                instrument_id: InstrumentId::new(*id),
            };

            let owners: Vec<_> = shard_configs
                .iter()
                .enumerate()
                .filter(|(_, config)| config.owns_key(id))
                .map(|(i, _)| i)
                .collect();

            assert_eq!(owners.len(), 1, "Exactly one shard should own {}", id);
        }
    }

    #[test]
    fn test_shard_for_node() {
        let graph = CalculationGraph::with_sharding(ShardConfig::hash_based(0, 4)).unwrap();

        let node = NodeId::BondPrice {
            instrument_id: InstrumentId::new("US912810TD00"),
        };

        // Should return the correct shard
        let shard = graph.shard_for_node(&node);
        assert!(shard < 4);

        // Node should be owned only if it maps to shard 0
        assert_eq!(graph.owns_node(&node), shard == 0);
    }

    #[test]
    fn test_currency_based_sharding() {
        let usd_config = ShardConfig::by_currency(0, 2, vec!["USD".to_string(), "CAD".to_string()]);
        let eur_config = ShardConfig::by_currency(1, 2, vec!["EUR".to_string(), "GBP".to_string()]);

        assert!(usd_config.owns_currency("USD"));
        assert!(usd_config.owns_currency("CAD"));
        assert!(!usd_config.owns_currency("EUR"));

        assert!(eur_config.owns_currency("EUR"));
        assert!(eur_config.owns_currency("GBP"));
        assert!(!eur_config.owns_currency("USD"));
    }

    #[test]
    fn test_node_shard_key_extraction() {
        // Test various node types
        let bond_node = NodeId::BondPrice {
            instrument_id: InstrumentId::new("US912810TD00"),
        };
        assert_eq!(node_shard_key(&bond_node), "US912810TD00");

        let quote_node = NodeId::Quote {
            instrument_id: InstrumentId::new("XS123456789"),
        };
        assert_eq!(node_shard_key(&quote_node), "XS123456789");

        let curve_node = NodeId::Curve {
            curve_id: CurveId::new("USD.SOFR"),
        };
        assert_eq!(node_shard_key(&curve_node), "USD.SOFR");
    }
}
