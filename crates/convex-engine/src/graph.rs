//! Calculation graph with dependency tracking and incremental recalculation.
//!
//! The calculation graph is the core data structure that manages dependencies
//! between pricing inputs and outputs. When market data changes, it propagates
//! dirty flags and triggers recalculation in topological order.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_engine::graph::{CalculationGraph, NodeId};
//!
//! let graph = CalculationGraph::new();
//!
//! // Register nodes (see nodes module)
//! graph.register(curve_node);
//! graph.register(bond_node);
//!
//! // When market data changes
//! graph.invalidate(&NodeId::curve("USD.GOVT"));
//!
//! // Recalculate all dirty nodes
//! let recalculated = graph.recalculate();
//! ```

use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::{DashMap, DashSet};
use parking_lot::RwLock;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::toposort;
use petgraph::Direction;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineResult};
use crate::nodes::CalculationNode;

// =============================================================================
// NODE IDENTIFIER
// =============================================================================

/// Unique identifier for a node in the calculation graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeId {
    /// Curve node (e.g., "USD.GOVT", "EUR.SOFR.OIS").
    Curve(String),

    /// Bond pricing node (by instrument ID).
    Bond(String),

    /// Portfolio node.
    Portfolio(String),

    /// Raw quote input.
    Quote(String),

    /// Curve input instrument (e.g., deposit rate, swap rate).
    CurveInput {
        /// Curve this input belongs to.
        curve_id: String,
        /// Instrument identifier.
        instrument: String,
    },

    /// Custom calculation node.
    Custom(String),
}

impl NodeId {
    /// Creates a curve node ID.
    pub fn curve(id: impl Into<String>) -> Self {
        Self::Curve(id.into())
    }

    /// Creates a bond node ID.
    pub fn bond(id: impl Into<String>) -> Self {
        Self::Bond(id.into())
    }

    /// Creates a portfolio node ID.
    pub fn portfolio(id: impl Into<String>) -> Self {
        Self::Portfolio(id.into())
    }

    /// Creates a quote node ID.
    pub fn quote(id: impl Into<String>) -> Self {
        Self::Quote(id.into())
    }

    /// Creates a curve input node ID.
    pub fn curve_input(curve_id: impl Into<String>, instrument: impl Into<String>) -> Self {
        Self::CurveInput {
            curve_id: curve_id.into(),
            instrument: instrument.into(),
        }
    }

    /// Creates a custom node ID.
    pub fn custom(id: impl Into<String>) -> Self {
        Self::Custom(id.into())
    }

    /// Returns the string representation of the node ID.
    pub fn as_str(&self) -> String {
        match self {
            Self::Curve(id) => format!("curve:{}", id),
            Self::Bond(id) => format!("bond:{}", id),
            Self::Portfolio(id) => format!("portfolio:{}", id),
            Self::Quote(id) => format!("quote:{}", id),
            Self::CurveInput { curve_id, instrument } => {
                format!("curve_input:{}:{}", curve_id, instrument)
            }
            Self::Custom(id) => format!("custom:{}", id),
        }
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// NODE VALUE
// =============================================================================

/// Value stored by a calculation node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeValue {
    /// No value (not yet calculated or error).
    Empty,

    /// Curve reference (stored in CurveCache).
    CurveRef {
        /// Curve identifier.
        curve_id: String,
        /// Version/revision of the curve.
        version: u64,
        /// Build timestamp.
        build_time: DateTime<Utc>,
    },

    /// Bond analytics result.
    BondAnalytics {
        /// Instrument ID.
        instrument_id: String,
        /// Clean price.
        clean_price: Option<Decimal>,
        /// Dirty price.
        dirty_price: Option<Decimal>,
        /// Yield to maturity.
        ytm: Option<Decimal>,
        /// Z-spread in basis points.
        z_spread: Option<Decimal>,
        /// Modified duration.
        modified_duration: Option<Decimal>,
        /// DV01.
        dv01: Option<Decimal>,
        /// Calculation timestamp.
        timestamp: DateTime<Utc>,
    },

    /// Portfolio analytics result.
    PortfolioAnalytics {
        /// Portfolio ID.
        portfolio_id: String,
        /// Total NAV.
        nav: Decimal,
        /// Weighted average duration.
        duration: Decimal,
        /// Total DV01.
        dv01: Decimal,
        /// Calculation timestamp.
        timestamp: DateTime<Utc>,
    },

    /// Raw quote value.
    Quote {
        /// Instrument ID.
        instrument_id: String,
        /// Bid price.
        bid: Option<Decimal>,
        /// Ask price.
        ask: Option<Decimal>,
        /// Mid price.
        mid: Option<Decimal>,
        /// Quote timestamp.
        timestamp: DateTime<Utc>,
    },

    /// Curve input value (rate).
    CurveInputValue {
        /// Rate value.
        rate: f64,
        /// Source.
        source: Option<String>,
        /// Timestamp.
        timestamp: DateTime<Utc>,
    },

    /// Generic decimal value.
    Decimal(Decimal),

    /// Generic floating point value.
    Float(f64),

    /// Custom serialized value.
    Custom(serde_json::Value),
}

impl NodeValue {
    /// Returns true if the value is empty.
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Creates a bond analytics value.
    pub fn bond_analytics(instrument_id: impl Into<String>) -> Self {
        Self::BondAnalytics {
            instrument_id: instrument_id.into(),
            clean_price: None,
            dirty_price: None,
            ytm: None,
            z_spread: None,
            modified_duration: None,
            dv01: None,
            timestamp: Utc::now(),
        }
    }

    /// Creates a quote value.
    pub fn quote(
        instrument_id: impl Into<String>,
        bid: Option<Decimal>,
        ask: Option<Decimal>,
    ) -> Self {
        let mid = match (bid, ask) {
            (Some(b), Some(a)) => Some((b + a) / Decimal::from(2)),
            (Some(b), None) => Some(b),
            (None, Some(a)) => Some(a),
            (None, None) => None,
        };
        Self::Quote {
            instrument_id: instrument_id.into(),
            bid,
            ask,
            mid,
            timestamp: Utc::now(),
        }
    }
}

// =============================================================================
// REVISION TRACKING
// =============================================================================

/// Revision number for cache invalidation tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct Revision(pub u64);

impl Revision {
    /// Creates a new revision.
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the next revision.
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

impl std::fmt::Display for Revision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rev:{}", self.0)
    }
}

// =============================================================================
// CACHED VALUE
// =============================================================================

/// A cached value with revision tracking.
#[derive(Debug, Clone)]
pub struct CachedValue {
    /// The cached value.
    pub value: NodeValue,
    /// Revision when this value was computed.
    pub revision: Revision,
    /// Timestamp when computed.
    pub computed_at: DateTime<Utc>,
    /// Computation duration in microseconds.
    pub compute_time_us: u64,
}

impl CachedValue {
    /// Creates a new cached value.
    pub fn new(value: NodeValue, revision: Revision) -> Self {
        Self {
            value,
            revision,
            computed_at: Utc::now(),
            compute_time_us: 0,
        }
    }

    /// Creates a cached value with timing.
    pub fn with_timing(value: NodeValue, revision: Revision, compute_time_us: u64) -> Self {
        Self {
            value,
            revision,
            computed_at: Utc::now(),
            compute_time_us,
        }
    }
}

// =============================================================================
// CALCULATION CONTEXT
// =============================================================================

/// Context passed to nodes during calculation.
pub struct CalculationContext<'a> {
    /// Reference to the graph for accessing other nodes.
    pub graph: &'a CalculationGraph,
    /// Current revision.
    pub revision: Revision,
    /// Settlement date for pricing.
    pub settlement_date: Option<chrono::NaiveDate>,
}

impl<'a> CalculationContext<'a> {
    /// Gets a dependency value from the graph.
    pub fn get_dependency(&self, node_id: &NodeId) -> Option<NodeValue> {
        self.graph.get_cached(node_id)
    }

    /// Gets a curve from the cache.
    pub fn get_curve(&self, curve_id: &str) -> Option<NodeValue> {
        self.graph.get_cached(&NodeId::curve(curve_id))
    }
}

// =============================================================================
// CALCULATION GRAPH
// =============================================================================

/// The calculation graph manages dependencies between pricing inputs and outputs.
///
/// When market data changes, it propagates dirty flags to all dependent nodes
/// and recalculates them in topological order.
pub struct CalculationGraph {
    /// Directed graph of node dependencies.
    graph: RwLock<DiGraph<NodeId, ()>>,

    /// Mapping from NodeId to graph index.
    node_indices: DashMap<NodeId, NodeIndex>,

    /// Node implementations.
    nodes: DashMap<NodeId, Arc<dyn CalculationNode>>,

    /// Memoized values with revision tracking.
    cache: DashMap<NodeId, CachedValue>,

    /// Set of dirty nodes pending recalculation.
    dirty: DashSet<NodeId>,

    /// Current global revision.
    current_revision: AtomicU64,

    /// Whether the graph is currently recalculating.
    recalculating: AtomicU64,
}

impl CalculationGraph {
    /// Creates a new empty calculation graph.
    pub fn new() -> Self {
        Self {
            graph: RwLock::new(DiGraph::new()),
            node_indices: DashMap::new(),
            nodes: DashMap::new(),
            cache: DashMap::new(),
            dirty: DashSet::new(),
            current_revision: AtomicU64::new(0),
            recalculating: AtomicU64::new(0),
        }
    }

    /// Returns the current revision.
    pub fn revision(&self) -> Revision {
        Revision(self.current_revision.load(Ordering::SeqCst))
    }

    /// Bumps the revision and returns the new value.
    fn bump_revision(&self) -> Revision {
        Revision(self.current_revision.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Registers a calculation node in the graph.
    ///
    /// If a node with the same ID already exists, it will be replaced.
    pub fn register(&self, node: Arc<dyn CalculationNode>) {
        let node_id = node.node_id();
        let dependencies = node.dependencies();

        // Add to nodes map
        self.nodes.insert(node_id.clone(), node);

        // Add to graph
        let mut graph = self.graph.write();
        let idx = if let Some(existing) = self.node_indices.get(&node_id) {
            *existing
        } else {
            let idx = graph.add_node(node_id.clone());
            self.node_indices.insert(node_id.clone(), idx);
            idx
        };

        // Add dependency edges
        for dep in dependencies {
            if let Some(dep_idx) = self.node_indices.get(&dep) {
                // Edge from dependency to this node
                graph.add_edge(*dep_idx, idx, ());
            }
        }

        // Mark as dirty
        self.dirty.insert(node_id);
    }

    /// Unregisters a node from the graph.
    pub fn unregister(&self, node_id: &NodeId) -> bool {
        if self.nodes.remove(node_id).is_some() {
            if let Some((_, idx)) = self.node_indices.remove(node_id) {
                let mut graph = self.graph.write();
                graph.remove_node(idx);
            }
            self.cache.remove(node_id);
            self.dirty.remove(node_id);
            true
        } else {
            false
        }
    }

    /// Marks a node as dirty and propagates to all dependents.
    ///
    /// This should be called when an input value changes (e.g., market data update).
    pub fn invalidate(&self, node_id: &NodeId) {
        let rev = self.bump_revision();
        self.dirty.insert(node_id.clone());

        // Propagate to dependents
        self.propagate_dirty(node_id);

        tracing::debug!(
            node = %node_id,
            revision = rev.0,
            "Node invalidated"
        );
    }

    /// Propagates dirty flag to all dependent nodes (downstream).
    fn propagate_dirty(&self, node_id: &NodeId) {
        // Collect dependent node IDs while holding the lock
        let dependent_ids: Vec<NodeId> = {
            let graph = self.graph.read();
            if let Some(idx) = self.node_indices.get(node_id) {
                // Get all nodes that depend on this one (outgoing edges)
                graph
                    .neighbors_directed(*idx, Direction::Outgoing)
                    .filter_map(|dep_idx| graph.node_weight(dep_idx).cloned())
                    .collect()
            } else {
                Vec::new()
            }
        };

        // Now propagate to each dependent (lock released)
        for dep_id in dependent_ids {
            if self.dirty.insert(dep_id.clone()) {
                // Recursively propagate
                self.propagate_dirty(&dep_id);
            }
        }
    }

    /// Returns all nodes that the given node depends on.
    pub fn dependencies_of(&self, node_id: &NodeId) -> Vec<NodeId> {
        let graph = self.graph.read();
        if let Some(idx) = self.node_indices.get(node_id) {
            graph
                .neighbors_directed(*idx, Direction::Incoming)
                .filter_map(|dep_idx| graph.node_weight(dep_idx).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Returns all nodes that depend on the given node.
    pub fn dependents_of(&self, node_id: &NodeId) -> Vec<NodeId> {
        let graph = self.graph.read();
        if let Some(idx) = self.node_indices.get(node_id) {
            graph
                .neighbors_directed(*idx, Direction::Outgoing)
                .filter_map(|dep_idx| graph.node_weight(dep_idx).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Gets the cached value for a node without recalculation.
    pub fn get_cached(&self, node_id: &NodeId) -> Option<NodeValue> {
        self.cache.get(node_id).map(|v| v.value.clone())
    }

    /// Gets the cached value with metadata.
    pub fn get_cached_with_meta(&self, node_id: &NodeId) -> Option<CachedValue> {
        self.cache.get(node_id).map(|v| v.clone())
    }

    /// Checks if a node is dirty.
    pub fn is_dirty(&self, node_id: &NodeId) -> bool {
        self.dirty.contains(node_id)
    }

    /// Returns the number of dirty nodes.
    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
    }

    /// Recalculates all dirty nodes in topological order.
    ///
    /// Returns the list of node IDs that were recalculated.
    pub fn recalculate(&self) -> EngineResult<Vec<NodeId>> {
        // Prevent concurrent recalculation
        if self.recalculating.swap(1, Ordering::SeqCst) != 0 {
            return Ok(Vec::new());
        }

        let result = self.do_recalculate();

        self.recalculating.store(0, Ordering::SeqCst);
        result
    }

    fn do_recalculate(&self) -> EngineResult<Vec<NodeId>> {
        // Collect dirty nodes
        let dirty_nodes: Vec<NodeId> = self.dirty.iter().map(|r| r.clone()).collect();

        if dirty_nodes.is_empty() {
            return Ok(Vec::new());
        }

        // Get topological order of the full graph
        let graph = self.graph.read();
        let sorted = toposort(&*graph, None)
            .map_err(|_| EngineError::CircularDependency("Graph contains a cycle".into()))?;

        // Filter to only dirty nodes, in topological order
        let sorted_dirty: Vec<NodeId> = sorted
            .into_iter()
            .filter_map(|idx| graph.node_weight(idx).cloned())
            .filter(|id| self.dirty.contains(id))
            .collect();

        drop(graph);

        let revision = self.revision();
        let ctx = CalculationContext {
            graph: self,
            revision,
            settlement_date: None,
        };

        let mut recalculated = Vec::new();

        for node_id in sorted_dirty {
            // Ensure all dependencies are fresh
            let deps = self.dependencies_of(&node_id);
            for dep in &deps {
                if self.dirty.contains(dep) {
                    // Dependency still dirty - skip for now (will be handled in order)
                    continue;
                }
            }

            // Get the node implementation
            if let Some(node) = self.nodes.get(&node_id) {
                let start = std::time::Instant::now();

                // Calculate
                match node.calculate(&ctx) {
                    Ok(value) => {
                        let elapsed_us = start.elapsed().as_micros() as u64;
                        let cached = CachedValue::with_timing(value, revision, elapsed_us);
                        self.cache.insert(node_id.clone(), cached);
                        self.dirty.remove(&node_id);
                        recalculated.push(node_id.clone());

                        tracing::trace!(
                            node = %node_id,
                            elapsed_us = elapsed_us,
                            "Node recalculated"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            node = %node_id,
                            error = %e,
                            "Node calculation failed"
                        );
                        // Keep node dirty for retry
                    }
                }
            }
        }

        tracing::debug!(
            count = recalculated.len(),
            revision = revision.0,
            "Recalculation complete"
        );

        Ok(recalculated)
    }

    /// Forces recalculation of a specific node and its dependents.
    pub fn force_recalculate(&self, node_id: &NodeId) -> EngineResult<Vec<NodeId>> {
        self.invalidate(node_id);
        self.recalculate()
    }

    /// Clears all cached values and marks all nodes as dirty.
    pub fn clear_cache(&self) {
        self.cache.clear();
        for node in self.nodes.iter() {
            self.dirty.insert(node.key().clone());
        }
    }

    /// Returns statistics about the graph.
    pub fn stats(&self) -> GraphStats {
        GraphStats {
            node_count: self.nodes.len(),
            edge_count: self.graph.read().edge_count(),
            dirty_count: self.dirty.len(),
            cached_count: self.cache.len(),
            current_revision: self.revision(),
        }
    }

    /// Returns all node IDs in the graph.
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.nodes.iter().map(|r| r.key().clone()).collect()
    }
}

impl Default for CalculationGraph {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// GRAPH STATISTICS
// =============================================================================

/// Statistics about the calculation graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    /// Number of nodes.
    pub node_count: usize,
    /// Number of edges (dependencies).
    pub edge_count: usize,
    /// Number of dirty nodes.
    pub dirty_count: usize,
    /// Number of cached values.
    pub cached_count: usize,
    /// Current revision.
    pub current_revision: Revision,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_creation() {
        let curve_id = NodeId::curve("USD.GOVT");
        assert_eq!(curve_id.as_str(), "curve:USD.GOVT");

        let bond_id = NodeId::bond("US912828Z229");
        assert_eq!(bond_id.as_str(), "bond:US912828Z229");

        let input_id = NodeId::curve_input("USD.SOFR", "2Y_SWAP");
        assert_eq!(input_id.as_str(), "curve_input:USD.SOFR:2Y_SWAP");
    }

    #[test]
    fn test_revision() {
        let rev = Revision::new(5);
        assert_eq!(rev.0, 5);
        assert_eq!(rev.next().0, 6);
    }

    #[test]
    fn test_graph_creation() {
        let graph = CalculationGraph::new();
        assert_eq!(graph.stats().node_count, 0);
        assert_eq!(graph.revision().0, 0);
    }

    #[test]
    fn test_node_value_quote() {
        use rust_decimal_macros::dec;

        let quote = NodeValue::quote("TEST", Some(dec!(99.50)), Some(dec!(100.50)));
        if let NodeValue::Quote { mid, .. } = quote {
            assert_eq!(mid, Some(dec!(100.00)));
        } else {
            panic!("Expected Quote variant");
        }
    }
}
