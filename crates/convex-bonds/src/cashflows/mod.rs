//! Cash flow generation for bonds.
//!
//! This module provides:
//! - Schedule generation with stub handling
//! - Cash flow generation for fixed, floating, and amortizing bonds
//! - Accrued interest calculations including ex-dividend handling
//!
//! # Performance Targets
//!
//! - Schedule generation: < 1μs
//! - Cash flow generation: < 500ns
//! - Accrued calculation: < 100ns

mod accrued;
mod schedule;

pub use accrued::AccruedInterestCalculator;
pub use schedule::{Schedule, ScheduleConfig, StubType};

use rust_decimal::Decimal;

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{CashFlow, CashFlowSchedule, Date};

use crate::error::{BondError, BondResult};
use crate::instruments::{Bond, FixedBond};
use crate::types::AmortizationSchedule;

/// Generates cash flows for bonds.
///
/// Supports multiple bond types:
/// - Fixed rate bonds
/// - Floating rate bonds (with forward curve projection)
/// - Amortizing bonds (with declining notional)
/// - Inflation-linked bonds
///
/// # Example
///
/// ```rust,ignore
/// use convex_bonds::cashflows::CashFlowGenerator;
/// use convex_bonds::instruments::FixedBondBuilder;
///
/// let bond = FixedBondBuilder::new()
///     .coupon_rate(dec!(0.05))
///     .maturity(Date::from_ymd(2030, 6, 15).unwrap())
///     .build()
///     .unwrap();
///
/// let schedule = CashFlowGenerator::generate(&bond, settlement).unwrap();
/// ```
pub struct CashFlowGenerator;

impl CashFlowGenerator {
    /// Generates the cash flow schedule for a fixed bond.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond to generate cash flows for
    /// * `settlement` - Settlement date (cash flows after this date)
    pub fn generate(bond: &FixedBond, settlement: Date) -> BondResult<CashFlowSchedule> {
        let maturity = bond.maturity();

        if settlement >= maturity {
            return Err(BondError::SettlementAfterMaturity {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        let frequency = bond.frequency();
        let coupon_amount = bond.coupon_per_period();
        let face_value = bond.face_value();

        let mut schedule = CashFlowSchedule::new();

        if frequency.is_zero() {
            // Zero coupon bond - single payment at maturity
            schedule.push(CashFlow::principal(maturity, face_value));
            return Ok(schedule);
        }

        // Generate coupon dates by working backwards from maturity
        let months_per_period = frequency.months_per_period() as i32;
        let mut coupon_dates = Vec::new();

        let mut date = maturity;
        while date > settlement {
            coupon_dates.push(date);
            date = date.add_months(-months_per_period)?;
        }

        // Store the previous coupon date for accrual info
        let prev_coupon = date;

        // Reverse to get chronological order
        coupon_dates.reverse();

        // Generate cash flows with accrual periods
        for (i, &cf_date) in coupon_dates.iter().enumerate() {
            let is_final = i == coupon_dates.len() - 1;

            // Determine accrual start date
            let accrual_start = if i == 0 {
                prev_coupon
            } else {
                coupon_dates[i - 1]
            };

            if is_final {
                // Final payment includes principal
                schedule.push(CashFlow::final_payment_with_accrual(
                    cf_date,
                    coupon_amount,
                    face_value,
                    accrual_start,
                    cf_date,
                ));
            } else {
                schedule.push(CashFlow::coupon_with_accrual(
                    cf_date,
                    coupon_amount,
                    accrual_start,
                    cf_date,
                ));
            }
        }

        Ok(schedule)
    }

    /// Generates cash flows for a fixed-rate bond using a schedule.
    ///
    /// # Arguments
    ///
    /// * `schedule` - The payment schedule
    /// * `coupon_rate` - Annual coupon rate as decimal (e.g., 0.05 for 5%)
    /// * `face_value` - Face value of the bond
    /// * `day_count` - Day count convention for year fraction calculation
    /// * `settlement` - Settlement date (cash flows on or before are excluded)
    pub fn fixed_rate_from_schedule(
        schedule: &Schedule,
        coupon_rate: Decimal,
        face_value: Decimal,
        day_count: DayCountConvention,
        settlement: Date,
    ) -> CashFlowSchedule {
        let dc = day_count.to_day_count();
        let periods: Vec<_> = schedule.unadjusted_periods().collect();
        let adjusted_dates: Vec<_> = schedule.dates().to_vec();

        let mut flows = CashFlowSchedule::with_capacity(periods.len());

        for (i, (start, end)) in periods.iter().enumerate() {
            // Use adjusted date for payment
            let payment_date = adjusted_dates.get(i + 1).copied().unwrap_or(*end);

            // Skip past periods
            if payment_date <= settlement {
                continue;
            }

            let year_frac = dc.year_fraction(*start, *end);
            let coupon_amount = face_value * coupon_rate * year_frac;

            let is_final = i == periods.len() - 1;

            if is_final {
                flows.push(CashFlow::final_payment_with_accrual(
                    payment_date,
                    coupon_amount,
                    face_value,
                    *start,
                    *end,
                ));
            } else {
                flows.push(CashFlow::coupon_with_accrual(
                    payment_date,
                    coupon_amount,
                    *start,
                    *end,
                ));
            }
        }

        flows
    }

    /// Generates cash flows for a floating-rate bond.
    ///
    /// # Arguments
    ///
    /// * `schedule` - The payment schedule
    /// * `spread` - Spread over reference rate in decimal (e.g., 0.005 for 50bps)
    /// * `face_value` - Face value of the bond
    /// * `day_count` - Day count convention
    /// * `settlement` - Settlement date
    /// * `forward_rates` - Iterator of forward rates for each period
    ///
    /// # Notes
    ///
    /// If forward_rates doesn't provide enough rates, remaining periods
    /// use the last provided rate or zero if none provided.
    pub fn floating_rate(
        schedule: &Schedule,
        spread: Decimal,
        face_value: Decimal,
        day_count: DayCountConvention,
        settlement: Date,
        forward_rates: impl IntoIterator<Item = Decimal>,
    ) -> CashFlowSchedule {
        let dc = day_count.to_day_count();
        let periods: Vec<_> = schedule.unadjusted_periods().collect();
        let adjusted_dates: Vec<_> = schedule.dates().to_vec();

        let mut rates: Vec<Decimal> = forward_rates.into_iter().collect();
        let last_rate = rates.last().copied().unwrap_or(Decimal::ZERO);

        // Extend rates if needed
        while rates.len() < periods.len() {
            rates.push(last_rate);
        }

        let mut flows = CashFlowSchedule::with_capacity(periods.len());

        for (i, (start, end)) in periods.iter().enumerate() {
            let payment_date = adjusted_dates.get(i + 1).copied().unwrap_or(*end);

            if payment_date <= settlement {
                continue;
            }

            let rate = rates.get(i).copied().unwrap_or(Decimal::ZERO) + spread;
            let year_frac = dc.year_fraction(*start, *end);
            let coupon_amount = face_value * rate * year_frac;

            let is_final = i == periods.len() - 1;

            if is_final {
                let mut cf = CashFlow::floating_coupon(
                    payment_date,
                    coupon_amount + face_value,
                    *start,
                    *end,
                    rate,
                );
                cf = cf.with_notional_after(Decimal::ZERO);
                flows.push(cf);
            } else {
                flows.push(CashFlow::floating_coupon(
                    payment_date,
                    coupon_amount,
                    *start,
                    *end,
                    rate,
                ));
            }
        }

        flows
    }

    /// Generates cash flows for an amortizing bond.
    ///
    /// # Arguments
    ///
    /// * `schedule` - The payment schedule
    /// * `coupon_rate` - Annual coupon rate as decimal
    /// * `amort_schedule` - Amortization schedule with principal factors
    /// * `initial_face` - Initial face value
    /// * `day_count` - Day count convention
    /// * `settlement` - Settlement date
    pub fn amortizing(
        schedule: &Schedule,
        coupon_rate: Decimal,
        amort_schedule: &AmortizationSchedule,
        initial_face: Decimal,
        day_count: DayCountConvention,
        settlement: Date,
    ) -> CashFlowSchedule {
        let dc = day_count.to_day_count();
        let periods: Vec<_> = schedule.unadjusted_periods().collect();
        let adjusted_dates: Vec<_> = schedule.dates().to_vec();

        let mut flows = CashFlowSchedule::with_capacity(periods.len());
        let mut current_notional = initial_face;

        for (i, (start, end)) in periods.iter().enumerate() {
            let payment_date = adjusted_dates.get(i + 1).copied().unwrap_or(*end);

            if payment_date <= settlement {
                // Update notional for past periods
                let factor = Decimal::try_from(amort_schedule.factor_as_of(payment_date))
                    .unwrap_or(Decimal::ONE);
                current_notional = initial_face * factor;
                continue;
            }

            let year_frac = dc.year_fraction(*start, *end);
            let coupon_amount = current_notional * coupon_rate * year_frac;

            // Get the new factor after this payment
            let new_factor = Decimal::try_from(amort_schedule.factor_as_of(payment_date))
                .unwrap_or(Decimal::ONE);
            let new_notional = initial_face * new_factor;
            let principal_payment = current_notional - new_notional;

            if principal_payment > Decimal::ZERO {
                // Has principal payment
                let cf = CashFlow::new(
                    payment_date,
                    coupon_amount + principal_payment,
                    convex_core::types::CashFlowType::CouponAndPrincipal,
                )
                .with_accrual(*start, *end)
                .with_notional_after(new_notional);
                flows.push(cf);
            } else {
                flows.push(CashFlow::coupon_with_accrual(
                    payment_date,
                    coupon_amount,
                    *start,
                    *end,
                ));
            }

            current_notional = new_notional;
        }

        flows
    }

    /// Generates cash flows for an inflation-linked bond.
    ///
    /// # Arguments
    ///
    /// * `schedule` - The payment schedule
    /// * `real_coupon` - Real coupon rate as decimal
    /// * `face_value` - Face value of the bond
    /// * `index_ratio` - Function returning inflation index ratio for each date
    /// * `settlement` - Settlement date
    pub fn inflation_linked<F>(
        schedule: &Schedule,
        real_coupon: Decimal,
        face_value: Decimal,
        settlement: Date,
        index_ratio: F,
    ) -> CashFlowSchedule
    where
        F: Fn(Date) -> Decimal,
    {
        let periods: Vec<_> = schedule.unadjusted_periods().collect();
        let adjusted_dates: Vec<_> = schedule.dates().to_vec();

        let periods_per_year = Decimal::from(
            (12 / (schedule.dates().len().saturating_sub(1).max(1) as u32))
                .max(1),
        );

        let mut flows = CashFlowSchedule::with_capacity(periods.len());

        for (i, (start, end)) in periods.iter().enumerate() {
            let payment_date = adjusted_dates.get(i + 1).copied().unwrap_or(*end);

            if payment_date <= settlement {
                continue;
            }

            let ratio = index_ratio(payment_date);
            let adjusted_face = face_value * ratio;
            let coupon_amount = adjusted_face * real_coupon / periods_per_year;

            let is_final = i == periods.len() - 1;

            if is_final {
                // Final payment: inflation-adjusted coupon + principal
                // Combine into single payment
                let combined = CashFlow::new(
                    payment_date,
                    coupon_amount + adjusted_face,
                    convex_core::types::CashFlowType::CouponAndPrincipal,
                )
                .with_accrual(*start, *end);
                flows.push(combined);
            } else {
                flows.push(CashFlow::inflation_coupon(
                    payment_date,
                    coupon_amount,
                    *start,
                    *end,
                ));
            }
        }

        flows
    }

    /// Calculates accrued interest for a bond.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond
    /// * `settlement` - Settlement date
    pub fn accrued_interest(bond: &FixedBond, settlement: Date) -> BondResult<Decimal> {
        let frequency = bond.frequency();

        if frequency.is_zero() {
            return Ok(Decimal::ZERO);
        }

        let coupon_amount = bond.coupon_per_period();
        let months_per_period = frequency.months_per_period() as i32;

        // Find the previous coupon date
        let mut prev_coupon = bond.maturity();
        while prev_coupon > settlement {
            prev_coupon = prev_coupon.add_months(-months_per_period)?;
        }

        // Find the next coupon date
        let next_coupon = prev_coupon.add_months(months_per_period)?;

        // Calculate accrued as proportion of period
        let days_accrued = prev_coupon.days_between(&settlement);
        let days_in_period = prev_coupon.days_between(&next_coupon);

        if days_in_period == 0 {
            return Ok(Decimal::ZERO);
        }

        let accrued_fraction = Decimal::from(days_accrued) / Decimal::from(days_in_period);

        Ok(coupon_amount * accrued_fraction)
    }

    /// Calculates accrued interest using the day count convention.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond
    /// * `settlement` - Settlement date
    /// * `day_count` - Day count convention to use
    pub fn accrued_interest_with_daycount(
        bond: &FixedBond,
        settlement: Date,
        day_count: DayCountConvention,
    ) -> BondResult<Decimal> {
        let frequency = bond.frequency();

        if frequency.is_zero() {
            return Ok(Decimal::ZERO);
        }

        let months_per_period = frequency.months_per_period() as i32;

        // Find the previous coupon date
        let mut prev_coupon = bond.maturity();
        while prev_coupon > settlement {
            prev_coupon = prev_coupon.add_months(-months_per_period)?;
        }

        // Find the next coupon date
        let next_coupon = prev_coupon.add_months(months_per_period)?;

        // Use AccruedInterestCalculator
        Ok(AccruedInterestCalculator::standard(
            settlement,
            prev_coupon,
            next_coupon,
            bond.coupon_rate(),
            bond.face_value(),
            day_count,
            frequency,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::FixedBondBuilder;
    use convex_core::types::{Currency, Frequency};
    use rust_decimal_macros::dec;

    fn create_test_bond() -> FixedBond {
        FixedBondBuilder::new()
            .isin("TEST")
            .coupon_rate(dec!(0.05))
            .maturity(Date::from_ymd(2027, 6, 15).unwrap())
            .frequency(Frequency::SemiAnnual)
            .currency(Currency::USD)
            .build()
            .unwrap()
    }

    #[test]
    fn test_generate_cashflows() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let schedule = CashFlowGenerator::generate(&bond, settlement).unwrap();

        assert!(!schedule.is_empty());

        // Last cash flow should include principal
        let last_cf = schedule.as_slice().last().unwrap();
        assert!(last_cf.is_principal());
    }

    #[test]
    fn test_cashflows_have_accrual_info() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let schedule = CashFlowGenerator::generate(&bond, settlement).unwrap();

        // All cash flows should have accrual period info
        for cf in schedule.iter() {
            assert!(cf.accrual_start().is_some());
            assert!(cf.accrual_end().is_some());
        }
    }

    #[test]
    fn test_accrued_interest() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 3, 15).unwrap();

        let accrued = CashFlowGenerator::accrued_interest(&bond, settlement).unwrap();

        // Should be positive (some days into the coupon period)
        assert!(accrued > Decimal::ZERO);
        // Should be less than full coupon
        assert!(accrued < bond.coupon_per_period());
    }

    #[test]
    fn test_settlement_after_maturity() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2028, 1, 15).unwrap();

        let result = CashFlowGenerator::generate(&bond, settlement);
        assert!(result.is_err());
    }

    #[test]
    fn test_fixed_rate_from_schedule() {
        let config = ScheduleConfig::new(
            Date::from_ymd(2020, 1, 15).unwrap(),
            Date::from_ymd(2025, 1, 15).unwrap(),
            Frequency::SemiAnnual,
        );

        let schedule = Schedule::generate(config).unwrap();
        let settlement = Date::from_ymd(2020, 1, 15).unwrap();

        let flows = CashFlowGenerator::fixed_rate_from_schedule(
            &schedule,
            dec!(0.05),
            dec!(100),
            DayCountConvention::Thirty360US,
            settlement,
        );

        assert_eq!(flows.len(), 10);

        // Check last flow includes principal
        let last = flows.as_slice().last().unwrap();
        assert!(last.is_principal());
        assert!(last.amount() > dec!(100)); // Includes coupon + principal
    }

    #[test]
    fn test_floating_rate_cashflows() {
        let config = ScheduleConfig::new(
            Date::from_ymd(2024, 1, 15).unwrap(),
            Date::from_ymd(2025, 1, 15).unwrap(),
            Frequency::Quarterly,
        );

        let schedule = Schedule::generate(config).unwrap();
        let settlement = Date::from_ymd(2024, 1, 15).unwrap();

        // Forward rates for each quarter
        let forward_rates = vec![dec!(0.04), dec!(0.0425), dec!(0.045), dec!(0.0475)];
        let spread = dec!(0.005); // 50bps

        let flows = CashFlowGenerator::floating_rate(
            &schedule,
            spread,
            dec!(100),
            DayCountConvention::Act360,
            settlement,
            forward_rates,
        );

        assert_eq!(flows.len(), 4);

        // All flows should have reference rates
        for cf in flows.iter() {
            assert!(cf.reference_rate().is_some());
        }
    }

    #[test]
    fn test_boeing_accrued_validation() {
        // Boeing 7.5% 06/15/2025
        // Per $1M face value
        // Settlement: 10/27/2019
        // Last coupon: 06/15/2019
        // Next coupon: 12/15/2019

        let settlement = Date::from_ymd(2019, 10, 27).unwrap();
        let last_coupon = Date::from_ymd(2019, 6, 15).unwrap();
        let next_coupon = Date::from_ymd(2019, 12, 15).unwrap();

        let accrued = AccruedInterestCalculator::standard(
            settlement,
            last_coupon,
            next_coupon,
            dec!(0.075),  // 7.5%
            dec!(1_000_000),
            DayCountConvention::Thirty360US,
            Frequency::SemiAnnual,
        );

        // 30/360: Jun 15 to Oct 27
        // Jun: 15 days (15 to 30)
        // Jul: 30 days
        // Aug: 30 days
        // Sep: 30 days
        // Oct: 27 days
        // Total: 15 + 30 + 30 + 30 + 27 = 132 days

        // Period coupon = 1,000,000 * 0.075 / 2 = 37,500
        // Accrued = 37,500 * 132 / 180 = 27,500

        // Allow some tolerance for rounding differences
        assert!(accrued >= dec!(27000));
        assert!(accrued <= dec!(28000));
    }
}
