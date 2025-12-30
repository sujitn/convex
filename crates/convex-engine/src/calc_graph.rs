//! Calculation graph for reactive repricing.
//!
//! The calculation graph manages dependencies between inputs and outputs.
//! When an input changes, it automatically propagates updates to all dependent calculations.

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
        /// Clean price
        clean_price: Option<Decimal>,
        /// Dirty price
        dirty_price: Option<Decimal>,
        /// YTM
        ytm: Option<Decimal>,
        /// Z-spread
        z_spread: Option<Decimal>,
        /// Modified duration
        modified_duration: Option<Decimal>,
        /// DV01
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
}

impl CalculationGraph {
    /// Create a new calculation graph.
    pub fn new() -> Self {
        Self {
            dependencies: DashMap::new(),
            dependents: DashMap::new(),
            configs: DashMap::new(),
            cache: DashMap::new(),
            dirty: DashSet::new(),
            last_calc_time: DashMap::new(),
            throttle_pending: DashSet::new(),
            current_revision: AtomicU64::new(0),
        }
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
}
