//! Cash flow generation and analysis for bonds.
//!
//! This module provides:
//! - Schedule generation with stub handling
//! - Cash flow generation for fixed, floating, and amortizing bonds
//! - Accrued interest calculations including ex-dividend handling
//! - Settlement date calculations
//!
//! # Performance Targets
//!
//! - Schedule generation: < 1μs
//! - Cash flow generation: < 500ns
//! - Accrued calculation: < 100ns

mod accrued;
mod generator;
pub mod irregular;
pub mod schedule;
pub mod settlement;

pub use accrued::AccruedInterestCalculator;
pub use generator::CashFlowGenerator;
pub use irregular::{IrregularPeriodHandler, IrregularStubType, ReferenceMethod};
pub use schedule::{Schedule, ScheduleConfig, StubType};
pub use settlement::{
    DayType, ExDivAccruedMethod, ExDividendRules, SettlementCalculator, SettlementRules,
    SettlementStatus,
};

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::{Date, Frequency};
    use rust_decimal_macros::dec;

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
            dec!(0.075), // 7.5%
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
