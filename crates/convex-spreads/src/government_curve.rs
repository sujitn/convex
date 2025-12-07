//! Government bond curves with benchmark support.
//!
//! This module provides government yield curves (Treasury, Gilt, Bund, etc.)
//! with support for on-the-run benchmarks and curve interpolation.

use std::collections::BTreeMap;

use convex_bonds::error::IdentifierError;
use convex_bonds::types::Tenor;
use convex_core::types::{Date, Yield};
use convex_math::interpolation::{Interpolator, LinearInterpolator};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::benchmark::SecurityId;
use crate::sovereign::Sovereign;

/// A government bond benchmark (on-the-run or designated benchmark).
///
/// Represents a specific government security used as a benchmark for
/// spread calculations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernmentBenchmark {
    /// The sovereign issuer
    pub sovereign: Sovereign,
    /// Tenor (2Y, 5Y, 10Y, etc.)
    pub tenor: Tenor,
    /// Security identifier (CUSIP, ISIN, or FIGI)
    pub id: SecurityId,
    /// Maturity date
    pub maturity: Date,
    /// Coupon rate (decimal)
    pub coupon: Decimal,
    /// Yield to maturity (decimal)
    pub yield_rate: Yield,
    /// Clean price (optional)
    pub price: Option<Decimal>,
    /// Whether this is the current on-the-run benchmark
    pub is_on_the_run: bool,
}

impl GovernmentBenchmark {
    /// Creates a new government benchmark.
    #[must_use]
    pub fn new(
        sovereign: Sovereign,
        tenor: Tenor,
        id: SecurityId,
        maturity: Date,
        coupon: Decimal,
        yield_rate: Yield,
    ) -> Self {
        Self {
            sovereign,
            tenor,
            id,
            maturity,
            coupon,
            yield_rate,
            price: None,
            is_on_the_run: true,
        }
    }

    /// Creates a benchmark with CUSIP (typically US).
    ///
    /// # Errors
    ///
    /// Returns `IdentifierError` if the CUSIP is invalid.
    pub fn with_cusip(
        sovereign: Sovereign,
        tenor: Tenor,
        cusip: &str,
        maturity: Date,
        coupon: Decimal,
        yield_rate: Yield,
    ) -> Result<Self, IdentifierError> {
        Ok(Self {
            sovereign,
            tenor,
            id: SecurityId::cusip(cusip)?,
            maturity,
            coupon,
            yield_rate,
            price: None,
            is_on_the_run: true,
        })
    }

    /// Creates a benchmark with ISIN (international standard).
    ///
    /// # Errors
    ///
    /// Returns `IdentifierError` if the ISIN is invalid.
    pub fn with_isin(
        sovereign: Sovereign,
        tenor: Tenor,
        isin: &str,
        maturity: Date,
        coupon: Decimal,
        yield_rate: Yield,
    ) -> Result<Self, IdentifierError> {
        Ok(Self {
            sovereign,
            tenor,
            id: SecurityId::isin(isin)?,
            maturity,
            coupon,
            yield_rate,
            price: None,
            is_on_the_run: true,
        })
    }

    /// Creates a benchmark with CUSIP without validation.
    #[must_use]
    pub fn with_cusip_unchecked(
        sovereign: Sovereign,
        tenor: Tenor,
        cusip: &str,
        maturity: Date,
        coupon: Decimal,
        yield_rate: Yield,
    ) -> Self {
        Self {
            sovereign,
            tenor,
            id: SecurityId::cusip_unchecked(cusip),
            maturity,
            coupon,
            yield_rate,
            price: None,
            is_on_the_run: true,
        }
    }

    /// Creates a benchmark with ISIN without validation.
    #[must_use]
    pub fn with_isin_unchecked(
        sovereign: Sovereign,
        tenor: Tenor,
        isin: &str,
        maturity: Date,
        coupon: Decimal,
        yield_rate: Yield,
    ) -> Self {
        Self {
            sovereign,
            tenor,
            id: SecurityId::isin_unchecked(isin),
            maturity,
            coupon,
            yield_rate,
            price: None,
            is_on_the_run: true,
        }
    }

    /// Sets the clean price.
    #[must_use]
    pub fn with_price(mut self, price: Decimal) -> Self {
        self.price = Some(price);
        self
    }

    /// Marks this as off-the-run.
    #[must_use]
    pub fn off_the_run(mut self) -> Self {
        self.is_on_the_run = false;
        self
    }

    /// Returns the yield as f64.
    #[must_use]
    pub fn yield_f64(&self) -> f64 {
        self.yield_rate.value().to_string().parse().unwrap_or(0.0)
    }
}

/// Government/Sovereign bond curve with benchmark support.
///
/// Provides yield interpolation and benchmark lookup for government bonds.
///
/// # Example
///
/// ```rust,ignore
/// use convex_spreads::government_curve::GovernmentCurve;
/// use convex_spreads::sovereign::Sovereign;
///
/// let curve = GovernmentCurve::us_treasury(date!(2024-01-15))
///     .with_benchmark(benchmark_2y)
///     .with_benchmark(benchmark_10y)
///     .with_benchmark(benchmark_30y);
///
/// // Interpolate yield at 5 years
/// let yield_5y = curve.interpolated_yield(5.0);
/// ```
#[derive(Debug)]
pub struct GovernmentCurve {
    /// The sovereign issuer
    sovereign: Sovereign,
    /// Reference date for the curve
    reference_date: Date,
    /// On-the-run/benchmark yields by tenor (months -> benchmark)
    benchmarks: BTreeMap<u32, GovernmentBenchmark>,
    /// Full curve points for interpolation (years to maturity -> yield)
    curve_points: Vec<(f64, f64)>,
}

impl GovernmentCurve {
    /// Creates a new government curve.
    #[must_use]
    pub fn new(sovereign: Sovereign, reference_date: Date) -> Self {
        Self {
            sovereign,
            reference_date,
            benchmarks: BTreeMap::new(),
            curve_points: Vec::new(),
        }
    }

    /// Creates a US Treasury curve.
    #[must_use]
    pub fn us_treasury(reference_date: Date) -> Self {
        Self::new(Sovereign::UST, reference_date)
    }

    /// Creates a UK Gilt curve.
    #[must_use]
    pub fn uk_gilt(reference_date: Date) -> Self {
        Self::new(Sovereign::UK, reference_date)
    }

    /// Creates a German Bund curve.
    #[must_use]
    pub fn german_bund(reference_date: Date) -> Self {
        Self::new(Sovereign::Germany, reference_date)
    }

    /// Creates a French OAT curve.
    #[must_use]
    pub fn french_oat(reference_date: Date) -> Self {
        Self::new(Sovereign::France, reference_date)
    }

    /// Creates a Japanese JGB curve.
    #[must_use]
    pub fn japanese_jgb(reference_date: Date) -> Self {
        Self::new(Sovereign::Japan, reference_date)
    }

    /// Returns the sovereign for this curve.
    #[must_use]
    pub fn sovereign(&self) -> Sovereign {
        self.sovereign
    }

    /// Returns the reference date.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.reference_date
    }

    /// Adds a benchmark to the curve.
    #[must_use]
    pub fn with_benchmark(mut self, benchmark: GovernmentBenchmark) -> Self {
        let months = benchmark.tenor.months();
        let years = benchmark.tenor.years();
        let yield_f64 = benchmark.yield_f64();

        // Add to curve points
        self.curve_points.push((years, yield_f64));
        self.curve_points
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        // Add to benchmarks map
        self.benchmarks.insert(months, benchmark);
        self
    }

    /// Adds a curve point for interpolation.
    #[must_use]
    pub fn with_point(mut self, years: f64, yield_rate: f64) -> Self {
        self.curve_points.push((years, yield_rate));
        self.curve_points
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        self
    }

    /// Builds curve from a vector of benchmarks.
    #[must_use]
    pub fn from_benchmarks(
        sovereign: Sovereign,
        reference_date: Date,
        benchmarks: Vec<GovernmentBenchmark>,
    ) -> Self {
        let mut curve = Self::new(sovereign, reference_date);
        for b in benchmarks {
            curve = curve.with_benchmark(b);
        }
        curve
    }

    /// Interpolates yield at a specific maturity.
    ///
    /// Uses linear interpolation between curve points with flat
    /// extrapolation outside the range.
    #[must_use]
    pub fn interpolated_yield(&self, years_to_maturity: f64) -> Yield {
        if self.curve_points.is_empty() {
            return Yield::from_bps(0, convex_core::types::Compounding::SemiAnnual);
        }

        let y = if self.curve_points.len() == 1 {
            self.curve_points[0].1
        } else {
            // Create linear interpolator
            let xs: Vec<f64> = self.curve_points.iter().map(|(x, _)| *x).collect();
            let ys: Vec<f64> = self.curve_points.iter().map(|(_, y)| *y).collect();

            match LinearInterpolator::new(xs, ys) {
                Ok(interp) => interp
                    .with_extrapolation()
                    .interpolate(years_to_maturity)
                    .unwrap_or(0.0),
                Err(_) => 0.0,
            }
        };

        Yield::new(
            Decimal::from_f64_retain(y).unwrap_or_default(),
            convex_core::types::Compounding::SemiAnnual,
        )
    }

    /// Gets benchmark yield for a specific tenor.
    #[must_use]
    pub fn benchmark_yield(&self, tenor: Tenor) -> Option<Yield> {
        self.benchmarks.get(&tenor.months()).map(|b| b.yield_rate)
    }

    /// Gets benchmark for a specific tenor.
    #[must_use]
    pub fn benchmark(&self, tenor: Tenor) -> Option<&GovernmentBenchmark> {
        self.benchmarks.get(&tenor.months())
    }

    /// Gets nearest benchmark to a given maturity.
    #[must_use]
    pub fn nearest_benchmark(&self, years_to_maturity: f64) -> Option<&GovernmentBenchmark> {
        let target_months = (years_to_maturity * 12.0).round() as u32;

        self.benchmarks
            .iter()
            .min_by_key(|(months, _)| (**months as i32 - target_months as i32).abs())
            .map(|(_, b)| b)
    }

    /// Gets specific security by SecurityId.
    #[must_use]
    pub fn security_by_id(&self, id: &SecurityId) -> Option<&GovernmentBenchmark> {
        self.benchmarks.values().find(|b| &b.id == id)
    }

    /// Lists all available benchmark tenors.
    #[must_use]
    pub fn available_tenors(&self) -> Vec<Tenor> {
        self.benchmarks.values().map(|b| b.tenor).collect()
    }

    /// Gets all on-the-run benchmarks.
    #[must_use]
    pub fn on_the_runs(&self) -> Vec<&GovernmentBenchmark> {
        self.benchmarks
            .values()
            .filter(|b| b.is_on_the_run)
            .collect()
    }

    /// Returns true if the curve has any benchmarks.
    #[must_use]
    pub fn has_benchmarks(&self) -> bool {
        !self.benchmarks.is_empty()
    }

    /// Returns the number of benchmarks.
    #[must_use]
    pub fn benchmark_count(&self) -> usize {
        self.benchmarks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::types::Compounding;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_test_benchmark(tenor: Tenor, yield_pct: f64) -> GovernmentBenchmark {
        let maturity = date(2030, 1, 15);
        let yield_rate = Yield::new(
            Decimal::from_f64_retain(yield_pct / 100.0).unwrap(),
            Compounding::SemiAnnual,
        );
        GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            tenor,
            "912828XX0",
            maturity,
            dec!(0.04),
            yield_rate,
        )
    }

    #[test]
    fn test_government_curve_creation() {
        let curve = GovernmentCurve::us_treasury(date(2024, 1, 15));
        assert_eq!(curve.sovereign(), Sovereign::UST);
        assert_eq!(curve.reference_date(), date(2024, 1, 15));
        assert!(!curve.has_benchmarks());
    }

    #[test]
    fn test_government_curve_with_benchmarks() {
        let curve = GovernmentCurve::us_treasury(date(2024, 1, 15))
            .with_benchmark(create_test_benchmark(Tenor::Y2, 4.35))
            .with_benchmark(create_test_benchmark(Tenor::Y10, 4.25));

        assert!(curve.has_benchmarks());
        assert_eq!(curve.benchmark_count(), 2);
        assert!(curve.benchmark_yield(Tenor::Y2).is_some());
        assert!(curve.benchmark_yield(Tenor::Y10).is_some());
        assert!(curve.benchmark_yield(Tenor::Y5).is_none());
    }

    #[test]
    fn test_interpolated_yield() {
        let curve = GovernmentCurve::us_treasury(date(2024, 1, 15))
            .with_point(2.0, 0.0435)
            .with_point(10.0, 0.0425);

        // Interpolate at 5 years (midpoint)
        let yield_5y = curve.interpolated_yield(5.0);
        let value = yield_5y.value().to_string().parse::<f64>().unwrap();

        // Should be between 4.25% and 4.35%
        assert!(value > 0.0425 && value < 0.0435);
    }

    #[test]
    fn test_nearest_benchmark() {
        let curve = GovernmentCurve::us_treasury(date(2024, 1, 15))
            .with_benchmark(create_test_benchmark(Tenor::Y2, 4.35))
            .with_benchmark(create_test_benchmark(Tenor::Y10, 4.25))
            .with_benchmark(create_test_benchmark(Tenor::Y30, 4.50));

        // 7 years should be nearest to 10Y
        let nearest = curve.nearest_benchmark(7.0).unwrap();
        assert_eq!(nearest.tenor, Tenor::Y10);

        // 3 years should be nearest to 2Y
        let nearest = curve.nearest_benchmark(3.0).unwrap();
        assert_eq!(nearest.tenor, Tenor::Y2);
    }

    #[test]
    fn test_from_benchmarks() {
        let benchmarks = vec![
            create_test_benchmark(Tenor::Y2, 4.35),
            create_test_benchmark(Tenor::Y10, 4.25),
        ];

        let curve = GovernmentCurve::from_benchmarks(Sovereign::UST, date(2024, 1, 15), benchmarks);

        assert_eq!(curve.benchmark_count(), 2);
    }

    #[test]
    fn test_available_tenors() {
        let curve = GovernmentCurve::us_treasury(date(2024, 1, 15))
            .with_benchmark(create_test_benchmark(Tenor::Y2, 4.35))
            .with_benchmark(create_test_benchmark(Tenor::Y10, 4.25));

        let tenors = curve.available_tenors();
        assert!(tenors.contains(&Tenor::Y2));
        assert!(tenors.contains(&Tenor::Y10));
    }

    #[test]
    fn test_on_the_runs() {
        let b1 = create_test_benchmark(Tenor::Y2, 4.35);
        let b2 = create_test_benchmark(Tenor::Y10, 4.25).off_the_run();

        let curve = GovernmentCurve::us_treasury(date(2024, 1, 15))
            .with_benchmark(b1)
            .with_benchmark(b2);

        let on_the_runs = curve.on_the_runs();
        assert_eq!(on_the_runs.len(), 1);
        assert_eq!(on_the_runs[0].tenor, Tenor::Y2);
    }

    #[test]
    fn test_security_by_id() {
        let benchmark = GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            Tenor::Y10,
            "912828XX0",
            date(2034, 1, 15),
            dec!(0.04),
            Yield::from_bps(425, Compounding::SemiAnnual),
        );

        let curve =
            GovernmentCurve::us_treasury(date(2024, 1, 15)).with_benchmark(benchmark.clone());

        let found = curve.security_by_id(&benchmark.id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().tenor, Tenor::Y10);
    }

    #[test]
    fn test_different_sovereigns() {
        let gilt = GovernmentCurve::uk_gilt(date(2024, 1, 15));
        assert_eq!(gilt.sovereign(), Sovereign::UK);

        let bund = GovernmentCurve::german_bund(date(2024, 1, 15));
        assert_eq!(bund.sovereign(), Sovereign::Germany);

        let jgb = GovernmentCurve::japanese_jgb(date(2024, 1, 15));
        assert_eq!(jgb.sovereign(), Sovereign::Japan);
    }
}
