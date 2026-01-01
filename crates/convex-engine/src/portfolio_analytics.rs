//! Portfolio analytics aggregation.
//!
//! Aggregates position-level bond analytics into portfolio-level risk metrics.

use std::collections::HashMap;

use rayon::prelude::*;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use tracing::{debug, info, warn};

use convex_core::Currency;
use convex_traits::ids::{InstrumentId, PortfolioId};
use convex_traits::output::{BondQuoteOutput, PortfolioAnalyticsOutput};

use crate::error::EngineError;

/// Portfolio position.
#[derive(Debug, Clone)]
pub struct Position {
    /// Instrument identifier
    pub instrument_id: InstrumentId,
    /// Notional/face value
    pub notional: Decimal,
    /// Sector classification
    pub sector: Option<String>,
    /// Credit rating
    pub rating: Option<String>,
}

/// Portfolio definition.
#[derive(Debug, Clone)]
pub struct Portfolio {
    /// Portfolio identifier
    pub portfolio_id: PortfolioId,
    /// Portfolio name
    pub name: String,
    /// Reporting currency
    pub currency: Currency,
    /// Positions
    pub positions: Vec<Position>,
}

/// Portfolio analytics calculator.
pub struct PortfolioAnalyzer {
    /// Minimum position coverage required (0.0 - 1.0)
    min_coverage: f64,
}

impl PortfolioAnalyzer {
    /// Create a new portfolio analyzer.
    pub fn new() -> Self {
        Self { min_coverage: 0.8 }
    }

    /// Set minimum coverage threshold.
    pub fn with_min_coverage(mut self, coverage: f64) -> Self {
        self.min_coverage = coverage.clamp(0.0, 1.0);
        self
    }

    /// Calculate portfolio analytics from position-level data.
    ///
    /// # Arguments
    /// * `portfolio` - Portfolio definition with positions
    /// * `bond_prices` - Bond quotes for the positions
    ///
    /// # Returns
    /// Aggregated portfolio analytics
    pub fn calculate(
        &self,
        portfolio: &Portfolio,
        bond_prices: &[BondQuoteOutput],
    ) -> Result<PortfolioAnalyticsOutput, EngineError> {
        // Build price lookup
        let price_map: HashMap<_, _> = bond_prices
            .iter()
            .map(|q| (q.instrument_id.clone(), q))
            .collect();

        let mut total_market_value = Decimal::ZERO;
        let mut weighted_duration = Decimal::ZERO;
        let mut weighted_convexity = Decimal::ZERO;
        let mut weighted_yield = Decimal::ZERO;
        let mut weighted_spread = Decimal::ZERO;
        let mut total_dv01 = Decimal::ZERO;
        let mut priced_count = 0u32;

        // Key rate duration aggregation
        let mut krd_totals: HashMap<String, Decimal> = HashMap::new();

        // Sector and rating breakdowns
        let mut sector_values: HashMap<String, Decimal> = HashMap::new();
        let mut rating_values: HashMap<String, Decimal> = HashMap::new();

        for position in &portfolio.positions {
            if let Some(quote) = price_map.get(&position.instrument_id) {
                if let Some(dirty_price) = quote.dirty_price_mid() {
                    // Calculate market value
                    let mv = position.notional * dirty_price / Decimal::from(100);
                    total_market_value += mv;
                    priced_count += 1;

                    // Aggregate weighted metrics
                    if let Some(dur) = quote.modified_duration {
                        weighted_duration += dur * mv;
                    }
                    if let Some(conv) = quote.convexity {
                        weighted_convexity += conv * mv;
                    }
                    if let Some(ytm) = quote.ytm_mid {
                        weighted_yield += ytm * mv;
                    }
                    if let Some(z_spread) = quote.z_spread_mid {
                        weighted_spread += z_spread * mv;
                    }

                    // Aggregate DV01 (scale by notional)
                    if let Some(dv01) = quote.dv01 {
                        // DV01 is typically per $100 face, scale to position
                        total_dv01 += dv01 * position.notional / Decimal::from(100);
                    }

                    // Aggregate key rate durations
                    if let Some(ref krds) = quote.key_rate_durations {
                        for (tenor, krd) in krds {
                            *krd_totals.entry(tenor.clone()).or_insert(Decimal::ZERO) += krd * mv;
                        }
                    }

                    // Sector breakdown
                    if let Some(ref sector) = position.sector {
                        *sector_values.entry(sector.clone()).or_insert(Decimal::ZERO) += mv;
                    }

                    // Rating breakdown
                    if let Some(ref rating) = position.rating {
                        *rating_values.entry(rating.clone()).or_insert(Decimal::ZERO) += mv;
                    }
                }
            } else {
                debug!(
                    "No price for position {} in portfolio {}",
                    position.instrument_id, portfolio.portfolio_id
                );
            }
        }

        // Check coverage
        let num_positions = portfolio.positions.len();
        let coverage = if num_positions > 0 {
            priced_count as f64 / num_positions as f64
        } else {
            0.0
        };

        if coverage < self.min_coverage {
            warn!(
                "Portfolio {} coverage {:.1}% below threshold {:.1}%",
                portfolio.portfolio_id,
                coverage * 100.0,
                self.min_coverage * 100.0
            );
        }

        // Normalize weighted metrics
        let (duration, convexity, yield_value, spread) = if total_market_value > Decimal::ZERO {
            (
                weighted_duration / total_market_value,
                weighted_convexity / total_market_value,
                weighted_yield / total_market_value,
                weighted_spread / total_market_value,
            )
        } else {
            (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO)
        };

        // Normalize key rate durations
        let key_rate_durations: Vec<(String, Decimal)> = if total_market_value > Decimal::ZERO {
            let mut krds: Vec<_> = krd_totals
                .into_iter()
                .map(|(tenor, total)| (tenor, total / total_market_value))
                .collect();
            // Sort by tenor
            krds.sort_by(|(a, _), (b, _)| {
                let parse_tenor = |t: &str| -> f64 {
                    if t.ends_with('M') {
                        t.trim_end_matches('M').parse::<f64>().unwrap_or(0.0) / 12.0
                    } else if t.ends_with('Y') {
                        t.trim_end_matches('Y').parse::<f64>().unwrap_or(0.0)
                    } else {
                        0.0
                    }
                };
                parse_tenor(a)
                    .partial_cmp(&parse_tenor(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            krds
        } else {
            Vec::new()
        };

        // Calculate percentage breakdowns
        let sector_breakdown: Vec<(String, Decimal)> = if total_market_value > Decimal::ZERO {
            sector_values
                .into_iter()
                .map(|(sector, mv)| (sector, mv / total_market_value))
                .collect()
        } else {
            Vec::new()
        };

        let rating_breakdown: Vec<(String, Decimal)> = if total_market_value > Decimal::ZERO {
            rating_values
                .into_iter()
                .map(|(rating, mv)| (rating, mv / total_market_value))
                .collect()
        } else {
            Vec::new()
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        info!(
            "Portfolio {} analytics: MV={}, Dur={:.2}, Yield={:.4}, Spread={:.0}bps, {} positions",
            portfolio.portfolio_id,
            total_market_value,
            duration.to_f64().unwrap_or(0.0),
            yield_value.to_f64().unwrap_or(0.0),
            spread.to_f64().unwrap_or(0.0),
            priced_count
        );

        Ok(PortfolioAnalyticsOutput {
            portfolio_id: portfolio.portfolio_id.clone(),
            name: portfolio.name.clone(),
            currency: portfolio.currency,

            market_value: total_market_value,
            num_positions: priced_count,

            duration,
            convexity,
            yield_value,
            spread,

            dv01: total_dv01,
            key_rate_durations,

            sector_breakdown,
            rating_breakdown,

            timestamp: now,
        })
    }

    /// Calculate analytics for multiple portfolios in parallel.
    pub fn calculate_batch(
        &self,
        portfolios: &[Portfolio],
        bond_prices: &[BondQuoteOutput],
    ) -> Vec<Result<PortfolioAnalyticsOutput, EngineError>> {
        portfolios
            .par_iter()
            .map(|portfolio| self.calculate(portfolio, bond_prices))
            .collect()
    }

    /// Calculate contribution analysis for a portfolio.
    ///
    /// Returns each position's contribution to portfolio duration.
    pub fn duration_contribution(
        &self,
        portfolio: &Portfolio,
        bond_prices: &[BondQuoteOutput],
    ) -> Vec<(InstrumentId, Decimal, Decimal)> {
        let price_map: HashMap<_, _> = bond_prices
            .iter()
            .map(|q| (q.instrument_id.clone(), q))
            .collect();

        let mut total_mv = Decimal::ZERO;
        let mut positions_with_data: Vec<(InstrumentId, Decimal, Decimal)> = Vec::new();

        // First pass: calculate total MV
        for position in &portfolio.positions {
            if let Some(quote) = price_map.get(&position.instrument_id) {
                if let (Some(dirty_price), Some(dur)) =
                    (quote.dirty_price_mid(), quote.modified_duration)
                {
                    let mv = position.notional * dirty_price / Decimal::from(100);
                    total_mv += mv;
                    positions_with_data.push((position.instrument_id.clone(), mv, dur));
                }
            }
        }

        // Second pass: calculate contributions
        if total_mv > Decimal::ZERO {
            positions_with_data
                .into_iter()
                .map(|(id, mv, dur)| {
                    let weight = mv / total_mv;
                    let contribution = weight * dur;
                    (id, weight, contribution)
                })
                .collect()
        } else {
            Vec::new()
        }
    }
}

impl Default for PortfolioAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::Date;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_test_portfolio() -> Portfolio {
        Portfolio {
            portfolio_id: PortfolioId::new("TEST_PORT"),
            name: "Test Portfolio".to_string(),
            currency: Currency::USD,
            positions: vec![
                Position {
                    instrument_id: InstrumentId::new("BOND_A"),
                    notional: dec!(1000000),
                    sector: Some("Technology".to_string()),
                    rating: Some("A".to_string()),
                },
                Position {
                    instrument_id: InstrumentId::new("BOND_B"),
                    notional: dec!(2000000),
                    sector: Some("Healthcare".to_string()),
                    rating: Some("BBB".to_string()),
                },
                Position {
                    instrument_id: InstrumentId::new("BOND_C"),
                    notional: dec!(500000),
                    sector: Some("Technology".to_string()),
                    rating: Some("A".to_string()),
                },
            ],
        }
    }

    fn create_test_quotes() -> Vec<BondQuoteOutput> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        vec![
            BondQuoteOutput {
                instrument_id: InstrumentId::new("BOND_A"),
                isin: None,
                currency: Currency::USD,
                settlement_date: date(2025, 6, 17),
                clean_price_bid: None,
                clean_price_mid: Some(dec!(99.00)),
                clean_price_ask: None,
                accrued_interest: Some(dec!(1.00)),
                ytm_bid: None,
                ytm_mid: Some(dec!(0.05)),
                ytm_ask: None,
                ytw: None,
                ytc: None,
                z_spread_bid: None,
                z_spread_mid: Some(dec!(100)),
                z_spread_ask: None,
                i_spread_bid: None,
                i_spread_mid: None,
                i_spread_ask: None,
                g_spread_bid: None,
                g_spread_mid: None,
                g_spread_ask: None,
                asw_bid: None,
                asw_mid: None,
                asw_ask: None,
                oas_bid: None,
                oas_mid: None,
                oas_ask: None,
                discount_margin_bid: None,
                discount_margin_mid: None,
                discount_margin_ask: None,
                simple_margin_bid: None,
                simple_margin_mid: None,
                simple_margin_ask: None,
                modified_duration: Some(dec!(5.0)),
                macaulay_duration: Some(dec!(5.2)),
                effective_duration: None,
                spread_duration: None,
                convexity: Some(dec!(30)),
                effective_convexity: None,
                dv01: Some(dec!(0.05)),
                pv01: None,
                key_rate_durations: Some(vec![
                    ("2Y".to_string(), dec!(1.0)),
                    ("5Y".to_string(), dec!(3.0)),
                    ("10Y".to_string(), dec!(1.0)),
                ]),
                cs01: None,
                timestamp: now,
                pricing_spec: "test".to_string(),
                source: "test".to_string(),
                is_stale: false,
                quality: 100,
            },
            BondQuoteOutput {
                instrument_id: InstrumentId::new("BOND_B"),
                isin: None,
                currency: Currency::USD,
                settlement_date: date(2025, 6, 17),
                clean_price_bid: None,
                clean_price_mid: Some(dec!(101.00)),
                clean_price_ask: None,
                accrued_interest: Some(dec!(1.00)),
                ytm_bid: None,
                ytm_mid: Some(dec!(0.045)),
                ytm_ask: None,
                ytw: None,
                ytc: None,
                z_spread_bid: None,
                z_spread_mid: Some(dec!(80)),
                z_spread_ask: None,
                i_spread_bid: None,
                i_spread_mid: None,
                i_spread_ask: None,
                g_spread_bid: None,
                g_spread_mid: None,
                g_spread_ask: None,
                asw_bid: None,
                asw_mid: None,
                asw_ask: None,
                oas_bid: None,
                oas_mid: None,
                oas_ask: None,
                discount_margin_bid: None,
                discount_margin_mid: None,
                discount_margin_ask: None,
                simple_margin_bid: None,
                simple_margin_mid: None,
                simple_margin_ask: None,
                modified_duration: Some(dec!(7.0)),
                macaulay_duration: Some(dec!(7.3)),
                effective_duration: None,
                spread_duration: None,
                convexity: Some(dec!(55)),
                effective_convexity: None,
                dv01: Some(dec!(0.071)),
                pv01: None,
                key_rate_durations: Some(vec![
                    ("2Y".to_string(), dec!(0.5)),
                    ("5Y".to_string(), dec!(2.0)),
                    ("10Y".to_string(), dec!(4.5)),
                ]),
                cs01: None,
                timestamp: now,
                pricing_spec: "test".to_string(),
                source: "test".to_string(),
                is_stale: false,
                quality: 100,
            },
            BondQuoteOutput {
                instrument_id: InstrumentId::new("BOND_C"),
                isin: None,
                currency: Currency::USD,
                settlement_date: date(2025, 6, 17),
                clean_price_bid: None,
                clean_price_mid: Some(dec!(98.00)),
                clean_price_ask: None,
                accrued_interest: Some(dec!(1.00)),
                ytm_bid: None,
                ytm_mid: Some(dec!(0.055)),
                ytm_ask: None,
                ytw: None,
                ytc: None,
                z_spread_bid: None,
                z_spread_mid: Some(dec!(120)),
                z_spread_ask: None,
                i_spread_bid: None,
                i_spread_mid: None,
                i_spread_ask: None,
                g_spread_bid: None,
                g_spread_mid: None,
                g_spread_ask: None,
                asw_bid: None,
                asw_mid: None,
                asw_ask: None,
                oas_bid: None,
                oas_mid: None,
                oas_ask: None,
                discount_margin_bid: None,
                discount_margin_mid: None,
                discount_margin_ask: None,
                simple_margin_bid: None,
                simple_margin_mid: None,
                simple_margin_ask: None,
                modified_duration: Some(dec!(3.0)),
                macaulay_duration: Some(dec!(3.1)),
                effective_duration: None,
                spread_duration: None,
                convexity: Some(dec!(12)),
                effective_convexity: None,
                dv01: Some(dec!(0.03)),
                pv01: None,
                key_rate_durations: Some(vec![
                    ("2Y".to_string(), dec!(2.0)),
                    ("5Y".to_string(), dec!(1.0)),
                ]),
                cs01: None,
                timestamp: now,
                pricing_spec: "test".to_string(),
                source: "test".to_string(),
                is_stale: false,
                quality: 100,
            },
        ]
    }

    #[test]
    fn test_portfolio_analytics() {
        let analyzer = PortfolioAnalyzer::new();
        let portfolio = create_test_portfolio();
        let quotes = create_test_quotes();

        let result = analyzer.calculate(&portfolio, &quotes);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert_eq!(output.portfolio_id, PortfolioId::new("TEST_PORT"));
        assert_eq!(output.num_positions, 3);

        // Check market value: 1M * 100% + 2M * 102% + 0.5M * 99%
        // = 1,000,000 + 2,040,000 + 495,000 = 3,535,000
        let mv = output.market_value.to_f64().unwrap();
        assert!(
            (mv - 3_535_000.0).abs() < 1.0,
            "Market value should be ~3.535M: {}",
            mv
        );

        // Duration should be weighted average
        let dur = output.duration.to_f64().unwrap();
        assert!(
            dur > 0.0 && dur < 10.0,
            "Duration should be reasonable: {}",
            dur
        );

        // Check sector breakdown sums to ~1.0
        let sector_sum: Decimal = output.sector_breakdown.iter().map(|(_, w)| *w).sum();
        assert!(
            (sector_sum.to_f64().unwrap() - 1.0).abs() < 0.01,
            "Sector weights should sum to 1: {}",
            sector_sum
        );
    }

    #[test]
    fn test_duration_contribution() {
        let analyzer = PortfolioAnalyzer::new();
        let portfolio = create_test_portfolio();
        let quotes = create_test_quotes();

        let contributions = analyzer.duration_contribution(&portfolio, &quotes);
        assert_eq!(contributions.len(), 3);

        // Contributions should sum to portfolio duration
        let total_contribution: Decimal = contributions.iter().map(|(_, _, c)| *c).sum();

        // Get portfolio duration for comparison
        let output = analyzer.calculate(&portfolio, &quotes).unwrap();
        let port_dur = output.duration;

        let diff = (total_contribution - port_dur).abs();
        assert!(
            diff < dec!(0.01),
            "Contributions should sum to portfolio duration: {} vs {}",
            total_contribution,
            port_dur
        );
    }

    #[test]
    fn test_key_rate_duration_aggregation() {
        let analyzer = PortfolioAnalyzer::new();
        let portfolio = create_test_portfolio();
        let quotes = create_test_quotes();

        let result = analyzer.calculate(&portfolio, &quotes).unwrap();

        // Should have key rate durations
        assert!(!result.key_rate_durations.is_empty());

        // Check that KRDs are sorted by tenor
        let tenors: Vec<&str> = result
            .key_rate_durations
            .iter()
            .map(|(t, _)| t.as_str())
            .collect();
        assert_eq!(tenors, vec!["2Y", "5Y", "10Y"]);
    }
}
