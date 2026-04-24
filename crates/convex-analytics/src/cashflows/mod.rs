//! Cash flow primitives: schedule, accrued interest, settlement rules.

mod accrued;
pub mod irregular;
pub mod schedule;
pub mod settlement;

pub use accrued::AccruedInterestCalculator;
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

    /// Boeing 7.5% 06/15/2025 · $1M face · settle 10/27/2019.
    /// Last coupon 06/15/2019, next 12/15/2019. 30/360 US gives 132 days
    /// accrued of 180 → 1,000,000 × 0.075 / 2 × 132/180 ≈ 27,500.
    #[test]
    fn test_boeing_accrued_validation() {
        let accrued = AccruedInterestCalculator::standard(
            Date::from_ymd(2019, 10, 27).unwrap(),
            Date::from_ymd(2019, 6, 15).unwrap(),
            Date::from_ymd(2019, 12, 15).unwrap(),
            dec!(0.075),
            dec!(1_000_000),
            DayCountConvention::Thirty360US,
            Frequency::SemiAnnual,
        );
        assert!((dec!(27000)..=dec!(28000)).contains(&accrued));
    }
}
