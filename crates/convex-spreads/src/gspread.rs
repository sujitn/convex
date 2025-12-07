//! G-Spread (Government Spread) calculation.
//!
//! G-spread is the spread over an interpolated government yield curve.
//! It measures the additional yield a bond pays over a risk-free government benchmark.
//!
//! # Overview
//!
//! The G-spread calculator supports multiple sovereign curves (UST, Gilts, Bunds, etc.)
//! and various benchmark specification methods:
//!
//! - **Interpolated**: Yield interpolated at bond's exact maturity (most common)
//! - **On-the-run tenor**: Spread to a specific benchmark (e.g., 10-year)
//! - **Nearest on-the-run**: Spread to nearest standard benchmark
//! - **Specific security**: Spread to a specific treasury by CUSIP/ISIN
//! - **Explicit yield**: Spread to a user-provided benchmark yield
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_spreads::gspread::GSpreadCalculator;
//! use convex_spreads::benchmark::BenchmarkSpec;
//! use convex_spreads::government_curve::GovernmentCurve;
//!
//! // Create US Treasury curve with benchmarks
//! let ust_curve = GovernmentCurve::us_treasury(settlement)
//!     .with_benchmark(benchmark_2y)
//!     .with_benchmark(benchmark_10y);
//!
//! // Create calculator
//! let calc = GSpreadCalculator::new(&ust_curve);
//!
//! // Calculate G-spread (interpolated - most common)
//! let spread = calc.calculate(bond_yield, maturity, settlement, BenchmarkSpec::interpolated())?;
//! println!("G-Spread: {} bps vs {}", spread.spread.as_bps(), spread.benchmark_info.description());
//! ```

use rust_decimal::Decimal;

use convex_bonds::instruments::{Bond, FixedBond};
use convex_bonds::pricing::BondPricer;
use convex_bonds::types::Tenor;
use convex_core::types::{Compounding, Date, Price, Spread, SpreadType, Yield};
use convex_curves::curves::ZeroCurve;

use crate::benchmark::{BenchmarkSpec, SecurityId};
use crate::error::{SpreadError, SpreadResult};
use crate::government_curve::GovernmentCurve;
use crate::sovereign::Sovereign;

/// Detailed information about the benchmark used.
#[derive(Debug, Clone)]
pub enum BenchmarkInfo {
    /// Interpolated from the government curve.
    Interpolated {
        /// Sovereign issuer
        sovereign: Sovereign,
        /// Years to maturity where yield was interpolated
        years: f64,
    },
    /// Specific benchmark tenor.
    Benchmark {
        /// Sovereign issuer
        sovereign: Sovereign,
        /// Benchmark tenor
        tenor: Tenor,
        /// Security identifier
        id: SecurityId,
        /// Benchmark maturity date
        maturity: Date,
    },
    /// Specific security by identifier.
    SpecificSecurity {
        /// Sovereign issuer
        sovereign: Sovereign,
        /// Security identifier
        id: SecurityId,
        /// Benchmark tenor
        tenor: Tenor,
        /// Benchmark maturity date
        maturity: Date,
    },
    /// Explicit yield provided by user.
    Explicit,
}

impl BenchmarkInfo {
    /// Returns a human-readable description of the benchmark.
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::Interpolated { sovereign, years } => {
                format!(
                    "{} {} @ {:.2}Y",
                    sovereign.bond_name(),
                    "interpolated",
                    years
                )
            }
            Self::Benchmark {
                sovereign,
                tenor,
                id,
                ..
            } => {
                format!("{} {} ({})", sovereign.bond_name(), tenor, id)
            }
            Self::SpecificSecurity { sovereign, id, .. } => {
                format!("{} {}", sovereign.bond_name(), id)
            }
            Self::Explicit => "Explicit yield".to_string(),
        }
    }

    /// Returns the sovereign if available.
    #[must_use]
    pub fn sovereign(&self) -> Option<Sovereign> {
        match self {
            Self::Interpolated { sovereign, .. }
            | Self::Benchmark { sovereign, .. }
            | Self::SpecificSecurity { sovereign, .. } => Some(*sovereign),
            Self::Explicit => None,
        }
    }
}

/// Result of G-spread calculation with full context.
#[derive(Debug, Clone)]
pub struct GSpreadResult {
    /// The calculated spread.
    pub spread: Spread,
    /// Bond yield used.
    pub bond_yield: Yield,
    /// Benchmark yield used.
    pub benchmark_yield: Yield,
    /// Information about the benchmark.
    pub benchmark_info: BenchmarkInfo,
    /// Years to maturity of the bond.
    pub years_to_maturity: f64,
}

/// Government spread calculator.
///
/// Calculates spread over any sovereign benchmark curve (Treasuries, Gilts,
/// Bunds, OATs, JGBs, etc.)
///
/// # Example
///
/// ```rust,ignore
/// let calc = GSpreadCalculator::new(&treasury_curve);
///
/// // Interpolated spread (most common)
/// let result = calc.calculate(bond_yield, maturity, settlement, BenchmarkSpec::interpolated())?;
///
/// // Spread to 10-year benchmark
/// let result = calc.calculate(bond_yield, maturity, settlement, BenchmarkSpec::ten_year())?;
///
/// // Spread to specific CUSIP
/// let result = calc.calculate(bond_yield, maturity, settlement, BenchmarkSpec::cusip("91282CJN6")?)?;
/// ```
#[derive(Debug, Clone)]
pub struct GSpreadCalculator<'a> {
    /// Reference to the government yield curve.
    government_curve: &'a GovernmentCurve,
}

impl<'a> GSpreadCalculator<'a> {
    /// Creates a new G-spread calculator.
    ///
    /// # Arguments
    ///
    /// * `government_curve` - Government bond yield curve (Treasury, Gilt, etc.)
    #[must_use]
    pub fn new(government_curve: &'a GovernmentCurve) -> Self {
        Self { government_curve }
    }

    /// Returns the sovereign for this calculator.
    #[must_use]
    pub fn sovereign(&self) -> Sovereign {
        self.government_curve.sovereign()
    }

    /// Returns the reference date from the curve.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.government_curve.reference_date()
    }

    /// Calculates G-spread with specified benchmark.
    ///
    /// # Arguments
    ///
    /// * `bond_yield` - The bond's yield-to-maturity
    /// * `maturity` - Bond maturity date
    /// * `settlement` - Settlement date
    /// * `benchmark` - How to determine the benchmark yield
    ///
    /// # Returns
    ///
    /// `GSpreadResult` containing the spread and benchmark details.
    ///
    /// # Errors
    ///
    /// Returns `SpreadError` if:
    /// - Settlement is after maturity
    /// - Specified benchmark is not found
    /// - Curve has no benchmarks (for tenor-based lookups)
    pub fn calculate(
        &self,
        bond_yield: Yield,
        maturity: Date,
        settlement: Date,
        benchmark: BenchmarkSpec,
    ) -> SpreadResult<GSpreadResult> {
        if settlement >= maturity {
            return Err(SpreadError::SettlementAfterMaturity {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        let years_to_maturity = settlement.days_between(&maturity) as f64 / 365.0;

        let (benchmark_yield, benchmark_info) = match benchmark {
            BenchmarkSpec::Interpolated => {
                let y = self.government_curve.interpolated_yield(years_to_maturity);
                let info = BenchmarkInfo::Interpolated {
                    sovereign: self.government_curve.sovereign(),
                    years: years_to_maturity,
                };
                (y, info)
            }

            BenchmarkSpec::OnTheRunTenor(tenor) => {
                let b = self.government_curve.benchmark(tenor).ok_or_else(|| {
                    SpreadError::BenchmarkNotFound {
                        description: format!("{} {}", self.sovereign(), tenor),
                    }
                })?;
                let info = BenchmarkInfo::Benchmark {
                    sovereign: b.sovereign,
                    tenor,
                    id: b.id.clone(),
                    maturity: b.maturity,
                };
                (b.yield_rate, info)
            }

            BenchmarkSpec::NearestOnTheRun => {
                let b = self
                    .government_curve
                    .nearest_benchmark(years_to_maturity)
                    .ok_or(SpreadError::NoBenchmarksAvailable)?;
                let info = BenchmarkInfo::Benchmark {
                    sovereign: b.sovereign,
                    tenor: b.tenor,
                    id: b.id.clone(),
                    maturity: b.maturity,
                };
                (b.yield_rate, info)
            }

            BenchmarkSpec::SpecificSecurity(ref id) => {
                let b = self.government_curve.security_by_id(id).ok_or_else(|| {
                    SpreadError::BenchmarkNotFound {
                        description: id.to_string(),
                    }
                })?;
                let info = BenchmarkInfo::SpecificSecurity {
                    sovereign: b.sovereign,
                    id: id.clone(),
                    tenor: b.tenor,
                    maturity: b.maturity,
                };
                (b.yield_rate, info)
            }

            BenchmarkSpec::ExplicitYield(y) => {
                let info = BenchmarkInfo::Explicit;
                (y, info)
            }
        };

        let spread_decimal = bond_yield.value() - benchmark_yield.value();
        let spread_bps = (spread_decimal * Decimal::from(10_000)).round();
        let spread = Spread::new(spread_bps, SpreadType::GSpread);

        Ok(GSpreadResult {
            spread,
            bond_yield,
            benchmark_yield,
            benchmark_info,
            years_to_maturity,
        })
    }

    /// Calculates G-spread from bond price.
    ///
    /// This method first calculates the bond's YTM from the price,
    /// then computes the G-spread.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond to calculate spread for
    /// * `clean_price` - Market clean price
    /// * `settlement` - Settlement date
    /// * `benchmark` - How to determine the benchmark yield
    pub fn from_price(
        &self,
        bond: &FixedBond,
        clean_price: Price,
        settlement: Date,
        benchmark: BenchmarkSpec,
    ) -> SpreadResult<GSpreadResult> {
        let maturity = bond.maturity();

        if settlement >= maturity {
            return Err(SpreadError::SettlementAfterMaturity {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        // Calculate YTM from price
        let bond_yield_decimal = BondPricer::yield_to_maturity(bond, clean_price, settlement)
            .map_err(|e| SpreadError::bond_error(e.to_string()))?;

        let bond_yield = Yield::new(bond_yield_decimal, Compounding::SemiAnnual);
        self.calculate(bond_yield, maturity, settlement, benchmark)
    }

    /// Convenience: Calculate interpolated G-spread (most common).
    ///
    /// # Arguments
    ///
    /// * `bond_yield` - The bond's yield-to-maturity
    /// * `maturity` - Bond maturity date
    /// * `settlement` - Settlement date
    #[inline]
    pub fn interpolated(
        &self,
        bond_yield: Yield,
        maturity: Date,
        settlement: Date,
    ) -> SpreadResult<Spread> {
        Ok(self
            .calculate(
                bond_yield,
                maturity,
                settlement,
                BenchmarkSpec::Interpolated,
            )?
            .spread)
    }

    /// Convenience: Calculate spread to specific tenor.
    ///
    /// # Arguments
    ///
    /// * `bond_yield` - The bond's yield-to-maturity
    /// * `benchmark_tenor` - The benchmark tenor (e.g., Y10 for 10-year)
    #[inline]
    pub fn to_tenor(&self, bond_yield: Yield, benchmark_tenor: Tenor) -> SpreadResult<Spread> {
        let benchmark_yield = self
            .government_curve
            .benchmark_yield(benchmark_tenor)
            .ok_or_else(|| SpreadError::BenchmarkNotFound {
                description: format!("{benchmark_tenor}"),
            })?;

        let spread_decimal = bond_yield.value() - benchmark_yield.value();
        let spread_bps = (spread_decimal * Decimal::from(10_000)).round();
        Ok(Spread::new(spread_bps, SpreadType::GSpread))
    }

    /// Static: Calculate spread given explicit benchmark yield (no curve needed).
    #[inline]
    #[must_use]
    pub fn spread_to_yield(bond_yield: Yield, benchmark_yield: Yield) -> Spread {
        let spread_decimal = bond_yield.value() - benchmark_yield.value();
        let spread_bps = (spread_decimal * Decimal::from(10_000)).round();
        Spread::new(spread_bps, SpreadType::GSpread)
    }
}

// =============================================================================
// Legacy API (for backwards compatibility)
// =============================================================================

/// Treasury benchmark tenors for spread calculations.
///
/// These represent the standard on-the-run Treasury securities
/// used as benchmarks in the US government bond market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TreasuryBenchmark {
    /// 2-year Treasury note
    TwoYear,
    /// 3-year Treasury note
    ThreeYear,
    /// 5-year Treasury note
    FiveYear,
    /// 7-year Treasury note
    SevenYear,
    /// 10-year Treasury note
    TenYear,
    /// 20-year Treasury bond
    TwentyYear,
    /// 30-year Treasury bond
    ThirtyYear,
    /// Interpolated to exact maturity
    Interpolated,
}

impl TreasuryBenchmark {
    /// Returns the tenor in years for this benchmark.
    #[must_use]
    pub fn years(&self) -> f64 {
        match self {
            TreasuryBenchmark::TwoYear => 2.0,
            TreasuryBenchmark::ThreeYear => 3.0,
            TreasuryBenchmark::FiveYear => 5.0,
            TreasuryBenchmark::SevenYear => 7.0,
            TreasuryBenchmark::TenYear => 10.0,
            TreasuryBenchmark::TwentyYear => 20.0,
            TreasuryBenchmark::ThirtyYear => 30.0,
            TreasuryBenchmark::Interpolated => 0.0,
        }
    }

    /// Returns the Bloomberg ticker for this benchmark.
    #[must_use]
    pub fn bloomberg_ticker(&self) -> &'static str {
        match self {
            TreasuryBenchmark::TwoYear => "GT2 Govt",
            TreasuryBenchmark::ThreeYear => "GT3 Govt",
            TreasuryBenchmark::FiveYear => "GT5 Govt",
            TreasuryBenchmark::SevenYear => "GT7 Govt",
            TreasuryBenchmark::TenYear => "GT10 Govt",
            TreasuryBenchmark::TwentyYear => "GT20 Govt",
            TreasuryBenchmark::ThirtyYear => "GT30 Govt",
            TreasuryBenchmark::Interpolated => "Interpolated",
        }
    }

    /// Returns the closest benchmark for a given number of years.
    #[must_use]
    pub fn closest_for_years(years: f64) -> Self {
        if years <= 2.5 {
            TreasuryBenchmark::TwoYear
        } else if years <= 4.0 {
            TreasuryBenchmark::ThreeYear
        } else if years <= 6.0 {
            TreasuryBenchmark::FiveYear
        } else if years <= 8.5 {
            TreasuryBenchmark::SevenYear
        } else if years <= 15.0 {
            TreasuryBenchmark::TenYear
        } else if years <= 25.0 {
            TreasuryBenchmark::TwentyYear
        } else {
            TreasuryBenchmark::ThirtyYear
        }
    }
}

impl std::fmt::Display for TreasuryBenchmark {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            TreasuryBenchmark::TwoYear => "2Y Treasury",
            TreasuryBenchmark::ThreeYear => "3Y Treasury",
            TreasuryBenchmark::FiveYear => "5Y Treasury",
            TreasuryBenchmark::SevenYear => "7Y Treasury",
            TreasuryBenchmark::TenYear => "10Y Treasury",
            TreasuryBenchmark::TwentyYear => "20Y Treasury",
            TreasuryBenchmark::ThirtyYear => "30Y Treasury",
            TreasuryBenchmark::Interpolated => "Interpolated",
        };
        write!(f, "{name}")
    }
}

// =============================================================================
// Backwards-compatible free function API
// =============================================================================

/// Calculates G-spread for a bond using a government zero curve.
///
/// This is a backwards-compatible function that calculates the spread
/// between a bond's yield and an interpolated government yield.
///
/// # Arguments
///
/// * `bond` - The bond to calculate spread for
/// * `govt_curve` - Government zero/yield curve
/// * `bond_yield` - The bond's yield to maturity (as decimal)
/// * `settlement` - Settlement date
///
/// # Errors
///
/// Returns `SpreadError` if the settlement date is after maturity or curve interpolation fails.
pub fn calculate(
    bond: &FixedBond,
    govt_curve: &ZeroCurve,
    bond_yield: Decimal,
    settlement: Date,
) -> SpreadResult<Spread> {
    let maturity = bond.maturity();

    if settlement >= maturity {
        return Err(SpreadError::SettlementAfterMaturity {
            settlement: settlement.to_string(),
            maturity: maturity.to_string(),
        });
    }

    // Get interpolated government yield from the curve at maturity date
    let govt_yield = govt_curve
        .zero_rate_at(maturity)
        .map_err(|e| SpreadError::curve_error(e.to_string()))?;

    // Calculate spread: bond yield - govt yield
    let spread_decimal = bond_yield - govt_yield;
    let spread_bps = (spread_decimal * Decimal::from(10_000)).round();

    Ok(Spread::new(spread_bps, SpreadType::GSpread))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::government_curve::GovernmentBenchmark;
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_ust_curve() -> GovernmentCurve {
        GovernmentCurve::us_treasury(date(2024, 1, 15))
            .with_benchmark(GovernmentBenchmark::with_cusip_unchecked(
                Sovereign::UST,
                Tenor::Y2,
                "91282CJN6",
                date(2026, 1, 15),
                dec!(0.04625),
                Yield::new(dec!(0.0435), Compounding::SemiAnnual),
            ))
            .with_benchmark(GovernmentBenchmark::with_cusip_unchecked(
                Sovereign::UST,
                Tenor::Y5,
                "91282CJP1",
                date(2029, 1, 15),
                dec!(0.04),
                Yield::new(dec!(0.0420), Compounding::SemiAnnual),
            ))
            .with_benchmark(GovernmentBenchmark::with_cusip_unchecked(
                Sovereign::UST,
                Tenor::Y10,
                "91282CJQ9",
                date(2034, 1, 15),
                dec!(0.04),
                Yield::new(dec!(0.0425), Compounding::SemiAnnual),
            ))
            .with_benchmark(GovernmentBenchmark::with_cusip_unchecked(
                Sovereign::UST,
                Tenor::Y30,
                "91282CJR7",
                date(2054, 1, 15),
                dec!(0.0425),
                Yield::new(dec!(0.0445), Compounding::SemiAnnual),
            ))
    }

    #[test]
    fn test_gspread_interpolated() {
        let curve = create_ust_curve();
        let calc = GSpreadCalculator::new(&curve);

        let bond_yield = Yield::new(dec!(0.055), Compounding::SemiAnnual);
        let maturity = date(2031, 6, 15);
        let settlement = date(2024, 1, 17);

        let result = calc
            .calculate(
                bond_yield,
                maturity,
                settlement,
                BenchmarkSpec::interpolated(),
            )
            .unwrap();

        // ~5.5% bond vs ~4.2% Treasury = ~130 bps spread
        let spread_bps = result.spread.as_bps().to_f64().unwrap();
        assert!(
            spread_bps > 100.0 && spread_bps < 150.0,
            "Expected ~130 bps, got {} bps",
            spread_bps
        );
        assert!(matches!(
            result.benchmark_info,
            BenchmarkInfo::Interpolated { .. }
        ));
    }

    #[test]
    fn test_gspread_to_tenor() {
        let curve = create_ust_curve();
        let calc = GSpreadCalculator::new(&curve);

        let bond_yield = Yield::new(dec!(0.055), Compounding::SemiAnnual);

        // Spread to 10Y
        let result = calc
            .calculate(
                bond_yield,
                date(2031, 6, 15),
                date(2024, 1, 17),
                BenchmarkSpec::ten_year(),
            )
            .unwrap();

        // 5.5% - 4.25% = 125 bps
        let spread_bps = result.spread.as_bps().to_f64().unwrap();
        assert!(
            (spread_bps - 125.0).abs() < 1.0,
            "Expected 125 bps, got {} bps",
            spread_bps
        );
    }

    #[test]
    fn test_gspread_to_specific_cusip() {
        let curve = create_ust_curve();
        let calc = GSpreadCalculator::new(&curve);

        let bond_yield = Yield::new(dec!(0.055), Compounding::SemiAnnual);
        let benchmark = BenchmarkSpec::security(SecurityId::cusip_unchecked("91282CJQ9"));

        let result = calc
            .calculate(bond_yield, date(2031, 6, 15), date(2024, 1, 17), benchmark)
            .unwrap();

        assert!(matches!(
            result.benchmark_info,
            BenchmarkInfo::SpecificSecurity { .. }
        ));
    }

    #[test]
    fn test_gspread_nearest_benchmark() {
        let curve = create_ust_curve();
        let calc = GSpreadCalculator::new(&curve);

        let bond_yield = Yield::new(dec!(0.055), Compounding::SemiAnnual);
        let maturity = date(2029, 6, 15); // ~5.5 years
        let settlement = date(2024, 1, 17);

        let result = calc
            .calculate(bond_yield, maturity, settlement, BenchmarkSpec::nearest())
            .unwrap();

        // Should use 5Y benchmark
        if let BenchmarkInfo::Benchmark { tenor, .. } = result.benchmark_info {
            assert_eq!(tenor, Tenor::Y5);
        } else {
            panic!("Expected Benchmark info");
        }
    }

    #[test]
    fn test_gspread_explicit_yield() {
        let curve = create_ust_curve();
        let calc = GSpreadCalculator::new(&curve);

        let bond_yield = Yield::new(dec!(0.055), Compounding::SemiAnnual);
        let benchmark_yield = Yield::new(dec!(0.0400), Compounding::SemiAnnual);
        let benchmark = BenchmarkSpec::explicit(benchmark_yield);

        let result = calc
            .calculate(bond_yield, date(2031, 6, 15), date(2024, 1, 17), benchmark)
            .unwrap();

        // 5.5% - 4.0% = 150 bps
        let spread_bps = result.spread.as_bps().to_f64().unwrap();
        assert!(
            (spread_bps - 150.0).abs() < 1.0,
            "Expected 150 bps, got {} bps",
            spread_bps
        );
        assert!(matches!(result.benchmark_info, BenchmarkInfo::Explicit));
    }

    #[test]
    fn test_gspread_spread_to_yield() {
        let bond_yield = Yield::new(dec!(0.055), Compounding::SemiAnnual);
        let benchmark_yield = Yield::new(dec!(0.0425), Compounding::SemiAnnual);

        let spread = GSpreadCalculator::spread_to_yield(bond_yield, benchmark_yield);

        let spread_bps = spread.as_bps().to_f64().unwrap();
        assert!(
            (spread_bps - 125.0).abs() < 1.0,
            "Expected 125 bps, got {} bps",
            spread_bps
        );
    }

    #[test]
    fn test_gspread_settlement_after_maturity() {
        let curve = create_ust_curve();
        let calc = GSpreadCalculator::new(&curve);

        let bond_yield = Yield::new(dec!(0.055), Compounding::SemiAnnual);
        let maturity = date(2024, 1, 15);
        let settlement = date(2024, 6, 15); // After maturity

        let result = calc.calculate(
            bond_yield,
            maturity,
            settlement,
            BenchmarkSpec::interpolated(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_gspread_benchmark_not_found() {
        let curve = GovernmentCurve::us_treasury(date(2024, 1, 15)).with_benchmark(
            GovernmentBenchmark::with_cusip_unchecked(
                Sovereign::UST,
                Tenor::Y10,
                "91282CJQ9",
                date(2034, 1, 15),
                dec!(0.04),
                Yield::new(dec!(0.0425), Compounding::SemiAnnual),
            ),
        );

        let calc = GSpreadCalculator::new(&curve);
        let bond_yield = Yield::new(dec!(0.055), Compounding::SemiAnnual);

        // 2Y not available
        let result = calc.calculate(
            bond_yield,
            date(2031, 6, 15),
            date(2024, 1, 17),
            BenchmarkSpec::two_year(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_different_sovereign_curves() {
        // UK Gilt curve
        let gilt_curve = GovernmentCurve::uk_gilt(date(2024, 1, 15)).with_benchmark(
            GovernmentBenchmark::with_isin_unchecked(
                Sovereign::UK,
                Tenor::Y10,
                "GB00BM8Z2V59",
                date(2034, 1, 31),
                dec!(0.0325),
                Yield::new(dec!(0.0410), Compounding::SemiAnnual),
            ),
        );

        let calc = GSpreadCalculator::new(&gilt_curve);
        assert_eq!(calc.sovereign(), Sovereign::UK);

        let gbp_corp_yield = Yield::new(dec!(0.055), Compounding::SemiAnnual);
        let result = calc
            .calculate(
                gbp_corp_yield,
                date(2032, 3, 15),
                date(2024, 1, 17),
                BenchmarkSpec::ten_year(),
            )
            .unwrap();

        // 5.5% - 4.1% = 140 bps
        let spread_bps = result.spread.as_bps().to_f64().unwrap();
        assert!(
            (spread_bps - 140.0).abs() < 1.0,
            "Expected 140 bps, got {} bps",
            spread_bps
        );
    }

    #[test]
    fn test_benchmark_info_description() {
        let info = BenchmarkInfo::Interpolated {
            sovereign: Sovereign::UST,
            years: 7.5,
        };
        assert!(info.description().contains("Treasury"));
        assert!(info.description().contains("7.50"));

        let info = BenchmarkInfo::Explicit;
        assert_eq!(info.description(), "Explicit yield");
    }

    // Legacy API tests
    #[test]
    fn test_treasury_benchmark_years() {
        assert!((TreasuryBenchmark::TwoYear.years() - 2.0).abs() < 0.001);
        assert!((TreasuryBenchmark::TenYear.years() - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_treasury_benchmark_closest() {
        assert_eq!(
            TreasuryBenchmark::closest_for_years(1.5),
            TreasuryBenchmark::TwoYear
        );
        assert_eq!(
            TreasuryBenchmark::closest_for_years(9.0),
            TreasuryBenchmark::TenYear
        );
    }
}
