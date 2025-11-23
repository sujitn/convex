//! Cash flow generation for bonds.

use rust_decimal::Decimal;

use convex_core::types::{CashFlow, CashFlowSchedule, Date};

use crate::error::{BondError, BondResult};
use crate::instruments::{Bond, FixedBond};

/// Generates cash flows for bonds.
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

        // Reverse to get chronological order
        coupon_dates.reverse();

        // Generate cash flows
        for (i, &cf_date) in coupon_dates.iter().enumerate() {
            let is_final = i == coupon_dates.len() - 1;

            if is_final {
                // Final payment includes principal
                schedule.push(CashFlow::final_payment(cf_date, coupon_amount, face_value));
            } else {
                schedule.push(CashFlow::coupon(cf_date, coupon_amount));
            }
        }

        Ok(schedule)
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

        let accrued_fraction =
            Decimal::from(days_accrued) / Decimal::from(days_in_period);

        Ok(coupon_amount * accrued_fraction)
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
}
