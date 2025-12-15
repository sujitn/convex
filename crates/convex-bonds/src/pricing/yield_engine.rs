//! Unified yield calculation engine.
//!
//! This module provides a comprehensive yield calculation engine that supports
//! all yield conventions, compounding methods, and edge cases. It consolidates
//! yield calculation logic to ensure consistency across the codebase.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_bonds::pricing::{StandardYieldEngine, YieldEngine};
//! use convex_bonds::types::YieldCalculationRules;
//!
//! let engine = StandardYieldEngine::default();
//! let rules = YieldCalculationRules::us_treasury();
//!
//! let result = engine.yield_from_price(
//!     &cash_flows,
//!     clean_price,
//!     accrued,
//!     settlement,
//!     &rules,
//! )?;
//! ```

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_core::types::Date;
use convex_math::solvers::{brent, newton_raphson, SolverConfig};

use crate::error::{BondError, BondResult};
use crate::traits::BondCashFlow;
use crate::types::{YieldCalculationRules, YieldConvention};

/// Result of a yield calculation.
#[derive(Debug, Clone, Copy)]
pub struct YieldEngineResult {
    /// The calculated yield (as a decimal, e.g., 0.05 for 5%).
    pub yield_value: f64,
    /// Number of iterations to converge.
    pub iterations: u32,
    /// Final residual (should be near zero).
    pub residual: f64,
    /// Convention used for calculation.
    pub convention: YieldConvention,
}

impl YieldEngineResult {
    /// Returns the yield as a percentage (e.g., 5.0 for 5%).
    #[must_use]
    pub fn yield_percent(&self) -> f64 {
        self.yield_value * 100.0
    }

    /// Returns the yield as a Decimal.
    #[must_use]
    pub fn yield_decimal(&self) -> Decimal {
        Decimal::from_f64_retain(self.yield_value).unwrap_or(Decimal::ZERO)
    }
}

/// Trait for yield calculation engines.
///
/// Implementations provide methods to calculate yields from prices and vice versa,
/// with full support for different conventions and edge cases.
pub trait YieldEngine: Send + Sync {
    /// Calculate yield from price.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - All future cash flows (coupons and principal)
    /// * `clean_price` - Market clean price (per 100 face value)
    /// * `accrued` - Accrued interest
    /// * `settlement` - Settlement date
    /// * `rules` - Yield calculation rules
    ///
    /// # Returns
    ///
    /// The yield that makes PV(cash flows) = dirty price.
    fn yield_from_price(
        &self,
        cash_flows: &[BondCashFlow],
        clean_price: Decimal,
        accrued: Decimal,
        settlement: Date,
        rules: &YieldCalculationRules,
    ) -> BondResult<YieldEngineResult>;

    /// Calculate price from yield.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - All future cash flows
    /// * `yield_rate` - Annual yield rate as decimal
    /// * `settlement` - Settlement date
    /// * `rules` - Yield calculation rules
    ///
    /// # Returns
    ///
    /// The dirty price (PV of cash flows).
    fn price_from_yield(
        &self,
        cash_flows: &[BondCashFlow],
        yield_rate: f64,
        settlement: Date,
        rules: &YieldCalculationRules,
    ) -> f64;

    /// Calculate accrued interest.
    ///
    /// # Arguments
    ///
    /// * `settlement` - Settlement date
    /// * `last_coupon` - Last coupon date
    /// * `next_coupon` - Next coupon date
    /// * `coupon_rate` - Annual coupon rate as decimal
    /// * `face_value` - Face value of the bond
    /// * `rules` - Yield calculation rules
    ///
    /// # Returns
    ///
    /// Accrued interest amount.
    fn accrued_interest(
        &self,
        settlement: Date,
        last_coupon: Date,
        next_coupon: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        rules: &YieldCalculationRules,
    ) -> Decimal;
}

/// Standard yield calculation engine.
///
/// Implements the `YieldEngine` trait with full support for all conventions.
/// Uses Newton-Raphson with Brent's method fallback for robust convergence.
#[derive(Debug, Clone)]
pub struct StandardYieldEngine {
    /// Solver tolerance.
    tolerance: f64,
    /// Maximum iterations.
    max_iterations: u32,
}

impl Default for StandardYieldEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl StandardYieldEngine {
    /// Creates a new standard yield engine with default settings.
    ///
    /// Default tolerance: 1e-10
    /// Default max iterations: 100
    #[must_use]
    pub fn new() -> Self {
        Self {
            tolerance: 1e-10,
            max_iterations: 100,
        }
    }

    /// Sets the solver tolerance.
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Sets the maximum iterations.
    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// Prepares cash flow data for calculation.
    fn prepare_cash_flows(
        &self,
        cash_flows: &[BondCashFlow],
        settlement: Date,
        rules: &YieldCalculationRules,
    ) -> Vec<(f64, f64)> {
        let dc = rules.accrual_day_count.to_day_count();

        cash_flows
            .iter()
            .filter(|cf| cf.date > settlement)
            .map(|cf| {
                let years = dc.year_fraction(settlement, cf.date);
                let amount = cf.amount.to_f64().unwrap_or(0.0);
                (years.to_f64().unwrap_or(0.0), amount)
            })
            .collect()
    }

    /// Estimates initial yield guess.
    fn estimate_initial_yield(
        &self,
        cf_data: &[(f64, f64)],
        dirty_price: f64,
        rules: &YieldCalculationRules,
    ) -> f64 {
        // Current yield approximation
        let total_coupons: f64 = cf_data.iter().map(|(_, amt)| amt).sum();
        let years_to_mat = cf_data.last().map_or(1.0, |(y, _)| *y);
        let face = cf_data.last().map_or(100.0, |(_, amt)| *amt).min(100.0);

        let annual_coupon = if years_to_mat > 0.0 {
            (total_coupons - face) / years_to_mat
        } else {
            0.0
        };

        let periods_per_year = rules.frequency.periods_per_year() as f64;
        let annual_coupon_adjusted = if periods_per_year > 0.0 {
            annual_coupon
        } else {
            0.0
        };

        if dirty_price > 0.0 {
            (annual_coupon_adjusted / dirty_price).clamp(-0.5, 1.0)
        } else {
            0.05
        }
    }

    /// Calculates present value at a given yield using the specified compounding.
    fn pv_at_yield(
        &self,
        cf_data: &[(f64, f64)],
        yield_rate: f64,
        rules: &YieldCalculationRules,
    ) -> f64 {
        cf_data
            .iter()
            .map(|(years, amount)| {
                let df = rules.compounding.discount_factor(yield_rate, *years);
                amount * df
            })
            .sum()
    }

    /// Calculates the derivative of PV with respect to yield.
    fn pv_derivative(
        &self,
        cf_data: &[(f64, f64)],
        yield_rate: f64,
        rules: &YieldCalculationRules,
    ) -> f64 {
        cf_data
            .iter()
            .map(|(years, amount)| {
                let ddf_dy = rules
                    .compounding
                    .discount_factor_derivative(yield_rate, *years);
                amount * ddf_dy
            })
            .sum()
    }

    /// Solves using Brent's method when Newton-Raphson fails.
    fn solve_with_brent<F>(
        &self,
        objective: F,
        initial_guess: f64,
        convention: YieldConvention,
    ) -> BondResult<YieldEngineResult>
    where
        F: Fn(f64) -> f64,
    {
        let config = SolverConfig::new(self.tolerance, self.max_iterations);

        // Try to find a bracket
        let brackets = [
            (initial_guess - 0.1, initial_guess + 0.1),
            (-0.1, 0.5),
            (-0.2, 1.0),
            (-0.5, 2.0),
        ];

        for (a, b) in brackets {
            if let Ok(result) = brent(&objective, a, b, &config) {
                return Ok(YieldEngineResult {
                    yield_value: result.root,
                    iterations: result.iterations,
                    residual: result.residual,
                    convention,
                });
            }
        }

        Err(BondError::YieldConvergenceFailed {
            iterations: self.max_iterations,
        })
    }
}

impl YieldEngine for StandardYieldEngine {
    fn yield_from_price(
        &self,
        cash_flows: &[BondCashFlow],
        clean_price: Decimal,
        accrued: Decimal,
        settlement: Date,
        rules: &YieldCalculationRules,
    ) -> BondResult<YieldEngineResult> {
        let dirty_price = clean_price + accrued;
        let target = dirty_price.to_f64().unwrap_or(100.0);

        let cf_data = self.prepare_cash_flows(cash_flows, settlement, rules);

        if cf_data.is_empty() {
            return Err(BondError::InvalidSpec {
                reason: "No future cash flows".to_string(),
            });
        }

        let initial_guess = self.estimate_initial_yield(&cf_data, target, rules);

        // Objective function: PV(yield) - target = 0
        let objective = |y: f64| self.pv_at_yield(&cf_data, y, rules) - target;

        // Analytical derivative for Newton-Raphson
        let derivative = |y: f64| self.pv_derivative(&cf_data, y, rules);

        let config = SolverConfig::new(self.tolerance, self.max_iterations);

        // Try Newton-Raphson first
        match newton_raphson(objective, derivative, initial_guess, &config) {
            Ok(result) => Ok(YieldEngineResult {
                yield_value: result.root,
                iterations: result.iterations,
                residual: result.residual,
                convention: rules.convention,
            }),
            Err(_) => {
                // Try multiple initial guesses
                let guesses = [0.01, 0.03, 0.05, 0.08, 0.10, 0.15];
                for guess in guesses {
                    if let Ok(result) = newton_raphson(objective, derivative, guess, &config) {
                        return Ok(YieldEngineResult {
                            yield_value: result.root,
                            iterations: result.iterations,
                            residual: result.residual,
                            convention: rules.convention,
                        });
                    }
                }

                // Fallback to Brent's method
                self.solve_with_brent(objective, initial_guess, rules.convention)
            }
        }
    }

    fn price_from_yield(
        &self,
        cash_flows: &[BondCashFlow],
        yield_rate: f64,
        settlement: Date,
        rules: &YieldCalculationRules,
    ) -> f64 {
        let cf_data = self.prepare_cash_flows(cash_flows, settlement, rules);
        self.pv_at_yield(&cf_data, yield_rate, rules)
    }

    fn accrued_interest(
        &self,
        settlement: Date,
        last_coupon: Date,
        next_coupon: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        rules: &YieldCalculationRules,
    ) -> Decimal {
        use crate::cashflows::AccruedInterestCalculator;
        use crate::types::AccruedConvention;

        let frequency = rules.frequency;
        let day_count = rules.accrual_day_count;

        match rules.accrued_convention {
            AccruedConvention::None => Decimal::ZERO,
            AccruedConvention::Standard => AccruedInterestCalculator::standard(
                settlement,
                last_coupon,
                next_coupon,
                coupon_rate,
                face_value,
                day_count,
                frequency,
            ),
            AccruedConvention::ExDividend => {
                if let Some(ex_div_rules) = &rules.ex_dividend_rules {
                    // Get calendar from settlement rules or use weekend-only
                    let calendar = crate::types::CalendarId::weekend_only();
                    AccruedInterestCalculator::ex_dividend(
                        settlement,
                        last_coupon,
                        next_coupon,
                        coupon_rate,
                        face_value,
                        day_count,
                        frequency,
                        ex_div_rules.days,
                        &calendar,
                    )
                } else {
                    AccruedInterestCalculator::standard(
                        settlement,
                        last_coupon,
                        next_coupon,
                        coupon_rate,
                        face_value,
                        day_count,
                        frequency,
                    )
                }
            }
            AccruedConvention::RecordDate | AccruedConvention::CumDividend => {
                // For record date, use standard accrued calculation
                // The record date logic affects who receives the coupon, not the accrued amount
                AccruedInterestCalculator::standard(
                    settlement,
                    last_coupon,
                    next_coupon,
                    coupon_rate,
                    face_value,
                    day_count,
                    frequency,
                )
            }
        }
    }
}

/// Calculate current yield.
///
/// Current yield = Annual Coupon / Clean Price
///
/// This is a simple measure that ignores time value of money
/// and capital gains/losses.
#[must_use]
pub fn current_yield_simple(annual_coupon: f64, clean_price: f64) -> f64 {
    if clean_price <= 0.0 {
        return 0.0;
    }
    annual_coupon / clean_price
}

/// Calculate simple yield (Japanese convention).
///
/// Simple Yield = (Annual Coupon + (100 - Price) / Years) / Price
///
/// No compounding - assumes simple interest.
#[must_use]
pub fn simple_yield(annual_coupon: f64, clean_price: f64, years_to_maturity: f64) -> f64 {
    if clean_price <= 0.0 || years_to_maturity <= 0.0 {
        return 0.0;
    }
    let capital_gain = (100.0 - clean_price) / years_to_maturity;
    (annual_coupon + capital_gain) / clean_price
}

/// Calculate discount yield (T-Bill convention).
///
/// Discount Yield = (Face - Price) / Face × (360 / Days)
#[must_use]
pub fn discount_yield(price: f64, face: f64, days_to_maturity: i64) -> f64 {
    if face <= 0.0 || days_to_maturity <= 0 {
        return 0.0;
    }
    ((face - price) / face) * (360.0 / days_to_maturity as f64)
}

/// Calculate bond equivalent yield from discount yield.
///
/// Converts money market discount yield to bond-equivalent basis.
#[must_use]
pub fn bond_equivalent_yield(discount_yield: f64, days_to_maturity: i64) -> f64 {
    if days_to_maturity <= 0 {
        return 0.0;
    }

    let days = days_to_maturity as f64;

    if days <= 182.0 {
        // Simple conversion for short-dated
        (365.0 * discount_yield) / (360.0 - discount_yield * days)
    } else {
        // More complex formula for longer-dated
        let price = 100.0 * (1.0 - discount_yield * days / 360.0);
        let term = days / 365.0;

        // Solve for semi-annual equivalent yield
        // This is an approximation
        let face = 100.0;
        let gain = face - price;
        2.0 * ((gain / price) / term + ((gain / price).powi(2) / term.powi(2) + 2.0 * gain / (price * term)).sqrt()) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_coupon_cash_flows(
        settlement: Date,
        maturity: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        frequency: convex_core::types::Frequency,
    ) -> Vec<BondCashFlow> {
        let mut flows = Vec::new();
        let periods_per_year = frequency.periods_per_year();
        let coupon_amount = coupon_rate * face_value / Decimal::from(periods_per_year);

        let mut coupon_date = maturity;
        let months_per_period = 12 / periods_per_year;
        let mut dates = Vec::new();

        while coupon_date > settlement {
            dates.push(coupon_date);
            let (y, m, d) = (coupon_date.year(), coupon_date.month(), coupon_date.day());
            let new_month = if m as i32 - months_per_period as i32 <= 0 {
                (m as i32 + 12 - months_per_period as i32) as u32
            } else {
                m - months_per_period
            };
            let new_year = if m as i32 - months_per_period as i32 <= 0 {
                y - 1
            } else {
                y
            };
            coupon_date = Date::from_ymd(new_year, new_month, d.min(28)).unwrap_or(coupon_date);
            if coupon_date <= settlement {
                break;
            }
        }

        dates.reverse();

        for (i, &cf_date) in dates.iter().enumerate() {
            if i == dates.len() - 1 {
                flows.push(BondCashFlow::coupon_and_principal(
                    cf_date,
                    coupon_amount,
                    face_value,
                ));
            } else {
                flows.push(BondCashFlow::coupon(cf_date, coupon_amount));
            }
        }

        flows
    }

    #[test]
    fn test_engine_yield_at_par() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            convex_core::types::Frequency::SemiAnnual,
        );

        let engine = StandardYieldEngine::new();
        let rules = YieldCalculationRules::us_corporate();

        let result = engine
            .yield_from_price(&cash_flows, dec!(100), dec!(0), settlement, &rules)
            .unwrap();

        // At par, yield should equal coupon rate
        assert_relative_eq!(result.yield_value, 0.05, epsilon = 0.001);
    }

    #[test]
    fn test_engine_yield_discount_bond() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            convex_core::types::Frequency::SemiAnnual,
        );

        let engine = StandardYieldEngine::new();
        let rules = YieldCalculationRules::us_corporate();

        let result = engine
            .yield_from_price(&cash_flows, dec!(95), dec!(0), settlement, &rules)
            .unwrap();

        // Discount bond should have yield > coupon rate
        assert!(result.yield_value > 0.05);
    }

    #[test]
    fn test_engine_price_from_yield() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            convex_core::types::Frequency::SemiAnnual,
        );

        let engine = StandardYieldEngine::new();
        let rules = YieldCalculationRules::us_corporate();

        let price = engine.price_from_yield(&cash_flows, 0.05, settlement, &rules);

        // At par yield, price should be close to par
        assert_relative_eq!(price, 100.0, epsilon = 1.0);
    }

    #[test]
    fn test_engine_roundtrip() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            convex_core::types::Frequency::SemiAnnual,
        );

        let engine = StandardYieldEngine::new();
        let rules = YieldCalculationRules::us_corporate();

        let original_price = dec!(98.50);
        let accrued = dec!(1.25);

        // Price -> Yield
        let result = engine
            .yield_from_price(&cash_flows, original_price, accrued, settlement, &rules)
            .unwrap();

        // Yield -> Price
        let calculated_dirty = engine.price_from_yield(&cash_flows, result.yield_value, settlement, &rules);
        let calculated_clean = calculated_dirty - accrued.to_f64().unwrap();

        // Should round-trip
        assert_relative_eq!(
            calculated_clean,
            original_price.to_f64().unwrap(),
            epsilon = 0.01
        );
    }

    #[test]
    fn test_uk_gilt_rules() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.04);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            convex_core::types::Frequency::SemiAnnual,
        );

        let engine = StandardYieldEngine::new();
        let rules = YieldCalculationRules::uk_gilt();

        let result = engine
            .yield_from_price(&cash_flows, dec!(100), dec!(0), settlement, &rules)
            .unwrap();

        // Should converge
        assert!(result.iterations < 50);
        assert!(result.residual.abs() < 1e-6);
    }

    #[test]
    fn test_german_bund_rules() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.03);
        let face_value = dec!(100);

        // Annual coupon for Bunds
        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            convex_core::types::Frequency::Annual,
        );

        let engine = StandardYieldEngine::new();
        let rules = YieldCalculationRules::german_bund();

        let result = engine
            .yield_from_price(&cash_flows, dec!(100), dec!(0), settlement, &rules)
            .unwrap();

        // Should converge with annual compounding
        assert!(result.iterations < 50);
    }

    #[test]
    fn test_current_yield() {
        let cy = current_yield_simple(5.0, 100.0);
        assert_relative_eq!(cy, 0.05, epsilon = 0.0001);

        let cy = current_yield_simple(5.0, 110.0);
        assert!(cy < 0.05);
    }

    #[test]
    fn test_simple_yield() {
        // At par, simple yield = coupon rate
        let sy = simple_yield(5.0, 100.0, 5.0);
        assert_relative_eq!(sy, 0.05, epsilon = 0.0001);

        // Discount bond
        let sy = simple_yield(5.0, 95.0, 5.0);
        assert!(sy > 0.05); // Should include capital gain
    }

    #[test]
    fn test_discount_yield() {
        // 3-month T-Bill at 99
        let dy = discount_yield(99.0, 100.0, 90);
        assert!(dy > 0.0);
        assert!(dy < 0.05);
    }
}
