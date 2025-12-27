//! ETF creation/redemption basket analytics.
//!
//! Provides analysis of ETF creation/redemption baskets:
//! - Basket composition and value
//! - Tracking difference from benchmark
//! - Cash component calculations
//! - Creation unit analysis

use crate::types::{AnalyticsConfig, Holding};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single component in a creation/redemption basket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasketComponent {
    /// Holding identifier.
    pub holding_id: String,

    /// ISIN or other security identifier.
    pub security_id: String,

    /// Quantity of shares/par amount in the basket.
    pub quantity: Decimal,

    /// Current market price.
    pub price: Decimal,

    /// Market value of this component.
    pub market_value: Decimal,

    /// Weight in the basket (%).
    pub weight_pct: f64,

    /// Is this a substitution from the published basket?
    pub is_substitution: bool,
}

/// Creation/redemption basket details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreationBasket {
    /// Creation unit size (number of ETF shares per creation unit).
    pub creation_unit_size: Decimal,

    /// Components in the basket.
    pub components: Vec<BasketComponent>,

    /// Total number of securities in the basket.
    pub security_count: usize,

    /// Total market value of securities in the basket.
    pub securities_value: Decimal,

    /// Cash component (can be positive or negative).
    pub cash_component: Decimal,

    /// Total creation unit value (securities + cash).
    pub total_value: Decimal,

    /// NAV per creation unit.
    pub nav_per_cu: f64,

    /// Estimated transaction cost to create (bps).
    pub estimated_cost_bps: Option<f64>,

    /// Number of substitutions from published basket.
    pub substitution_count: usize,
}

impl CreationBasket {
    /// Number of ETF shares that would be created.
    #[must_use]
    pub fn shares_created(&self) -> Decimal {
        self.creation_unit_size
    }

    /// NAV per share of the ETF.
    #[must_use]
    pub fn nav_per_share(&self) -> f64 {
        self.total_value.to_f64().unwrap_or(0.0) / self.creation_unit_size.to_f64().unwrap_or(1.0)
    }

    /// Cash as a percentage of total value.
    #[must_use]
    pub fn cash_pct(&self) -> f64 {
        if !self.total_value.is_zero() {
            self.cash_component.to_f64().unwrap_or(0.0) / self.total_value.to_f64().unwrap_or(1.0)
                * 100.0
        } else {
            0.0
        }
    }
}

/// Basket analysis results comparing basket to portfolio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasketAnalysis {
    /// Holdings in portfolio but not in basket.
    pub excluded_holdings: Vec<String>,

    /// Holdings in basket but not in portfolio (if any).
    pub added_holdings: Vec<String>,

    /// Largest weight differences (holding_id, diff%).
    pub weight_differences: Vec<(String, f64)>,

    /// Tracking difference in duration.
    pub duration_diff: Option<f64>,

    /// Tracking difference in yield.
    pub yield_diff: Option<f64>,

    /// Estimated tracking error (bps).
    pub tracking_error_bps: Option<f64>,
}

/// Builds a creation basket from portfolio holdings.
///
/// # Arguments
///
/// * `holdings` - Portfolio holdings
/// * `creation_unit_size` - Number of ETF shares per creation unit (typically 25,000 or 50,000)
/// * `total_shares` - Total shares outstanding in the ETF
/// * `cash_balance` - Current cash balance of the ETF
///
/// # Returns
///
/// A creation basket representing holdings scaled to one creation unit.
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::etf::build_creation_basket;
///
/// let basket = build_creation_basket(&holdings, dec!(50_000), dec!(1_000_000), dec!(100_000));
/// println!("Basket has {} securities", basket.security_count);
/// println!("Cash component: ${}", basket.cash_component);
/// ```
#[must_use]
pub fn build_creation_basket(
    holdings: &[Holding],
    creation_unit_size: Decimal,
    total_shares: Decimal,
    cash_balance: Decimal,
) -> CreationBasket {
    if total_shares.is_zero() {
        return CreationBasket {
            creation_unit_size,
            components: vec![],
            security_count: 0,
            securities_value: Decimal::ZERO,
            cash_component: Decimal::ZERO,
            total_value: Decimal::ZERO,
            nav_per_cu: 0.0,
            estimated_cost_bps: None,
            substitution_count: 0,
        };
    }

    // Scale factor: creation unit size / total shares
    let scale_factor = creation_unit_size / total_shares;

    // Build components
    let mut components: Vec<BasketComponent> = holdings
        .iter()
        .map(|h| {
            let scaled_quantity = h.par_amount * scale_factor;
            let mv = h.market_value() * scale_factor;
            BasketComponent {
                holding_id: h.id.clone(),
                security_id: h
                    .identifiers
                    .isin()
                    .map(|i| i.to_string())
                    .unwrap_or_default(),
                quantity: scaled_quantity,
                price: h.market_price,
                market_value: mv,
                weight_pct: 0.0, // Will be calculated below
                is_substitution: false,
            }
        })
        .collect();

    // Calculate total securities value
    let securities_value: Decimal = components.iter().map(|c| c.market_value).sum();

    // Cash component (scaled)
    let cash_component = cash_balance * scale_factor;

    // Total value
    let total_value = securities_value + cash_component;

    // Calculate weights
    let total_f64 = total_value.to_f64().unwrap_or(1.0);
    for comp in &mut components {
        if total_f64 > 0.0 {
            comp.weight_pct = comp.market_value.to_f64().unwrap_or(0.0) / total_f64 * 100.0;
        }
    }

    // Sort by weight descending
    components.sort_by(|a, b| {
        b.weight_pct
            .partial_cmp(&a.weight_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    CreationBasket {
        creation_unit_size,
        security_count: components.len(),
        components,
        securities_value,
        cash_component,
        total_value,
        nav_per_cu: total_value.to_f64().unwrap_or(0.0),
        estimated_cost_bps: None,
        substitution_count: 0,
    }
}

/// Analyzes differences between creation basket and target portfolio.
///
/// # Arguments
///
/// * `basket` - The creation basket
/// * `target_holdings` - Target portfolio holdings (e.g., benchmark or published basket)
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Analysis of differences including excluded holdings and weight mismatches.
#[must_use]
pub fn analyze_basket(
    basket: &CreationBasket,
    target_holdings: &[Holding],
    _config: &AnalyticsConfig,
) -> BasketAnalysis {
    // Build maps for comparison
    let basket_ids: HashMap<&str, &BasketComponent> = basket
        .components
        .iter()
        .map(|c| (c.holding_id.as_str(), c))
        .collect();

    let target_weights: HashMap<&str, f64> = {
        let total_mv: Decimal = target_holdings.iter().map(|h| h.market_value()).sum();
        let total_f64 = total_mv.to_f64().unwrap_or(1.0);
        target_holdings
            .iter()
            .map(|h| {
                let weight = if total_f64 > 0.0 {
                    h.market_value().to_f64().unwrap_or(0.0) / total_f64 * 100.0
                } else {
                    0.0
                };
                (h.id.as_str(), weight)
            })
            .collect()
    };

    // Find excluded holdings (in target but not in basket)
    let excluded_holdings: Vec<String> = target_holdings
        .iter()
        .filter(|h| !basket_ids.contains_key(h.id.as_str()))
        .map(|h| h.id.clone())
        .collect();

    // Find added holdings (in basket but not in target)
    let target_ids: std::collections::HashSet<&str> =
        target_holdings.iter().map(|h| h.id.as_str()).collect();
    let added_holdings: Vec<String> = basket
        .components
        .iter()
        .filter(|c| !target_ids.contains(c.holding_id.as_str()))
        .map(|c| c.holding_id.clone())
        .collect();

    // Calculate weight differences
    let mut weight_differences: Vec<(String, f64)> = basket
        .components
        .iter()
        .filter_map(|c| {
            let target_weight = target_weights
                .get(c.holding_id.as_str())
                .copied()
                .unwrap_or(0.0);
            let diff = c.weight_pct - target_weight;
            if diff.abs() > 0.01 {
                Some((c.holding_id.clone(), diff))
            } else {
                None
            }
        })
        .collect();

    // Sort by absolute difference
    weight_differences.sort_by(|a, b| {
        b.1.abs()
            .partial_cmp(&a.1.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    BasketAnalysis {
        excluded_holdings,
        added_holdings,
        weight_differences,
        duration_diff: None, // Would require analytics on both sides
        yield_diff: None,
        tracking_error_bps: None,
    }
}

/// Calculates creation/redemption arbitrage opportunity.
///
/// # Arguments
///
/// * `nav_per_share` - ETF NAV per share
/// * `market_price` - Current market price of ETF
/// * `creation_cost_bps` - Estimated cost to create in basis points
/// * `redemption_cost_bps` - Estimated cost to redeem in basis points
///
/// # Returns
///
/// Tuple of (create_profit_bps, redeem_profit_bps) - positive values indicate profit opportunity.
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::etf::arbitrage_opportunity;
///
/// let (create_profit, redeem_profit) = arbitrage_opportunity(100.0, 100.50, 10.0, 10.0);
/// if create_profit > 0.0 {
///     println!("Creation arbitrage: {}bp profit", create_profit);
/// }
/// ```
#[must_use]
pub fn arbitrage_opportunity(
    nav_per_share: f64,
    market_price: f64,
    creation_cost_bps: f64,
    redemption_cost_bps: f64,
) -> (f64, f64) {
    if nav_per_share <= 0.0 {
        return (0.0, 0.0);
    }

    // Premium/discount in bps
    let premium_bps = (market_price - nav_per_share) / nav_per_share * 10_000.0;

    // Creation profit: if trading at premium, create shares and sell
    // Profit = premium - creation_cost
    let create_profit = premium_bps - creation_cost_bps;

    // Redemption profit: if trading at discount, buy shares and redeem
    // Profit = |discount| - redemption_cost = -premium - redemption_cost (when premium < 0)
    let redeem_profit = -premium_bps - redemption_cost_bps;

    (create_profit.max(0.0), redeem_profit.max(0.0))
}

/// Basket creation/redemption flow summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BasketFlowSummary {
    /// Net creation units (positive) or redemption units (negative).
    pub net_units: i64,

    /// Total creation units.
    pub creation_units: u64,

    /// Total redemption units.
    pub redemption_units: u64,

    /// Net cash flow.
    pub net_cash_flow: Decimal,

    /// Average premium at creation.
    pub avg_creation_premium_bps: Option<f64>,

    /// Average discount at redemption.
    pub avg_redemption_discount_bps: Option<f64>,
}

impl BasketFlowSummary {
    /// Is there net creation activity?
    #[must_use]
    pub fn is_net_creation(&self) -> bool {
        self.net_units > 0
    }

    /// Is there net redemption activity?
    #[must_use]
    pub fn is_net_redemption(&self) -> bool {
        self.net_units < 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{HoldingAnalytics, HoldingBuilder};
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal_macros::dec;

    fn create_test_holding(id: &str, par: Decimal, price: Decimal) -> Holding {
        HoldingBuilder::new()
            .id(id)
            .identifiers(BondIdentifiers::from_isin_str("US912828Z229").unwrap())
            .par_amount(par)
            .market_price(price)
            .analytics(HoldingAnalytics::new())
            .build()
            .unwrap()
    }

    #[test]
    fn test_build_creation_basket_basic() {
        let holdings = vec![
            create_test_holding("H1", dec!(1_000_000), dec!(100)),
            create_test_holding("H2", dec!(500_000), dec!(100)),
        ];

        let basket = build_creation_basket(
            &holdings,
            dec!(50_000),    // creation unit size
            dec!(1_000_000), // total shares
            dec!(100_000),   // cash balance
        );

        // Scale factor = 50,000 / 1,000,000 = 0.05
        assert_eq!(basket.creation_unit_size, dec!(50_000));
        assert_eq!(basket.security_count, 2);

        // Securities value = (1,000,000 + 500,000) × 0.05 = 75,000
        assert!((basket.securities_value - dec!(75_000)).abs() < dec!(0.01));

        // Cash = 100,000 × 0.05 = 5,000
        assert!((basket.cash_component - dec!(5_000)).abs() < dec!(0.01));

        // Total = 80,000
        assert!((basket.total_value - dec!(80_000)).abs() < dec!(0.01));
    }

    #[test]
    fn test_creation_basket_nav_per_share() {
        let holdings = vec![create_test_holding("H1", dec!(1_000_000), dec!(100))];

        let basket = build_creation_basket(
            &holdings,
            dec!(50_000),    // creation unit size
            dec!(1_000_000), // total shares
            dec!(0),         // no cash
        );

        // Total value = 1,000,000 × 0.05 = 50,000
        // NAV per share = 50,000 / 50,000 = 1.0
        assert!((basket.nav_per_share() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_creation_basket_empty() {
        let basket = build_creation_basket(&[], dec!(50_000), dec!(0), dec!(0));

        assert_eq!(basket.security_count, 0);
        assert!(basket.total_value.is_zero());
    }

    #[test]
    fn test_analyze_basket_differences() {
        let basket_holdings = vec![
            create_test_holding("H1", dec!(600_000), dec!(100)),
            create_test_holding("H2", dec!(400_000), dec!(100)),
        ];

        let basket =
            build_creation_basket(&basket_holdings, dec!(50_000), dec!(1_000_000), dec!(0));

        // Target has different composition
        let target_holdings = vec![
            create_test_holding("H1", dec!(500_000), dec!(100)),
            create_test_holding("H2", dec!(400_000), dec!(100)),
            create_test_holding("H3", dec!(100_000), dec!(100)), // Not in basket
        ];

        let config = AnalyticsConfig::default();
        let analysis = analyze_basket(&basket, &target_holdings, &config);

        // H3 is in target but not in basket
        assert_eq!(analysis.excluded_holdings.len(), 1);
        assert_eq!(analysis.excluded_holdings[0], "H3");

        // Weight differences exist for H1
        assert!(!analysis.weight_differences.is_empty());
    }

    #[test]
    fn test_arbitrage_opportunity_premium() {
        let (create_profit, redeem_profit) = arbitrage_opportunity(
            100.0,  // NAV
            100.50, // Price (50bp premium)
            10.0,   // Creation cost
            10.0,   // Redemption cost
        );

        // Premium = 50bp, creation cost = 10bp → profit = 40bp
        assert!((create_profit - 40.0).abs() < 0.1);
        assert_eq!(redeem_profit, 0.0);
    }

    #[test]
    fn test_arbitrage_opportunity_discount() {
        let (create_profit, redeem_profit) = arbitrage_opportunity(
            100.0, // NAV
            99.50, // Price (50bp discount)
            10.0,  // Creation cost
            10.0,  // Redemption cost
        );

        // Discount = 50bp, redemption cost = 10bp → profit = 40bp
        assert_eq!(create_profit, 0.0);
        assert!((redeem_profit - 40.0).abs() < 0.1);
    }

    #[test]
    fn test_arbitrage_opportunity_no_arb() {
        let (create_profit, redeem_profit) = arbitrage_opportunity(
            100.0,  // NAV
            100.05, // Price (5bp premium)
            10.0,   // Creation cost > premium
            10.0,   // Redemption cost
        );

        // Premium too small for creation arbitrage
        assert_eq!(create_profit, 0.0);
        assert_eq!(redeem_profit, 0.0);
    }

    #[test]
    fn test_basket_flow_summary() {
        let flow = BasketFlowSummary {
            net_units: 5,
            creation_units: 10,
            redemption_units: 5,
            net_cash_flow: dec!(1_000_000),
            avg_creation_premium_bps: Some(15.0),
            avg_redemption_discount_bps: Some(10.0),
        };

        assert!(flow.is_net_creation());
        assert!(!flow.is_net_redemption());
    }

    #[test]
    fn test_cash_pct() {
        let holdings = vec![create_test_holding("H1", dec!(900_000), dec!(100))];

        let basket = build_creation_basket(
            &holdings,
            dec!(50_000),
            dec!(1_000_000),
            dec!(100_000), // 10% cash
        );

        // Securities = 900,000 × 0.05 = 45,000
        // Cash = 100,000 × 0.05 = 5,000
        // Total = 50,000
        // Cash % = 5,000 / 50,000 × 100 = 10%
        assert!((basket.cash_pct() - 10.0).abs() < 0.1);
    }
}
