//! ETF pricing and iNAV calculation.
//!
//! Calculates indicative Net Asset Value (iNAV) for bond ETFs by aggregating
//! individual bond prices weighted by holdings.

use rayon::prelude::*;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use tracing::{debug, info};

use convex_core::Date;
use convex_traits::output::{BondQuoteOutput, EtfQuoteOutput};
use convex_traits::reference_data::EtfHoldings;

use crate::error::EngineError;

/// ETF pricing calculator.
pub struct EtfPricer {
    /// Stale threshold in seconds
    stale_threshold_secs: i64,
}

impl EtfPricer {
    /// Create a new ETF pricer.
    pub fn new() -> Self {
        Self {
            stale_threshold_secs: 300, // 5 minutes
        }
    }

    /// Set the stale threshold.
    pub fn with_stale_threshold(mut self, secs: i64) -> Self {
        self.stale_threshold_secs = secs;
        self
    }

    /// Calculate iNAV for an ETF from its holdings and bond prices.
    ///
    /// # Arguments
    /// * `holdings` - ETF holdings data
    /// * `bond_prices` - Map of instrument_id -> bond quote output
    /// * `settlement` - Settlement date for pricing
    ///
    /// # Returns
    /// ETF quote output with iNAV and aggregated analytics
    pub fn calculate_inav(
        &self,
        holdings: &EtfHoldings,
        bond_prices: &[BondQuoteOutput],
        _settlement: Date,
    ) -> Result<EtfQuoteOutput, EngineError> {
        use std::collections::HashMap;

        // Build lookup map for bond prices
        let price_map: HashMap<_, _> = bond_prices
            .iter()
            .map(|q| (q.instrument_id.clone(), q))
            .collect();

        let mut total_value = Decimal::ZERO;
        let mut total_weight_priced = Decimal::ZERO;
        let mut weighted_duration = Decimal::ZERO;
        let mut weighted_yield = Decimal::ZERO;
        let mut weighted_spread = Decimal::ZERO;
        let mut priced_count = 0u32;
        let mut stale_count = 0u32;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        for holding in &holdings.holdings {
            if let Some(quote) = price_map.get(&holding.instrument_id) {
                // Check staleness
                let is_stale = (now - quote.timestamp / 1000) > self.stale_threshold_secs;
                if is_stale {
                    stale_count += 1;
                }

                // Use dirty price for valuation
                if let Some(dirty_price) = quote.dirty_price {
                    let position_value = holding.notional_value * dirty_price / Decimal::from(100);
                    total_value += position_value;
                    total_weight_priced += holding.weight;
                    priced_count += 1;

                    // Aggregate analytics weighted by market value
                    if let Some(dur) = quote.modified_duration {
                        weighted_duration += dur * holding.weight;
                    }
                    if let Some(ytm) = quote.ytm {
                        weighted_yield += ytm * holding.weight;
                    }
                    if let Some(z_spread) = quote.z_spread {
                        weighted_spread += z_spread * holding.weight;
                    }
                }
            } else {
                debug!(
                    "No price for holding {} in ETF {}",
                    holding.instrument_id, holdings.etf_id
                );
            }
        }

        let num_holdings = holdings.holdings.len() as u32;
        let coverage = if num_holdings > 0 {
            Decimal::from(priced_count) / Decimal::from(num_holdings)
        } else {
            Decimal::ZERO
        };

        // Normalize weighted metrics
        let (duration, yield_value, spread) = if total_weight_priced > Decimal::ZERO {
            (
                Some(weighted_duration / total_weight_priced),
                Some(weighted_yield / total_weight_priced),
                Some(weighted_spread / total_weight_priced),
            )
        } else {
            (None, None, None)
        };

        // Calculate iNAV per share
        let inav = if holdings.shares_outstanding > Decimal::ZERO {
            Some(total_value / holdings.shares_outstanding)
        } else {
            None
        };

        // Check if result is stale
        let is_stale = stale_count > 0 || coverage < Decimal::from_f64_retain(0.9).unwrap();

        info!(
            "ETF {} iNAV: {:?}, coverage: {:.1}%, {} holdings priced, {} stale",
            holdings.etf_id,
            inav,
            coverage.to_f64().unwrap_or(0.0) * 100.0,
            priced_count,
            stale_count
        );

        Ok(EtfQuoteOutput {
            etf_id: holdings.etf_id.clone(),
            name: holdings.name.clone(),
            currency: holdings.currency,

            nav: holdings.nav_per_share,
            inav,
            price: None, // Market price would come from exchange feed
            premium_discount: match (inav, holdings.nav_per_share) {
                (Some(i), Some(n)) if n > Decimal::ZERO => Some((i - n) / n),
                _ => None,
            },

            num_holdings,
            coverage,

            duration,
            yield_value,
            spread,

            timestamp: now * 1000,
            is_stale,
        })
    }

    /// Calculate iNAV for multiple ETFs in parallel.
    pub fn calculate_inav_batch(
        &self,
        etf_holdings: &[EtfHoldings],
        bond_prices: &[BondQuoteOutput],
        settlement: Date,
    ) -> Vec<Result<EtfQuoteOutput, EngineError>> {
        etf_holdings
            .par_iter()
            .map(|holdings| self.calculate_inav(holdings, bond_prices, settlement))
            .collect()
    }
}

impl Default for EtfPricer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_traits::ids::{EtfId, InstrumentId};
    use convex_traits::reference_data::EtfHoldingEntry;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_test_holdings() -> EtfHoldings {
        EtfHoldings {
            etf_id: EtfId::new("LQD"),
            name: "iShares iBoxx Investment Grade Corporate Bond ETF".to_string(),
            currency: convex_core::Currency::USD,
            as_of_date: date(2025, 6, 15),
            holdings: vec![
                EtfHoldingEntry {
                    instrument_id: InstrumentId::new("US912810TD00"),
                    weight: dec!(0.10),
                    shares: dec!(1000),
                    market_value: dec!(100000),
                    notional_value: dec!(100000),
                    accrued_interest: Some(dec!(500)),
                },
                EtfHoldingEntry {
                    instrument_id: InstrumentId::new("US037833DV24"),
                    weight: dec!(0.15),
                    shares: dec!(1500),
                    market_value: dec!(150000),
                    notional_value: dec!(150000),
                    accrued_interest: Some(dec!(750)),
                },
                EtfHoldingEntry {
                    instrument_id: InstrumentId::new("US594918BY90"),
                    weight: dec!(0.05),
                    shares: dec!(500),
                    market_value: dec!(50000),
                    notional_value: dec!(50000),
                    accrued_interest: Some(dec!(250)),
                },
            ],
            total_market_value: dec!(300000),
            shares_outstanding: dec!(3000),
            nav_per_share: Some(dec!(100.00)),
            last_updated: 1718409600000,
            source: "test".to_string(),
        }
    }

    fn create_test_bond_prices() -> Vec<BondQuoteOutput> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        vec![
            BondQuoteOutput {
                instrument_id: InstrumentId::new("US912810TD00"),
                isin: Some("US912810TD00".to_string()),
                currency: convex_core::Currency::USD,
                settlement_date: date(2025, 6, 17),
                clean_price: Some(dec!(99.50)),
                dirty_price: Some(dec!(100.00)),
                accrued_interest: Some(dec!(0.50)),
                ytm: Some(dec!(0.0525)),
                ytw: None,
                ytc: None,
                z_spread: Some(dec!(50)),
                i_spread: Some(dec!(45)),
                g_spread: Some(dec!(55)),
                asw: Some(dec!(48)),
                oas: None,
                discount_margin: None,
                simple_margin: None,
                modified_duration: Some(dec!(5.5)),
                macaulay_duration: Some(dec!(5.7)),
                effective_duration: Some(dec!(5.5)),
                spread_duration: Some(dec!(5.4)),
                convexity: Some(dec!(35)),
                effective_convexity: Some(dec!(34)),
                dv01: Some(dec!(0.055)),
                pv01: Some(dec!(0.054)),
                key_rate_durations: None,
                cs01: Some(dec!(0.054)),
                timestamp: now,
                pricing_model: "DiscountToMaturity".to_string(),
                source: "test".to_string(),
                is_stale: false,
                quality: 100,
            },
            BondQuoteOutput {
                instrument_id: InstrumentId::new("US037833DV24"),
                isin: Some("US037833DV24".to_string()),
                currency: convex_core::Currency::USD,
                settlement_date: date(2025, 6, 17),
                clean_price: Some(dec!(101.00)),
                dirty_price: Some(dec!(102.00)),
                accrued_interest: Some(dec!(1.00)),
                ytm: Some(dec!(0.0475)),
                ytw: None,
                ytc: None,
                z_spread: Some(dec!(40)),
                i_spread: Some(dec!(35)),
                g_spread: Some(dec!(45)),
                asw: Some(dec!(38)),
                oas: None,
                discount_margin: None,
                simple_margin: None,
                modified_duration: Some(dec!(7.2)),
                macaulay_duration: Some(dec!(7.5)),
                effective_duration: Some(dec!(7.2)),
                spread_duration: Some(dec!(7.1)),
                convexity: Some(dec!(65)),
                effective_convexity: Some(dec!(64)),
                dv01: Some(dec!(0.073)),
                pv01: Some(dec!(0.072)),
                key_rate_durations: None,
                cs01: Some(dec!(0.071)),
                timestamp: now,
                pricing_model: "DiscountToMaturity".to_string(),
                source: "test".to_string(),
                is_stale: false,
                quality: 100,
            },
        ]
    }

    #[test]
    fn test_calculate_inav() {
        let pricer = EtfPricer::new();
        let holdings = create_test_holdings();
        let prices = create_test_bond_prices();
        let settlement = date(2025, 6, 17);

        let result = pricer.calculate_inav(&holdings, &prices, settlement);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert_eq!(output.etf_id, EtfId::new("LQD"));
        assert!(output.inav.is_some(), "iNAV should be calculated");
        assert!(output.duration.is_some(), "Duration should be calculated");
        assert!(output.yield_value.is_some(), "Yield should be calculated");

        // 2 out of 3 holdings priced
        let coverage = output.coverage.to_f64().unwrap();
        assert!(
            (coverage - 0.6667).abs() < 0.01,
            "Coverage should be ~66.7%: {}",
            coverage
        );
    }

    #[test]
    fn test_inav_with_full_coverage() {
        let pricer = EtfPricer::new();
        let mut holdings = create_test_holdings();
        // Remove the third holding that doesn't have a price
        holdings.holdings.pop();
        holdings.total_market_value = dec!(250000);

        let prices = create_test_bond_prices();
        let settlement = date(2025, 6, 17);

        let result = pricer.calculate_inav(&holdings, &prices, settlement);
        assert!(result.is_ok());

        let output = result.unwrap();
        let coverage = output.coverage.to_f64().unwrap();
        assert!(
            (coverage - 1.0).abs() < 0.001,
            "Coverage should be 100%: {}",
            coverage
        );
        assert!(!output.is_stale, "Should not be stale with full coverage");
    }
}
