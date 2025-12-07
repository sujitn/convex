//! YAS Calculator - Bloomberg YAS Replication.
//!
//! This module provides the main YAS calculator that integrates yield, spread,
//! and risk calculations into a single comprehensive analysis matching Bloomberg's
//! YAS (Yield Analysis System) function.
//!
//! # Bloomberg Validation Target
//!
//! Boeing 7.5% 06/15/2025 (CUSIP: 097023AH7)
//! Settlement: 04/29/2020, Price: 110.503
//!
//! | Metric          | Bloomberg | Target Tolerance |
//! |-----------------|-----------|------------------|
//! | Street YTM      | 4.905895% | ±0.0001%         |
//! | G-Spread        | 448.5 bps | ±0.5 bps         |
//! | Z-Spread        | 444.7 bps | ±1.0 bps         |
//! | Mod Duration    | 4.209     | ±0.001           |
//! | Convexity       | 0.219     | ±0.001           |

use crate::invoice::SettlementInvoice;
use crate::yields::{current_yield, simple_yield, street_convention_yield};
use crate::YasError;
use chrono::NaiveDate;
use convex_bonds::prelude::Bond;
use convex_bonds::traits::BondCashFlow;
use convex_core::types::{Date, Spread, SpreadType};
use convex_curves::curves::ZeroCurve;
use convex_risk::{BondRiskCalculator, BondRiskMetrics};
use convex_spreads::ZSpreadCalculator;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;

/// Complete YAS result matching Bloomberg YAS output.
///
/// This struct provides all metrics that would be displayed on a
/// Bloomberg YAS screen, using proper typed values for spreads and risk metrics.
#[derive(Debug, Clone)]
pub struct YASResult {
    // ===== Yield Metrics =====
    /// Street convention yield-to-maturity (standard market quote)
    pub ytm: Decimal,

    /// True yield (accounts for actual settlement)
    pub true_yield: Decimal,

    /// Current yield (annual coupon / clean price)
    pub current_yield: Decimal,

    /// Simple yield
    pub simple_yield: Decimal,

    // ===== Spread Metrics =====
    /// G-Spread (yield - interpolated government yield)
    pub g_spread: Spread,

    /// Z-Spread (constant spread over spot curve)
    pub z_spread: Spread,

    /// Asset swap spread (par-par)
    pub asw_spread: Option<Spread>,

    /// Option-adjusted spread (for callable/putable bonds)
    pub oas: Option<Spread>,

    // ===== Risk Metrics =====
    /// Complete risk metrics from BondRiskCalculator
    pub risk: BondRiskMetrics,

    // ===== Settlement Invoice =====
    /// Settlement calculation details
    pub invoice: SettlementInvoice,
}

impl YASResult {
    /// Returns the modified duration for quick access.
    #[must_use]
    pub fn modified_duration(&self) -> Decimal {
        self.risk.modified_duration.years()
    }

    /// Returns the convexity for quick access.
    #[must_use]
    pub fn convexity(&self) -> Decimal {
        self.risk.convexity.value()
    }

    /// Returns the DV01 per 100 face for quick access.
    #[must_use]
    pub fn dv01(&self) -> Decimal {
        self.risk.dv01_per_100.value()
    }

    /// Validates against Bloomberg reference values.
    ///
    /// Returns a vector of validation failures (empty if all pass).
    #[must_use]
    pub fn validate_bloomberg(&self, reference: &BloombergReference) -> Vec<ValidationFailure> {
        let mut failures = Vec::new();

        // YTM validation
        let ytm_diff = (self.ytm - reference.ytm).abs();
        if ytm_diff > reference.ytm_tolerance {
            failures.push(ValidationFailure {
                metric: "YTM".to_string(),
                expected: reference.ytm,
                actual: self.ytm,
                tolerance: reference.ytm_tolerance,
            });
        }

        // G-Spread validation
        let g_diff = (self.g_spread.as_bps() - reference.g_spread_bps).abs();
        if g_diff > reference.spread_tolerance_bps {
            failures.push(ValidationFailure {
                metric: "G-Spread".to_string(),
                expected: reference.g_spread_bps,
                actual: self.g_spread.as_bps(),
                tolerance: reference.spread_tolerance_bps,
            });
        }

        // Z-Spread validation
        let z_diff = (self.z_spread.as_bps() - reference.z_spread_bps).abs();
        if z_diff > reference.z_spread_tolerance_bps {
            failures.push(ValidationFailure {
                metric: "Z-Spread".to_string(),
                expected: reference.z_spread_bps,
                actual: self.z_spread.as_bps(),
                tolerance: reference.z_spread_tolerance_bps,
            });
        }

        // Modified Duration validation
        let dur_diff = (self.modified_duration() - reference.modified_duration).abs();
        if dur_diff > reference.duration_tolerance {
            failures.push(ValidationFailure {
                metric: "Modified Duration".to_string(),
                expected: reference.modified_duration,
                actual: self.modified_duration(),
                tolerance: reference.duration_tolerance,
            });
        }

        // Convexity validation
        let conv_diff = (self.convexity() - reference.convexity).abs();
        if conv_diff > reference.convexity_tolerance {
            failures.push(ValidationFailure {
                metric: "Convexity".to_string(),
                expected: reference.convexity,
                actual: self.convexity(),
                tolerance: reference.convexity_tolerance,
            });
        }

        failures
    }
}

impl std::fmt::Display for YASResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "╔══════════════════════════════════════════════════════════╗")?;
        writeln!(f, "║                    YAS ANALYSIS                          ║")?;
        writeln!(f, "╠══════════════════════════════════════════════════════════╣")?;
        writeln!(f, "║ YIELDS                                                   ║")?;
        writeln!(f, "║   Street Convention:  {:>10.6}%                      ║", self.ytm)?;
        writeln!(f, "║   True Yield:         {:>10.6}%                      ║", self.true_yield)?;
        writeln!(f, "║   Current Yield:      {:>10.3}%                       ║", self.current_yield)?;
        writeln!(f, "║   Simple Yield:       {:>10.3}%                       ║", self.simple_yield)?;
        writeln!(f, "╠══════════════════════════════════════════════════════════╣")?;
        writeln!(f, "║ SPREADS                                                  ║")?;
        writeln!(f, "║   G-Spread:           {:>10.1} bps                    ║", self.g_spread.as_bps())?;
        writeln!(f, "║   Z-Spread:           {:>10.1} bps                    ║", self.z_spread.as_bps())?;
        if let Some(asw) = &self.asw_spread {
            writeln!(f, "║   ASW Spread:         {:>10.1} bps                    ║", asw.as_bps())?;
        }
        if let Some(oas) = &self.oas {
            writeln!(f, "║   OAS:                {:>10.1} bps                    ║", oas.as_bps())?;
        }
        writeln!(f, "╠══════════════════════════════════════════════════════════╣")?;
        writeln!(f, "║ RISK METRICS                                             ║")?;
        writeln!(f, "║   Macaulay Duration:  {:>10.3}                        ║", self.risk.macaulay_duration.years())?;
        writeln!(f, "║   Modified Duration:  {:>10.3}                        ║", self.modified_duration())?;
        writeln!(f, "║   Convexity:          {:>10.3}                        ║", self.convexity())?;
        writeln!(f, "║   DV01 (per $100):    ${:>9.4}                        ║", self.dv01())?;
        writeln!(f, "╠══════════════════════════════════════════════════════════╣")?;
        writeln!(f, "║ SETTLEMENT                                               ║")?;
        writeln!(f, "║   Date:               {}                         ║", self.invoice.settlement_date)?;
        writeln!(f, "║   Clean Price:        {:>10.6}%                      ║", self.invoice.clean_price)?;
        writeln!(f, "║   Accrued Interest:   {:>10.6}%                      ║", self.invoice.accrued_interest)?;
        writeln!(f, "║   Dirty Price:        {:>10.6}%                      ║", self.invoice.dirty_price)?;
        writeln!(f, "╚══════════════════════════════════════════════════════════╝")?;
        Ok(())
    }
}

/// Bloomberg reference values for validation.
#[derive(Debug, Clone)]
pub struct BloombergReference {
    /// Expected YTM
    pub ytm: Decimal,
    /// YTM tolerance (e.g., 0.0001 for ±0.0001%)
    pub ytm_tolerance: Decimal,
    /// Expected G-spread in basis points
    pub g_spread_bps: Decimal,
    /// Spread tolerance in basis points
    pub spread_tolerance_bps: Decimal,
    /// Expected Z-spread in basis points
    pub z_spread_bps: Decimal,
    /// Z-spread tolerance in basis points
    pub z_spread_tolerance_bps: Decimal,
    /// Expected modified duration
    pub modified_duration: Decimal,
    /// Duration tolerance
    pub duration_tolerance: Decimal,
    /// Expected convexity
    pub convexity: Decimal,
    /// Convexity tolerance
    pub convexity_tolerance: Decimal,
}

impl BloombergReference {
    /// Boeing 7.5% 06/15/2025 reference values (Settlement: 04/29/2020, Price: 110.503)
    #[must_use]
    pub fn boeing_2025() -> Self {
        Self {
            ytm: dec!(4.905895),
            ytm_tolerance: dec!(0.0001),
            g_spread_bps: dec!(448.5),
            spread_tolerance_bps: dec!(0.5),
            z_spread_bps: dec!(444.7),
            z_spread_tolerance_bps: dec!(1.0),
            modified_duration: dec!(4.209),
            duration_tolerance: dec!(0.001),
            convexity: dec!(0.219),
            convexity_tolerance: dec!(0.001),
        }
    }
}

/// Validation failure information.
#[derive(Debug, Clone)]
pub struct ValidationFailure {
    /// Name of the metric that failed
    pub metric: String,
    /// Expected value
    pub expected: Decimal,
    /// Actual computed value
    pub actual: Decimal,
    /// Allowed tolerance
    pub tolerance: Decimal,
}

impl std::fmt::Display for ValidationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: expected {} ± {}, got {} (diff: {})",
            self.metric,
            self.expected,
            self.tolerance,
            self.actual,
            (self.expected - self.actual).abs()
        )
    }
}

/// YAS Calculator - main calculator for Bloomberg YAS replication.
///
/// This calculator integrates yield, spread, and risk calculations to produce
/// a complete YAS analysis matching Bloomberg's output.
///
/// # Example
///
/// ```ignore
/// use convex_yas::calculator::YASCalculator;
/// use convex_bonds::FixedRateBond;
/// use convex_curves::curves::ZeroCurve;
///
/// let curve = // ... create ZeroCurve
/// let calculator = YASCalculator::new(&curve);
///
/// let bond = FixedRateBond::builder()
///     .coupon_rate(0.075)
///     .maturity(date!(2025-06-15))
///     .build()?;
///
/// let result = calculator.analyze(&bond, settlement, 110.503)?;
/// println!("{}", result);
/// ```
pub struct YASCalculator<'a> {
    /// Government/benchmark curve for G-spread
    govt_curve: &'a ZeroCurve,
    /// Swap curve for I-spread (optional)
    swap_curve: Option<&'a ZeroCurve>,
    /// Spot curve for Z-spread
    spot_curve: &'a ZeroCurve,
    /// Face value for calculations (default: 100)
    face_value: f64,
    /// Compounding frequency (default: 2 for semi-annual)
    frequency: u32,
}

impl<'a> YASCalculator<'a> {
    /// Creates a new YAS calculator with a single curve for all spread calculations.
    ///
    /// # Arguments
    ///
    /// * `curve` - The ZeroCurve to use for both G-spread and Z-spread calculations
    pub fn new(curve: &'a ZeroCurve) -> Self {
        Self {
            govt_curve: curve,
            swap_curve: None,
            spot_curve: curve,
            face_value: 100.0,
            frequency: 2,
        }
    }

    /// Creates a calculator with separate government and spot curves.
    ///
    /// # Arguments
    ///
    /// * `govt_curve` - Government curve for G-spread interpolation
    /// * `spot_curve` - Spot curve for Z-spread calculation
    pub fn with_curves(govt_curve: &'a ZeroCurve, spot_curve: &'a ZeroCurve) -> Self {
        Self {
            govt_curve,
            swap_curve: None,
            spot_curve,
            face_value: 100.0,
            frequency: 2,
        }
    }

    /// Sets the swap curve for I-spread calculation.
    #[must_use]
    pub fn with_swap_curve(mut self, swap_curve: &'a ZeroCurve) -> Self {
        self.swap_curve = Some(swap_curve);
        self
    }

    /// Sets the face value for calculations.
    #[must_use]
    pub fn with_face_value(mut self, face_value: f64) -> Self {
        self.face_value = face_value;
        self
    }

    /// Sets the compounding frequency.
    #[must_use]
    pub fn with_frequency(mut self, frequency: u32) -> Self {
        self.frequency = frequency;
        self
    }

    /// Performs a complete YAS analysis on a bond.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond to analyze (must implement Bond trait)
    /// * `settlement` - Settlement date
    /// * `clean_price` - Clean price as percentage of par (e.g., 110.503)
    ///
    /// # Returns
    ///
    /// Complete YAS result with yields, spreads, risk metrics, and settlement invoice
    pub fn analyze(
        &self,
        bond: &dyn Bond,
        settlement: NaiveDate,
        clean_price: Decimal,
    ) -> Result<YASResult, YasError> {
        // Convert settlement to Date type
        let settlement_date: Date = settlement.into();

        // Get cash flows
        let cash_flows = bond.cash_flows(settlement_date);
        if cash_flows.is_empty() {
            return Err(YasError::InvalidInput("bond has no cash flows".to_string()));
        }

        // Calculate accrued interest (returns Decimal directly)
        let accrued_decimal = bond.accrued_interest(settlement_date);
        let dirty_price = clean_price + accrued_decimal;
        let dirty_price_f64 = dirty_price
            .to_string()
            .parse::<f64>()
            .map_err(|_| YasError::CalculationFailed("invalid dirty price".to_string()))?;

        // Prepare cash flow vectors
        let mut times: Vec<f64> = Vec::new();
        let mut cf_values: Vec<f64> = Vec::new();

        for cf in &cash_flows {
            let t = settlement_date.days_between(&cf.date) as f64 / 365.25;
            if t > 0.0 {
                times.push(t);
                cf_values.push(cf.amount.to_string().parse::<f64>().unwrap_or(0.0));
            }
        }

        // Calculate yields
        let ytm = street_convention_yield(dirty_price_f64, &cf_values, &times, self.frequency, 0.05)?;

        // Estimate annual coupon from first cash flow (assume semi-annual)
        let periodic_coupon = cash_flows.first().map(|cf| cf.amount).unwrap_or(Decimal::ZERO);
        let annual_coupon = periodic_coupon * Decimal::from(self.frequency);
        let current = current_yield(annual_coupon, clean_price)?;

        // Calculate years to maturity for simple yield
        let years_decimal = Decimal::from_f64_retain(times.last().copied().unwrap_or(1.0)).unwrap_or(dec!(1));
        let simple = simple_yield(annual_coupon, clean_price, dec!(100), years_decimal)?;

        // Calculate G-spread
        // Get the maturity date from the last cash flow
        let maturity_date = cash_flows.last().map(|cf| cf.date).unwrap_or(settlement_date);
        let benchmark_rate = self.govt_curve.zero_rate_at(maturity_date)
            .map_err(|e| YasError::CurveError(format!("benchmark rate: {e}")))?;
        // Convert benchmark rate to percentage and calculate spread
        let benchmark_pct = benchmark_rate * Decimal::ONE_HUNDRED;
        let g_spread_bps = (ytm - benchmark_pct) * Decimal::ONE_HUNDRED;
        let g_spread_value = Spread::new(g_spread_bps, SpreadType::GSpread);

        // Calculate Z-spread
        let z_spread_value = self.calculate_z_spread(&cash_flows, dirty_price, settlement_date)?;

        // Calculate risk metrics
        let ytm_f64 = ytm
            .to_string()
            .parse::<f64>()
            .map_err(|_| YasError::CalculationFailed("invalid ytm".to_string()))?
            / 100.0;

        let risk = BondRiskCalculator::from_bond(
            bond,
            settlement_date,
            dirty_price_f64,
            ytm_f64,
            self.frequency,
        )
        .map_err(|e| YasError::CalculationFailed(format!("risk calculator: {e}")))?
        .all_metrics()
        .map_err(|e| YasError::CalculationFailed(format!("risk metrics: {e}")))?;

        // Build settlement invoice
        let face_value = bond.face_value();
        let accrued_days = self.calculate_accrued_days(&cash_flows, settlement_date);

        let invoice = SettlementInvoice::builder()
            .settlement_date(settlement)
            .clean_price(clean_price)
            .accrued_interest(accrued_decimal)
            .accrued_days(accrued_days)
            .face_value(face_value)
            .build()
            .map_err(|e| YasError::MissingData(e.to_string()))?;

        Ok(YASResult {
            ytm,
            true_yield: ytm, // For now, true yield = street convention
            current_yield: current,
            simple_yield: simple,
            g_spread: g_spread_value,
            z_spread: z_spread_value,
            asw_spread: None, // TODO: implement ASW spread
            oas: None,        // Only for callable/putable bonds
            risk,
            invoice,
        })
    }

    /// Calculates Z-spread using Brent solver.
    fn calculate_z_spread(
        &self,
        cash_flows: &[BondCashFlow],
        dirty_price: Decimal,
        settlement: Date,
    ) -> Result<Spread, YasError> {
        let calculator = ZSpreadCalculator::new(self.spot_curve);

        calculator
            .calculate_from_cash_flows(cash_flows, dirty_price, settlement)
            .map_err(|e| YasError::CalculationFailed(format!("z-spread: {e}")))
    }

    /// Calculates accrued days from last coupon.
    fn calculate_accrued_days(&self, cash_flows: &[BondCashFlow], settlement: Date) -> i32 {
        if cash_flows.is_empty() {
            return 0;
        }

        // Find accrual start date from first remaining cash flow
        if let Some(first_cf) = cash_flows.first() {
            if let Some(accrual_start) = first_cf.accrual_start {
                return accrual_start.days_between(&settlement) as i32;
            }
        }

        0
    }
}

/// Batch YAS Calculator for parallel processing of multiple bonds.
///
/// This calculator efficiently processes multiple bonds in parallel using Rayon.
///
/// # Example
///
/// ```ignore
/// use convex_yas::calculator::BatchYASCalculator;
///
/// let calculator = BatchYASCalculator::new(&curve);
/// let results = calculator.analyze_batch(&bonds, settlement, &prices)?;
/// ```
#[cfg(feature = "parallel")]
pub struct BatchYASCalculator<'a> {
    calculator: YASCalculator<'a>,
}

#[cfg(feature = "parallel")]
impl<'a> BatchYASCalculator<'a> {
    /// Creates a new batch calculator.
    pub fn new(curve: &'a ZeroCurve) -> Self {
        Self {
            calculator: YASCalculator::new(curve),
        }
    }

    /// Creates a batch calculator with separate curves.
    pub fn with_curves(govt_curve: &'a ZeroCurve, spot_curve: &'a ZeroCurve) -> Self {
        Self {
            calculator: YASCalculator::with_curves(govt_curve, spot_curve),
        }
    }

    /// Analyzes a batch of bonds in parallel.
    pub fn analyze_batch(
        &self,
        bonds: &[Arc<dyn Bond + Send + Sync>],
        settlement: NaiveDate,
        prices: &[Decimal],
    ) -> Vec<Result<YASResult, YasError>> {
        use rayon::prelude::*;

        bonds
            .par_iter()
            .zip(prices.par_iter())
            .map(|(bond, price)| self.calculator.analyze(bond.as_ref(), settlement, *price))
            .collect()
    }
}

/// Batch YAS Calculator (non-parallel fallback).
#[cfg(not(feature = "parallel"))]
pub struct BatchYASCalculator<'a> {
    calculator: YASCalculator<'a>,
}

#[cfg(not(feature = "parallel"))]
impl<'a> BatchYASCalculator<'a> {
    /// Creates a new batch calculator.
    pub fn new(curve: &'a ZeroCurve) -> Self {
        Self {
            calculator: YASCalculator::new(curve),
        }
    }

    /// Creates a batch calculator with separate curves.
    pub fn with_curves(govt_curve: &'a ZeroCurve, spot_curve: &'a ZeroCurve) -> Self {
        Self {
            calculator: YASCalculator::with_curves(govt_curve, spot_curve),
        }
    }

    /// Analyzes a batch of bonds sequentially.
    pub fn analyze_batch(
        &self,
        bonds: &[Arc<dyn Bond + Send + Sync>],
        settlement: NaiveDate,
        prices: &[Decimal],
    ) -> Vec<Result<YASResult, YasError>> {
        bonds
            .iter()
            .zip(prices.iter())
            .map(|(bond, price)| self.calculator.analyze(bond.as_ref(), settlement, *price))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_curves::prelude::{InterpolationMethod, ZeroCurveBuilder};
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> convex_core::types::Date {
        convex_core::types::Date::from_ymd(y, m, d).unwrap()
    }

    fn create_test_curve() -> ZeroCurve {
        // Create a simple upward-sloping curve
        ZeroCurveBuilder::new()
            .reference_date(date(2020, 4, 29))
            .add_rate(date(2020, 7, 29), dec!(0.005))  // 3M: 0.5%
            .add_rate(date(2020, 10, 29), dec!(0.006)) // 6M: 0.6%
            .add_rate(date(2021, 4, 29), dec!(0.008))  // 1Y: 0.8%
            .add_rate(date(2022, 4, 29), dec!(0.012))  // 2Y: 1.2%
            .add_rate(date(2023, 4, 29), dec!(0.015))  // 3Y: 1.5%
            .add_rate(date(2025, 4, 29), dec!(0.020))  // 5Y: 2.0%
            .add_rate(date(2030, 4, 29), dec!(0.025))  // 10Y: 2.5%
            .interpolation(InterpolationMethod::Linear)
            .build()
            .unwrap()
    }

    // Helper to create a simple test bond (Boeing 7.5% 06/15/2025)
    fn create_test_bond() -> convex_bonds::FixedRateBond {
        use convex_core::daycounts::DayCountConvention;
        use convex_core::types::Frequency;

        convex_bonds::FixedRateBond::builder()
            .cusip_unchecked("097023AH7") // Boeing CUSIP
            .face_value(dec!(100))
            .coupon_rate(dec!(0.075))
            .maturity(date(2025, 6, 15))
            .issue_date(date(1995, 6, 15))
            .day_count(DayCountConvention::Thirty360US)
            .frequency(Frequency::SemiAnnual)
            .build()
            .unwrap()
    }

    #[test]
    fn test_yas_calculator_basic() {
        let curve = create_test_curve();
        let calculator = YASCalculator::new(&curve);

        let bond = create_test_bond();
        let settlement = NaiveDate::from_ymd_opt(2020, 4, 29).unwrap();

        let result = calculator.analyze(&bond, settlement, dec!(110.503));

        assert!(result.is_ok());
        let yas = result.unwrap();

        // Verify we got reasonable values
        assert!(yas.ytm > Decimal::ZERO, "YTM should be positive");
        // G-spread can be positive or negative depending on benchmark curve level
        // For a 7.5% coupon bond with our low test curve, it should be large positive
        assert!(yas.g_spread.as_bps() != Decimal::ZERO, "G-spread should be calculated");
        assert!(yas.modified_duration() > Decimal::ZERO, "Duration should be positive");
    }

    #[test]
    fn test_yas_result_display() {
        let curve = create_test_curve();
        let calculator = YASCalculator::new(&curve);

        let bond = create_test_bond();
        let settlement = NaiveDate::from_ymd_opt(2020, 4, 29).unwrap();

        let result = calculator.analyze(&bond, settlement, dec!(110.503)).unwrap();

        let display = format!("{}", result);
        assert!(display.contains("YAS ANALYSIS"));
        assert!(display.contains("YIELDS"));
        assert!(display.contains("SPREADS"));
        assert!(display.contains("RISK METRICS"));
    }

    #[test]
    fn test_bloomberg_reference() {
        let reference = BloombergReference::boeing_2025();

        assert_eq!(reference.ytm, dec!(4.905895));
        assert_eq!(reference.g_spread_bps, dec!(448.5));
        assert_eq!(reference.z_spread_bps, dec!(444.7));
        assert_eq!(reference.modified_duration, dec!(4.209));
        assert_eq!(reference.convexity, dec!(0.219));
    }

    #[test]
    fn test_validation_failure_display() {
        let failure = ValidationFailure {
            metric: "YTM".to_string(),
            expected: dec!(4.905895),
            actual: dec!(4.906000),
            tolerance: dec!(0.0001),
        };

        let display = format!("{}", failure);
        assert!(display.contains("YTM"));
        assert!(display.contains("4.905895"));
    }

    #[test]
    fn test_yas_result_accessors() {
        let curve = create_test_curve();
        let calculator = YASCalculator::new(&curve);

        let bond = create_test_bond();
        let settlement = NaiveDate::from_ymd_opt(2020, 4, 29).unwrap();

        let result = calculator.analyze(&bond, settlement, dec!(110.503)).unwrap();

        // Test convenience accessors
        assert_eq!(
            result.modified_duration(),
            result.risk.modified_duration.years()
        );
        assert_eq!(result.convexity(), result.risk.convexity.value());
        assert_eq!(result.dv01(), result.risk.dv01_per_100.value());
    }

    #[test]
    fn test_with_frequency() {
        let curve = create_test_curve();
        let calculator = YASCalculator::new(&curve).with_frequency(4); // Quarterly

        let bond = create_test_bond();
        let settlement = NaiveDate::from_ymd_opt(2020, 4, 29).unwrap();

        let result = calculator.analyze(&bond, settlement, dec!(110.503));
        assert!(result.is_ok());
    }

    #[test]
    fn test_invoice_calculation() {
        let curve = create_test_curve();
        let calculator = YASCalculator::new(&curve);

        let bond = create_test_bond();
        let settlement = NaiveDate::from_ymd_opt(2020, 4, 29).unwrap();

        let result = calculator.analyze(&bond, settlement, dec!(110.503)).unwrap();

        // Invoice should have correct values
        assert_eq!(result.invoice.clean_price, dec!(110.503));
        assert!(result.invoice.accrued_interest >= Decimal::ZERO);
        assert!(result.invoice.dirty_price > result.invoice.clean_price);
    }
}
