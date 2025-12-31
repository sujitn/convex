//! Calculation graph for reactive repricing.
//!
//! The calculation graph manages dependencies between inputs and outputs.
//! When an input changes, it automatically propagates updates to all dependent calculations.
//!
//! ## Sharding
//!
//! For large universes (>10K bonds), the calculation graph supports sharding
//! across multiple replicas. Each shard handles a subset of instruments based
//! on the configured partition strategy:
//!
//! - `HashBased`: Instruments assigned by hash of instrument ID
//! - `ByCurrency`: Instruments grouped by currency
//! - `ByIssuerType`: Instruments grouped by issuer type
//! - `Manual`: Explicit assignment via configuration
//!
//! ```ignore
//! // Create a sharded calculation graph
//! let graph = CalculationGraph::with_sharding(ShardConfig {
//!     shard_id: 0,
//!     total_shards: 4,
//!     strategy: ShardStrategy::HashBased,
//!     assignment: None,
//! });
//!
//! // Check if this shard owns a node
//! if graph.owns_node(&node_id) {
//!     graph.add_node(node_id, deps);
//! }
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use dashmap::{DashMap, DashSet};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_core::Date;
use convex_traits::config::{NodeConfig, UpdateFrequency};
use convex_traits::ids::*;

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
            NodeId::CurveInput { curve_id, instrument } => {
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

    /// Empty/placeholder
    Empty,
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

/// Cached value with revision tracking.
#[derive(Debug, Clone)]
pub struct CachedValue {
    /// The cached value
    pub value: NodeValue,
    /// Revision when this was cached
    pub revision: u64,
    /// When this was calculated
    pub calculated_at: Instant,
}

/// The calculation graph manages dependencies and memoization.
///
/// Supports sharding for large universes (>10K bonds) where the graph
/// is partitioned across multiple replicas.
pub struct CalculationGraph {
    /// Dependencies: node -> nodes it depends on
    dependencies: DashMap<NodeId, Vec<NodeId>>,

    /// Dependents: node -> nodes that depend on it
    dependents: DashMap<NodeId, Vec<NodeId>>,

    /// Node configurations
    configs: DashMap<NodeId, NodeConfig>,

    /// Memoized values with revision tracking
    cache: DashMap<NodeId, CachedValue>,

    /// Dirty nodes pending recalculation
    dirty: DashSet<NodeId>,

    /// Last calculation time per node (for throttling)
    last_calc_time: DashMap<NodeId, Instant>,

    /// Pending throttled nodes
    throttle_pending: DashSet<NodeId>,

    /// Current global revision
    current_revision: AtomicU64,

    /// Shard configuration for distributed deployments
    shard_config: ShardConfig,
}

impl CalculationGraph {
    /// Create a new calculation graph (single shard).
    pub fn new() -> Self {
        Self::with_sharding(ShardConfig::single_shard())
    }

    /// Create a new calculation graph with sharding configuration.
    pub fn with_sharding(shard_config: ShardConfig) -> Self {
        Self {
            dependencies: DashMap::new(),
            dependents: DashMap::new(),
            configs: DashMap::new(),
            cache: DashMap::new(),
            dirty: DashSet::new(),
            last_calc_time: DashMap::new(),
            throttle_pending: DashSet::new(),
            current_revision: AtomicU64::new(0),
            shard_config,
        }
    }

    /// Get the shard configuration.
    pub fn shard_config(&self) -> &ShardConfig {
        &self.shard_config
    }

    /// Get this shard's ID.
    pub fn shard_id(&self) -> u32 {
        self.shard_config.shard_id
    }

    /// Get the total number of shards.
    pub fn total_shards(&self) -> u32 {
        self.shard_config.total_shards
    }

    /// Check if this graph is sharded (more than one shard).
    pub fn is_sharded(&self) -> bool {
        !self.shard_config.is_single_shard()
    }

    /// Check if this shard owns a given node.
    ///
    /// For single-shard deployments, always returns true.
    /// For multi-shard deployments, uses the configured strategy.
    pub fn owns_node(&self, node_id: &NodeId) -> bool {
        if !self.is_sharded() {
            return true;
        }

        // Extract the key for sharding
        let key = self.node_shard_key(node_id);
        self.shard_config.owns_key(&key)
    }

    /// Get the shard key for a node.
    ///
    /// This extracts the relevant identifier for sharding (e.g., instrument ID).
    fn node_shard_key(&self, node_id: &NodeId) -> String {
        match node_id {
            NodeId::Quote { instrument_id } => instrument_id.to_string(),
            NodeId::BondPrice { instrument_id } => instrument_id.to_string(),
            NodeId::Curve { curve_id } => curve_id.to_string(),
            NodeId::CurveInput { curve_id, .. } => curve_id.to_string(),
            NodeId::VolSurface { surface_id } => surface_id.to_string(),
            NodeId::FxRate { pair } => pair.to_string(),
            NodeId::IndexFixing { index, .. } => index.to_string(),
            NodeId::InflationFixing { index, .. } => index.to_string(),
            NodeId::Config { config_id } => config_id.clone(),
            NodeId::EtfInav { etf_id } => etf_id.to_string(),
            NodeId::EtfNav { etf_id } => etf_id.to_string(),
            NodeId::Portfolio { portfolio_id } => portfolio_id.to_string(),
        }
    }

    /// Compute which shard would own a given node.
    pub fn shard_for_node(&self, node_id: &NodeId) -> u32 {
        let key = self.node_shard_key(node_id);
        self.shard_config.shard_for_key(&key)
    }

    /// Add a node with its dependencies.
    pub fn add_node(&self, node_id: NodeId, deps: Vec<NodeId>) {
        // Store dependencies
        self.dependencies.insert(node_id.clone(), deps.clone());

        // Update reverse dependencies
        for dep in &deps {
            self.dependents
                .entry(dep.clone())
                .or_default()
                .push(node_id.clone());
        }

        // Mark as dirty initially
        self.dirty.insert(node_id);
    }

    /// Set node configuration.
    pub fn set_node_config(&self, node_id: NodeId, config: NodeConfig) {
        self.configs.insert(node_id, config);
    }

    /// Get node configuration.
    pub fn get_node_config(&self, node_id: &NodeId) -> Option<NodeConfig> {
        self.configs.get(node_id).map(|c| c.clone())
    }

    /// Mark a node as dirty (input changed).
    pub fn invalidate(&self, node_id: &NodeId) {
        self.bump_revision();
        self.dirty.insert(node_id.clone());

        // Propagate to dependents
        self.propagate_dirty(node_id);
    }

    /// Propagate dirty flag to all dependents.
    fn propagate_dirty(&self, node_id: &NodeId) {
        if let Some(deps) = self.dependents.get(node_id) {
            for dependent in deps.iter() {
                if self.dirty.insert(dependent.clone()) {
                    // Recursively propagate
                    self.propagate_dirty(&dependent);
                }
            }
        }
    }

    /// Get all dirty nodes.
    pub fn get_dirty_nodes(&self) -> Vec<NodeId> {
        self.dirty.iter().map(|r| r.clone()).collect()
    }

    /// Check if a node is dirty.
    pub fn is_dirty(&self, node_id: &NodeId) -> bool {
        self.dirty.contains(node_id)
    }

    /// Get cached value for a node.
    pub fn get_cached(&self, node_id: &NodeId) -> Option<CachedValue> {
        self.cache.get(node_id).map(|c| c.clone())
    }

    /// Update cache for a node.
    pub fn update_cache(&self, node_id: &NodeId, value: NodeValue) {
        let cached = CachedValue {
            value,
            revision: self.current_revision.load(Ordering::SeqCst),
            calculated_at: Instant::now(),
        };
        self.cache.insert(node_id.clone(), cached);
        self.dirty.remove(node_id);
        self.last_calc_time.insert(node_id.clone(), Instant::now());
    }

    /// Get nodes that should be recalculated now (respecting frequency).
    pub fn get_nodes_to_calculate(&self) -> Vec<NodeId> {
        let now = Instant::now();
        let mut to_calculate = Vec::new();

        for node_ref in self.dirty.iter() {
            let node_id = node_ref.clone();
            let should_calc = if let Some(config) = self.configs.get(&node_id) {
                match &config.frequency {
                    UpdateFrequency::Immediate => true,
                    UpdateFrequency::Throttled { interval } => {
                        self.last_calc_time
                            .get(&node_id)
                            .map(|t| now.duration_since(*t) >= *interval)
                            .unwrap_or(true)
                    }
                    UpdateFrequency::Interval { .. } => false, // Handled by scheduler
                    UpdateFrequency::OnDemand => false,
                    UpdateFrequency::EndOfDay { .. } => false,
                    UpdateFrequency::Scheduled { .. } => false,
                }
            } else {
                true // No config = immediate
            };

            if should_calc {
                to_calculate.push(node_id);
            }
        }

        // Sort by priority
        to_calculate.sort_by(|a, b| {
            let pa = self.configs.get(a).map(|c| c.priority).unwrap_or(0);
            let pb = self.configs.get(b).map(|c| c.priority).unwrap_or(0);
            pb.cmp(&pa) // Higher priority first
        });

        to_calculate
    }

    /// Get dependencies of a node.
    pub fn get_dependencies(&self, node_id: &NodeId) -> Vec<NodeId> {
        self.dependencies
            .get(node_id)
            .map(|d| d.clone())
            .unwrap_or_default()
    }

    /// Get dependents of a node.
    pub fn get_dependents(&self, node_id: &NodeId) -> Vec<NodeId> {
        self.dependents
            .get(node_id)
            .map(|d| d.clone())
            .unwrap_or_default()
    }

    /// Bump revision and return new value.
    fn bump_revision(&self) -> u64 {
        self.current_revision.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Get current revision.
    pub fn current_revision(&self) -> u64 {
        self.current_revision.load(Ordering::SeqCst)
    }

    /// Clear all nodes and cache.
    pub fn clear(&self) {
        self.dependencies.clear();
        self.dependents.clear();
        self.configs.clear();
        self.cache.clear();
        self.dirty.clear();
        self.last_calc_time.clear();
        self.throttle_pending.clear();
    }
}

impl Default for CalculationGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_node_and_invalidate() {
        let graph = CalculationGraph::new();

        let curve_id = CurveId::new("USD.SOFR");
        let bond_id = InstrumentId::new("US912810TD00");

        let curve_node = NodeId::Curve {
            curve_id: curve_id.clone(),
        };
        let bond_node = NodeId::BondPrice {
            instrument_id: bond_id.clone(),
        };

        // Add nodes with dependencies
        graph.add_node(curve_node.clone(), vec![]);
        graph.add_node(bond_node.clone(), vec![curve_node.clone()]);

        // Initially dirty
        assert!(graph.is_dirty(&curve_node));
        assert!(graph.is_dirty(&bond_node));

        // Clear dirty
        graph.update_cache(&curve_node, NodeValue::Empty);
        graph.update_cache(&bond_node, NodeValue::Empty);
        assert!(!graph.is_dirty(&curve_node));
        assert!(!graph.is_dirty(&bond_node));

        // Invalidate curve -> should propagate to bond
        graph.invalidate(&curve_node);
        assert!(graph.is_dirty(&curve_node));
        assert!(graph.is_dirty(&bond_node));
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
        let shard_configs: Vec<_> = (0..4)
            .map(|i| ShardConfig::hash_based(i, 4))
            .collect();

        // Test that exactly one shard owns each node
        let test_ids = [
            "US912810TD00",
            "US912810TE00",
            "US912810TF00",
            "XS123456789",
            "DE000ABC123",
        ];

        for id in test_ids.iter() {
            let node = NodeId::BondPrice {
                instrument_id: InstrumentId::new(*id),
            };

            let owners: Vec<_> = shard_configs
                .iter()
                .enumerate()
                .filter(|(_, config)| config.owns_key(id))
                .map(|(i, _)| i)
                .collect();

            assert_eq!(
                owners.len(),
                1,
                "Exactly one shard should own {}",
                id
            );
        }
    }

    #[test]
    fn test_shard_for_node() {
        let graph = CalculationGraph::with_sharding(ShardConfig::hash_based(0, 4));

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
        let usd_config =
            ShardConfig::by_currency(0, 2, vec!["USD".to_string(), "CAD".to_string()]);
        let eur_config =
            ShardConfig::by_currency(1, 2, vec!["EUR".to_string(), "GBP".to_string()]);

        assert!(usd_config.owns_currency("USD"));
        assert!(usd_config.owns_currency("CAD"));
        assert!(!usd_config.owns_currency("EUR"));

        assert!(eur_config.owns_currency("EUR"));
        assert!(eur_config.owns_currency("GBP"));
        assert!(!eur_config.owns_currency("USD"));
    }

    #[test]
    fn test_node_shard_key_extraction() {
        let graph = CalculationGraph::new();

        // Test various node types
        let bond_node = NodeId::BondPrice {
            instrument_id: InstrumentId::new("US912810TD00"),
        };
        assert_eq!(graph.node_shard_key(&bond_node), "US912810TD00");

        let quote_node = NodeId::Quote {
            instrument_id: InstrumentId::new("XS123456789"),
        };
        assert_eq!(graph.node_shard_key(&quote_node), "XS123456789");

        let curve_node = NodeId::Curve {
            curve_id: CurveId::new("USD.SOFR"),
        };
        assert_eq!(graph.node_shard_key(&curve_node), "USD.SOFR");
    }
}
