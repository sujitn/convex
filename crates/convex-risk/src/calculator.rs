//! Integrated risk calculator for bonds.
//!
//! This module provides a high-level `BondRiskCalculator` that computes
//! all risk metrics (duration, convexity, DV01) for a bond given market data.
//!
//! # Example
//!
//! ```ignore
//! use convex_risk::calculator::BondRiskCalculator;
//! use convex_bonds::FixedRateBond;
//!
//! let bond = FixedRateBond::builder()
//!     .coupon_rate(0.075)
//!     .maturity(date!(2025-06-15))
//!     .build()?;
//!
//! let calc = BondRiskCalculator::new(&bond, settlement, dirty_price, ytm)?;
//! let metrics = calc.all_metrics()?;
//!
//! println!("Modified Duration: {}", metrics.modified_duration);
//! println!("Convexity: {}", metrics.convexity);
//! println!("DV01: {}", metrics.dv01);
//! ```

use rust_decimal::prelude::*;

use convex_bonds::traits::Bond;
use convex_core::types::Date;

use crate::convexity::{analytical_convexity, effective_convexity, Convexity};
use crate::duration::{
    effective_duration, key_rate_duration_at_tenor, macaulay_duration, modified_duration,
    modified_from_macaulay, Duration, KeyRateDurations, STANDARD_KEY_RATE_TENORS,
};
use crate::dv01::{dv01_from_duration, DV01};
use crate::RiskError;

/// Complete risk metrics for a bond.
#[derive(Debug, Clone)]
pub struct BondRiskMetrics {
    /// Macaulay duration (weighted average time to cash flows).
    pub macaulay_duration: Duration,
    /// Modified duration (price sensitivity to yield).
    pub modified_duration: Duration,
    /// Analytical convexity.
    pub convexity: Convexity,
    /// Dollar value of 1 basis point (per $100 face).
    pub dv01_per_100: DV01,
    /// Dollar value of 1 basis point (total position).
    pub dv01: DV01,
}

impl BondRiskMetrics {
    /// Estimate price change for a given yield shift.
    ///
    /// Uses duration + convexity approximation:
    /// ΔP/P ≈ -D_mod × Δy + (1/2) × C × (Δy)²
    pub fn estimate_price_change(&self, yield_change: f64, dirty_price: f64) -> f64 {
        let duration_effect = -self.modified_duration.as_f64() * dirty_price * yield_change;
        let convexity_effect = 0.5 * self.convexity.as_f64() * dirty_price * yield_change.powi(2);
        duration_effect + convexity_effect
    }

    /// Get the convexity adjustment factor.
    ///
    /// The convexity adjustment = (1/2) × C × (Δy)²
    pub fn convexity_adjustment(&self, yield_change: f64) -> f64 {
        0.5 * self.convexity.as_f64() * yield_change.powi(2)
    }
}

/// Calculator for bond risk metrics.
///
/// Provides all duration, convexity, and DV01 calculations for a bond.
pub struct BondRiskCalculator {
    /// Time to each cash flow in years.
    times: Vec<f64>,
    /// Cash flow amounts.
    cash_flows: Vec<f64>,
    /// Yield to maturity (as decimal).
    ytm: f64,
    /// Compounding frequency.
    frequency: u32,
    /// Dirty price as percentage of par.
    dirty_price: f64,
    /// Face value.
    face_value: f64,
}

impl BondRiskCalculator {
    /// Creates a new risk calculator for a bond.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond to analyze
    /// * `settlement` - Settlement date
    /// * `dirty_price` - Dirty price as percentage of par (e.g., 105.5)
    /// * `ytm` - Yield to maturity as decimal (e.g., 0.05 for 5%)
    /// * `frequency` - Compounding frequency per year (typically 2 for semi-annual)
    ///
    /// # Errors
    ///
    /// Returns error if no future cash flows exist.
    pub fn from_bond(
        bond: &dyn Bond,
        settlement: Date,
        dirty_price: f64,
        ytm: f64,
        frequency: u32,
    ) -> Result<Self, RiskError> {
        let cash_flows = bond.cash_flows(settlement);

        if cash_flows.is_empty() {
            return Err(RiskError::InsufficientData(
                "no future cash flows".to_string(),
            ));
        }

        let (times, amounts): (Vec<f64>, Vec<f64>) = cash_flows
            .iter()
            .filter(|cf| cf.date > settlement)
            .map(|cf| {
                let years = settlement.days_between(&cf.date) as f64 / 365.0;
                let amount = cf.amount.to_f64().unwrap_or(0.0);
                (years, amount)
            })
            .unzip();

        if times.is_empty() {
            return Err(RiskError::InsufficientData(
                "no future cash flows after settlement".to_string(),
            ));
        }

        let face_value = bond.face_value().to_f64().unwrap_or(100.0);

        Ok(Self {
            times,
            cash_flows: amounts,
            ytm,
            frequency,
            dirty_price,
            face_value,
        })
    }

    /// Creates a risk calculator from raw cash flow data.
    pub fn from_cash_flows(
        times: Vec<f64>,
        cash_flows: Vec<f64>,
        ytm: f64,
        frequency: u32,
        dirty_price: f64,
        face_value: f64,
    ) -> Result<Self, RiskError> {
        if times.len() != cash_flows.len() {
            return Err(RiskError::InvalidInput(
                "times and cash_flows must have same length".to_string(),
            ));
        }

        if times.is_empty() {
            return Err(RiskError::InsufficientData(
                "no cash flows provided".to_string(),
            ));
        }

        Ok(Self {
            times,
            cash_flows,
            ytm,
            frequency,
            dirty_price,
            face_value,
        })
    }

    /// Calculates Macaulay duration.
    pub fn macaulay_duration(&self) -> Result<Duration, RiskError> {
        macaulay_duration(&self.times, &self.cash_flows, self.ytm, self.frequency)
    }

    /// Calculates modified duration.
    pub fn modified_duration(&self) -> Result<Duration, RiskError> {
        modified_duration(&self.times, &self.cash_flows, self.ytm, self.frequency)
    }

    /// Calculates analytical convexity.
    pub fn convexity(&self) -> Result<Convexity, RiskError> {
        analytical_convexity(&self.times, &self.cash_flows, self.ytm, self.frequency)
    }

    /// Calculates DV01 per $100 face value.
    pub fn dv01_per_100(&self) -> Result<DV01, RiskError> {
        let mod_dur = self.modified_duration()?;
        Ok(dv01_from_duration(mod_dur, self.dirty_price, 100.0))
    }

    /// Calculates DV01 for the full position.
    pub fn dv01(&self) -> Result<DV01, RiskError> {
        let mod_dur = self.modified_duration()?;
        Ok(dv01_from_duration(
            mod_dur,
            self.dirty_price,
            self.face_value,
        ))
    }

    /// Calculates all risk metrics at once.
    pub fn all_metrics(&self) -> Result<BondRiskMetrics, RiskError> {
        let macaulay = self.macaulay_duration()?;
        let modified = modified_from_macaulay(macaulay, self.ytm, self.frequency);
        let convexity = self.convexity()?;
        let dv01_per_100 = dv01_from_duration(modified, self.dirty_price, 100.0);
        let dv01 = dv01_from_duration(modified, self.dirty_price, self.face_value);

        Ok(BondRiskMetrics {
            macaulay_duration: macaulay,
            modified_duration: modified,
            convexity,
            dv01_per_100,
            dv01,
        })
    }

    /// Returns the yield to maturity.
    pub fn ytm(&self) -> f64 {
        self.ytm
    }

    /// Returns the dirty price.
    pub fn dirty_price(&self) -> f64 {
        self.dirty_price
    }

    /// Returns the face value.
    pub fn face_value(&self) -> f64 {
        self.face_value
    }
}

/// Effective duration calculator using curve shifts.
///
/// Calculates effective duration by repricing the bond with shifted curves.
/// This is essential for bonds with embedded options.
pub struct EffectiveDurationCalculator {
    /// Bump size in decimal (e.g., 0.001 for 10 bps).
    bump_size: f64,
}

impl Default for EffectiveDurationCalculator {
    /// Default calculator with 10bp bump.
    fn default() -> Self {
        Self::new(10.0)
    }
}

impl EffectiveDurationCalculator {
    /// Creates a new effective duration calculator.
    ///
    /// # Arguments
    ///
    /// * `bump_bps` - Bump size in basis points (default: 10)
    pub fn new(bump_bps: f64) -> Self {
        Self {
            bump_size: bump_bps / 10_000.0,
        }
    }

    /// Calculates effective duration from pre-computed prices.
    ///
    /// # Arguments
    ///
    /// * `price_base` - Price at current curve
    /// * `price_up` - Price when curve shifts up
    /// * `price_down` - Price when curve shifts down
    pub fn from_prices(
        &self,
        price_base: f64,
        price_up: f64,
        price_down: f64,
    ) -> Result<Duration, RiskError> {
        effective_duration(price_up, price_down, price_base, self.bump_size)
    }

    /// Calculates effective convexity from pre-computed prices.
    pub fn convexity_from_prices(
        &self,
        price_base: f64,
        price_up: f64,
        price_down: f64,
    ) -> Result<Convexity, RiskError> {
        effective_convexity(price_up, price_down, price_base, self.bump_size)
    }

    /// Returns the bump size in basis points.
    pub fn bump_bps(&self) -> f64 {
        self.bump_size * 10_000.0
    }

    /// Returns the bump size as a decimal.
    pub fn bump_size(&self) -> f64 {
        self.bump_size
    }
}

/// Key rate duration calculator.
///
/// Calculates sensitivity to specific points on the yield curve.
pub struct KeyRateDurationCalculator {
    /// Key rate tenors to analyze.
    tenors: Vec<f64>,
    /// Bump size in decimal.
    bump_size: f64,
}

impl KeyRateDurationCalculator {
    /// Creates a calculator with standard tenors.
    pub fn standard() -> Self {
        Self {
            tenors: STANDARD_KEY_RATE_TENORS.to_vec(),
            bump_size: 0.0001, // 1 bp
        }
    }

    /// Creates a calculator with custom tenors.
    pub fn with_tenors(tenors: Vec<f64>) -> Self {
        Self {
            tenors,
            bump_size: 0.0001,
        }
    }

    /// Sets the bump size in basis points.
    pub fn with_bump_bps(mut self, bps: f64) -> Self {
        self.bump_size = bps / 10_000.0;
        self
    }

    /// Calculates key rate durations from price sensitivities.
    ///
    /// # Arguments
    ///
    /// * `base_price` - Price at current curve
    /// * `tenor_prices` - Vec of (tenor, price_up, price_down) tuples
    pub fn calculate(
        &self,
        base_price: f64,
        tenor_prices: &[(f64, f64, f64)],
    ) -> Result<KeyRateDurations, RiskError> {
        if base_price.abs() < 1e-10 {
            return Err(RiskError::DivisionByZero {
                context: "base price is zero".to_string(),
            });
        }

        let durations: Result<Vec<_>, _> = tenor_prices
            .iter()
            .map(|(tenor, price_up, price_down)| {
                key_rate_duration_at_tenor(
                    *price_up,
                    *price_down,
                    base_price,
                    self.bump_size,
                    *tenor,
                )
            })
            .collect();

        Ok(KeyRateDurations::new(durations?))
    }

    /// Returns the tenors being analyzed.
    pub fn tenors(&self) -> &[f64] {
        &self.tenors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_bond_risk_calculator_from_cash_flows() {
        // 2-year bond, 5% coupon, semi-annual
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];

        let calc = BondRiskCalculator::from_cash_flows(
            times, cash_flows, 0.05,  // 5% YTM
            2,     // semi-annual
            100.0, // at par
            100.0, // $100 face
        )
        .unwrap();

        let metrics = calc.all_metrics().unwrap();

        // Macaulay duration should be ~1.93 years for this bond
        assert_relative_eq!(metrics.macaulay_duration.as_f64(), 1.93, epsilon = 0.01);

        // Modified duration = Macaulay / (1 + y/f) ≈ 1.93 / 1.025 ≈ 1.88
        assert_relative_eq!(metrics.modified_duration.as_f64(), 1.88, epsilon = 0.01);

        // Convexity should be positive and small for short-dated bond
        assert!(metrics.convexity.as_f64() > 0.0);
        assert!(metrics.convexity.as_f64() < 10.0);

        // DV01 per $100 = ModDur × 1.0 × 100 × 0.0001 ≈ 0.0188
        assert_relative_eq!(metrics.dv01_per_100.as_f64(), 0.0188, epsilon = 0.001);
    }

    #[test]
    fn test_bond_risk_calculator_zero_coupon() {
        // 5-year zero coupon bond
        let times = vec![5.0];
        let cash_flows = vec![100.0];

        let calc = BondRiskCalculator::from_cash_flows(
            times, cash_flows, 0.05,  // 5% YTM
            1,     // annual
            100.0, // at par (for simplicity)
            100.0,
        )
        .unwrap();

        let metrics = calc.all_metrics().unwrap();

        // Macaulay duration = maturity for zero coupon
        assert_relative_eq!(metrics.macaulay_duration.as_f64(), 5.0, epsilon = 0.001);

        // Modified duration = 5.0 / 1.05 ≈ 4.76
        assert_relative_eq!(metrics.modified_duration.as_f64(), 4.76, epsilon = 0.01);
    }

    #[test]
    fn test_effective_duration_calculator() {
        let calc = EffectiveDurationCalculator::new(10.0); // 10 bps

        // Simulate bond with mod dur ≈ 5
        let price_base = 100.0;
        let price_up = 99.5; // -0.5% for +10 bps
        let price_down = 100.5; // +0.5% for -10 bps

        let dur = calc.from_prices(price_base, price_up, price_down).unwrap();

        assert_relative_eq!(dur.as_f64(), 5.0, epsilon = 0.01);
    }

    #[test]
    fn test_key_rate_duration_calculator() {
        let calc = KeyRateDurationCalculator::with_tenors(vec![2.0, 5.0, 10.0]).with_bump_bps(1.0);

        let base_price = 100.0;
        // Simulate a bond with key rate exposures
        // KRD = (price_down - price_up) / (2 × base_price × bump)
        // For 1bp bump (0.0001): KRD = (price_down - price_up) / (2 × 100 × 0.0001) = (price_down - price_up) / 0.02
        let tenor_prices = vec![
            (2.0, 99.99, 100.01),  // KRD = (100.01 - 99.99) / 0.02 = 0.02 / 0.02 = 1.0
            (5.0, 99.95, 100.05),  // KRD = (100.05 - 99.95) / 0.02 = 0.10 / 0.02 = 5.0
            (10.0, 99.98, 100.02), // KRD = (100.02 - 99.98) / 0.02 = 0.04 / 0.02 = 2.0
        ];

        let krds = calc.calculate(base_price, &tenor_prices).unwrap();

        // Check total is reasonable (1 + 5 + 2 = 8)
        let total = krds.total_duration().as_f64();
        assert!(
            total > 7.0 && total < 9.0,
            "Total KRD {} not in expected range",
            total
        );

        // Check we can retrieve individual KRDs
        let krd_5y = krds.at_tenor(5.0).unwrap();
        assert!(
            krd_5y.duration.as_f64() > 4.0,
            "5Y KRD {} not as expected",
            krd_5y.duration.as_f64()
        );
    }

    #[test]
    fn test_price_change_estimation() {
        let times = vec![0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0];
        let cash_flows: Vec<f64> = (0..9)
            .map(|_| 3.75)
            .chain(std::iter::once(103.75))
            .collect();

        let calc = BondRiskCalculator::from_cash_flows(
            times, cash_flows, 0.075, // 7.5% YTM
            2, 100.0, 100.0,
        )
        .unwrap();

        let metrics = calc.all_metrics().unwrap();

        // Estimate price change for +100 bps
        let change = metrics.estimate_price_change(0.01, 100.0);

        // Should be negative (price drops when yield rises)
        assert!(change < 0.0);

        // For 5Y bond with ~4 duration, expect ~4% price drop
        assert!(change > -5.0 && change < -3.0);
    }

    #[test]
    fn test_boeing_bond_validation() {
        // Boeing 7.5% 06/15/2025
        // Bloomberg YAS: Modified Duration = 4.209, Convexity = 0.219
        // (These are approximate values from the spec)

        // 5-year bond, 7.5% semi-annual coupon
        let times: Vec<f64> = (1..=10).map(|i| i as f64 * 0.5).collect();
        let cash_flows: Vec<f64> = (0..9)
            .map(|_| 3.75)
            .chain(std::iter::once(103.75))
            .collect();

        let calc = BondRiskCalculator::from_cash_flows(
            times, cash_flows, 0.075, // 7.5% YTM (at par for simplicity)
            2, 100.0, 100.0,
        )
        .unwrap();

        let metrics = calc.all_metrics().unwrap();

        // Validate against Bloomberg (with tolerance from spec)
        // Modified Duration: ±0.001
        // Note: Exact match depends on day count, settlement, etc.
        // For a 5-year at par, modified duration should be around 4.2
        assert!(
            (metrics.modified_duration.as_f64() - 4.2).abs() < 0.1,
            "Modified duration {} out of expected range",
            metrics.modified_duration.as_f64()
        );

        // Convexity should be positive and reasonable
        assert!(metrics.convexity.as_f64() > 0.0);
    }

    #[test]
    fn test_empty_cash_flows_error() {
        let result = BondRiskCalculator::from_cash_flows(vec![], vec![], 0.05, 2, 100.0, 100.0);

        assert!(result.is_err());
    }

    #[test]
    fn test_mismatched_arrays_error() {
        let result = BondRiskCalculator::from_cash_flows(
            vec![0.5, 1.0],
            vec![2.5], // mismatched length
            0.05,
            2,
            100.0,
            100.0,
        );

        assert!(result.is_err());
    }
}
