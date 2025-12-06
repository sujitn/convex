//! Bond pricing calculations.
//!
//! This module provides:
//! - [`YieldSolver`]: Bloomberg YAS-style yield-to-maturity solver
//! - [`BondPricer`]: High-level pricing interface
//! - [`PriceResult`]: Result type for pricing calculations
//! - [`current_yield`]: Current yield calculation

mod yield_solver;

pub use yield_solver::{current_yield, current_yield_from_bond, YieldResult, YieldSolver};

use rust_decimal::Decimal;

use convex_core::traits::YieldCurve;
use convex_core::types::{Date, Price};
use convex_math::solvers::{newton_raphson, SolverConfig};

use crate::cashflows::CashFlowGenerator;
use crate::error::{BondError, BondResult};
use crate::instruments::{Bond, FixedBond};

/// Result of a bond pricing calculation.
#[derive(Debug, Clone)]
pub struct PriceResult {
    /// Clean price (excluding accrued interest).
    pub clean_price: Price,
    /// Dirty price (including accrued interest).
    pub dirty_price: Price,
    /// Accrued interest.
    pub accrued_interest: Decimal,
    /// Present value of cash flows.
    pub present_value: Decimal,
}

/// Bond pricing engine.
pub struct BondPricer;

impl BondPricer {
    /// Prices a bond using a yield curve.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond to price
    /// * `curve` - The discount curve
    /// * `settlement` - Settlement date
    pub fn price(
        bond: &FixedBond,
        curve: &dyn YieldCurve,
        settlement: Date,
    ) -> BondResult<PriceResult> {
        let schedule = CashFlowGenerator::generate(bond, settlement)?;
        let accrued = CashFlowGenerator::accrued_interest(bond, settlement)?;

        // Calculate PV of all cash flows
        let mut pv = Decimal::ZERO;
        for cf in schedule.iter() {
            let df = curve.discount_factor(cf.date()).map_err(|e| {
                BondError::pricing_failed(format!("Failed to get discount factor: {e}"))
            })?;
            pv += cf.amount() * df;
        }

        // Clean price = PV - Accrued
        let dirty_price = pv;
        let clean_price = dirty_price - accrued;

        Ok(PriceResult {
            clean_price: Price::new(clean_price, bond.currency()),
            dirty_price: Price::new(dirty_price, bond.currency()),
            accrued_interest: accrued,
            present_value: pv,
        })
    }

    /// Calculates the yield-to-maturity given a price.
    ///
    /// Uses Newton-Raphson iteration to find the yield.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond
    /// * `clean_price` - Market clean price
    /// * `settlement` - Settlement date
    pub fn yield_to_maturity(
        bond: &FixedBond,
        clean_price: Price,
        settlement: Date,
    ) -> BondResult<Decimal> {
        let schedule = CashFlowGenerator::generate(bond, settlement)?;
        let accrued = CashFlowGenerator::accrued_interest(bond, settlement)?;

        let target_dirty_price = clean_price.as_percentage() + accrued;
        let target = target_dirty_price.to_string().parse::<f64>().unwrap_or(100.0);

        // Initial guess based on coupon rate
        let coupon_rate = bond.coupon_rate().to_string().parse::<f64>().unwrap_or(0.05);
        let initial_guess = coupon_rate;

        // Objective: PV(yield) - target_price = 0
        let objective = |y: f64| {
            let mut pv = 0.0;
            for cf in schedule.iter() {
                let t = settlement.days_between(&cf.date()) as f64 / 365.0;
                let df = 1.0 / (1.0 + y / 2.0).powf(2.0 * t); // Semi-annual compounding
                let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);
                pv += amount * df;
            }
            pv - target
        };

        let derivative = |y: f64| {
            let mut dpv = 0.0;
            for cf in schedule.iter() {
                let t = settlement.days_between(&cf.date()) as f64 / 365.0;
                let df = 1.0 / (1.0 + y / 2.0).powf(2.0 * t);
                let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);
                // d(df)/dy = -t * df / (1 + y/2)
                dpv += amount * (-t) * df / (1.0 + y / 2.0);
            }
            dpv
        };

        let config = SolverConfig::new(1e-10, 100);
        let result = newton_raphson(objective, derivative, initial_guess, &config)
            .map_err(|_| BondError::YieldConvergenceFailed { iterations: 100 })?;

        Ok(Decimal::from_f64_retain(result.root).unwrap_or(Decimal::ZERO))
    }

    /// Calculates the price given a yield.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond
    /// * `yield_value` - Yield (as decimal, e.g., 0.05 for 5%)
    /// * `settlement` - Settlement date
    pub fn price_from_yield(
        bond: &FixedBond,
        yield_value: Decimal,
        settlement: Date,
    ) -> BondResult<PriceResult> {
        let schedule = CashFlowGenerator::generate(bond, settlement)?;
        let accrued = CashFlowGenerator::accrued_interest(bond, settlement)?;

        let y = yield_value.to_string().parse::<f64>().unwrap_or(0.05);

        // Calculate PV using yield
        let mut pv = 0.0;
        for cf in schedule.iter() {
            let t = settlement.days_between(&cf.date()) as f64 / 365.0;
            let df = 1.0 / (1.0 + y / 2.0).powf(2.0 * t);
            let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);
            pv += amount * df;
        }

        let dirty_price = Decimal::from_f64_retain(pv).unwrap_or(Decimal::ONE_HUNDRED);
        let clean_price = dirty_price - accrued;

        Ok(PriceResult {
            clean_price: Price::new(clean_price, bond.currency()),
            dirty_price: Price::new(dirty_price, bond.currency()),
            accrued_interest: accrued,
            present_value: dirty_price,
        })
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
            .maturity(Date::from_ymd(2030, 6, 15).unwrap())
            .frequency(Frequency::SemiAnnual)
            .currency(Currency::USD)
            .build()
            .unwrap()
    }

    #[test]
    fn test_price_from_yield() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let result = BondPricer::price_from_yield(&bond, dec!(0.05), settlement).unwrap();

        // At par yield, price should be close to par
        let clean = result.clean_price.as_percentage();
        assert!(clean > dec!(95) && clean < dec!(105));
    }

    #[test]
    fn test_yield_to_maturity() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let clean_price = Price::new(dec!(100.0), Currency::USD);

        let ytm = BondPricer::yield_to_maturity(&bond, clean_price, settlement).unwrap();

        // At par, YTM should be close to coupon rate
        assert!(ytm > dec!(0.04) && ytm < dec!(0.06));
    }

    #[test]
    fn test_price_yield_roundtrip() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        // Price at 5% yield
        let result1 = BondPricer::price_from_yield(&bond, dec!(0.05), settlement).unwrap();

        // Calculate YTM from that price
        let ytm = BondPricer::yield_to_maturity(&bond, result1.clean_price, settlement).unwrap();

        // Should get back approximately 5%
        let ytm_f64 = ytm.to_string().parse::<f64>().unwrap();
        assert!((ytm_f64 - 0.05).abs() < 0.001);
    }
}
