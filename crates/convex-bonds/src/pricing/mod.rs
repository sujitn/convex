//! Bond pricing calculations.
//!
//! This module provides:
//! - [`YieldSolver`]: Bloomberg YAS-style yield-to-maturity solver
//! - [`YieldEngine`]: Unified yield calculation trait
//! - [`StandardYieldEngine`]: Standard implementation of `YieldEngine`
//! - [`BondPricer`]: High-level pricing interface
//! - [`PriceResult`]: Result type for pricing calculations
//! - [`current_yield`]: Current yield calculation

pub mod short_date;
mod yield_engine;
mod yield_solver;

pub use short_date::{RollForwardMethod, ShortDateCalculator};
pub use yield_engine::{
    bond_equivalent_yield, current_yield_simple, discount_yield, simple_yield, StandardYieldEngine,
    YieldEngine, YieldEngineResult,
};
pub use yield_solver::{current_yield, current_yield_from_bond, YieldResult, YieldSolver};

use std::str::FromStr;

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_core::daycounts::DayCountConvention;
use convex_core::traits::YieldCurve;
use convex_core::types::{Date, Price};

use crate::cashflows::CashFlowGenerator;
use crate::error::{BondError, BondResult};
use crate::instruments::{Bond, FixedBond};
use crate::traits::BondCashFlow;

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

impl PriceResult {
    /// Returns the clean price as a percentage of par.
    #[must_use]
    pub fn clean_price_percent(&self) -> Decimal {
        self.clean_price.as_percentage()
    }

    /// Returns the dirty price as a percentage of par.
    #[must_use]
    pub fn dirty_price_percent(&self) -> Decimal {
        self.dirty_price.as_percentage()
    }
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

    /// Calculates the yield-to-maturity given a clean price.
    ///
    /// Uses the frequency and day-count convention declared on the bond,
    /// delegating to [`YieldSolver`] for the actual root-finding.
    pub fn yield_to_maturity(
        bond: &FixedBond,
        clean_price: Price,
        settlement: Date,
    ) -> BondResult<Decimal> {
        let cash_flows = bond_cash_flows(bond, settlement)?;
        let accrued = CashFlowGenerator::accrued_interest(bond, settlement)?;
        let day_count = parse_bond_day_count(bond)?;

        let result = YieldSolver::new().solve(
            &cash_flows,
            clean_price.as_percentage(),
            accrued,
            settlement,
            day_count,
            bond.frequency(),
        )?;

        Ok(Decimal::from_f64_retain(result.yield_value).unwrap_or(Decimal::ZERO))
    }

    /// Calculates price from yield.
    ///
    /// `yield_value` is a decimal rate (0.05 == 5%).
    pub fn price_from_yield(
        bond: &FixedBond,
        yield_value: Decimal,
        settlement: Date,
    ) -> BondResult<PriceResult> {
        let cash_flows = bond_cash_flows(bond, settlement)?;
        let accrued = CashFlowGenerator::accrued_interest(bond, settlement)?;
        let day_count = parse_bond_day_count(bond)?;
        let y = yield_value.to_f64().unwrap_or(0.05);

        let dirty_f64 = YieldSolver::new().dirty_price_from_yield(
            &cash_flows,
            y,
            settlement,
            day_count,
            bond.frequency(),
        );
        let dirty_price =
            Decimal::from_f64_retain(dirty_f64).unwrap_or(Decimal::ONE_HUNDRED);
        let clean_price = dirty_price - accrued;

        Ok(PriceResult {
            clean_price: Price::new(clean_price, bond.currency()),
            dirty_price: Price::new(dirty_price, bond.currency()),
            accrued_interest: accrued,
            present_value: dirty_price,
        })
    }
}

fn bond_cash_flows(bond: &FixedBond, settlement: Date) -> BondResult<Vec<BondCashFlow>> {
    let schedule = CashFlowGenerator::generate(bond, settlement)?;
    Ok(schedule
        .iter()
        .map(|cf| BondCashFlow::coupon(cf.date(), cf.amount()))
        .collect())
}

fn parse_bond_day_count(bond: &FixedBond) -> BondResult<DayCountConvention> {
    DayCountConvention::from_str(bond.day_count()).map_err(|e| {
        BondError::invalid_spec(format!(
            "bond '{}' has unrecognized day count '{}': {}",
            bond.identifier(),
            bond.day_count(),
            e
        ))
    })
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
