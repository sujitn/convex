//! Spread analytics for portfolios.
//!
//! Provides weighted average spread calculations including:
//! - Z-spread
//! - OAS (Option-Adjusted Spread)
//! - G-spread (Government spread)
//! - I-spread (Interpolated swap spread)
//! - ASW (Asset Swap Spread)
//! - CS01 (Credit Spread DV01)
//!
//! All spreads are in basis points.

use super::metrics::{self, MetricBook};
use crate::types::{AnalyticsConfig, Holding};
use crate::PortfolioResult;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Aggregated spread metrics for a portfolio.
///
/// All spreads are in basis points.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpreadMetrics {
    /// Weighted average Z-spread (bps).
    pub z_spread: Option<f64>,

    /// Weighted average OAS (bps).
    pub oas: Option<f64>,

    /// Weighted average G-spread (bps).
    pub g_spread: Option<f64>,

    /// Weighted average I-spread (bps).
    pub i_spread: Option<f64>,

    /// Weighted average ASW (bps).
    pub asw: Option<f64>,

    /// Best available spread (prefers OAS over Z-spread).
    pub best_spread: Option<f64>,

    /// Total CS01 (credit spread DV01) - sum across portfolio.
    #[serde(with = "convex_core::contracts::decimal_serde::optional")]
    pub total_cs01: Option<Decimal>,

    /// CS01 per share (if shares outstanding is set).
    #[serde(with = "convex_core::contracts::decimal_serde::optional")]
    pub cs01_per_share: Option<Decimal>,

    /// Number of holdings with Z-spread data.
    pub z_spread_coverage: usize,

    /// Number of holdings with OAS data.
    pub oas_coverage: usize,

    /// Total number of holdings.
    pub total_holdings: usize,
}

impl SpreadMetrics {
    /// Returns the Z-spread coverage as a percentage.
    pub fn z_spread_coverage_pct(&self) -> f64 {
        if self.total_holdings > 0 {
            self.z_spread_coverage as f64 / self.total_holdings as f64 * 100.0
        } else {
            0.0
        }
    }

    /// Returns the OAS coverage as a percentage.
    pub fn oas_coverage_pct(&self) -> f64 {
        if self.total_holdings > 0 {
            self.oas_coverage as f64 / self.total_holdings as f64 * 100.0
        } else {
            0.0
        }
    }
}

/// Calculates weighted average Z-spread.
///
/// Z-spread is the constant spread added to the Treasury curve to
/// discount a bond's cash flows to its market price.
///
/// # Returns
///
/// Returns `None` if any holdings lack Z-spread data.
pub fn weighted_z_spread(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> PortfolioResult<Option<f64>> {
    MetricBook::new(holdings, config)?.weighted("z_spread", |h| h.analytics.z_spread)
}

/// Calculates weighted average OAS.
///
/// Option-Adjusted Spread accounts for embedded options and is preferred
/// for callable bonds.
///
/// # Returns
///
/// Returns `None` if any holdings lack OAS data.
pub fn weighted_oas(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> PortfolioResult<Option<f64>> {
    MetricBook::new(holdings, config)?.weighted("oas", |h| h.analytics.oas)
}

/// Calculates weighted average G-spread.
///
/// G-spread is the spread over the government bond with matching maturity.
///
/// # Returns
///
/// Returns `None` if any holdings lack G-spread data.
pub fn weighted_g_spread(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> PortfolioResult<Option<f64>> {
    MetricBook::new(holdings, config)?.weighted("g_spread", |h| h.analytics.g_spread)
}

/// Calculates weighted average I-spread.
///
/// I-spread is the spread over the interpolated swap curve.
///
/// # Returns
///
/// Returns `None` if any holdings lack I-spread data.
pub fn weighted_i_spread(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> PortfolioResult<Option<f64>> {
    MetricBook::new(holdings, config)?.weighted("i_spread", |h| h.analytics.i_spread)
}

/// Calculates weighted average ASW.
///
/// Asset Swap Spread represents the spread earned in an asset swap.
///
/// # Returns
///
/// Returns `None` if any holdings lack ASW data.
pub fn weighted_asw(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> PortfolioResult<Option<f64>> {
    MetricBook::new(holdings, config)?.weighted("asw", |h| h.analytics.asw)
}

/// Calculates weighted best spread (prefers OAS over Z-spread).
///
/// For each holding, uses OAS if available, otherwise Z-spread.
///
/// # Returns
///
/// Returns `None` if any holdings lack spread data.
pub fn weighted_best_spread(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> PortfolioResult<Option<f64>> {
    MetricBook::new(holdings, config)?.weighted("best_spread", |h| h.analytics.best_spread())
}

/// Calculates total portfolio CS01 (Credit Spread DV01).
///
/// ## Formula
///
/// ```text
/// CS01_portfolio = Σ(CS01_i × par_i / 100 × fx_rate_i)
/// ```
///
/// CS01 is the change in value for a 1bp widening in credit spreads.
/// It is additive across the portfolio.
pub fn total_cs01(
    holdings: &[Holding],
    _config: &AnalyticsConfig,
) -> PortfolioResult<Option<Decimal>> {
    metrics::currency_sum(holdings, "cs01", |h| h.analytics.cs01)
}

/// Calculates CS01 per share.
///
/// # Returns
///
/// Returns `None` if shares outstanding is not set or is zero.
pub fn cs01_per_share(
    holdings: &[Holding],
    shares: Option<Decimal>,
    _config: &AnalyticsConfig,
) -> PortfolioResult<Option<Decimal>> {
    metrics::per_share_exact(
        metrics::currency_exact(holdings, "cs01", |h| h.analytics.cs01)?,
        shares,
    )
}

/// Calculates all spread metrics for a portfolio.
///
/// # Example
///
/// ```ignore
/// let metrics = calculate_spread_metrics(&portfolio.holdings, portfolio.shares_outstanding, &config);
/// println!("Portfolio Z-Spread: {:?} bps", metrics.z_spread);
/// println!("Total CS01: ${:.2}", metrics.total_cs01);
/// ```
pub fn calculate_spread_metrics(
    holdings: &[Holding],
    shares_outstanding: Option<Decimal>,
    config: &AnalyticsConfig,
) -> PortfolioResult<SpreadMetrics> {
    let z_spread_coverage = holdings
        .iter()
        .filter(|h| h.analytics.z_spread.is_some())
        .count();
    let oas_coverage = holdings
        .iter()
        .filter(|h| h.analytics.oas.is_some())
        .count();

    let total_cs01_val = total_cs01(holdings, config)?;

    Ok(SpreadMetrics {
        z_spread: weighted_z_spread(holdings, config)?,
        oas: weighted_oas(holdings, config)?,
        g_spread: weighted_g_spread(holdings, config)?,
        i_spread: weighted_i_spread(holdings, config)?,
        asw: weighted_asw(holdings, config)?,
        best_spread: weighted_best_spread(holdings, config)?,
        total_cs01: total_cs01_val,
        cs01_per_share: cs01_per_share(holdings, shares_outstanding, config)?,
        z_spread_coverage,
        oas_coverage,
        total_holdings: holdings.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::HoldingAnalytics;
    use crate::WeightingMethod;
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal_macros::dec;

    fn create_holding(
        id: &str,
        par: Decimal,
        price: Decimal,
        z_spread: f64,
        oas: Option<f64>,
        cs01: f64,
    ) -> Holding {
        let mut analytics = HoldingAnalytics::new().with_z_spread(z_spread);
        if let Some(oas_val) = oas {
            analytics = analytics.with_oas(oas_val);
        }
        // Add CS01 manually
        analytics.cs01 = Some(cs01);

        Holding::builder()
            .id(id)
            .identifiers(BondIdentifiers::new().with_ticker(format!("TST{}", id)))
            .par_amount(par)
            .market_price(price)
            .analytics(analytics)
            .build()
            .unwrap()
    }

    #[test]
    fn test_weighted_z_spread() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(100), 100.0, None, 0.05),
            create_holding("BOND2", dec!(500_000), dec!(100), 150.0, None, 0.07),
        ];

        let config = AnalyticsConfig::default();
        let spread = weighted_z_spread(&holdings, &config).unwrap().unwrap();

        // MV weighted: (1M × 100 + 0.5M × 150) / 1.5M = 116.67 bps
        assert!((spread - 116.67).abs() < 0.1);
    }

    #[test]
    fn test_weighted_oas() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(100), 100.0, Some(95.0), 0.05),
            create_holding(
                "BOND2",
                dec!(1_000_000),
                dec!(100),
                150.0,
                Some(140.0),
                0.07,
            ),
        ];

        let config = AnalyticsConfig::default();
        let oas = weighted_oas(&holdings, &config).unwrap().unwrap();

        // Equal MV: (95 + 140) / 2 = 117.5 bps
        assert!((oas - 117.5).abs() < 0.1);
    }

    #[test]
    fn test_weighted_best_spread() {
        let holdings = vec![
            // Has OAS, uses 95 bps
            create_holding("BOND1", dec!(1_000_000), dec!(100), 100.0, Some(95.0), 0.05),
            // No OAS, uses Z-spread 150 bps
            create_holding("BOND2", dec!(1_000_000), dec!(100), 150.0, None, 0.07),
        ];

        let config = AnalyticsConfig::default();
        let best = weighted_best_spread(&holdings, &config).unwrap().unwrap();

        // Equal MV: (95 + 150) / 2 = 122.5 bps
        assert!((best - 122.5).abs() < 0.1);
    }

    #[test]
    fn test_total_cs01() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(100), 100.0, None, 0.05),
            create_holding("BOND2", dec!(500_000), dec!(100), 150.0, None, 0.07),
        ];

        let config = AnalyticsConfig::default();
        let cs01 = total_cs01(&holdings, &config)
            .unwrap()
            .unwrap()
            .to_string()
            .parse::<f64>()
            .unwrap();

        // Bond1: 0.05 × 1M / 100 = 500
        // Bond2: 0.07 × 500k / 100 = 350
        // Total: 850
        assert!((cs01 - 850.0).abs() < 0.1);
    }

    #[test]
    fn test_cs01_with_fx() {
        let eur_holding = Holding::builder()
            .id("EUR_BOND")
            .identifiers(BondIdentifiers::new().with_ticker("EUR001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .currency(convex_core::types::Currency::EUR)
            .fx_rate(dec!(1.10))
            .analytics({
                let mut a = HoldingAnalytics::new();
                a.cs01 = Some(0.05);
                a
            })
            .build()
            .unwrap();

        let holdings = vec![eur_holding];
        let config = AnalyticsConfig::default();
        let cs01 = total_cs01(&holdings, &config)
            .unwrap()
            .unwrap()
            .to_string()
            .parse::<f64>()
            .unwrap();

        // CS01 = 0.05 × 1M / 100 × 1.10 = 550
        assert!((cs01 - 550.0).abs() < 0.1);
    }

    #[test]
    fn test_cs01_per_share() {
        let holdings = vec![create_holding(
            "BOND1",
            dec!(1_000_000),
            dec!(100),
            100.0,
            None,
            0.05,
        )];

        let config = AnalyticsConfig::default();
        let per_share = cs01_per_share(&holdings, Some(dec!(1_000_000)), &config)
            .unwrap()
            .unwrap()
            .to_string()
            .parse::<f64>()
            .unwrap();

        // CS01 = 500, shares = 1M, per share = 0.0005
        assert!((per_share - 0.0005).abs() < 0.0001);
    }

    #[test]
    fn test_spread_metrics() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(100), 100.0, Some(95.0), 0.05),
            create_holding("BOND2", dec!(1_000_000), dec!(100), 150.0, None, 0.07),
        ];

        let config = AnalyticsConfig::default();
        let metrics = calculate_spread_metrics(&holdings, Some(dec!(1_000_000)), &config).unwrap();

        assert!(metrics.z_spread.is_some());
        assert!(metrics.oas.is_none()); // The second holding has no supplied OAS.
        assert!(metrics.best_spread.is_some());
        assert!(
            (metrics
                .total_cs01
                .unwrap()
                .to_string()
                .parse::<f64>()
                .unwrap()
                - 1200.0)
                .abs()
                < 0.1
        ); // 500 + 700

        assert_eq!(metrics.z_spread_coverage, 2);
        assert_eq!(metrics.oas_coverage, 1);
        assert!((metrics.z_spread_coverage_pct() - 100.0).abs() < 0.01);
        assert!((metrics.oas_coverage_pct() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_empty_portfolio() {
        let holdings: Vec<Holding> = vec![];
        let config = AnalyticsConfig::default();

        assert!(weighted_z_spread(&holdings, &config).unwrap().is_none());
        assert!(
            (total_cs01(&holdings, &config)
                .unwrap()
                .unwrap()
                .to_string()
                .parse::<f64>()
                .unwrap())
            .abs()
                < 0.001
        );
    }

    #[test]
    fn test_no_spread_data() {
        let holding = Holding::builder()
            .id("NOSP")
            .identifiers(BondIdentifiers::new().with_ticker("NOSP001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new()) // No spreads
            .build()
            .unwrap();

        let holdings = vec![holding];
        let config = AnalyticsConfig::default();

        assert!(weighted_z_spread(&holdings, &config).unwrap().is_none());
        assert!(weighted_oas(&holdings, &config).unwrap().is_none());
    }

    #[test]
    fn test_par_value_weighting() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(90), 100.0, None, 0.05), // Par 1M
            create_holding("BOND2", dec!(500_000), dec!(110), 150.0, None, 0.07),  // Par 500k
        ];

        let config = AnalyticsConfig::default().with_weighting(WeightingMethod::ParValue);
        let spread = weighted_z_spread(&holdings, &config).unwrap().unwrap();

        // Par weighted: (1M × 100 + 0.5M × 150) / 1.5M = 116.67 bps
        assert!((spread - 116.67).abs() < 0.1);
    }
}
