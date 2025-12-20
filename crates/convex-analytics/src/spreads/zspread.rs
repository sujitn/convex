//! Z-spread (zero-volatility spread) calculator.
//!
//! The Z-spread is the constant spread that, when added to the spot rate curve,
//! makes the present value of a bond's cash flows equal to its market price.

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_bonds::traits::{Bond, FixedCouponBond};
use convex_core::types::{Date, Spread, SpreadType};
use convex_curves::traits::Curve;
use convex_math::solvers::{brent, SolverConfig};

use crate::error::{AnalyticsError, AnalyticsResult};

/// Z-spread calculator for fixed rate bonds.
///
/// The Z-spread is found by solving:
/// ```text
/// Dirty Price = Σ CF_i × DF(t_i) × exp(-Z × t_i)
/// ```
///
/// Where:
/// - CF_i = cash flow at time t_i
/// - DF(t_i) = discount factor from the spot curve at time t_i
/// - Z = Z-spread (constant)
pub struct ZSpreadCalculator<'a> {
    /// Reference to the spot/zero curve.
    curve: &'a dyn Curve,
    /// Solver configuration.
    config: SolverConfig,
}

impl std::fmt::Debug for ZSpreadCalculator<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZSpreadCalculator")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl<'a> ZSpreadCalculator<'a> {
    /// Creates a new Z-spread calculator.
    ///
    /// # Arguments
    ///
    /// * `curve` - The spot rate/discount curve
    #[must_use]
    pub fn new(curve: &'a dyn Curve) -> Self {
        Self {
            curve,
            config: SolverConfig::new(1e-10, 100),
        }
    }

    /// Sets the solver tolerance.
    ///
    /// Default tolerance is 1e-10 (0.0001 basis points).
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.config = SolverConfig::new(tolerance, self.config.max_iterations);
        self
    }

    /// Sets the maximum iterations for the solver.
    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.config = SolverConfig::new(self.config.tolerance, max_iterations);
        self
    }

    /// Calculates the Z-spread for a fixed rate bond.
    ///
    /// # Arguments
    ///
    /// * `bond` - The fixed rate bond
    /// * `dirty_price` - Market dirty price (as percentage of par, e.g., 100.50)
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The Z-spread as a `Spread` in basis points.
    ///
    /// # Errors
    ///
    /// Returns `AnalyticsError` if:
    /// - Settlement is at or after maturity
    /// - The solver fails to converge
    /// - No future cash flows exist
    pub fn calculate<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        dirty_price: Decimal,
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

        let target_price = dirty_price.to_f64().unwrap_or(100.0);

        // Get cash flows after settlement
        let cash_flows = bond.cash_flows(settlement);
        if cash_flows.is_empty() {
            return Err(AnalyticsError::InvalidInput(
                "No cash flows after settlement".to_string(),
            ));
        }

        let ref_date = self.curve.reference_date();

        // Objective function: price(z) - target = 0
        let objective = |z: f64| {
            let mut pv = 0.0;
            for cf in &cash_flows {
                if cf.date <= settlement {
                    continue;
                }
                let t = ref_date.days_between(&cf.date) as f64 / 365.0;
                let df = self.curve.discount_factor(t).unwrap_or(1.0);
                let cf_amount = cf.amount.to_f64().unwrap_or(0.0);
                // Adjust DF by Z-spread: DF_adj = DF × exp(-z × t)
                pv += cf_amount * df * (-z * t).exp();
            }
            // Normalize to percentage of face value
            let face = bond.face_value().to_f64().unwrap_or(100.0);
            pv / face * 100.0 - target_price
        };

        // Search for Z-spread between -5% and +20%
        let result = brent(objective, -0.05, 0.20, &self.config).map_err(|_| {
            AnalyticsError::SolverConvergenceFailed {
                solver: "Z-spread Brent".to_string(),
                iterations: self.config.max_iterations,
                residual: 0.0,
            }
        })?;

        // Convert to basis points
        let z_spread_bps = (result.root * 10_000.0).round();
        Ok(Spread::new(
            Decimal::from_f64_retain(z_spread_bps).unwrap_or_default(),
            SpreadType::ZSpread,
        ))
    }

    /// Prices a bond given a Z-spread.
    ///
    /// # Arguments
    ///
    /// * `bond` - The fixed rate bond
    /// * `z_spread` - Z-spread as a decimal (e.g., 0.0050 for 50 bps)
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The dirty price as a percentage of par.
    pub fn price_with_spread<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        z_spread: f64,
        settlement: Date,
    ) -> f64 {
        let Some(maturity) = bond.maturity() else {
            return 0.0;
        };

        if settlement >= maturity {
            return 0.0;
        }

        let cash_flows = bond.cash_flows(settlement);
        let ref_date = self.curve.reference_date();

        let mut pv = 0.0;
        for cf in &cash_flows {
            if cf.date <= settlement {
                continue;
            }
            let t = ref_date.days_between(&cf.date) as f64 / 365.0;
            let df = self.curve.discount_factor(t).unwrap_or(1.0);
            let cf_amount = cf.amount.to_f64().unwrap_or(0.0);
            pv += cf_amount * df * (-z_spread * t).exp();
        }

        let face = bond.face_value().to_f64().unwrap_or(100.0);
        pv / face * 100.0
    }

    /// Calculates the spread DV01 (price sensitivity to 1bp spread change).
    ///
    /// # Arguments
    ///
    /// * `bond` - The fixed rate bond
    /// * `z_spread` - Current Z-spread
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The price change for a 1 basis point increase in spread.
    pub fn spread_dv01<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        z_spread: Spread,
        settlement: Date,
    ) -> Decimal {
        let base_spread = z_spread.as_decimal().to_f64().unwrap_or(0.0) / 10_000.0;

        let base_price = self.price_with_spread(bond, base_spread, settlement);
        let bumped_price = self.price_with_spread(bond, base_spread + 0.0001, settlement);

        // DV01 is positive (price decreases when spread increases)
        Decimal::from_f64_retain(base_price - bumped_price).unwrap_or(Decimal::ZERO)
    }

    /// Calculates the Z-spread from pre-computed cash flows.
    ///
    /// This is useful when you already have the cash flows and don't need
    /// to recompute them from the bond.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - Pre-computed bond cash flows
    /// * `dirty_price` - Market dirty price (as percentage of par)
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The Z-spread as a `Spread` in basis points.
    ///
    /// # Errors
    ///
    /// Returns `AnalyticsError` if:
    /// - No future cash flows exist
    /// - The solver fails to converge
    pub fn calculate_from_cash_flows(
        &self,
        cash_flows: &[convex_bonds::traits::BondCashFlow],
        dirty_price: Decimal,
        settlement: Date,
    ) -> AnalyticsResult<Spread> {
        let target = dirty_price.to_f64().unwrap_or(100.0);

        // Convert cash flows to (time, amount) pairs
        let cf_data: Vec<(f64, f64)> = cash_flows
            .iter()
            .filter(|cf| cf.date > settlement)
            .map(|cf| {
                let t = settlement.days_between(&cf.date) as f64 / 365.0;
                let amount = cf.amount.to_f64().unwrap_or(0.0);
                (t, amount)
            })
            .collect();

        if cf_data.is_empty() {
            return Err(AnalyticsError::InvalidInput(
                "No cash flows after settlement".to_string(),
            ));
        }

        let ref_date = self.curve.reference_date();

        // Objective function: PV(z) - target = 0
        let objective = |z: f64| {
            let mut pv = 0.0;
            for (t, amount) in &cf_data {
                // Get discount factor from curve
                let curve_t =
                    ref_date.days_between(&settlement.add_days((*t * 365.0) as i64)) as f64 / 365.0;
                let df = self.curve.discount_factor(curve_t).unwrap_or(1.0);
                // Adjust for z-spread
                pv += amount * df * (-z * t).exp();
            }
            pv - target
        };

        // Search for Z-spread between -5% and +20%
        let result = brent(objective, -0.05, 0.20, &self.config).map_err(|_| {
            AnalyticsError::SolverConvergenceFailed {
                solver: "Z-spread Brent".to_string(),
                iterations: self.config.max_iterations,
                residual: 0.0,
            }
        })?;

        // Convert to basis points
        let z_spread_bps = (result.root * 10_000.0).round();
        Ok(Spread::new(
            Decimal::from_f64_retain(z_spread_bps).unwrap_or_default(),
            SpreadType::ZSpread,
        ))
    }
}

/// Convenience function to calculate Z-spread.
///
/// # Arguments
///
/// * `bond` - The fixed rate bond
/// * `dirty_price` - Market dirty price
/// * `curve` - Spot rate curve
/// * `settlement` - Settlement date
///
/// # Returns
///
/// Z-spread in basis points.
pub fn z_spread<B: Bond + FixedCouponBond>(
    bond: &B,
    dirty_price: Decimal,
    curve: &dyn Curve,
    settlement: Date,
) -> AnalyticsResult<Spread> {
    ZSpreadCalculator::new(curve).calculate(bond, dirty_price, settlement)
}

/// Calculate Z-spread from a pre-built curve.
///
/// This is a convenience wrapper that uses default solver settings.
pub fn z_spread_from_curve<B: Bond + FixedCouponBond>(
    bond: &B,
    dirty_price: Decimal,
    curve: &dyn Curve,
    settlement: Date,
) -> AnalyticsResult<Spread> {
    z_spread(bond, dirty_price, curve, settlement)
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_curves::curves::DiscountCurveBuilder;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_flat_curve(rate: f64) -> impl Curve {
        let ref_date = date(2024, 1, 15);
        DiscountCurveBuilder::new(ref_date)
            .add_pillar(0.5, (-rate * 0.5).exp())
            .add_pillar(1.0, (-rate * 1.0).exp())
            .add_pillar(2.0, (-rate * 2.0).exp())
            .add_pillar(5.0, (-rate * 5.0).exp())
            .add_pillar(10.0, (-rate * 10.0).exp())
            .with_extrapolation()
            .build()
            .unwrap()
    }

    // Mock bond for testing
    struct MockBond {
        maturity: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        calendar: convex_bonds::types::CalendarId,
    }

    impl MockBond {
        fn new(maturity: Date, coupon_rate: Decimal) -> Self {
            Self {
                maturity,
                coupon_rate,
                face_value: dec!(100),
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
            self.face_value
        }

        fn frequency(&self) -> convex_core::types::Frequency {
            convex_core::types::Frequency::SemiAnnual
        }

        fn cash_flows(&self, from: Date) -> Vec<convex_bonds::traits::BondCashFlow> {
            use convex_bonds::traits::{BondCashFlow, CashFlowType};

            let mut cfs = Vec::new();
            let semi_annual_coupon = self.face_value * self.coupon_rate / dec!(2);

            // Generate semi-annual coupons
            let mut cf_date = date(2024, 7, 15);
            while cf_date <= self.maturity && cf_date > from {
                let cf_type = if cf_date == self.maturity {
                    CashFlowType::Principal
                } else {
                    CashFlowType::Coupon
                };

                let amount = if cf_type == CashFlowType::Principal {
                    semi_annual_coupon + self.face_value
                } else {
                    semi_annual_coupon
                };

                cfs.push(BondCashFlow {
                    date: cf_date,
                    amount,
                    flow_type: cf_type,
                    accrual_start: None,
                    accrual_end: None,
                    factor: Decimal::ONE,
                    reference_rate: None,
                });

                // Move to next coupon date
                cf_date = cf_date.add_months(6).unwrap();
            }

            cfs
        }

        fn next_coupon_date(&self, _after: Date) -> Option<Date> {
            Some(date(2024, 7, 15))
        }

        fn previous_coupon_date(&self, _before: Date) -> Option<Date> {
            Some(date(2024, 1, 15))
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
            self.coupon_rate
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
    fn test_calculator_creation() {
        let curve = create_flat_curve(0.05);
        let calc = ZSpreadCalculator::new(&curve);
        let _ = calc.with_tolerance(1e-8).with_max_iterations(50);
    }

    #[test]
    fn test_z_spread_at_par() {
        let curve = create_flat_curve(0.05);
        let calc = ZSpreadCalculator::new(&curve);

        let bond = MockBond::new(date(2029, 1, 15), dec!(0.05));
        let settlement = date(2024, 1, 17);

        // Price the bond at zero spread
        let price_at_zero = calc.price_with_spread(&bond, 0.0, settlement);

        // Calculate spread from that price
        let spread = calc
            .calculate(
                &bond,
                Decimal::from_f64_retain(price_at_zero).unwrap(),
                settlement,
            )
            .unwrap();

        // Should be close to zero
        assert!(
            spread.as_bps().abs() < dec!(5),
            "Expected near-zero spread, got {}",
            spread.as_bps()
        );
    }

    #[test]
    fn test_z_spread_roundtrip() {
        let curve = create_flat_curve(0.05);
        let calc = ZSpreadCalculator::new(&curve);

        let bond = MockBond::new(date(2029, 1, 15), dec!(0.05));
        let settlement = date(2024, 1, 17);

        // Price at 50 bps spread
        let price_at_50bps = calc.price_with_spread(&bond, 0.005, settlement);

        // Calculate spread from that price
        let spread = calc
            .calculate(
                &bond,
                Decimal::from_f64_retain(price_at_50bps).unwrap(),
                settlement,
            )
            .unwrap();

        // Should be close to 50 bps
        let diff = (spread.as_bps() - dec!(50)).abs();
        assert!(
            diff < dec!(1),
            "Expected ~50 bps, got {} bps",
            spread.as_bps()
        );
    }

    #[test]
    fn test_spread_dv01() {
        let curve = create_flat_curve(0.05);
        let calc = ZSpreadCalculator::new(&curve);

        let bond = MockBond::new(date(2029, 1, 15), dec!(0.05));
        let settlement = date(2024, 1, 17);
        let z = Spread::new(dec!(50), SpreadType::ZSpread);

        let dv01 = calc.spread_dv01(&bond, z, settlement);

        // DV01 should be positive
        assert!(dv01 > Decimal::ZERO, "DV01 should be positive");
    }

    #[test]
    fn test_settlement_after_maturity() {
        let curve = create_flat_curve(0.05);
        let calc = ZSpreadCalculator::new(&curve);

        let bond = MockBond::new(date(2024, 1, 15), dec!(0.05));
        let settlement = date(2024, 6, 15); // After maturity

        let result = calc.calculate(&bond, dec!(100), settlement);
        assert!(result.is_err());
    }
}
