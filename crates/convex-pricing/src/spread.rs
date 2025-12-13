//! Generic spread solving.
//!
//! This module provides [`GenericSpreadSolver`] which implements the
//! [`SpreadSolver`] trait for finding spreads over a discount curve.

use rust_decimal::Decimal;

use convex_core::error::ConvexResult;
use convex_core::traits::SpreadSolver;
use convex_core::types::{CashFlow, Date};
use convex_curves::traits::Curve;
use convex_math::solvers::{brent, ConfigurableCalculator, SolverConfig};

use crate::error::{PricingError, PricingResult};
use crate::pricer::CurvePricer;

/// Default lower bound for spread search (-500 bps).
pub const DEFAULT_SPREAD_LOWER_BOUND: f64 = -0.05;

/// Default upper bound for spread search (+2000 bps).
pub const DEFAULT_SPREAD_UPPER_BOUND: f64 = 0.20;

/// Generic spread solver for finding constant spreads over a curve.
///
/// This solver finds the spread `z` such that:
/// ```text
/// Σ CF(i) * DF(t_i) * exp(-z * t_i) = TargetPrice
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use convex_pricing::GenericSpreadSolver;
/// use convex_core::traits::SpreadSolver;
///
/// let solver = GenericSpreadSolver::new(&discount_curve)
///     .with_tolerance(1e-10)
///     .with_bounds(-0.05, 0.20);
///
/// let z_spread = solver.solve_spread(&cash_flows, dirty_price, settlement)?;
/// println!("Z-spread: {:.2} bps", z_spread * 10000.0);
/// ```
pub struct GenericSpreadSolver<'a, C: Curve + ?Sized> {
    curve: &'a C,
    config: SolverConfig,
    lower_bound: f64,
    upper_bound: f64,
}

impl<'a, C: Curve + ?Sized> GenericSpreadSolver<'a, C> {
    /// Creates a new spread solver with the given curve.
    pub fn new(curve: &'a C) -> Self {
        Self {
            curve,
            config: SolverConfig::default(),
            lower_bound: DEFAULT_SPREAD_LOWER_BOUND,
            upper_bound: DEFAULT_SPREAD_UPPER_BOUND,
        }
    }

    /// Sets the search bounds for the spread.
    ///
    /// # Arguments
    ///
    /// * `lower` - Lower bound (e.g., -0.05 for -500 bps)
    /// * `upper` - Upper bound (e.g., 0.20 for 2000 bps)
    #[must_use]
    pub fn with_bounds(mut self, lower: f64, upper: f64) -> Self {
        self.lower_bound = lower;
        self.upper_bound = upper;
        self
    }

    /// Returns the underlying curve.
    pub fn curve(&self) -> &C {
        self.curve
    }

    /// Returns the current bounds.
    pub fn bounds(&self) -> (f64, f64) {
        (self.lower_bound, self.upper_bound)
    }

    /// Calculates the present value with a given spread.
    ///
    /// This is the internal calculation used by the solver.
    pub fn price_with_spread(
        &self,
        cash_flows: &[CashFlow],
        spread: f64,
        settlement: Date,
    ) -> PricingResult<f64> {
        let pricer = CurvePricer::new(self.curve);
        pricer.present_value_with_spread_f64(cash_flows, spread, settlement)
    }

    /// Solves for the spread using internal pricing result type.
    pub fn solve_spread_internal(
        &self,
        cash_flows: &[CashFlow],
        target_price: f64,
        settlement: Date,
    ) -> PricingResult<f64> {
        // Extract cash flow data for the solver
        let pricer = CurvePricer::new(self.curve);
        let cf_data = pricer.extract_cash_flow_data(cash_flows, settlement)?;

        // Objective function: PV(spread) - target = 0
        let objective = |spread: f64| {
            let mut pv = 0.0;
            for (t, amount, spot_rate) in &cf_data {
                // DF = exp(-(r + spread) * t)
                let df = (-(spot_rate + spread) * t).exp();
                pv += amount * df;
            }
            pv - target_price
        };

        // Use Brent's method for robust convergence
        let result =
            brent(objective, self.lower_bound, self.upper_bound, &self.config).map_err(|_| {
                PricingError::SpreadNotConverged {
                    iterations: self.config.max_iterations,
                }
            })?;

        Ok(result.root)
    }

    /// Calculates the spread DV01 (price sensitivity to 1bp spread change).
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - The cash flows
    /// * `spread` - The current spread
    /// * `settlement` - The settlement date
    ///
    /// # Returns
    ///
    /// The price change for a 1 basis point increase in spread.
    pub fn spread_dv01(
        &self,
        cash_flows: &[CashFlow],
        spread: f64,
        settlement: Date,
    ) -> PricingResult<f64> {
        let bump = 0.0001; // 1 basis point

        let price_up = self.price_with_spread(cash_flows, spread + bump, settlement)?;
        let price_down = self.price_with_spread(cash_flows, spread - bump, settlement)?;

        // DV01 = (P_down - P_up) / 2
        Ok((price_down - price_up) / 2.0)
    }

    /// Calculates the spread duration.
    ///
    /// Spread duration = DV01 / Price * 10000
    ///
    /// This measures the percentage price change for a 1% change in spread.
    pub fn spread_duration(
        &self,
        cash_flows: &[CashFlow],
        spread: f64,
        settlement: Date,
    ) -> PricingResult<f64> {
        let price = self.price_with_spread(cash_flows, spread, settlement)?;
        let dv01 = self.spread_dv01(cash_flows, spread, settlement)?;

        if price.abs() < 1e-10 {
            return Ok(0.0);
        }

        Ok(dv01 / price * 10000.0)
    }
}

impl<C: Curve + ?Sized> ConfigurableCalculator for GenericSpreadSolver<'_, C> {
    fn solver_config(&self) -> &SolverConfig {
        &self.config
    }

    fn solver_config_mut(&mut self) -> &mut SolverConfig {
        &mut self.config
    }
}

impl<C: Curve + ?Sized> SpreadSolver for GenericSpreadSolver<'_, C> {
    fn solve_spread(
        &self,
        cash_flows: &[CashFlow],
        target_price: Decimal,
        settlement: Date,
    ) -> ConvexResult<f64> {
        let target = target_price.to_string().parse::<f64>().unwrap_or(100.0);

        self.solve_spread_internal(cash_flows, target, settlement)
            .map_err(|e| convex_core::error::ConvexError::pricing_error(e.to_string()))
    }

    fn solve_spread_bounded(
        &self,
        cash_flows: &[CashFlow],
        target_price: Decimal,
        settlement: Date,
        lower_bound: f64,
        upper_bound: f64,
    ) -> ConvexResult<f64> {
        let target = target_price.to_string().parse::<f64>().unwrap_or(100.0);

        // Extract cash flow data for the solver
        let pricer = CurvePricer::new(self.curve);
        let cf_data = pricer
            .extract_cash_flow_data(cash_flows, settlement)
            .map_err(|e| convex_core::error::ConvexError::pricing_error(e.to_string()))?;

        // Objective function
        let objective = |spread: f64| {
            let mut pv = 0.0;
            for (t, amount, spot_rate) in &cf_data {
                let df = (-(spot_rate + spread) * t).exp();
                pv += amount * df;
            }
            pv - target
        };

        let result = brent(objective, lower_bound, upper_bound, &self.config).map_err(|_| {
            convex_core::error::ConvexError::pricing_error("Spread solver did not converge")
        })?;

        Ok(result.root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::types::CashFlowType;
    use rust_decimal_macros::dec;

    /// A simple flat curve for testing
    struct FlatCurve {
        rate: f64,
        ref_date: Date,
    }

    impl FlatCurve {
        fn new(rate: f64, ref_date: Date) -> Self {
            Self { rate, ref_date }
        }
    }

    impl Curve for FlatCurve {
        fn discount_factor(&self, t: f64) -> convex_curves::error::CurveResult<f64> {
            Ok((-self.rate * t).exp())
        }

        fn reference_date(&self) -> Date {
            self.ref_date
        }

        fn max_date(&self) -> Date {
            self.ref_date.add_years(100).unwrap()
        }
    }

    fn create_test_cash_flows(settlement: Date) -> Vec<CashFlow> {
        vec![
            CashFlow::new(
                settlement.add_months(6).unwrap(),
                dec!(2.5),
                CashFlowType::Coupon,
            ),
            CashFlow::new(
                settlement.add_months(12).unwrap(),
                dec!(2.5),
                CashFlowType::Coupon,
            ),
            CashFlow::new(
                settlement.add_months(12).unwrap(),
                dec!(100.0),
                CashFlowType::Principal,
            ),
        ]
    }

    #[test]
    fn test_spread_solver_creation() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let solver = GenericSpreadSolver::new(&curve);

        assert_eq!(
            solver.bounds(),
            (DEFAULT_SPREAD_LOWER_BOUND, DEFAULT_SPREAD_UPPER_BOUND)
        );
    }

    #[test]
    fn test_price_with_spread() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let solver = GenericSpreadSolver::new(&curve);

        let cash_flows = create_test_cash_flows(settlement);

        let price_0 = solver
            .price_with_spread(&cash_flows, 0.0, settlement)
            .unwrap();
        let price_100bp = solver
            .price_with_spread(&cash_flows, 0.01, settlement)
            .unwrap();

        // Higher spread = lower price
        assert!(price_100bp < price_0);
    }

    #[test]
    fn test_solve_spread_roundtrip() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let solver = GenericSpreadSolver::new(&curve);

        let cash_flows = create_test_cash_flows(settlement);

        // Price at 100bp spread
        let target_spread = 0.01; // 100 bps
        let target_price = solver
            .price_with_spread(&cash_flows, target_spread, settlement)
            .unwrap();

        // Solve for spread
        let solved_spread = solver
            .solve_spread_internal(&cash_flows, target_price, settlement)
            .unwrap();

        // Should get back the original spread
        assert!((solved_spread - target_spread).abs() < 1e-6);
    }

    #[test]
    fn test_solve_spread_trait() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let solver = GenericSpreadSolver::new(&curve);

        let cash_flows = create_test_cash_flows(settlement);

        // Price at 50bp spread
        let target_spread = 0.005;
        let target_price = solver
            .price_with_spread(&cash_flows, target_spread, settlement)
            .unwrap();

        // Use trait method
        let solved_spread = solver
            .solve_spread(
                &cash_flows,
                Decimal::from_f64_retain(target_price).unwrap(),
                settlement,
            )
            .unwrap();

        assert!((solved_spread - target_spread).abs() < 1e-6);
    }

    #[test]
    fn test_spread_dv01() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let solver = GenericSpreadSolver::new(&curve);

        let cash_flows = create_test_cash_flows(settlement);

        let dv01 = solver.spread_dv01(&cash_flows, 0.01, settlement).unwrap();

        // DV01 should be positive (price decreases as spread increases)
        assert!(dv01 > 0.0);
    }

    #[test]
    fn test_spread_duration() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let solver = GenericSpreadSolver::new(&curve);

        let cash_flows = create_test_cash_flows(settlement);

        let duration = solver
            .spread_duration(&cash_flows, 0.01, settlement)
            .unwrap();

        // Duration should be positive and reasonable for a 1-year bond
        assert!(duration > 0.0);
        assert!(duration < 2.0); // Roughly 1 year
    }

    #[test]
    fn test_with_bounds() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let solver = GenericSpreadSolver::new(&curve).with_bounds(-0.10, 0.30);

        assert_eq!(solver.bounds(), (-0.10, 0.30));
    }

    #[test]
    fn test_configurable_calculator() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let solver = GenericSpreadSolver::new(&curve)
            .with_tolerance(1e-8)
            .with_max_iterations(50);

        assert!((solver.tolerance() - 1e-8).abs() < f64::EPSILON);
        assert_eq!(solver.max_iterations(), 50);
    }

    #[test]
    fn test_multiple_spread_roundtrips() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let solver = GenericSpreadSolver::new(&curve);

        let cash_flows = create_test_cash_flows(settlement);

        // Test various spreads
        for target_spread in &[-0.01, 0.0, 0.005, 0.01, 0.02, 0.05] {
            let target_price = solver
                .price_with_spread(&cash_flows, *target_spread, settlement)
                .unwrap();
            let solved_spread = solver
                .solve_spread_internal(&cash_flows, target_price, settlement)
                .unwrap();

            assert!(
                (solved_spread - target_spread).abs() < 1e-6,
                "Failed for spread {}: got {}",
                target_spread,
                solved_spread
            );
        }
    }
}
