//! Generic yield-to-maturity calculation.
//!
//! This module provides [`GenericYieldSolver`] which implements the
//! [`YieldSolver`] trait for finding yields from cash flows.

use rust_decimal::Decimal;

use convex_core::error::ConvexResult;
use convex_core::traits::YieldSolver;
use convex_core::types::{CashFlow, Date};
use convex_curves::traits::Curve;
use convex_math::solvers::{hybrid_numerical, ConfigurableCalculator, SolverConfig};

use crate::error::{PricingError, PricingResult};

/// Default lower bound for yield search (0%).
pub const DEFAULT_YIELD_LOWER_BOUND: f64 = -0.05;

/// Default upper bound for yield search (50%).
pub const DEFAULT_YIELD_UPPER_BOUND: f64 = 0.50;

/// Generic yield solver for finding yield-to-maturity.
///
/// This solver finds the yield `y` such that:
/// ```text
/// Σ CF(i) / (1 + y/f)^(f*t_i) = TargetPrice
/// ```
///
/// Where `f` is the compounding frequency (e.g., 2 for semi-annual).
///
/// # Example
///
/// ```rust,ignore
/// use convex_pricing::GenericYieldSolver;
/// use convex_core::traits::YieldSolver;
///
/// let solver = GenericYieldSolver::new()
///     .with_tolerance(1e-10);
///
/// // Solve for yield with semi-annual compounding
/// let ytm = solver.solve_yield(&cash_flows, dirty_price, settlement, 2)?;
/// println!("YTM: {:.4}%", ytm * 100.0);
/// ```
pub struct GenericYieldSolver {
    config: SolverConfig,
    lower_bound: f64,
    upper_bound: f64,
}

impl Default for GenericYieldSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl GenericYieldSolver {
    /// Creates a new yield solver.
    pub fn new() -> Self {
        Self {
            config: SolverConfig::default(),
            lower_bound: DEFAULT_YIELD_LOWER_BOUND,
            upper_bound: DEFAULT_YIELD_UPPER_BOUND,
        }
    }

    /// Sets the search bounds for the yield.
    ///
    /// # Arguments
    ///
    /// * `lower` - Lower bound (e.g., -0.05 for -5%)
    /// * `upper` - Upper bound (e.g., 0.50 for 50%)
    #[must_use]
    pub fn with_bounds(mut self, lower: f64, upper: f64) -> Self {
        self.lower_bound = lower;
        self.upper_bound = upper;
        self
    }

    /// Returns the current bounds.
    pub fn bounds(&self) -> (f64, f64) {
        (self.lower_bound, self.upper_bound)
    }

    /// Calculates the price given a yield (internal f64 version).
    ///
    /// Uses the standard bond pricing formula with periodic compounding.
    pub fn price_from_yield_f64(
        &self,
        cash_flows: &[CashFlow],
        yield_value: f64,
        settlement: Date,
        frequency: u32,
    ) -> PricingResult<f64> {
        if cash_flows.is_empty() {
            return Err(PricingError::NoCashFlows);
        }

        let f = frequency as f64;
        let mut pv = 0.0;
        let mut has_future_cf = false;

        for cf in cash_flows {
            let cf_date = cf.date();
            if cf_date <= settlement {
                continue;
            }

            has_future_cf = true;

            // Calculate time in years
            let t = settlement.days_between(&cf_date) as f64 / 365.0;

            // Discount factor: 1 / (1 + y/f)^(f*t)
            let df = 1.0 / (1.0 + yield_value / f).powf(f * t);
            let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);
            pv += amount * df;
        }

        if !has_future_cf {
            return Err(PricingError::no_future_cash_flows(settlement));
        }

        Ok(pv)
    }

    /// Solves for the yield (internal f64 version).
    pub fn solve_yield_f64(
        &self,
        cash_flows: &[CashFlow],
        target_price: f64,
        settlement: Date,
        frequency: u32,
    ) -> PricingResult<f64> {
        if cash_flows.is_empty() {
            return Err(PricingError::NoCashFlows);
        }

        // Extract cash flow data for efficiency
        let mut cf_data: Vec<(f64, f64)> = Vec::with_capacity(cash_flows.len());

        for cf in cash_flows {
            let cf_date = cf.date();
            if cf_date <= settlement {
                continue;
            }

            let t = settlement.days_between(&cf_date) as f64 / 365.0;
            let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);
            cf_data.push((t, amount));
        }

        if cf_data.is_empty() {
            return Err(PricingError::no_future_cash_flows(settlement));
        }

        let f = frequency as f64;

        // Objective function: PV(yield) - target = 0
        let objective = |y: f64| {
            let mut pv = 0.0;
            for (t, amount) in &cf_data {
                let df = 1.0 / (1.0 + y / f).powf(f * t);
                pv += amount * df;
            }
            pv - target_price
        };

        // Initial guess based on current yield approximation
        let total_cf: f64 = cf_data.iter().map(|(_, amt)| amt).sum();
        let initial_guess = if target_price > 0.0 && total_cf > target_price {
            (total_cf / target_price - 1.0) / cf_data.last().map(|(t, _)| *t).unwrap_or(1.0)
        } else {
            0.05
        }
        .clamp(self.lower_bound, self.upper_bound);

        // Use hybrid solver (Newton with Brent fallback)
        let result = hybrid_numerical(
            objective,
            initial_guess,
            Some((self.lower_bound, self.upper_bound)),
            &self.config,
        )
        .map_err(|_| PricingError::YieldNotConverged {
            iterations: self.config.max_iterations,
        })?;

        Ok(result.root)
    }

    /// Calculates the modified duration using yield.
    ///
    /// Modified Duration = Macaulay Duration / (1 + y/f)
    pub fn modified_duration(
        &self,
        cash_flows: &[CashFlow],
        yield_value: f64,
        settlement: Date,
        frequency: u32,
    ) -> PricingResult<f64> {
        let price = self.price_from_yield_f64(cash_flows, yield_value, settlement, frequency)?;

        if price.abs() < 1e-10 {
            return Ok(0.0);
        }

        let f = frequency as f64;
        let mut weighted_time = 0.0;

        for cf in cash_flows {
            let cf_date = cf.date();
            if cf_date <= settlement {
                continue;
            }

            let t = settlement.days_between(&cf_date) as f64 / 365.0;
            let df = 1.0 / (1.0 + yield_value / f).powf(f * t);
            let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);

            // Weighted time (Macaulay duration numerator)
            weighted_time += t * amount * df;
        }

        // Macaulay duration
        let mac_dur = weighted_time / price;

        // Modified duration
        Ok(mac_dur / (1.0 + yield_value / f))
    }

    /// Calculates the convexity using yield.
    pub fn convexity(
        &self,
        cash_flows: &[CashFlow],
        yield_value: f64,
        settlement: Date,
        frequency: u32,
    ) -> PricingResult<f64> {
        let price = self.price_from_yield_f64(cash_flows, yield_value, settlement, frequency)?;

        if price.abs() < 1e-10 {
            return Ok(0.0);
        }

        let f = frequency as f64;
        let mut weighted_sum = 0.0;

        for cf in cash_flows {
            let cf_date = cf.date();
            if cf_date <= settlement {
                continue;
            }

            let t = settlement.days_between(&cf_date) as f64 / 365.0;
            let df = 1.0 / (1.0 + yield_value / f).powf(f * t);
            let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);

            // Convexity numerator: t * (t + 1/f) * CF * DF
            weighted_sum += t * (t + 1.0 / f) * amount * df;
        }

        // Convexity = weighted_sum / (Price * (1 + y/f)^2)
        let denom = price * (1.0 + yield_value / f).powi(2);
        Ok(weighted_sum / denom)
    }
}

impl ConfigurableCalculator for GenericYieldSolver {
    fn solver_config(&self) -> &SolverConfig {
        &self.config
    }

    fn solver_config_mut(&mut self) -> &mut SolverConfig {
        &mut self.config
    }
}

impl YieldSolver for GenericYieldSolver {
    fn solve_yield(
        &self,
        cash_flows: &[CashFlow],
        target_price: Decimal,
        settlement: Date,
        frequency: u32,
    ) -> ConvexResult<f64> {
        let target = target_price.to_string().parse::<f64>().unwrap_or(100.0);

        self.solve_yield_f64(cash_flows, target, settlement, frequency)
            .map_err(|e| convex_core::error::ConvexError::pricing_error(e.to_string()))
    }

    fn price_from_yield(
        &self,
        cash_flows: &[CashFlow],
        yield_value: f64,
        settlement: Date,
        frequency: u32,
    ) -> ConvexResult<Decimal> {
        let price = self
            .price_from_yield_f64(cash_flows, yield_value, settlement, frequency)
            .map_err(|e| convex_core::error::ConvexError::pricing_error(e.to_string()))?;

        Ok(Decimal::from_f64_retain(price).unwrap_or(Decimal::ZERO))
    }
}

/// Creates a yield solver that uses a curve for the initial rate estimate.
///
/// This is useful when you have a benchmark curve to provide better
/// initial guesses for the yield solver.
pub struct CurveAwareYieldSolver<'a, C: Curve + ?Sized> {
    curve: &'a C,
    inner: GenericYieldSolver,
}

impl<'a, C: Curve + ?Sized> CurveAwareYieldSolver<'a, C> {
    /// Creates a new curve-aware yield solver.
    pub fn new(curve: &'a C) -> Self {
        Self {
            curve,
            inner: GenericYieldSolver::new(),
        }
    }

    /// Returns the underlying curve.
    pub fn curve(&self) -> &C {
        self.curve
    }

    /// Sets the search bounds.
    #[must_use]
    pub fn with_bounds(mut self, lower: f64, upper: f64) -> Self {
        self.inner = self.inner.with_bounds(lower, upper);
        self
    }

    /// Solves for yield using the curve for initial guess.
    pub fn solve_yield_f64(
        &self,
        cash_flows: &[CashFlow],
        target_price: f64,
        settlement: Date,
        frequency: u32,
    ) -> PricingResult<f64> {
        // Use curve's par yield as initial guess if available
        // For now, just delegate to the inner solver
        self.inner
            .solve_yield_f64(cash_flows, target_price, settlement, frequency)
    }
}

impl<C: Curve + ?Sized> ConfigurableCalculator for CurveAwareYieldSolver<'_, C> {
    fn solver_config(&self) -> &SolverConfig {
        self.inner.solver_config()
    }

    fn solver_config_mut(&mut self) -> &mut SolverConfig {
        self.inner.solver_config_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::types::CashFlowType;
    use rust_decimal_macros::dec;

    fn create_test_cash_flows(settlement: Date, coupon: f64, maturity_years: i32) -> Vec<CashFlow> {
        let mut cfs = Vec::new();
        let semi_annual_coupon = coupon / 2.0;

        for i in 1..=(maturity_years * 2) {
            let cf_date = settlement.add_months(i * 6).unwrap();
            cfs.push(CashFlow::new(
                cf_date,
                Decimal::from_f64_retain(semi_annual_coupon).unwrap(),
                CashFlowType::Coupon,
            ));
        }

        // Add principal at maturity
        let maturity_date = settlement.add_months(maturity_years * 12).unwrap();
        cfs.push(CashFlow::new(
            maturity_date,
            dec!(100.0),
            CashFlowType::Principal,
        ));

        cfs
    }

    #[test]
    fn test_yield_solver_creation() {
        let solver = GenericYieldSolver::new();
        assert_eq!(
            solver.bounds(),
            (DEFAULT_YIELD_LOWER_BOUND, DEFAULT_YIELD_UPPER_BOUND)
        );
    }

    #[test]
    fn test_price_from_yield_par_bond() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let solver = GenericYieldSolver::new();

        // 5% coupon, 5 year bond
        let cash_flows = create_test_cash_flows(settlement, 5.0, 5);

        // At par yield, price should be close to 100
        let price = solver
            .price_from_yield_f64(&cash_flows, 0.05, settlement, 2)
            .unwrap();
        assert!((price - 100.0).abs() < 1.0);
    }

    #[test]
    fn test_solve_yield_par_bond() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let solver = GenericYieldSolver::new();

        // 5% coupon, 5 year bond at par
        let cash_flows = create_test_cash_flows(settlement, 5.0, 5);

        let ytm = solver
            .solve_yield_f64(&cash_flows, 100.0, settlement, 2)
            .unwrap();

        // YTM should be close to coupon rate
        assert!((ytm - 0.05).abs() < 0.005);
    }

    #[test]
    fn test_solve_yield_roundtrip() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let solver = GenericYieldSolver::new();

        let cash_flows = create_test_cash_flows(settlement, 6.0, 10);

        // Various yields
        for target_yield in &[0.04, 0.05, 0.06, 0.07, 0.08] {
            let price = solver
                .price_from_yield_f64(&cash_flows, *target_yield, settlement, 2)
                .unwrap();
            let solved_yield = solver
                .solve_yield_f64(&cash_flows, price, settlement, 2)
                .unwrap();

            assert!(
                (solved_yield - target_yield).abs() < 1e-6,
                "Failed for yield {}: got {}",
                target_yield,
                solved_yield
            );
        }
    }

    #[test]
    fn test_solve_yield_trait() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let solver = GenericYieldSolver::new();

        let cash_flows = create_test_cash_flows(settlement, 5.0, 5);

        let target_price = dec!(100.0);
        let ytm = solver
            .solve_yield(&cash_flows, target_price, settlement, 2)
            .unwrap();

        assert!((ytm - 0.05).abs() < 0.005);
    }

    #[test]
    fn test_discount_bond() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let solver = GenericYieldSolver::new();

        // 5% coupon bond at discount price
        let cash_flows = create_test_cash_flows(settlement, 5.0, 5);
        let ytm = solver
            .solve_yield_f64(&cash_flows, 95.0, settlement, 2)
            .unwrap();

        // YTM should be higher than coupon rate
        assert!(ytm > 0.05);
    }

    #[test]
    fn test_premium_bond() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let solver = GenericYieldSolver::new();

        // 7% coupon bond at premium price
        let cash_flows = create_test_cash_flows(settlement, 7.0, 5);
        let ytm = solver
            .solve_yield_f64(&cash_flows, 105.0, settlement, 2)
            .unwrap();

        // YTM should be lower than coupon rate
        assert!(ytm < 0.07);
    }

    #[test]
    fn test_modified_duration() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let solver = GenericYieldSolver::new();

        let cash_flows = create_test_cash_flows(settlement, 5.0, 5);
        let duration = solver
            .modified_duration(&cash_flows, 0.05, settlement, 2)
            .unwrap();

        // Duration should be reasonable for a 5-year bond
        assert!(duration > 3.0);
        assert!(duration < 5.0);
    }

    #[test]
    fn test_convexity() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let solver = GenericYieldSolver::new();

        let cash_flows = create_test_cash_flows(settlement, 5.0, 5);
        let convexity = solver.convexity(&cash_flows, 0.05, settlement, 2).unwrap();

        // Convexity should be positive
        assert!(convexity > 0.0);
    }

    #[test]
    fn test_configurable_calculator() {
        let solver = GenericYieldSolver::new()
            .with_tolerance(1e-8)
            .with_max_iterations(50);

        assert!((solver.tolerance() - 1e-8).abs() < f64::EPSILON);
        assert_eq!(solver.max_iterations(), 50);
    }

    #[test]
    fn test_with_bounds() {
        let solver = GenericYieldSolver::new().with_bounds(-0.10, 0.40);

        assert_eq!(solver.bounds(), (-0.10, 0.40));
    }
}
