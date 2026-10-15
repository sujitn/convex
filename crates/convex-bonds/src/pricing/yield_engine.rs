//! Supplied-cash yield diagnostics and checked scalar reporting.
//!
//! Scalar reporting and simple/current-yield measures have checked native owners.
//! The retained general solver and accrued methods do not qualify missing market
//! conventions or contextual cash mechanics; use the complete dated contract routes.
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

use super::discount_time::{prepare_bond_cash, DiscountTimeBasis};
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
    pub fn yield_percent(&self) -> BondResult<f64> {
        super::yield_scalars::to_percent(self.yield_value)
    }

    /// Returns the yield as a Decimal.
    pub fn yield_decimal(&self) -> BondResult<Decimal> {
        super::yield_scalars::to_decimal(self.yield_value)
    }
}

/// Trait for yield calculation engines.
///
/// Implementations provide methods to calculate yields from prices and vice versa,
/// for their explicitly supplied cash and rules; this is not convention qualification.
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
    ) -> BondResult<f64>;

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
    ) -> BondResult<Decimal>;
}

/// Standard yield calculation engine.
///
/// Retained supplied-cash diagnostics; complete contextual conventions need their
/// original checked contract owners.
/// Uses Newton-Raphson with Brent's method fallback for robust convergence.
#[derive(Debug, Clone)]
pub struct StandardYieldEngine {
    /// Solver tolerance.
    tolerance: f64,
    /// Maximum iterations.
    max_iterations: u32,
    discount_basis: Option<DiscountTimeBasis>,
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
            discount_basis: None,
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

    /// Bind the original discount-time context, independently of accrued rules.
    /// Its day count must match `rules.discount_day_count` on every call.
    #[must_use]
    pub fn with_discount_basis(mut self, basis: DiscountTimeBasis) -> Self {
        self.discount_basis = Some(basis);
        self
    }

    fn prepare_cash_flows(
        &self,
        cash_flows: &[BondCashFlow],
        settlement: Date,
        rules: &YieldCalculationRules,
    ) -> BondResult<Vec<(f64, f64)>> {
        prepare_bond_cash(
            cash_flows,
            settlement,
            rules.discount_day_count,
            self.discount_basis.as_ref(),
        )
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

    fn pv_at_yield(&self, cash: &[(f64, f64)], rate: f64, rules: &YieldCalculationRules) -> f64 {
        match super::yield_cash::standard_value(cash, rate, rules.compounding) {
            Ok((price, _)) => price,
            Err(_) => f64::NAN,
        }
    }

    fn pv_derivative(&self, cash: &[(f64, f64)], rate: f64, rules: &YieldCalculationRules) -> f64 {
        match super::yield_cash::standard_value(cash, rate, rules.compounding) {
            Ok((_, first)) => first,
            Err(_) => f64::NAN,
        }
    }

    fn accept_result(
        &self,
        result: YieldEngineResult,
        cash: &[(f64, f64)],
        target: f64,
        rules: &YieldCalculationRules,
    ) -> BondResult<YieldEngineResult> {
        let residual =
            super::yield_cash::standard_value(cash, result.yield_value, rules.compounding)?.0
                - target;
        if !residual.is_finite() || residual.abs() >= self.tolerance {
            return Err(BondError::YieldConvergenceFailed {
                iterations: result.iterations,
            });
        }
        Ok(YieldEngineResult { residual, ..result })
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
        SolverConfig::new(self.tolerance, self.max_iterations)
            .validate()
            .map_err(|e| BondError::invalid_spec(e.to_string()))?;
        let dirty_price = clean_price
            .checked_add(accrued)
            .ok_or_else(|| BondError::pricing_failed("yield-engine dirty target overflow"))?;
        let target = dirty_price
            .to_f64()
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| BondError::invalid_spec("positive finite dirty target required"))?;

        let cf_data = self.prepare_cash_flows(cash_flows, settlement, rules)?;

        if cf_data.is_empty() {
            return Err(BondError::InvalidSpec {
                reason: "No future cash flows".to_string(),
            });
        }

        super::yield_cash::validate_cash(&cf_data, true)?;
        let initial_guess = self.estimate_initial_yield(&cf_data, target, rules);

        // Objective function: PV(yield) - target = 0
        let objective = |y: f64| self.pv_at_yield(&cf_data, y, rules) - target;

        // Analytical derivative for Newton-Raphson
        let derivative = |y: f64| self.pv_derivative(&cf_data, y, rules);

        let config = SolverConfig::new(self.tolerance, self.max_iterations);

        // Try Newton-Raphson first
        if let Ok(result) = newton_raphson(objective, derivative, initial_guess, &config) {
            self.accept_result(
                YieldEngineResult {
                    yield_value: result.root,
                    iterations: result.iterations,
                    residual: result.residual,
                    convention: rules.convention,
                },
                &cf_data,
                target,
                rules,
            )
        } else {
            // Try multiple initial guesses
            let guesses = [0.01, 0.03, 0.05, 0.08, 0.10, 0.15];
            for guess in guesses {
                if let Ok(result) = newton_raphson(objective, derivative, guess, &config) {
                    return self.accept_result(
                        YieldEngineResult {
                            yield_value: result.root,
                            iterations: result.iterations,
                            residual: result.residual,
                            convention: rules.convention,
                        },
                        &cf_data,
                        target,
                        rules,
                    );
                }
            }

            // Fallback to Brent's method
            let result = self.solve_with_brent(objective, initial_guess, rules.convention)?;
            self.accept_result(result, &cf_data, target, rules)
        }
    }

    fn price_from_yield(
        &self,
        cash_flows: &[BondCashFlow],
        yield_rate: f64,
        settlement: Date,
        rules: &YieldCalculationRules,
    ) -> BondResult<f64> {
        let cf_data = self.prepare_cash_flows(cash_flows, settlement, rules)?;
        super::yield_cash::standard_value(&cf_data, yield_rate, rules.compounding).map(|v| v.0)
    }

    fn accrued_interest(
        &self,
        settlement: Date,
        last_coupon: Date,
        next_coupon: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        rules: &YieldCalculationRules,
    ) -> BondResult<Decimal> {
        use crate::cashflows::AccruedInterestCalculator;
        use crate::types::AccruedConvention;
        match rules.accrued_convention {
            AccruedConvention::Standard => AccruedInterestCalculator::standard(
                settlement,last_coupon,next_coupon,coupon_rate,face_value,
                rules.accrual_day_count,rules.frequency),
            AccruedConvention::None => {
                // Validate even when the explicitly selected convention is zero.
                AccruedInterestCalculator::standard(settlement,last_coupon,next_coupon,
                    coupon_rate,face_value,rules.accrual_day_count,rules.frequency)?;
                Ok(Decimal::ZERO)
            }
            AccruedConvention::ExDividend => Err(BondError::missing_field(
                "actual ex-dividend calendar; use the checked supplied-calendar calculator or full contract")),
            AccruedConvention::RecordDate | AccruedConvention::CumDividend =>
                Err(BondError::invalid_spec("record-date/cum-dividend accrual requires original contract/entitlement context")),
        }
    }
}

/// Calculate current yield.
///
/// Current yield = Annual Coupon / Clean Price
///
/// This is a simple measure that ignores time value of money
/// and capital gains/losses.
pub fn current_yield_simple(annual_coupon: f64, clean_price: f64) -> BondResult<f64> {
    super::yield_scalars::current_binary64(annual_coupon, clean_price)
}

/// Arithmetic per100 simple-yield estimate as an annual decimal rate.
/// This does not identify a dated market convention.
pub fn simple_yield(
    annual_coupon: f64,
    clean_price: f64,
    years_to_maturity: f64,
) -> BondResult<f64> {
    super::yield_scalars::simple_binary64(annual_coupon, clean_price, years_to_maturity)
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

    // The original synthetic test now supplies its missing nominal context.
    // Reference construction uses original schedule terms, not cash order.
    fn supplied_icma_basis(
        start: Date,
        end: Date,
        frequency: convex_core::types::Frequency,
    ) -> crate::pricing::discount_time::DiscountTimeBasis {
        use convex_core::{
            calendars::WeekendCalendar,
            cashflows::{CouponSchedule, DateRules, ScheduleSpec, StubType},
        };
        let spec = ScheduleSpec {
            start_date: start,
            end_date: end,
            frequency,
            stub_type: StubType::ShortFirst,
            end_of_month: false,
            first_regular_date: None,
            penultimate_date: None,
        };
        let schedule =
            CouponSchedule::generate(&spec, &DateRules::default(), &WeekendCalendar).unwrap();
        let mut references = Vec::new();
        for p in schedule.periods() {
            for r in p.reference_periods() {
                if references.last() != Some(r) {
                    references.push(*r);
                }
            }
        }
        crate::pricing::discount_time::DiscountTimeBasis::Icma {
            schedule: spec,
            reference_periods: references,
        }
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

        let price = engine
            .price_from_yield(&cash_flows, 0.05, settlement, &rules)
            .unwrap();

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
        let calculated_dirty = engine
            .price_from_yield(&cash_flows, result.yield_value, settlement, &rules)
            .unwrap();
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

        let engine =
            engine.with_discount_basis(supplied_icma_basis(settlement, maturity, rules.frequency));

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

        let engine =
            engine.with_discount_basis(supplied_icma_basis(settlement, maturity, rules.frequency));

        let result = engine
            .yield_from_price(&cash_flows, dec!(100), dec!(0), settlement, &rules)
            .unwrap();

        // Should converge with annual compounding
        assert!(result.iterations < 50);
    }

    #[test]
    fn test_current_yield() {
        let cy = current_yield_simple(5.0, 100.0).unwrap();
        assert_relative_eq!(cy, 0.05, epsilon = 0.0001);

        let cy = current_yield_simple(5.0, 110.0).unwrap();
        assert!(cy < 0.05);
    }

    #[test]
    fn test_simple_yield() {
        // At par, simple yield = coupon rate
        let sy = simple_yield(5.0, 100.0, 5.0).unwrap();
        assert_relative_eq!(sy, 0.05, epsilon = 0.0001);

        // Discount bond
        let sy = simple_yield(5.0, 95.0, 5.0).unwrap();
        assert!(sy > 0.05); // Should include capital gain
    }

    #[test]
    fn test_discount_yield() {
        // 3-month T-Bill at 99
        let dy = super::super::bill::bank_discount_yield(dec!(99), dec!(100), 90).unwrap();
        assert!(dy > dec!(0));
        assert!(dy < dec!(0.05));
    }
}
