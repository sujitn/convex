//! Calculation node trait and implementations.
//!
//! Nodes are the building blocks of the calculation graph. Each node represents
//! a calculable entity (curve, bond price, portfolio analytics, etc.) and declares
//! its dependencies on other nodes.

use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::EngineResult;
use crate::graph::{CalculationContext, NodeId, NodeValue};

// =============================================================================
// CALCULATION NODE TRAIT
// =============================================================================

/// Trait for nodes in the calculation graph.
///
/// Each node has a unique identifier, declares its dependencies, and can be
/// calculated given a context.
pub trait CalculationNode: Send + Sync + std::fmt::Debug {
    /// Returns the unique identifier for this node.
    fn node_id(&self) -> NodeId;

    /// Returns the node type.
    fn node_type(&self) -> NodeType;

    /// Returns the IDs of all nodes this node depends on.
    fn dependencies(&self) -> Vec<NodeId>;

    /// Calculates the node's value given the context.
    ///
    /// The context provides access to dependency values and global state.
    fn calculate(&self, ctx: &CalculationContext<'_>) -> EngineResult<NodeValue>;

    /// Returns an estimated priority for calculation ordering.
    ///
    /// Lower values are calculated first. Default is 100.
    fn priority(&self) -> u32 {
        100
    }

    /// Returns true if this node can be calculated in parallel with other nodes.
    ///
    /// Nodes that access external resources or have side effects should return false.
    fn is_parallelizable(&self) -> bool {
        true
    }
}

// =============================================================================
// NODE TYPE
// =============================================================================

/// Type of calculation node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    /// Yield curve node.
    Curve,
    /// Bond pricing node.
    BondPricing,
    /// Portfolio analytics node.
    Portfolio,
    /// Market quote input node.
    Quote,
    /// Curve input instrument node.
    CurveInput,
    /// Custom calculation node.
    Custom,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Curve => write!(f, "Curve"),
            Self::BondPricing => write!(f, "BondPricing"),
            Self::Portfolio => write!(f, "Portfolio"),
            Self::Quote => write!(f, "Quote"),
            Self::CurveInput => write!(f, "CurveInput"),
            Self::Custom => write!(f, "Custom"),
        }
    }
}

// =============================================================================
// CURVE NODE
// =============================================================================

/// Configuration for building a curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveConfig {
    /// Curve identifier (e.g., "USD.GOVT", "EUR.SOFR.OIS").
    pub curve_id: String,
    /// Currency.
    pub currency: String,
    /// Curve type (e.g., "government", "ois", "swap").
    pub curve_type: String,
    /// Interpolation method.
    pub interpolation: String,
    /// Reference date for the curve.
    pub reference_date: Option<NaiveDate>,
}

impl CurveConfig {
    /// Creates a new curve configuration.
    pub fn new(curve_id: impl Into<String>, currency: impl Into<String>) -> Self {
        Self {
            curve_id: curve_id.into(),
            currency: currency.into(),
            curve_type: "generic".into(),
            interpolation: "linear".into(),
            reference_date: None,
        }
    }

    /// Sets the curve type.
    pub fn with_type(mut self, curve_type: impl Into<String>) -> Self {
        self.curve_type = curve_type.into();
        self
    }

    /// Sets the interpolation method.
    pub fn with_interpolation(mut self, interpolation: impl Into<String>) -> Self {
        self.interpolation = interpolation.into();
        self
    }

    /// Sets the reference date.
    pub fn with_reference_date(mut self, date: NaiveDate) -> Self {
        self.reference_date = Some(date);
        self
    }
}

/// A curve calculation node.
///
/// Curve nodes depend on their input instruments (curve inputs) and produce
/// a built curve stored in the CurveCache.
#[derive(Debug)]
pub struct CurveNode {
    /// Node configuration.
    config: CurveConfig,
    /// Dependencies (curve input nodes).
    dependencies: Vec<NodeId>,
}

impl CurveNode {
    /// Creates a new curve node.
    pub fn new(config: CurveConfig) -> Self {
        Self {
            config,
            dependencies: Vec::new(),
        }
    }

    /// Creates a curve node from just an ID.
    pub fn from_id(curve_id: impl Into<String>) -> Self {
        let curve_id = curve_id.into();
        Self {
            config: CurveConfig::new(&curve_id, "USD"),
            dependencies: Vec::new(),
        }
    }

    /// Adds a dependency on a curve input.
    pub fn depends_on(mut self, node_id: NodeId) -> Self {
        self.dependencies.push(node_id);
        self
    }

    /// Adds a dependency on a curve input instrument.
    pub fn depends_on_input(mut self, instrument: impl Into<String>) -> Self {
        self.dependencies.push(NodeId::curve_input(
            &self.config.curve_id,
            instrument.into(),
        ));
        self
    }

    /// Returns the curve configuration.
    pub fn config(&self) -> &CurveConfig {
        &self.config
    }
}

impl CalculationNode for CurveNode {
    fn node_id(&self) -> NodeId {
        NodeId::curve(&self.config.curve_id)
    }

    fn node_type(&self) -> NodeType {
        NodeType::Curve
    }

    fn dependencies(&self) -> Vec<NodeId> {
        self.dependencies.clone()
    }

    fn calculate(&self, ctx: &CalculationContext<'_>) -> EngineResult<NodeValue> {
        // Collect input rates from dependencies
        let mut inputs: Vec<(String, f64)> = Vec::new();

        for dep_id in &self.dependencies {
            if let Some(NodeValue::CurveInputValue { rate, .. }) = ctx.get_dependency(dep_id) {
                if let NodeId::CurveInput { instrument, .. } = dep_id {
                    inputs.push((instrument.clone(), rate));
                }
            }
        }

        // In a full implementation, this would call convex-curves to build the curve
        // For now, return a reference that indicates the curve was built
        let curve_ref = NodeValue::CurveRef {
            curve_id: self.config.curve_id.clone(),
            version: ctx.revision.0,
            build_time: Utc::now(),
        };

        tracing::debug!(
            curve_id = %self.config.curve_id,
            input_count = inputs.len(),
            "Curve node calculated"
        );

        Ok(curve_ref)
    }

    fn priority(&self) -> u32 {
        // Curves should be calculated early
        10
    }
}

// =============================================================================
// BOND PRICING NODE
// =============================================================================

/// Configuration for bond pricing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondPricingConfig {
    /// Instrument identifier (e.g., CUSIP, ISIN).
    pub instrument_id: String,
    /// Pricing curve ID.
    pub pricing_curve: Option<String>,
    /// Discount curve ID (if different from pricing curve).
    pub discount_curve: Option<String>,
    /// Spread curve ID for spread calculations.
    pub spread_curve: Option<String>,
    /// Whether to calculate risk metrics.
    pub calculate_risk: bool,
    /// Whether to calculate spread metrics.
    pub calculate_spreads: bool,
}

impl BondPricingConfig {
    /// Creates a new bond pricing configuration.
    pub fn new(instrument_id: impl Into<String>) -> Self {
        Self {
            instrument_id: instrument_id.into(),
            pricing_curve: None,
            discount_curve: None,
            spread_curve: None,
            calculate_risk: true,
            calculate_spreads: true,
        }
    }

    /// Sets the pricing curve.
    pub fn with_pricing_curve(mut self, curve_id: impl Into<String>) -> Self {
        self.pricing_curve = Some(curve_id.into());
        self
    }

    /// Sets the discount curve.
    pub fn with_discount_curve(mut self, curve_id: impl Into<String>) -> Self {
        self.discount_curve = Some(curve_id.into());
        self
    }

    /// Sets the spread curve.
    pub fn with_spread_curve(mut self, curve_id: impl Into<String>) -> Self {
        self.spread_curve = Some(curve_id.into());
        self
    }

    /// Enables or disables risk calculation.
    pub fn with_risk(mut self, enabled: bool) -> Self {
        self.calculate_risk = enabled;
        self
    }

    /// Enables or disables spread calculation.
    pub fn with_spreads(mut self, enabled: bool) -> Self {
        self.calculate_spreads = enabled;
        self
    }
}

/// A bond pricing calculation node.
///
/// Bond pricing nodes depend on curves and quotes to calculate analytics.
#[derive(Debug)]
pub struct BondPricingNode {
    /// Node configuration.
    config: BondPricingConfig,
    /// Dependencies.
    dependencies: Vec<NodeId>,
}

impl BondPricingNode {
    /// Creates a new bond pricing node.
    pub fn new(config: BondPricingConfig) -> Self {
        let mut deps = Vec::new();

        // Add curve dependencies
        if let Some(ref curve) = config.pricing_curve {
            deps.push(NodeId::curve(curve));
        }
        if let Some(ref curve) = config.discount_curve {
            deps.push(NodeId::curve(curve));
        }
        if let Some(ref curve) = config.spread_curve {
            deps.push(NodeId::curve(curve));
        }

        // Add quote dependency
        deps.push(NodeId::quote(&config.instrument_id));

        Self {
            config,
            dependencies: deps,
        }
    }

    /// Creates a bond pricing node from just an ID.
    pub fn from_id(instrument_id: impl Into<String>) -> Self {
        let config = BondPricingConfig::new(instrument_id);
        Self::new(config)
    }

    /// Adds a curve dependency.
    pub fn depends_on_curve(mut self, curve_id: impl Into<String>) -> Self {
        self.dependencies.push(NodeId::curve(curve_id.into()));
        self
    }

    /// Adds a dependency on another node.
    pub fn depends_on(mut self, node_id: NodeId) -> Self {
        self.dependencies.push(node_id);
        self
    }

    /// Returns the configuration.
    pub fn config(&self) -> &BondPricingConfig {
        &self.config
    }
}

impl CalculationNode for BondPricingNode {
    fn node_id(&self) -> NodeId {
        NodeId::bond(&self.config.instrument_id)
    }

    fn node_type(&self) -> NodeType {
        NodeType::BondPricing
    }

    fn dependencies(&self) -> Vec<NodeId> {
        self.dependencies.clone()
    }

    fn calculate(&self, ctx: &CalculationContext<'_>) -> EngineResult<NodeValue> {
        // Get the quote for pricing
        let quote = ctx.get_dependency(&NodeId::quote(&self.config.instrument_id));

        // Extract price from quote
        let price = match quote {
            Some(NodeValue::Quote { mid, .. }) => mid,
            _ => None,
        };

        // In a full implementation, this would:
        // 1. Load the bond from BondService
        // 2. Get curves from CurveCache
        // 3. Call convex-analytics for calculations

        // For now, create a placeholder analytics result
        let analytics = NodeValue::BondAnalytics {
            instrument_id: self.config.instrument_id.clone(),
            clean_price: price,
            dirty_price: None, // Would add accrued interest
            ytm: None,         // Would calculate from price
            z_spread: None,    // Would calculate with spread curve
            modified_duration: None,
            dv01: None,
            timestamp: Utc::now(),
        };

        tracing::debug!(
            instrument_id = %self.config.instrument_id,
            has_price = price.is_some(),
            "Bond pricing node calculated"
        );

        Ok(analytics)
    }

    fn priority(&self) -> u32 {
        // Bond pricing after curves
        50
    }
}

// =============================================================================
// PORTFOLIO NODE
// =============================================================================

/// Configuration for portfolio analytics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioConfig {
    /// Portfolio identifier.
    pub portfolio_id: String,
    /// Holdings (instrument IDs).
    pub holdings: Vec<String>,
    /// Whether to calculate duration contribution.
    pub calculate_contributions: bool,
}

impl PortfolioConfig {
    /// Creates a new portfolio configuration.
    pub fn new(portfolio_id: impl Into<String>) -> Self {
        Self {
            portfolio_id: portfolio_id.into(),
            holdings: Vec::new(),
            calculate_contributions: true,
        }
    }

    /// Adds holdings to the portfolio.
    pub fn with_holdings(mut self, holdings: Vec<String>) -> Self {
        self.holdings = holdings;
        self
    }

    /// Adds a single holding.
    pub fn add_holding(mut self, instrument_id: impl Into<String>) -> Self {
        self.holdings.push(instrument_id.into());
        self
    }
}

/// A portfolio analytics calculation node.
///
/// Portfolio nodes depend on bond pricing nodes for each holding.
#[derive(Debug)]
pub struct PortfolioNode {
    /// Node configuration.
    config: PortfolioConfig,
    /// Dependencies (bond nodes for each holding).
    dependencies: Vec<NodeId>,
}

impl PortfolioNode {
    /// Creates a new portfolio node.
    pub fn new(config: PortfolioConfig) -> Self {
        let deps = config
            .holdings
            .iter()
            .map(NodeId::bond)
            .collect();

        Self {
            config,
            dependencies: deps,
        }
    }

    /// Creates a portfolio node from just an ID.
    pub fn from_id(portfolio_id: impl Into<String>) -> Self {
        let config = PortfolioConfig::new(portfolio_id);
        Self::new(config)
    }

    /// Adds a holding to the portfolio.
    pub fn add_holding(mut self, instrument_id: impl Into<String>) -> Self {
        let id = instrument_id.into();
        self.config.holdings.push(id.clone());
        self.dependencies.push(NodeId::bond(id));
        self
    }

    /// Returns the configuration.
    pub fn config(&self) -> &PortfolioConfig {
        &self.config
    }
}

impl CalculationNode for PortfolioNode {
    fn node_id(&self) -> NodeId {
        NodeId::portfolio(&self.config.portfolio_id)
    }

    fn node_type(&self) -> NodeType {
        NodeType::Portfolio
    }

    fn dependencies(&self) -> Vec<NodeId> {
        self.dependencies.clone()
    }

    fn calculate(&self, ctx: &CalculationContext<'_>) -> EngineResult<NodeValue> {
        // Aggregate analytics from holdings
        let mut total_nav = Decimal::ZERO;
        let mut total_duration_contribution = Decimal::ZERO;
        let mut total_dv01 = Decimal::ZERO;

        for holding_id in &self.config.holdings {
            if let Some(NodeValue::BondAnalytics {
                dirty_price,
                modified_duration,
                dv01,
                ..
            }) = ctx.get_dependency(&NodeId::bond(holding_id))
            {
                if let Some(price) = dirty_price {
                    total_nav += price;
                }
                if let Some(dur) = modified_duration {
                    // Simplified - would weight by market value
                    total_duration_contribution += dur;
                }
                if let Some(d) = dv01 {
                    total_dv01 += d;
                }
            }
        }

        // Calculate weighted average duration
        let avg_duration = if !self.config.holdings.is_empty() {
            total_duration_contribution / Decimal::from(self.config.holdings.len())
        } else {
            Decimal::ZERO
        };

        let analytics = NodeValue::PortfolioAnalytics {
            portfolio_id: self.config.portfolio_id.clone(),
            nav: total_nav,
            duration: avg_duration,
            dv01: total_dv01,
            timestamp: Utc::now(),
        };

        tracing::debug!(
            portfolio_id = %self.config.portfolio_id,
            holdings_count = self.config.holdings.len(),
            "Portfolio node calculated"
        );

        Ok(analytics)
    }

    fn priority(&self) -> u32 {
        // Portfolio after bonds
        80
    }
}

// =============================================================================
// QUOTE NODE
// =============================================================================

/// A market quote input node.
///
/// Quote nodes have no dependencies and are updated directly with market data.
#[derive(Debug)]
pub struct QuoteNode {
    /// Instrument identifier.
    instrument_id: String,
    /// Current bid price.
    bid: Option<Decimal>,
    /// Current ask price.
    ask: Option<Decimal>,
}

impl QuoteNode {
    /// Creates a new quote node.
    pub fn new(instrument_id: impl Into<String>) -> Self {
        Self {
            instrument_id: instrument_id.into(),
            bid: None,
            ask: None,
        }
    }

    /// Creates a quote node with initial values.
    pub fn with_prices(
        instrument_id: impl Into<String>,
        bid: Option<Decimal>,
        ask: Option<Decimal>,
    ) -> Self {
        Self {
            instrument_id: instrument_id.into(),
            bid,
            ask,
        }
    }

    /// Updates the quote prices.
    pub fn update(&mut self, bid: Option<Decimal>, ask: Option<Decimal>) {
        self.bid = bid;
        self.ask = ask;
    }

    /// Returns the instrument ID.
    pub fn instrument_id(&self) -> &str {
        &self.instrument_id
    }
}

impl CalculationNode for QuoteNode {
    fn node_id(&self) -> NodeId {
        NodeId::quote(&self.instrument_id)
    }

    fn node_type(&self) -> NodeType {
        NodeType::Quote
    }

    fn dependencies(&self) -> Vec<NodeId> {
        // Quote nodes have no dependencies
        Vec::new()
    }

    fn calculate(&self, _ctx: &CalculationContext<'_>) -> EngineResult<NodeValue> {
        Ok(NodeValue::quote(&self.instrument_id, self.bid, self.ask))
    }

    fn priority(&self) -> u32 {
        // Quotes are calculated first
        1
    }
}

// =============================================================================
// CURVE INPUT NODE
// =============================================================================

/// A curve input node (e.g., deposit rate, swap rate).
///
/// These nodes represent the market instruments used to bootstrap curves.
#[derive(Debug)]
pub struct CurveInputNode {
    /// Curve this input belongs to.
    curve_id: String,
    /// Instrument identifier (e.g., "3M_DEPO", "2Y_SWAP").
    instrument: String,
    /// Rate value.
    rate: f64,
    /// Source of the rate.
    source: Option<String>,
}

impl CurveInputNode {
    /// Creates a new curve input node.
    pub fn new(
        curve_id: impl Into<String>,
        instrument: impl Into<String>,
        rate: f64,
    ) -> Self {
        Self {
            curve_id: curve_id.into(),
            instrument: instrument.into(),
            rate,
            source: None,
        }
    }

    /// Sets the source.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Updates the rate.
    pub fn update_rate(&mut self, rate: f64) {
        self.rate = rate;
    }

    /// Returns the rate.
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Returns the instrument identifier.
    pub fn instrument(&self) -> &str {
        &self.instrument
    }
}

impl CalculationNode for CurveInputNode {
    fn node_id(&self) -> NodeId {
        NodeId::curve_input(&self.curve_id, &self.instrument)
    }

    fn node_type(&self) -> NodeType {
        NodeType::CurveInput
    }

    fn dependencies(&self) -> Vec<NodeId> {
        // Input nodes have no dependencies
        Vec::new()
    }

    fn calculate(&self, _ctx: &CalculationContext<'_>) -> EngineResult<NodeValue> {
        Ok(NodeValue::CurveInputValue {
            rate: self.rate,
            source: self.source.clone(),
            timestamp: Utc::now(),
        })
    }

    fn priority(&self) -> u32 {
        // Input nodes calculated first
        1
    }
}

// =============================================================================
// CUSTOM NODE
// =============================================================================

/// A custom calculation node with user-defined logic.
pub struct CustomNode<F>
where
    F: Fn(&CalculationContext<'_>) -> EngineResult<NodeValue> + Send + Sync,
{
    /// Node identifier.
    node_id: String,
    /// Dependencies.
    dependencies: Vec<NodeId>,
    /// Calculation function.
    calculate_fn: F,
}

impl<F> std::fmt::Debug for CustomNode<F>
where
    F: Fn(&CalculationContext<'_>) -> EngineResult<NodeValue> + Send + Sync,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomNode")
            .field("node_id", &self.node_id)
            .field("dependencies", &self.dependencies)
            .finish()
    }
}

impl<F> CustomNode<F>
where
    F: Fn(&CalculationContext<'_>) -> EngineResult<NodeValue> + Send + Sync,
{
    /// Creates a new custom node.
    pub fn new(node_id: impl Into<String>, calculate_fn: F) -> Self {
        Self {
            node_id: node_id.into(),
            dependencies: Vec::new(),
            calculate_fn,
        }
    }

    /// Adds a dependency.
    pub fn depends_on(mut self, node_id: NodeId) -> Self {
        self.dependencies.push(node_id);
        self
    }
}

impl<F> CalculationNode for CustomNode<F>
where
    F: Fn(&CalculationContext<'_>) -> EngineResult<NodeValue> + Send + Sync,
{
    fn node_id(&self) -> NodeId {
        NodeId::custom(&self.node_id)
    }

    fn node_type(&self) -> NodeType {
        NodeType::Custom
    }

    fn dependencies(&self) -> Vec<NodeId> {
        self.dependencies.clone()
    }

    fn calculate(&self, ctx: &CalculationContext<'_>) -> EngineResult<NodeValue> {
        (self.calculate_fn)(ctx)
    }
}

// =============================================================================
// NODE BUILDER
// =============================================================================

/// Builder for creating calculation nodes with a fluent API.
pub struct NodeBuilder {
    node_id: Option<String>,
    node_type: Option<NodeType>,
    dependencies: Vec<NodeId>,
}

impl NodeBuilder {
    /// Creates a new node builder.
    pub fn new() -> Self {
        Self {
            node_id: None,
            node_type: None,
            dependencies: Vec::new(),
        }
    }

    /// Sets the node ID.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.node_id = Some(id.into());
        self
    }

    /// Sets the node type.
    pub fn node_type(mut self, typ: NodeType) -> Self {
        self.node_type = Some(typ);
        self
    }

    /// Adds a dependency.
    pub fn depends_on(mut self, node_id: NodeId) -> Self {
        self.dependencies.push(node_id);
        self
    }

    /// Builds a curve node.
    pub fn curve(self) -> CurveNode {
        let curve_id = self.node_id.unwrap_or_else(|| "unnamed".into());
        let mut node = CurveNode::from_id(curve_id);
        for dep in self.dependencies {
            node = node.depends_on(dep);
        }
        node
    }

    /// Builds a bond pricing node.
    pub fn bond(self) -> BondPricingNode {
        let instrument_id = self.node_id.unwrap_or_else(|| "unnamed".into());
        let mut node = BondPricingNode::from_id(instrument_id);
        for dep in self.dependencies {
            node = node.depends_on(dep);
        }
        node
    }

    /// Builds a portfolio node.
    pub fn portfolio(self) -> PortfolioNode {
        let portfolio_id = self.node_id.unwrap_or_else(|| "unnamed".into());
        // Holdings added separately via add_holding()
        PortfolioNode::from_id(portfolio_id)
    }
}

impl Default for NodeBuilder {
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
    use rust_decimal_macros::dec;

    #[test]
    fn test_quote_node() {
        let quote = QuoteNode::with_prices("TEST001", Some(dec!(99.50)), Some(dec!(100.50)));
        assert_eq!(quote.node_id(), NodeId::quote("TEST001"));
        assert_eq!(quote.node_type(), NodeType::Quote);
        assert!(quote.dependencies().is_empty());
    }

    #[test]
    fn test_curve_node() {
        let config = CurveConfig::new("USD.GOVT", "USD")
            .with_type("government")
            .with_interpolation("cubic");

        let node = CurveNode::new(config)
            .depends_on_input("3M_DEPO")
            .depends_on_input("2Y_SWAP");

        assert_eq!(node.node_id(), NodeId::curve("USD.GOVT"));
        assert_eq!(node.node_type(), NodeType::Curve);
        assert_eq!(node.dependencies().len(), 2);
    }

    #[test]
    fn test_bond_pricing_node() {
        let config = BondPricingConfig::new("US912828Z229")
            .with_pricing_curve("USD.GOVT")
            .with_spread_curve("USD.SOFR.OIS");

        let node = BondPricingNode::new(config);

        assert_eq!(node.node_id(), NodeId::bond("US912828Z229"));
        assert_eq!(node.node_type(), NodeType::BondPricing);
        // Quote + 2 curves = 3 dependencies
        assert_eq!(node.dependencies().len(), 3);
    }

    #[test]
    fn test_portfolio_node() {
        let config = PortfolioConfig::new("PORTFOLIO_001")
            .with_holdings(vec!["BOND_A".into(), "BOND_B".into(), "BOND_C".into()]);

        let node = PortfolioNode::new(config);

        assert_eq!(node.node_id(), NodeId::portfolio("PORTFOLIO_001"));
        assert_eq!(node.node_type(), NodeType::Portfolio);
        assert_eq!(node.dependencies().len(), 3);
    }

    #[test]
    fn test_curve_input_node() {
        let node = CurveInputNode::new("USD.SOFR", "2Y_SWAP", 0.0425)
            .with_source("BLOOMBERG");

        assert_eq!(node.node_id(), NodeId::curve_input("USD.SOFR", "2Y_SWAP"));
        assert_eq!(node.node_type(), NodeType::CurveInput);
        assert_eq!(node.rate(), 0.0425);
    }

    #[test]
    fn test_node_builder() {
        let node = NodeBuilder::new()
            .id("USD.GOVT")
            .depends_on(NodeId::curve_input("USD.GOVT", "3M_DEPO"))
            .depends_on(NodeId::curve_input("USD.GOVT", "6M_DEPO"))
            .curve();

        assert_eq!(node.node_id(), NodeId::curve("USD.GOVT"));
        assert_eq!(node.dependencies().len(), 2);
    }
}
