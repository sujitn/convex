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
