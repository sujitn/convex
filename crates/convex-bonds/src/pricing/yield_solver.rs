//! Checked supplied-cash yield calculations and retained dated-time preparation.
//! Supplied numeric discount laws do not establish a market quotation convention.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_bonds::pricing::{YieldSolver, YieldResult};
//! use convex_bonds::types::YieldConvention;
//!
//! let solver = YieldSolver::new()
//!     .with_convention(YieldConvention::StreetConvention);
//!
//! let result = solver.solve(&cash_flows, clean_price, settlement, day_count, frequency)?;
//! println!("YTM: {:.6}%", result.yield_value * 100.0);
//! ```

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Date, Frequency};
use convex_math::solvers::{brent, newton_raphson, SolverConfig};

use super::discount_time::{prepare_bond_cash, DiscountTimeBasis, PreparedDatedCash};
use super::yield_cash::{self, CashYieldCompounding};
use crate::error::{BondError, BondResult};
use crate::traits::{BondCashFlow, FixedCouponBond};
use crate::types::YieldConvention;

/// Result of a yield calculation.
#[derive(Debug, Clone, Copy)]
pub struct YieldResult {
    /// The calculated yield (as a decimal, e.g., 0.05 for 5%).
    pub yield_value: f64,
    /// Number of iterations to converge.
    pub iterations: u32,
    /// Final residual (should be near zero).
    pub residual: f64,
}

impl YieldResult {
    /// Returns the yield as a percentage (e.g., 5.0 for 5%).
    pub fn yield_percent(&self) -> BondResult<f64> {
        super::yield_scalars::to_percent(self.yield_value)
    }

    /// Returns the yield as a Decimal.
    pub fn yield_decimal(&self) -> BondResult<Decimal> {
        super::yield_scalars::to_decimal(self.yield_value)
    }
}

/// Yield-to-maturity solver.
///
/// Uses Newton-Raphson iteration and Brent's method fallback on supplied cash.
/// This does not establish full dated market-convention support.
#[derive(Debug, Clone)]
pub struct YieldSolver {
    /// Solver configuration.
    config: SolverConfig,
    /// Yield convention to use.
    convention: YieldConvention,
    discount_basis: Option<DiscountTimeBasis>,
}

impl Default for YieldSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl YieldSolver {
    /// Creates a new yield solver with default settings.
    ///
    /// Default tolerance: 1e-10
    /// Default max iterations: 100
    /// Default convention: Street Convention
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: SolverConfig::new(1e-10, 100),
            convention: YieldConvention::StreetConvention,
            discount_basis: None,
        }
    }

    /// Bind original reference/coupon/termination context for dated methods.
    /// The context's exact day count must match the separately supplied role.
    #[must_use]
    pub fn with_discount_basis(mut self, basis: DiscountTimeBasis) -> Self {
        self.discount_basis = Some(basis);
        self
    }

    /// Sets the yield convention.
    #[must_use]
    pub fn with_convention(mut self, convention: YieldConvention) -> Self {
        self.convention = convention;
        self
    }

    /// Sets the solver tolerance.
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.config = SolverConfig::new(tolerance, self.config.max_iterations);
        self
    }

    /// Sets the maximum iterations.
    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.config = SolverConfig::new(self.config.tolerance, max_iterations);
        self
    }

    /// Returns the current yield convention.
    #[must_use]
    pub const fn convention(&self) -> YieldConvention {
        self.convention
    }

    /// Solves for yield given cash flows and price.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - All future cash flows (coupons and principal)
    /// * `clean_price` - Market clean price (per 100 face value)
    /// * `accrued` - Accrued interest
    /// * `settlement` - Settlement date
    /// * `day_count` - Day count convention
    /// * `frequency` - Coupon frequency
    ///
    /// # Returns
    ///
    /// The yield that makes PV(cash flows) = dirty price.
    pub fn solve(
        &self,
        cash_flows: &[BondCashFlow],
        clean_price: Decimal,
        accrued: Decimal,
        settlement: Date,
        day_count: DayCountConvention,
        frequency: Frequency,
    ) -> BondResult<YieldResult> {
        let dirty_price = clean_price
            .checked_add(accrued)
            .ok_or_else(|| BondError::pricing_failed("dirty target Decimal overflow"))?;
        let target = dirty_price
            .to_f64()
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| BondError::invalid_spec("positive finite dirty target required"))?;

        let periods_per_year = f64::from(frequency.periods_per_year());
        let cf_data = prepare_bond_cash(
            cash_flows,
            settlement,
            day_count,
            self.discount_basis.as_ref(),
        )?;

        if cf_data.is_empty() {
            return Err(BondError::InvalidSpec {
                reason: "No future cash flows".to_string(),
            });
        }

        // Initial guess based on current yield approximation
        let total_coupons: f64 = cf_data.iter().map(|(_, amt)| amt).sum();
        let years_to_mat = cf_data.last().map_or(1.0, |(y, _)| *y);
        let face = cf_data.last().map_or(100.0, |(_, amt)| *amt).min(100.0);
        let annual_coupon = if years_to_mat > 0.0 {
            (total_coupons - face) / years_to_mat
        } else {
            0.0
        };

        let initial_guess = if target > 0.0 {
            (annual_coupon / target).clamp(-0.5, 1.0)
        } else {
            0.05
        };

        if cf_data.iter().any(|(time, _)| *time == 0.0) {
            return self.solve_dated_cash(
                &PreparedDatedCash::from_validated_dates(cf_data),
                target,
                self.cash_basis(periods_per_year)?,
                initial_guess,
            );
        }
        self.solve_primitive(&cf_data, target, periods_per_year, initial_guess)
    }

    /// Solves for yield from pre-computed (year_fraction, amount) pairs.
    ///
    /// Lower-level companion to [`Self::solve`] for callers that already have flat
    /// arrays of discount times and cash flow amounts (e.g. a Bloomberg YAS
    /// pipeline). Uses Newton-Raphson with Brent's method as a fallback.
    pub fn solve_primitive(
        &self,
        cash_flows: &[(f64, f64)],
        target_dirty_price: f64,
        periods_per_year: f64,
        initial_guess: f64,
    ) -> BondResult<YieldResult> {
        self.solve_cash(
            cash_flows,
            target_dirty_price,
            self.cash_basis(periods_per_year)?,
            initial_guess,
        )
    }

    /// Checked inverse over explicit mathematical compounding and cash units.
    /// Nonnegative cash is required so the positive-price yield is unique.
    pub fn solve_cash(
        &self,
        cash_flows: &[(f64, f64)],
        target_dirty_price: f64,
        compounding: CashYieldCompounding,
        initial_guess: f64,
    ) -> BondResult<YieldResult> {
        self.config
            .validate()
            .map_err(|e| BondError::invalid_spec(e.to_string()))?;
        yield_cash::validate_cash(cash_flows, true)?;
        yield_cash::price_cash(cash_flows, initial_guess, compounding)?;
        if !target_dirty_price.is_finite() || target_dirty_price <= 0.0 {
            return Err(BondError::invalid_spec(
                "positive finite yield target required",
            ));
        }
        // The existing solvers reject nonfinite trial evaluations. A failed
        // trial terminates that attempt; it is never an accepted price/root.
        let objective = |y| match yield_cash::value_cash(cash_flows, y, compounding) {
            Ok((price, _)) => price - target_dirty_price,
            Err(_) => f64::NAN,
        };
        let derivative = |y| match yield_cash::value_cash(cash_flows, y, compounding) {
            Ok((_, first)) => first,
            Err(_) => f64::NAN,
        };
        let result = match newton_raphson(objective, derivative, initial_guess, &self.config) {
            Ok(result) => YieldResult {
                yield_value: result.root,
                iterations: result.iterations,
                residual: result.residual,
            },
            Err(_) => self.solve_with_brent(objective, initial_guess)?,
        };
        let residual = yield_cash::price_cash(cash_flows, result.yield_value, compounding)?
            - target_dirty_price;
        if !residual.is_finite() || residual.abs() >= self.config.tolerance {
            return Err(BondError::YieldConvergenceFailed {
                iterations: result.iterations,
            });
        }
        Ok(YieldResult { residual, ..result })
    }

    /// Invert cash prepared from actual future dates, retaining constant cash.
    /// Ordinary positive-time cash uses the unchanged raw solver. Constant price
    /// has no identified yield; distinct typed outcomes report nonunique/no root.
    pub fn solve_dated_cash(
        &self,
        prepared: &PreparedDatedCash,
        target_dirty_price: f64,
        compounding: CashYieldCompounding,
        initial_guess: f64,
    ) -> BondResult<YieldResult> {
        let cash = prepared.discount_cash();
        if cash.iter().all(|(time, _)| *time > 0.0) {
            return self.solve_cash(cash, target_dirty_price, compounding, initial_guess);
        }
        self.config
            .validate()
            .map_err(|error| BondError::invalid_spec(error.to_string()))?;
        yield_cash::validate_dated_cash(cash, true)?;
        let initial_price = prepared.price(initial_guess, compounding)?;
        if !target_dirty_price.is_finite() || target_dirty_price <= 0.0 {
            return Err(BondError::invalid_spec(
                "positive finite yield target required",
            ));
        }
        let mut constant = 0.0;
        let mut variable = Vec::with_capacity(cash.len());
        for &(time, amount) in cash {
            if time == 0.0 {
                constant += amount;
                if !constant.is_finite() {
                    return Err(BondError::pricing_failed("constant dated cash overflow"));
                }
            } else {
                variable.push((time, amount));
            }
        }
        if !variable.iter().any(|(_, amount)| *amount > 0.0) {
            return Err(if initial_price == target_dirty_price {
                BondError::YieldNonUnique
            } else {
                BondError::YieldNoRoot
            });
        }
        if target_dirty_price <= constant {
            return Err(BondError::YieldNoRoot);
        }
        let result = self.solve_cash(
            &variable,
            target_dirty_price - constant,
            compounding,
            initial_guess,
        )?;
        let residual = prepared.price(result.yield_value, compounding)? - target_dirty_price;
        if !residual.is_finite() || residual.abs() >= self.config.tolerance {
            return Err(BondError::YieldConvergenceFailed {
                iterations: result.iterations,
            });
        }
        Ok(YieldResult { residual, ..result })
    }

    /// Checked price over explicit positive years and unchanged cash units.
    pub fn price_primitive(
        &self,
        cash: &[(f64, f64)],
        yield_rate: f64,
        frequency: f64,
    ) -> BondResult<f64> {
        yield_cash::price_cash(cash, yield_rate, self.cash_basis(frequency)?)
    }

    pub(crate) fn cash_basis(&self, frequency: f64) -> BondResult<CashYieldCompounding> {
        if !frequency.is_finite() || frequency <= 0.0 {
            return Err(BondError::invalid_spec(
                "positive finite supplied frequency required",
            ));
        }
        match self.convention {
            YieldConvention::StreetConvention => Ok(CashYieldCompounding::Periodic { frequency }),
            YieldConvention::TrueYield | YieldConvention::Annual => {
                Ok(CashYieldCompounding::Annual)
            }
            YieldConvention::Continuous => Ok(CashYieldCompounding::Continuous),
            YieldConvention::SimpleYield => Ok(CashYieldCompounding::Simple),
            _ => Err(BondError::invalid_spec(
                "yield convention requires its complete contextual pricing owner",
            )),
        }
    }

    /// Solves using Brent's method when Newton-Raphson fails.
    fn solve_with_brent<F>(&self, objective: F, initial_guess: f64) -> BondResult<YieldResult>
    where
        F: Fn(f64) -> f64,
    {
        // Try to find a bracket
        let brackets = [
            (initial_guess - 0.1, initial_guess + 0.1),
            (-0.1, 0.5),
            (-0.2, 1.0),
            (-0.5, 2.0),
        ];

        for (a, b) in brackets {
            if let Ok(result) = brent(&objective, a, b, &self.config) {
                return Ok(YieldResult {
                    yield_value: result.root,
                    iterations: result.iterations,
                    residual: result.residual,
                });
            }
        }

        Err(BondError::YieldConvergenceFailed {
            iterations: self.config.max_iterations,
        })
    }

    /// Calculates dirty price from yield.
    pub fn dirty_price_from_yield(
        &self,
        cash_flows: &[BondCashFlow],
        yield_rate: f64,
        settlement: Date,
        day_count: DayCountConvention,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let periods_per_year = f64::from(frequency.periods_per_year());
        let cf_data = prepare_bond_cash(
            cash_flows,
            settlement,
            day_count,
            self.discount_basis.as_ref(),
        )?;
        if cf_data.iter().any(|(time, _)| *time == 0.0) {
            return PreparedDatedCash::from_validated_dates(cf_data)
                .price(yield_rate, self.cash_basis(periods_per_year)?);
        }
        self.price_primitive(&cf_data, yield_rate, periods_per_year)
    }

    /// Calculates clean price from yield.
    pub fn clean_price_from_yield(
        &self,
        cash_flows: &[BondCashFlow],
        yield_rate: f64,
        accrued: Decimal,
        settlement: Date,
        day_count: DayCountConvention,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let dirty =
            self.dirty_price_from_yield(cash_flows, yield_rate, settlement, day_count, frequency)?;
        let accrued = accrued
            .to_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| BondError::pricing_failed("accrued conversion"))?;
        let clean = dirty - accrued;
        if !clean.is_finite() || clean <= 0.0 {
            return Err(BondError::invalid_spec(
                "positive finite clean price required",
            ));
        }
        Ok(clean)
    }
}

/// Calculates current yield.
///
/// Current yield = Annual Coupon / Clean Price
///
/// This is a simple measure that ignores time value of money
/// and capital gains/losses.
pub fn current_yield(annual_coupon: Decimal, clean_price: Decimal) -> BondResult<f64> {
    super::yield_scalars::current_decimal_rate(annual_coupon, clean_price)
}

/// Current annual decimal yield from a constant-coupon bond and a PER100 price.
pub fn current_yield_from_bond(
    bond: &dyn FixedCouponBond,
    clean_price: Decimal,
) -> BondResult<f64> {
    super::yield_scalars::fixed_current_decimal_rate(
        bond.coupon_rate(),
        bond.face_value(),
        clean_price,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_coupon_cash_flows(
        settlement: Date,
        maturity: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        frequency: Frequency,
    ) -> Vec<BondCashFlow> {
        let mut flows = Vec::new();
        let periods_per_year = frequency.periods_per_year();
        let coupon_amount = coupon_rate * face_value / Decimal::from(periods_per_year);

        // Generate regular coupon schedule
        let mut coupon_date = maturity;
        let months_per_period = 12 / periods_per_year;
        let mut dates = Vec::new();

        while coupon_date > settlement {
            dates.push(coupon_date);
            // Go back by period
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
                // Final payment includes principal
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
    fn test_ytm_at_par() {
        // A bond priced at par should have YTM = coupon rate
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05); // 5%
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            Frequency::SemiAnnual,
        );

        let solver = YieldSolver::new();
        let result = solver
            .solve(
                &cash_flows,
                dec!(100), // Clean price at par
                dec!(0),   // No accrued for simplicity
                settlement,
                DayCountConvention::Thirty360US,
                Frequency::SemiAnnual,
            )
            .unwrap();

        // YTM should be approximately equal to coupon rate
        assert!((result.yield_value - 0.05).abs() < 0.001);
    }

    #[test]
    fn test_ytm_discount_bond() {
        // A bond priced below par should have YTM > coupon rate
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            Frequency::SemiAnnual,
        );

        let solver = YieldSolver::new();
        let result = solver
            .solve(
                &cash_flows,
                dec!(95), // Discount price
                dec!(0),
                settlement,
                DayCountConvention::Thirty360US,
                Frequency::SemiAnnual,
            )
            .unwrap();

        // YTM should be higher than coupon rate
        assert!(result.yield_value > 0.05);
        assert!(result.yield_value < 0.10);
    }

    #[test]
    fn test_ytm_premium_bond() {
        // A bond priced above par should have YTM < coupon rate
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            Frequency::SemiAnnual,
        );

        let solver = YieldSolver::new();
        let result = solver
            .solve(
                &cash_flows,
                dec!(105), // Premium price
                dec!(0),
                settlement,
                DayCountConvention::Thirty360US,
                Frequency::SemiAnnual,
            )
            .unwrap();

        // YTM should be lower than coupon rate
        assert!(result.yield_value < 0.05);
        assert!(result.yield_value > 0.0);
    }

    #[test]
    fn test_price_yield_roundtrip() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            Frequency::SemiAnnual,
        );

        let solver = YieldSolver::new();
        let original_price = dec!(98.50);
        let accrued = dec!(1.25);

        // Calculate YTM from price
        let result = solver
            .solve(
                &cash_flows,
                original_price,
                accrued,
                settlement,
                DayCountConvention::Thirty360US,
                Frequency::SemiAnnual,
            )
            .unwrap();

        // Calculate clean price from YTM
        let calculated_clean = solver
            .clean_price_from_yield(
                &cash_flows,
                result.yield_value,
                accrued,
                settlement,
                DayCountConvention::Thirty360US,
                Frequency::SemiAnnual,
            )
            .unwrap();

        // Should round-trip to within tolerance
        let diff = (calculated_clean - original_price.to_f64().unwrap()).abs();
        assert!(diff < 0.0001, "Price roundtrip error: {}", diff);
    }

    #[test]
    fn test_current_yield() {
        let annual_coupon = dec!(7.5); // 7.5% coupon on 100 face
        let clean_price = dec!(110.503);

        let cy = current_yield(annual_coupon, clean_price).unwrap();

        // Current yield should be approximately 6.787%
        // (7.5 / 110.503 = 0.0678...)
        assert!((cy - 0.0679).abs() < 0.001);
    }

    #[test]
    fn test_boeing_bond_ytm() {
        // Boeing 7.5% 06/15/2025
        // Settlement: 04/29/2020
        // Clean Price: 110.503
        // Expected YTM (Street): 4.905895% (approximately)

        let settlement = date(2020, 4, 29);
        let _maturity = date(2025, 6, 15);

        // Generate cash flows for Boeing bond
        // Issue: ~1990, 7.5% coupon, semi-annual
        let mut cash_flows = Vec::new();

        // Coupon dates (June 15 and December 15)
        let coupon_dates = [
            date(2020, 6, 15),
            date(2020, 12, 15),
            date(2021, 6, 15),
            date(2021, 12, 15),
            date(2022, 6, 15),
            date(2022, 12, 15),
            date(2023, 6, 15),
            date(2023, 12, 15),
            date(2024, 6, 15),
            date(2024, 12, 15),
            date(2025, 6, 15), // Maturity
        ];

        let coupon_amount = dec!(3.75); // 7.5% / 2 per period
        let face_value = dec!(100);

        for (i, &cf_date) in coupon_dates.iter().enumerate() {
            if i == coupon_dates.len() - 1 {
                cash_flows.push(BondCashFlow::coupon_and_principal(
                    cf_date,
                    coupon_amount,
                    face_value,
                ));
            } else {
                cash_flows.push(BondCashFlow::coupon(cf_date, coupon_amount));
            }
        }

        // Calculate accrued interest
        // Last coupon: Dec 15, 2019, Next coupon: Jun 15, 2020
        // Settlement: Apr 29, 2020
        // Days from Dec 15 to Apr 29 = 135 days (30/360)
        // Days in period = 180 days (30/360)
        // Accrued = 3.75 * 135/180 = 2.8125
        let accrued = dec!(2.8125);
        let clean_price = dec!(110.503);

        let solver = YieldSolver::new()
            .with_convention(YieldConvention::StreetConvention)
            .with_tolerance(1e-10);

        let result = solver
            .solve(
                &cash_flows,
                clean_price,
                accrued,
                settlement,
                DayCountConvention::Thirty360US,
                Frequency::SemiAnnual,
            )
            .unwrap();

        // Expected YTM around 4.9-5.2%
        // Note: Exact match to Bloomberg requires precise day count handling
        // including irregular first coupon stub treatment
        let ytm_percent = result.yield_value * 100.0;
        assert!(
            ytm_percent > 4.5 && ytm_percent < 6.0,
            "Boeing YTM out of range: expected 4.5-6.0%, got {:.6}%",
            ytm_percent
        );

        // Current yield should be around 6.79%
        let cy = current_yield(dec!(7.5), clean_price).unwrap();
        assert!(
            (cy * 100.0 - 6.787).abs() < 0.1,
            "Boeing current yield mismatch: expected ~6.787%, got {:.6}%",
            cy * 100.0
        );
    }

    #[test]
    fn test_true_yield_convention() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            Frequency::SemiAnnual,
        );

        let street_solver = YieldSolver::new().with_convention(YieldConvention::StreetConvention);
        let true_solver = YieldSolver::new().with_convention(YieldConvention::TrueYield);

        let price = dec!(98.50);
        let accrued = dec!(0);

        let street_result = street_solver
            .solve(
                &cash_flows,
                price,
                accrued,
                settlement,
                DayCountConvention::Thirty360US,
                Frequency::SemiAnnual,
            )
            .unwrap();

        let true_result = true_solver
            .solve(
                &cash_flows,
                price,
                accrued,
                settlement,
                DayCountConvention::Thirty360US,
                Frequency::SemiAnnual,
            )
            .unwrap();

        // True yield should be slightly different from street yield
        // but both should converge
        assert!(street_result.iterations < 20);
        assert!(true_result.iterations < 20);

        // Both should be close but not identical
        let diff = (street_result.yield_value - true_result.yield_value).abs();
        assert!(
            diff < 0.005,
            "Street vs True yield difference too large: {}",
            diff
        );
    }

    #[test]
    fn test_solver_convergence() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            Frequency::SemiAnnual,
        );

        let solver = YieldSolver::new();

        // Test various prices
        for price in [80, 90, 100, 110, 120] {
            let result = solver.solve(
                &cash_flows,
                Decimal::from(price),
                dec!(0),
                settlement,
                DayCountConvention::Thirty360US,
                Frequency::SemiAnnual,
            );

            assert!(result.is_ok(), "Failed to converge for price {}", price);
            let result = result.unwrap();
            assert!(
                result.residual.abs() < 1e-8,
                "Large residual for price {}",
                price
            );
        }
    }

    #[test]
    fn test_annual_frequency() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 1, 15);
        let coupon_rate = dec!(0.04);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            Frequency::Annual,
        );

        let solver = YieldSolver::new().with_discount_basis(supplied_icma_basis(
            settlement,
            maturity,
            Frequency::Annual,
        ));
        let result = solver
            .solve(
                &cash_flows,
                dec!(100),
                dec!(0),
                settlement,
                DayCountConvention::ActActIcma,
                Frequency::Annual,
            )
            .unwrap();

        // At par, YTM = coupon rate
        assert!((result.yield_value - 0.04).abs() < 0.001);
    }

    #[test]
    fn test_off_cycle_sink_uses_fractional_period() {
        // Semi-annual ACT/ACT ICMA bond with periods 1/1 → 7/1 → 1/1.
        // Settlement at start of first period (v=0), three CFs:
        //  - on-cycle coupon on 7/1 (period 1, frac=1)
        //  - off-cycle sink on 10/1 (period 2, mid-period)
        //  - on-cycle coupon+principal on 1/1 (period 2, frac=1)
        // Expected years: 0.5, ~0.75, 1.0 (semi-annual = 2 periods/year).
        let settlement = date(2025, 1, 1);
        let p1_start = date(2025, 1, 1);
        let p1_end = date(2025, 7, 1);
        let p2_end = date(2026, 1, 1);
        let off_cycle = date(2025, 10, 1); // ~half-way through period 2

        let cfs = vec![
            BondCashFlow::coupon(p1_end, dec!(2.5)).with_accrual(p1_start, p1_end),
            BondCashFlow::principal(off_cycle, dec!(20.0)).with_accrual(p1_end, p2_end),
            BondCashFlow::coupon_and_principal(p2_end, dec!(2.5), dec!(80.0))
                .with_accrual(p1_end, p2_end),
        ];

        let projected = prepare_bond_cash(
            &cfs,
            settlement,
            DayCountConvention::ActActIcma,
            Some(&supplied_icma_basis(
                settlement,
                p2_end,
                Frequency::SemiAnnual,
            )),
        )
        .unwrap();

        assert_eq!(projected.len(), 3);
        // First coupon: full first period elapsed → 0.5y.
        assert!(
            (projected[0].0 - 0.5).abs() < 1e-9,
            "year[0]={}",
            projected[0].0
        );
        // Off-cycle sink: 0.5y (full first period) + frac*(1/2) of second period.
        // frac = days(7/1, 10/1) / days(7/1, 1/1) under ICMA day count.
        // Both periods nominal-length → frac ≈ 92/184 ≈ 0.5, so years ≈ 0.5 + 0.25 = 0.75.
        assert!(
            (projected[1].0 - 0.75).abs() < 0.01,
            "year[1]={}",
            projected[1].0
        );
        // Final coupon+principal: end of second period → 1.0y.
        assert!(
            (projected[2].0 - 1.0).abs() < 1e-9,
            "year[2]={}",
            projected[2].0
        );

        // Sanity: the off-cycle sink and the final coupon must NOT share an index
        // (regression for the old date-change-detection bug).
        assert!(projected[2].0 > projected[1].0);
        // And the off-cycle sink must sit strictly between the two coupon dates.
        assert!(projected[1].0 > projected[0].0);
        assert!(projected[1].0 < projected[2].0);
    }

    #[test]
    fn test_quarterly_frequency() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2027, 1, 15);
        let coupon_rate = dec!(0.06);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            Frequency::Quarterly,
        );

        let solver = YieldSolver::new();
        let result = solver
            .solve(
                &cash_flows,
                dec!(100),
                dec!(0),
                settlement,
                DayCountConvention::Act360,
                Frequency::Quarterly,
            )
            .unwrap();

        // At par, YTM = coupon rate
        assert!((result.yield_value - 0.06).abs() < 0.001);
    }
}
