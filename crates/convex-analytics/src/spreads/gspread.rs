//! G-spread (government spread) calculator.
//!
//! The G-spread is the yield spread of a bond over the interpolated
//! government benchmark yield at the same maturity.

use rust_decimal::Decimal;

use convex_bonds::traits::{Bond, FixedCouponBond};
use convex_core::types::{Date, Spread, SpreadType, Yield};

use crate::error::{AnalyticsError, AnalyticsResult};

use super::benchmark::BenchmarkSpec;
use super::government_curve::GovernmentCurve;

/// G-spread calculator for fixed rate bonds.
///
/// Calculates the spread over a government benchmark yield.
#[derive(Debug)]
pub struct GSpreadCalculator<'a> {
    /// Reference to the government curve.
    gov_curve: &'a GovernmentCurve,
    /// Benchmark specification.
    benchmark_spec: BenchmarkSpec,
}

impl<'a> GSpreadCalculator<'a> {
    /// Creates a new G-spread calculator with interpolated benchmark.
    ///
    /// # Arguments
    ///
    /// * `gov_curve` - Government yield curve with benchmarks
    #[must_use]
    pub fn new(gov_curve: &'a GovernmentCurve) -> Self {
        Self {
            gov_curve,
            benchmark_spec: BenchmarkSpec::Interpolated,
        }
    }

    /// Sets the benchmark specification.
    #[must_use]
    pub fn with_benchmark(mut self, spec: BenchmarkSpec) -> Self {
        self.benchmark_spec = spec;
        self
    }

    /// Calculates the G-spread for a bond.
    ///
    /// # Arguments
    ///
    /// * `bond` - The fixed rate bond
    /// * `bond_yield` - The bond's yield to maturity
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The G-spread as a `Spread` in basis points.
    ///
    /// # Errors
    ///
    /// Returns `AnalyticsError` if:
    /// - Settlement is at or after maturity
    /// - Benchmark yield cannot be determined
    pub fn calculate<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        bond_yield: Yield,
        settlement: Date,
    ) -> AnalyticsResult<Spread> {
        let maturity = bond.maturity().ok_or_else(|| {
            AnalyticsError::InvalidInput("Bond has no maturity (perpetual)".to_string())
        })?;

        if settlement >= maturity {
            return Err(AnalyticsError::InvalidSettlement {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        // Get benchmark yield based on specification
        let benchmark_yield = self.get_benchmark_yield(settlement, maturity)?;

        // G-spread = Bond yield - Benchmark yield
        let bond_yield_val = bond_yield.value();
        let benchmark_yield_val = benchmark_yield.value();

        let spread = bond_yield_val - benchmark_yield_val;
        let spread_bps = (spread * Decimal::from(10_000)).round();

        Ok(Spread::new(spread_bps, SpreadType::GSpread))
    }

    /// Gets the benchmark yield based on the specification.
    fn get_benchmark_yield(&self, settlement: Date, maturity: Date) -> AnalyticsResult<Yield> {
        let years_to_maturity = settlement.days_between(&maturity) as f64 / 365.0;

        match &self.benchmark_spec {
            BenchmarkSpec::Interpolated => Ok(self.gov_curve.interpolated_yield(years_to_maturity)),
            BenchmarkSpec::OnTheRunTenor(tenor) => {
                self.gov_curve.benchmark_yield(*tenor).ok_or_else(|| {
                    AnalyticsError::BenchmarkNotFound(format!("No benchmark for tenor {:?}", tenor))
                })
            }
            BenchmarkSpec::NearestOnTheRun => self
                .gov_curve
                .nearest_benchmark(years_to_maturity)
                .map(|b| b.yield_rate)
                .ok_or_else(|| {
                    AnalyticsError::BenchmarkNotFound("No benchmarks available".to_string())
                }),
            BenchmarkSpec::SpecificSecurity(id) => self
                .gov_curve
                .security_by_id(id)
                .map(|b| b.yield_rate)
                .ok_or_else(|| {
                    AnalyticsError::BenchmarkNotFound(format!("Security {} not found", id))
                }),
            BenchmarkSpec::ExplicitYield(y) => Ok(*y),
        }
    }

    /// Returns the benchmark yield that would be used.
    ///
    /// Useful for debugging or displaying the benchmark.
    pub fn benchmark_yield(&self, settlement: Date, maturity: Date) -> AnalyticsResult<Yield> {
        self.get_benchmark_yield(settlement, maturity)
    }
}

/// Convenience function to calculate G-spread with interpolated benchmark.
///
/// # Arguments
///
/// * `bond` - The fixed rate bond
/// * `bond_yield` - Bond's yield to maturity
/// * `gov_curve` - Government yield curve
/// * `settlement` - Settlement date
///
/// # Returns
///
/// G-spread in basis points.
pub fn g_spread<B: Bond + FixedCouponBond>(
    bond: &B,
    bond_yield: Yield,
    gov_curve: &GovernmentCurve,
    settlement: Date,
) -> AnalyticsResult<Spread> {
    GSpreadCalculator::new(gov_curve).calculate(bond, bond_yield, settlement)
}

/// Calculate G-spread with explicit benchmark specification.
pub fn g_spread_with_benchmark<B: Bond + FixedCouponBond>(
    bond: &B,
    bond_yield: Yield,
    gov_curve: &GovernmentCurve,
    settlement: Date,
    benchmark_spec: BenchmarkSpec,
) -> AnalyticsResult<Spread> {
    GSpreadCalculator::new(gov_curve)
        .with_benchmark(benchmark_spec)
        .calculate(bond, bond_yield, settlement)
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_bonds::types::Tenor;
    use convex_core::types::Compounding;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_test_curve() -> GovernmentCurve {
        use super::super::benchmark::SecurityId;
        use super::super::government_curve::GovernmentBenchmark;
        use super::super::sovereign::Sovereign;

        let y2 = GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            Tenor::Y2,
            "912828XX0",
            date(2026, 1, 15),
            dec!(0.04),
            Yield::new(dec!(0.0435), Compounding::SemiAnnual),
        );

        let y10 = GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            Tenor::Y10,
            "912828YY0",
            date(2034, 1, 15),
            dec!(0.04),
            Yield::new(dec!(0.0425), Compounding::SemiAnnual),
        );

        GovernmentCurve::us_treasury(date(2024, 1, 15))
            .with_benchmark(y2)
            .with_benchmark(y10)
    }

    // Mock bond for testing
    struct MockBond {
        maturity: Date,
        calendar: convex_bonds::types::CalendarId,
    }

    impl MockBond {
        fn new(maturity: Date) -> Self {
            Self {
                maturity,
                calendar: convex_bonds::types::CalendarId::us_government(),
            }
        }
    }

    impl Bond for MockBond {
        fn identifiers(&self) -> &convex_bonds::types::BondIdentifiers {
            unimplemented!("Not needed for test")
        }

        fn bond_type(&self) -> convex_bonds::types::BondType {
            convex_bonds::types::BondType::FixedRateCorporate
        }

        fn currency(&self) -> convex_core::Currency {
            convex_core::Currency::USD
        }

        fn maturity(&self) -> Option<Date> {
            Some(self.maturity)
        }

        fn issue_date(&self) -> Date {
            date(2020, 1, 15)
        }

        fn first_settlement_date(&self) -> Date {
            date(2020, 1, 15)
        }

        fn dated_date(&self) -> Date {
            date(2020, 1, 15)
        }

        fn face_value(&self) -> Decimal {
            dec!(100)
        }

        fn frequency(&self) -> convex_core::types::Frequency {
            convex_core::types::Frequency::SemiAnnual
        }

        fn cash_flows(&self, _from: Date) -> Vec<convex_bonds::traits::BondCashFlow> {
            Vec::new()
        }

        fn next_coupon_date(&self, _after: Date) -> Option<Date> {
            None
        }

        fn previous_coupon_date(&self, _before: Date) -> Option<Date> {
            None
        }

        fn accrued_interest(&self, _settlement: Date) -> Decimal {
            dec!(0)
        }

        fn day_count_convention(&self) -> &'static str {
            "ACT/ACT"
        }

        fn calendar(&self) -> &convex_bonds::types::CalendarId {
            &self.calendar
        }
    }

    impl FixedCouponBond for MockBond {
        fn coupon_rate(&self) -> Decimal {
            dec!(0.05)
        }

        fn coupon_frequency(&self) -> u32 {
            2
        }

        fn first_coupon_date(&self) -> Option<Date> {
            None
        }

        fn last_coupon_date(&self) -> Option<Date> {
            None
        }
    }

    #[test]
    fn test_g_spread_interpolated() {
        let gov_curve = create_test_curve();
        let calc = GSpreadCalculator::new(&gov_curve);

        let bond = MockBond::new(date(2030, 1, 15));
        let settlement = date(2024, 1, 17);

        // Bond yield = 5%, benchmark ≈ 4.30%
        let bond_yield = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        let spread = calc.calculate(&bond, bond_yield, settlement).unwrap();

        // Spread should be positive (bond yields more than treasury)
        assert!(spread.as_bps() > Decimal::ZERO);
        // Should be around 70 bps (5% - 4.3%)
        assert!(spread.as_bps() > dec!(50) && spread.as_bps() < dec!(100));
    }

    #[test]
    fn test_g_spread_explicit_benchmark() {
        let gov_curve = create_test_curve();
        let calc = GSpreadCalculator::new(&gov_curve).with_benchmark(BenchmarkSpec::ten_year());

        let bond = MockBond::new(date(2030, 1, 15));
        let settlement = date(2024, 1, 17);

        // Bond yield = 5%, 10Y benchmark = 4.25%
        let bond_yield = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        let spread = calc.calculate(&bond, bond_yield, settlement).unwrap();

        // Spread should be 75 bps (5% - 4.25%)
        let diff = (spread.as_bps() - dec!(75)).abs();
        assert!(diff < dec!(1), "Expected ~75 bps, got {}", spread.as_bps());
    }

    #[test]
    fn test_g_spread_explicit_yield() {
        let gov_curve = create_test_curve();

        let explicit_benchmark = Yield::new(dec!(0.04), Compounding::SemiAnnual);
        let calc = GSpreadCalculator::new(&gov_curve)
            .with_benchmark(BenchmarkSpec::explicit(explicit_benchmark));

        let bond = MockBond::new(date(2030, 1, 15));
        let settlement = date(2024, 1, 17);

        // Bond yield = 5%, explicit benchmark = 4%
        let bond_yield = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        let spread = calc.calculate(&bond, bond_yield, settlement).unwrap();

        // Spread should be 100 bps (5% - 4%)
        let diff = (spread.as_bps() - dec!(100)).abs();
        assert!(diff < dec!(1), "Expected ~100 bps, got {}", spread.as_bps());
    }

    #[test]
    fn test_g_spread_nearest_benchmark() {
        let gov_curve = create_test_curve();
        let calc = GSpreadCalculator::new(&gov_curve).with_benchmark(BenchmarkSpec::nearest());

        let bond = MockBond::new(date(2030, 1, 15)); // ~6 years
        let settlement = date(2024, 1, 17);

        // Should use 10Y (closer to 6Y than 2Y)
        let bond_yield = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        let spread = calc.calculate(&bond, bond_yield, settlement).unwrap();

        // 10Y benchmark = 4.25%, so spread ≈ 75 bps
        assert!(spread.as_bps() > dec!(50));
    }

    #[test]
    fn test_settlement_after_maturity() {
        let gov_curve = create_test_curve();
        let calc = GSpreadCalculator::new(&gov_curve);

        let bond = MockBond::new(date(2024, 1, 15)); // Already matured
        let settlement = date(2024, 6, 15);
        let bond_yield = Yield::new(dec!(0.05), Compounding::SemiAnnual);

        let result = calc.calculate(&bond, bond_yield, settlement);
        assert!(result.is_err());
    }

    #[test]
    fn test_benchmark_not_found() {
        let gov_curve = create_test_curve();
        let calc = GSpreadCalculator::new(&gov_curve).with_benchmark(BenchmarkSpec::five_year()); // Not in curve

        let bond = MockBond::new(date(2030, 1, 15));
        let settlement = date(2024, 1, 17);
        let bond_yield = Yield::new(dec!(0.05), Compounding::SemiAnnual);

        let result = calc.calculate(&bond, bond_yield, settlement);
        assert!(result.is_err());
    }
}
