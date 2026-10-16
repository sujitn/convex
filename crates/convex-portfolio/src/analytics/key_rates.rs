//! Key rate duration analytics for portfolios.
//!
//! Aggregates key rate duration profiles across holdings to understand
//! interest rate sensitivity at different points on the curve.
//!
//! Supplied key-rate durations use exact factor identity and explicit coverage.
//! Currency risk adds actual position exposures independently of presentation weights.

use super::exact_risk::ExactRisk;
use crate::types::{AnalyticsConfig, Holding, WeightingMethod};
use crate::{PortfolioError, PortfolioResult};
use convex_analytics::risk::{
    Duration, KeyRateDuration, KeyRateDurations, STANDARD_KEY_RATE_TENORS,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Aggregated key rate duration profile for a portfolio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRateProfile {
    /// Key rate durations at each tenor point.
    pub durations: Vec<KeyRateDuration>,

    /// Sum of known weighted factors; missing requested factors are not inferred.
    pub total_duration: f64,

    /// Number of holdings explicitly declaring every requested factor.
    pub coverage: usize,

    /// Total number of holdings.
    pub total_holdings: usize,

    /// Tenor points used.
    pub tenors: Vec<f64>,
}

impl KeyRateProfile {
    /// Returns the coverage as a percentage.
    #[must_use]
    pub fn coverage_pct(&self) -> f64 {
        if self.total_holdings > 0 {
            self.coverage as f64 / self.total_holdings as f64 * 100.0
        } else {
            0.0
        }
    }

    /// Gets the duration at a specific tenor.
    #[must_use]
    pub fn at_tenor(&self, tenor: f64) -> Option<f64> {
        if !tenor.is_finite() || tenor <= 0.0 || self.durations.len() > MAX_KEY_RATE_FACTORS {
            return None;
        }
        let mut seen = HashSet::with_capacity(self.durations.len());
        if self.durations.iter().any(|row| {
            !row.tenor.is_finite() || row.tenor <= 0.0 || !seen.insert(row.tenor.to_bits())
        }) {
            return None;
        }
        self.durations
            .iter()
            .find(|row| row.tenor.to_bits() == tenor.to_bits())
            .map(|row| row.duration.as_f64())
    }

    /// Gets the duration for tenors in a range.
    #[must_use]
    pub fn in_range(&self, min_tenor: f64, max_tenor: f64) -> Vec<&KeyRateDuration> {
        self.durations
            .iter()
            .filter(|krd| krd.tenor >= min_tenor && krd.tenor <= max_tenor)
            .collect()
    }

    /// Converts to a KeyRateDurations type for compatibility.
    #[must_use]
    pub fn to_key_rate_durations(&self) -> KeyRateDurations {
        KeyRateDurations::new(self.durations.clone())
    }

    /// Returns the short-end duration (< 2 years).
    #[must_use]
    pub fn short_duration(&self) -> f64 {
        self.durations
            .iter()
            .filter(|krd| krd.tenor < 2.0)
            .map(|krd| krd.duration.as_f64())
            .sum()
    }

    /// Returns the intermediate duration (2-10 years).
    #[must_use]
    pub fn intermediate_duration(&self) -> f64 {
        self.durations
            .iter()
            .filter(|krd| krd.tenor >= 2.0 && krd.tenor <= 10.0)
            .map(|krd| krd.duration.as_f64())
            .sum()
    }

    /// Returns the long-end duration (> 10 years).
    #[must_use]
    pub fn long_duration(&self) -> f64 {
        self.durations
            .iter()
            .filter(|krd| krd.tenor > 10.0)
            .map(|krd| krd.duration.as_f64())
            .sum()
    }
}

/// Hard bound on holding count admitted by supplied key-rate aggregation.
pub const MAX_KEY_RATE_HOLDINGS: usize = 65_536;
/// Hard bound on requested or per-holding factors.
pub const MAX_KEY_RATE_FACTORS: usize = 4096;
/// Bound both the requested holding/factor product and all supplied factor rows.
pub const MAX_KEY_RATE_WORK: usize = 1_048_576;

/// One exactly requested factor with explicit known and missing exposure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRateExposurePoint {
    /// Exact supplied tenor, in years.
    pub tenor: f64,
    /// Known holding-weighted ratio; unavailable if no covered weight exists.
    pub duration: Option<Duration>,
    /// Known common-base currency loss for an upward one-basis-point move.
    /// Missing holdings are excluded, and never assumed to have zero risk.
    pub known_partial_dv01: Option<Decimal>,
    /// Actual market value of only holdings that declare this factor.
    pub covered_market_value: Decimal,
    /// Number of holdings that explicitly declare this factor, including zeros.
    pub covered_holdings: usize,
    /// Number of supplied holdings, including those without risk.
    pub total_holdings: usize,
}

impl KeyRateExposurePoint {
    /// Whether every supplied holding explicitly declares this factor.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.total_holdings > 0 && self.covered_holdings == self.total_holdings
    }
}

/// Checked factor exposures and optional known weighted-duration presentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRateExposure {
    /// Factors retain caller order, including unavailable factors.
    pub factors: Vec<KeyRateExposurePoint>,
    /// Known weighted factors only. No defined ratio exists at zero covered weight.
    pub profile: Option<KeyRateProfile>,
    /// Holdings explicitly declaring every requested factor.
    pub complete_holdings: usize,
    /// All admitted supplied holdings.
    pub total_holdings: usize,
}

/// Aggregate supplied holdings through one bounded checked native owner.
///
/// Duration presentation uses the configured weight of only holdings declaring
/// each exact factor. Currency DV01 always sums each covered holding's actual
/// par * (clean price + supplied accrued) * explicit FX / 100 * duration / 10,000. Presentation weighting
/// cannot alter it. Signed sums and their products remain exact through cancellation.
/// Final Decimal results use nearest/even at the greatest representable scale up
/// to 28; a nonzero result that rounds to zero or exceeds Decimal's range fails.
/// Supplied prices and durations must use one declared price
/// basis and caller-declared common base currency; this function does no pricing
/// or FX inference. Missing factors remain unavailable with explicit coverage.
/// `tenors` overrides configured tenors; otherwise the configured or standard
/// tenors apply. Parallel flags do not change deterministic Decimal aggregation.
///
/// # Errors
/// Rejects invalid/duplicate identities or factors, invalid holding amounts/FX,
/// bounded-work excess, Decimal overflow and nonzero arithmetic underflow.
pub fn aggregate_key_rate_exposure(
    holdings: &[Holding],
    config: &AnalyticsConfig,
    tenors: Option<&[f64]>,
) -> PortfolioResult<KeyRateExposure> {
    let requested = tenors
        .or(config.key_rate_tenors.as_deref())
        .unwrap_or(STANDARD_KEY_RATE_TENORS);
    if holdings.len() > MAX_KEY_RATE_HOLDINGS
        || requested.is_empty()
        || requested.len() > MAX_KEY_RATE_FACTORS
        || holdings
            .len()
            .checked_mul(requested.len())
            .is_none_or(|work| work > MAX_KEY_RATE_WORK)
    {
        return Err(PortfolioError::invalid_portfolio(
            "key-rate holding/factor work bound exceeded",
        ));
    }
    let mut unique = HashSet::with_capacity(requested.len());
    for &tenor in requested {
        if !tenor.is_finite() || tenor <= 0.0 || !unique.insert(tenor.to_bits()) {
            return Err(PortfolioError::invalid_portfolio(
                "finite positive unique key-rate tenors required",
            ));
        }
    }
    let mut ids = HashSet::with_capacity(holdings.len());
    let mut admitted = Vec::with_capacity(holdings.len());
    let mut supplied_work = 0usize;
    let mut complete_holdings = 0usize;
    for holding in holdings {
        if holding.id.trim().is_empty()
            || holding.id.len() > 8192
            || !ids.insert(holding.id.as_str())
            || holding.par_amount <= Decimal::ZERO
            || holding.market_price < Decimal::ZERO
            || holding.fx_rate <= Decimal::ZERO
        {
            return Err(PortfolioError::invalid_holding(
                &holding.id,
                "bounded unique identity, positive par/FX and nonnegative actual price required",
            ));
        }
        let mut dirty_price = ExactRisk::decimal(holding.market_price);
        dirty_price.add(&ExactRisk::decimal(holding.accrued_interest))?;
        if dirty_price.is_negative() {
            return Err(PortfolioError::invalid_holding(
                &holding.id,
                "supplied dirty price cannot be negative",
            ));
        }
        if dirty_price.is_zero()
            && holding
                .analytics
                .key_rate_durations
                .as_ref()
                .is_some_and(|risk| !risk.durations.is_empty())
        {
            return Err(PortfolioError::invalid_holding(&holding.id,"relative key-rate durations require a positive dirty valuation basis; supply absolute sensitivities for zero-value instruments"));
        }
        let market_value = dirty_price
            .multiply(holding.par_amount)?
            .multiply(holding.fx_rate)?
            .divide_power_of_ten(2)?;
        let weight = match config.weighting {
            WeightingMethod::MarketValue => market_value.clone(),
            WeightingMethod::ParValue => {
                ExactRisk::decimal(holding.par_amount).multiply(holding.fx_rate)?
            }
            WeightingMethod::EqualWeight => ExactRisk::decimal(Decimal::ONE),
        };
        let mut factors = HashMap::new();
        if let Some(risk) = &holding.analytics.key_rate_durations {
            supplied_work = supplied_work
                .checked_add(risk.durations.len())
                .ok_or_else(|| {
                    PortfolioError::invalid_portfolio("supplied key-rate row bound exceeded")
                })?;
            if risk.durations.len() > MAX_KEY_RATE_FACTORS || supplied_work > MAX_KEY_RATE_WORK {
                return Err(PortfolioError::invalid_portfolio(
                    "supplied key-rate row bound exceeded",
                ));
            }
            for row in &risk.durations {
                if !row.tenor.is_finite()
                    || row.tenor <= 0.0
                    || factors
                        .insert(row.tenor.to_bits(), row.duration.years())
                        .is_some()
                {
                    return Err(PortfolioError::invalid_holding(
                        &holding.id,
                        "finite positive distinct supplied factors required",
                    ));
                }
            }
        }
        if requested
            .iter()
            .all(|tenor| factors.contains_key(&tenor.to_bits()))
        {
            complete_holdings += 1;
        }
        admitted.push((weight, market_value, factors));
    }
    let mut points = Vec::with_capacity(requested.len());
    let mut durations = Vec::with_capacity(requested.len());
    for &tenor in requested {
        let mut weighted = ExactRisk::zero();
        let mut weight_sum = ExactRisk::zero();
        let mut market_sum = ExactRisk::zero();
        let mut currency_risk = ExactRisk::zero();
        let mut covered = 0usize;
        for (weight, market_value, factors) in &admitted {
            if let Some(&duration) = factors.get(&tenor.to_bits()) {
                covered += 1;
                weighted.add(&weight.multiply(duration)?)?;
                weight_sum.add(weight)?;
                market_sum.add(market_value)?;
                currency_risk.add(&market_value.multiply(duration)?.divide_power_of_ten(4)?)?;
            }
        }
        let duration = if !weight_sum.is_zero() {
            Some(Duration::new(weighted.ratio(&weight_sum)?))
        } else {
            None
        };
        if let Some(duration) = duration {
            durations.push(KeyRateDuration { tenor, duration });
        }
        points.push(KeyRateExposurePoint {
            tenor,
            duration,
            known_partial_dv01: if covered > 0 {
                Some(currency_risk.finish()?)
            } else {
                None
            },
            covered_market_value: market_sum.finish()?,
            covered_holdings: covered,
            total_holdings: holdings.len(),
        });
    }
    let profile = if durations.is_empty() {
        None
    } else {
        let mut total = ExactRisk::zero();
        for row in &durations {
            total.add(&ExactRisk::decimal(row.duration.years()))?;
        }
        let total = total.finish()?;
        Some(KeyRateProfile {
            durations,
            total_duration: total.as_f64(),
            coverage: complete_holdings,
            total_holdings: holdings.len(),
            tenors: requested.to_vec(),
        })
    };
    Ok(KeyRateExposure {
        factors: points,
        profile,
        complete_holdings,
        total_holdings: holdings.len(),
    })
}

/// Known weighted factors, with unavailable factors omitted from `durations`.
///
/// Use `aggregate_key_rate_exposure` for typed errors and per-factor coverage.
/// Invalid inputs or no defined weighted factors return `None`; missing input
/// and zero covered weight never become a successful zero duration.
#[must_use]
pub fn aggregate_key_rate_profile(
    holdings: &[Holding],
    config: &AnalyticsConfig,
    tenors: Option<&[f64]>,
) -> Option<KeyRateProfile> {
    aggregate_key_rate_exposure(holdings, config, tenors)
        .ok()?
        .profile
}

/// Known common-base currency DV01 at each actually declared requested factor.
///
/// This is the sum of individual known exposures, regardless of presentation
/// weights. Missing factors are omitted; a supplied zero remains an actual zero.
/// Use `aggregate_key_rate_exposure` to inspect missing holding coverage or errors.
#[must_use]
pub fn partial_dv01s(
    holdings: &[Holding],
    config: &AnalyticsConfig,
    tenors: Option<&[f64]>,
) -> Option<Vec<(f64, f64)>> {
    let exposure = aggregate_key_rate_exposure(holdings, config, tenors).ok()?;
    let known: Vec<_> = exposure
        .factors
        .iter()
        .filter_map(|point| {
            point
                .known_partial_dv01
                .map(|risk| (point.tenor, risk.as_f64()))
        })
        .collect();
    (!known.is_empty()).then_some(known)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::HoldingAnalytics;
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn create_krd(tenors: &[(f64, f64)]) -> KeyRateDurations {
        let durations = tenors
            .iter()
            .map(|(tenor, dur)| KeyRateDuration {
                tenor: *tenor,
                duration: Duration::try_from(*dur).unwrap(),
            })
            .collect();
        KeyRateDurations::new(durations)
    }

    fn create_holding_with_krd(
        id: &str,
        par: Decimal,
        price: Decimal,
        krd: KeyRateDurations,
    ) -> Holding {
        let mut analytics = HoldingAnalytics::new();
        analytics.key_rate_durations = Some(krd);

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
    fn test_aggregate_key_rate_profile() {
        // Two bonds with equal market value
        let krd1 = create_krd(&[(2.0, 0.5), (5.0, 1.5), (10.0, 2.0)]);
        let krd2 = create_krd(&[(2.0, 0.3), (5.0, 1.0), (10.0, 1.5)]);

        let holdings = vec![
            create_holding_with_krd("BOND1", dec!(1_000_000), dec!(100), krd1),
            create_holding_with_krd("BOND2", dec!(1_000_000), dec!(100), krd2),
        ];

        let config = AnalyticsConfig::default();
        let tenors = &[2.0, 5.0, 10.0];
        let profile = aggregate_key_rate_profile(&holdings, &config, Some(tenors)).unwrap();

        assert_eq!(profile.coverage, 2);
        assert_eq!(profile.total_holdings, 2);
        assert!((profile.coverage_pct() - 100.0).abs() < 0.01);

        // Equal MV: average KRDs
        // 2Y: (0.5 + 0.3) / 2 = 0.4
        // 5Y: (1.5 + 1.0) / 2 = 1.25
        // 10Y: (2.0 + 1.5) / 2 = 1.75
        let dur_2y = profile.at_tenor(2.0).unwrap();
        assert!((dur_2y - 0.4).abs() < 0.01);

        let dur_5y = profile.at_tenor(5.0).unwrap();
        assert!((dur_5y - 1.25).abs() < 0.01);

        let dur_10y = profile.at_tenor(10.0).unwrap();
        assert!((dur_10y - 1.75).abs() < 0.01);

        // Total: 0.4 + 1.25 + 1.75 = 3.4
        assert!((profile.total_duration - 3.4).abs() < 0.01);
    }

    #[test]
    fn test_weighted_aggregation() {
        // Bond1 has 2x the market value of Bond2
        let krd1 = create_krd(&[(5.0, 3.0)]);
        let krd2 = create_krd(&[(5.0, 6.0)]);

        let holdings = vec![
            create_holding_with_krd("BOND1", dec!(2_000_000), dec!(100), krd1), // MV = 2M
            create_holding_with_krd("BOND2", dec!(1_000_000), dec!(100), krd2), // MV = 1M
        ];

        let config = AnalyticsConfig::default();
        let profile = aggregate_key_rate_profile(&holdings, &config, Some(&[5.0])).unwrap();

        // MV weighted: (2M × 3.0 + 1M × 6.0) / 3M = 12M / 3M = 4.0
        let dur_5y = profile.at_tenor(5.0).unwrap();
        assert!((dur_5y - 4.0).abs() < 0.01);
    }

    #[test]
    fn test_duration_buckets() {
        let krd = create_krd(&[
            (0.5, 0.1),  // short
            (1.0, 0.2),  // short
            (2.0, 0.3),  // intermediate
            (5.0, 1.0),  // intermediate
            (10.0, 1.5), // intermediate
            (20.0, 2.0), // long
            (30.0, 1.0), // long
        ]);

        let holdings = vec![create_holding_with_krd(
            "BOND1",
            dec!(1_000_000),
            dec!(100),
            krd,
        )];

        let config = AnalyticsConfig::default();
        let tenors = &[0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0];
        let profile = aggregate_key_rate_profile(&holdings, &config, Some(tenors)).unwrap();

        // Short: 0.1 + 0.2 = 0.3
        assert!((profile.short_duration() - 0.3).abs() < 0.01);

        // Intermediate: 0.3 + 1.0 + 1.5 = 2.8
        assert!((profile.intermediate_duration() - 2.8).abs() < 0.01);

        // Long: 2.0 + 1.0 = 3.0
        assert!((profile.long_duration() - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_partial_dv01s() {
        let krd = create_krd(&[(5.0, 5.0), (10.0, 3.0)]);

        let holdings = vec![create_holding_with_krd(
            "BOND1",
            dec!(1_000_000),
            dec!(100),
            krd,
        )];

        let config = AnalyticsConfig::default();
        let partials = partial_dv01s(&holdings, &config, Some(&[5.0, 10.0])).unwrap();

        // MV = 1,000,000
        // 5Y partial DV01 = 1,000,000 × 5.0 × 0.0001 = 500
        // 10Y partial DV01 = 1,000,000 × 3.0 × 0.0001 = 300
        let (tenor_5y, dv01_5y) = partials
            .iter()
            .find(|(t, _)| (*t - 5.0).abs() < 0.01)
            .unwrap();
        assert!((*tenor_5y - 5.0).abs() < 0.01);
        assert!((*dv01_5y - 500.0).abs() < 1.0);

        let (tenor_10y, dv01_10y) = partials
            .iter()
            .find(|(t, _)| (*t - 10.0).abs() < 0.01)
            .unwrap();
        assert!((*tenor_10y - 10.0).abs() < 0.01);
        assert!((*dv01_10y - 300.0).abs() < 1.0);
    }

    #[test]
    fn test_no_krd_data() {
        let holding = Holding::builder()
            .id("BOND1")
            .identifiers(BondIdentifiers::new().with_ticker("TEST001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new()) // No KRD data
            .build()
            .unwrap();

        let holdings = vec![holding];
        let config = AnalyticsConfig::default();

        assert!(aggregate_key_rate_profile(&holdings, &config, None).is_none());
    }

    #[test]
    fn test_partial_coverage() {
        let krd = create_krd(&[(5.0, 2.0)]);

        let holdings = vec![
            create_holding_with_krd("BOND1", dec!(1_000_000), dec!(100), krd),
            Holding::builder()
                .id("BOND2")
                .identifiers(BondIdentifiers::new().with_ticker("TEST002"))
                .par_amount(dec!(1_000_000))
                .market_price(dec!(100))
                .analytics(HoldingAnalytics::new()) // No KRD
                .build()
                .unwrap(),
        ];

        let config = AnalyticsConfig::default();
        let profile = aggregate_key_rate_profile(&holdings, &config, Some(&[5.0])).unwrap();

        assert_eq!(profile.coverage, 1);
        assert_eq!(profile.total_holdings, 2);
        assert!((profile.coverage_pct() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_in_range() {
        let krd = create_krd(&[(1.0, 0.5), (2.0, 1.0), (5.0, 2.0), (10.0, 3.0), (30.0, 2.0)]);

        let holdings = vec![create_holding_with_krd(
            "BOND1",
            dec!(1_000_000),
            dec!(100),
            krd,
        )];

        let config = AnalyticsConfig::default();
        let tenors = &[1.0, 2.0, 5.0, 10.0, 30.0];
        let profile = aggregate_key_rate_profile(&holdings, &config, Some(tenors)).unwrap();

        // Get 2-10 year range
        let mid_range = profile.in_range(2.0, 10.0);
        assert_eq!(mid_range.len(), 3); // 2Y, 5Y, 10Y
    }
}
