//! `BondRiskCalculator`: duration, convexity, DV01 for a single bond.

use convex_bonds::pricing::risk_math;
use rust_decimal::prelude::*;
use std::collections::HashMap;

use convex_bonds::{
    pricing::{dated_cash::DatedCashYieldRisk, discount_time::PreparedDatedCash},
    traits::Bond,
};
use convex_core::types::{Compounding, Date, Frequency};

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::risk::convexity::{effective_convexity, Convexity};
use crate::risk::duration::{
    effective_duration, key_rate_duration_at_tenor, Duration, KeyRateDurations,
    STANDARD_KEY_RATE_TENORS,
};
use crate::risk::dv01::DV01;

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
    pub fn estimate_price_change(
        &self,
        yield_change: f64,
        dirty_price: f64,
    ) -> AnalyticsResult<f64> {
        risk_math::price_change(
            self.modified_duration.as_f64(),
            self.convexity.as_f64(),
            dirty_price,
            yield_change,
        )
        .map_err(Into::into)
    }

    /// Get the convexity adjustment factor.
    ///
    /// The convexity adjustment = (1/2) × C × (Δy)²
    pub fn convexity_adjustment(&self, yield_change: f64) -> AnalyticsResult<f64> {
        risk_math::convexity_adjustment(self.convexity.as_f64(), yield_change).map_err(Into::into)
    }
}

/// Calculator for bond risk metrics.
pub struct BondRiskCalculator {
    risk: DatedCashYieldRisk,
    ytm: f64,
    dirty_price: f64,
    face_value: f64,
}

impl BondRiskCalculator {
    /// Prepare original normalized bond cash and its complete native time context.
    /// `quote_frequency` is the actual periodic quotation basis, independent of coupons.
    pub fn from_bond(
        bond: &dyn Bond,
        settlement: Date,
        dirty_price: f64,
        ytm: f64,
        quote_frequency: Frequency,
    ) -> AnalyticsResult<Self> {
        let maturity = bond
            .maturity()
            .ok_or_else(|| AnalyticsError::InvalidInput("bond has no maturity".into()))?;
        if settlement < bond.issue_date() || settlement >= maturity {
            return Err(AnalyticsError::InvalidSettlement {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }
        let cash = bond.yield_cash_flows(settlement, quote_frequency)?;
        let face = bond
            .face_value()
            .to_f64()
            .ok_or_else(|| AnalyticsError::InvalidInput("invalid face".into()))?;
        Self::from_prepared_cash(
            &cash,
            ytm,
            Compounding::from(quote_frequency),
            dirty_price,
            face,
        )
    }

    /// Reuse full supplied dated context for periodic, continuous or Simple risk.
    /// Amounts use per100quote units; `face_value` scales the reported position DV01.
    pub fn from_prepared_cash(
        cash: &PreparedDatedCash,
        ytm: f64,
        compounding: Compounding,
        dirty_price: f64,
        face_value: f64,
    ) -> AnalyticsResult<Self> {
        Self::from_risk(
            cash.yield_risk(ytm, compounding)?,
            ytm,
            dirty_price,
            face_value,
        )
    }

    /// Explicit positive numerical times and original per100cash, without inferred dates.
    /// Complete cash owns derivatives; dirty price is the caller's DV01 price basis.
    /// Finite signed face preserves long/short and zero position scaling.
    pub fn from_cash_flows(
        times: Vec<f64>,
        cash_flows: Vec<f64>,
        ytm: f64,
        compounding: Compounding,
        dirty_price: f64,
        face_value: f64,
    ) -> AnalyticsResult<Self> {
        Self::from_risk(
            super::cash::evaluate(&times, &cash_flows, ytm, compounding)?,
            ytm,
            dirty_price,
            face_value,
        )
    }

    fn from_risk(
        risk: DatedCashYieldRisk,
        ytm: f64,
        dirty_price: f64,
        face_value: f64,
    ) -> AnalyticsResult<Self> {
        if !ytm.is_finite()
            || !dirty_price.is_finite()
            || dirty_price <= 0.0
            || !face_value.is_finite()
        {
            return Err(AnalyticsError::InvalidInput(
                "finite yield/face and positive finite DV01 price required".into(),
            ));
        }
        Ok(Self {
            risk,
            ytm,
            dirty_price,
            face_value,
        })
    }

    /// Same complete-cash PV-weighted time in years.
    pub fn macaulay_duration(&self) -> AnalyticsResult<Duration> {
        Ok(Duration::new(super::cash::decimal(
            self.risk.macaulay_duration,
        )?))
    }

    /// Minus the complete first derivative divided by actual cash PV.
    pub fn modified_duration(&self) -> AnalyticsResult<Duration> {
        Ok(Duration::new(super::cash::decimal(
            self.risk.modified_duration,
        )?))
    }

    /// Complete second derivative divided by actual cash PV.
    pub fn convexity(&self) -> AnalyticsResult<Convexity> {
        Ok(Convexity::new(super::cash::decimal(self.risk.convexity)?))
    }

    fn per100_dv01(&self) -> AnalyticsResult<f64> {
        risk_math::dv01(self.risk.modified_duration, self.dirty_price, 100.0).map_err(Into::into)
    }

    /// First-order loss at the supplied dirty price for +1bp annual yield.
    pub fn dv01_per_100(&self) -> AnalyticsResult<DV01> {
        Ok(DV01::new(super::cash::decimal(self.per100_dv01()?)?))
    }

    /// Same supplied-price DV01 scaled by the actual signed face exposure.
    pub fn dv01(&self) -> AnalyticsResult<DV01> {
        Ok(DV01::new(super::cash::decimal(risk_math::dv01(
            self.risk.modified_duration,
            self.dirty_price,
            self.face_value,
        )?)?))
    }

    /// One checked cash evaluation owns all measures, including Simple derivatives.
    pub fn all_metrics(&self) -> AnalyticsResult<BondRiskMetrics> {
        Ok(BondRiskMetrics {
            macaulay_duration: self.macaulay_duration()?,
            modified_duration: self.modified_duration()?,
            convexity: self.convexity()?,
            dv01_per_100: self.dv01_per_100()?,
            dv01: self.dv01()?,
        })
    }

    /// Actual supplied annual decimal yield.
    pub fn ytm(&self) -> f64 {
        self.ytm
    }
    /// Actual supplied per100DV01 price basis.
    pub fn dirty_price(&self) -> f64 {
        self.dirty_price
    }
    /// Actual signed face exposure used for position scaling.
    pub fn face_value(&self) -> f64 {
        self.face_value
    }
}

fn checked_bump(bps: f64, minimum: f64) -> AnalyticsResult<f64> {
    let bump = bps / 10_000.0;
    if !bps.is_finite() || !bump.is_finite() || bump == 0.0 || bump.abs() < minimum {
        return Err(AnalyticsError::InvalidInput(
            "finite representable nonzero bump required".into(),
        ));
    }
    Ok(bump)
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
        Self { bump_size: 0.001 }
    }
}

impl EffectiveDurationCalculator {
    /// Creates a new effective duration calculator.
    ///
    /// # Arguments
    ///
    /// * `bump_bps` - Bump size in basis points (default: 10)
    pub fn new(bump_bps: f64) -> AnalyticsResult<Self> {
        let bump_size = checked_bump(bump_bps, 1e-12)?;
        Ok(Self { bump_size })
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
    ) -> AnalyticsResult<Duration> {
        effective_duration(price_up, price_down, price_base, self.bump_size)
    }

    /// Calculates effective convexity from pre-computed prices.
    pub fn convexity_from_prices(
        &self,
        price_base: f64,
        price_up: f64,
        price_down: f64,
    ) -> AnalyticsResult<Convexity> {
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
    pub fn with_tenors(tenors: Vec<f64>) -> AnalyticsResult<Self> {
        KeyRateDurations::validate_tenors(&tenors)?;
        Ok(Self {
            tenors,
            bump_size: 0.0001,
        })
    }

    /// Sets the bump size in basis points.
    pub fn with_bump_bps(mut self, bps: f64) -> AnalyticsResult<Self> {
        self.bump_size = checked_bump(bps, 0.0)?;
        Ok(self)
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
    ) -> AnalyticsResult<KeyRateDurations> {
        if base_price.abs() < 1e-10 {
            return Err(AnalyticsError::CalculationFailed(
                "base price is zero".to_string(),
            ));
        }

        KeyRateDurations::validate_tenors(&self.tenors)?;
        if tenor_prices.len() != self.tenors.len() {
            return Err(AnalyticsError::InvalidInput(
                "exact declared key rate prices required".into(),
            ));
        }
        let mut supplied = HashMap::with_capacity(tenor_prices.len());
        for &(tenor, up, down) in tenor_prices {
            if !tenor.is_finite()
                || tenor <= 0.0
                || supplied.insert(tenor.to_bits(), (up, down)).is_some()
            {
                return Err(AnalyticsError::InvalidInput(
                    "finite positive unique supplied key rate tenors required".into(),
                ));
            }
        }
        let durations = self
            .tenors
            .iter()
            .map(|tenor| {
                let (up, down) = supplied.remove(&tenor.to_bits()).ok_or_else(|| {
                    AnalyticsError::InvalidInput(
                        "supplied key rate tenor does not match declaration".into(),
                    )
                })?;
                key_rate_duration_at_tenor(up, down, base_price, self.bump_size, *tenor)
            })
            .collect::<AnalyticsResult<Vec<_>>>()?;
        Ok(KeyRateDurations::new(durations))
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
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];

        let calc = BondRiskCalculator::from_cash_flows(
            times,
            cash_flows,
            0.05,
            Compounding::SemiAnnual,
            100.0,
            100.0,
        )
        .unwrap();

        let metrics = calc.all_metrics().unwrap();
        assert_relative_eq!(metrics.macaulay_duration.as_f64(), 1.93, epsilon = 0.01);
        assert_relative_eq!(metrics.modified_duration.as_f64(), 1.88, epsilon = 0.01);
        assert!(metrics.convexity.as_f64() > 0.0 && metrics.convexity.as_f64() < 10.0);
        assert_relative_eq!(metrics.dv01_per_100.as_f64(), 0.0188, epsilon = 0.001);
    }

    #[test]
    fn test_bond_risk_calculator_zero_coupon() {
        let calc = BondRiskCalculator::from_cash_flows(
            vec![5.0],
            vec![100.0],
            0.05,
            Compounding::Annual,
            100.0,
            100.0,
        )
        .unwrap();

        let metrics = calc.all_metrics().unwrap();
        assert_relative_eq!(metrics.macaulay_duration.as_f64(), 5.0, epsilon = 0.001);
        assert_relative_eq!(metrics.modified_duration.as_f64(), 4.76, epsilon = 0.01);
    }

    #[test]
    fn test_continuous_compounding_skips_periodic_divisor() {
        let times = vec![10.0];
        let cfs = vec![100.0];
        let periodic = BondRiskCalculator::from_cash_flows(
            times.clone(),
            cfs.clone(),
            0.05,
            Compounding::SemiAnnual,
            100.0,
            100.0,
        )
        .unwrap();
        let continuous = BondRiskCalculator::from_cash_flows(
            times,
            cfs,
            0.05,
            Compounding::Continuous,
            100.0,
            100.0,
        )
        .unwrap();

        let m_periodic = periodic.modified_duration().unwrap().as_f64();
        let m_continuous = continuous.modified_duration().unwrap().as_f64();
        let mac = continuous.macaulay_duration().unwrap().as_f64();

        assert_relative_eq!(m_continuous, mac, epsilon = 1e-10);
        assert!(m_periodic < m_continuous - 0.1);
    }

    #[test]
    fn test_effective_duration_calculator() {
        let calc = EffectiveDurationCalculator::new(10.0).unwrap(); // 10 bps

        // Simulate bond with mod dur ≈ 5
        let price_base = 100.0;
        let price_up = 99.5; // -0.5% for +10 bps
        let price_down = 100.5; // +0.5% for -10 bps

        let dur = calc.from_prices(price_base, price_up, price_down).unwrap();

        assert_relative_eq!(dur.as_f64(), 5.0, epsilon = 0.01);
    }

    #[test]
    fn test_key_rate_duration_calculator() {
        let calc = KeyRateDurationCalculator::with_tenors(vec![2.0, 5.0, 10.0])
            .unwrap()
            .with_bump_bps(1.0)
            .unwrap();

        let base_price = 100.0;
        let tenor_prices = vec![
            (2.0, 99.99, 100.01),
            (5.0, 99.95, 100.05),
            (10.0, 99.98, 100.02),
        ];

        let krds = calc.calculate(base_price, &tenor_prices).unwrap();

        // Check total is reasonable
        let total = krds.total_duration().unwrap().as_f64();
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
            times,
            cash_flows,
            0.075,
            Compounding::SemiAnnual,
            100.0,
            100.0,
        )
        .unwrap();

        let metrics = calc.all_metrics().unwrap();

        // Estimate price change for +100 bps
        let change = metrics.estimate_price_change(0.01, 100.0).unwrap();

        // Should be negative (price drops when yield rises)
        assert!(change < 0.0);

        // For 5Y bond with ~4 duration, expect ~4% price drop
        assert!(change > -5.0 && change < -3.0);
    }

    #[test]
    fn test_empty_cash_flows_error() {
        assert!(BondRiskCalculator::from_cash_flows(
            vec![],
            vec![],
            0.05,
            Compounding::SemiAnnual,
            100.0,
            100.0,
        )
        .is_err());
    }

    #[test]
    fn test_mismatched_arrays_error() {
        assert!(BondRiskCalculator::from_cash_flows(
            vec![0.5, 1.0],
            vec![2.5],
            0.05,
            Compounding::SemiAnnual,
            100.0,
            100.0,
        )
        .is_err());
    }
}
