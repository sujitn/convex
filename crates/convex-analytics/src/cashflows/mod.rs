//! Cash flow primitives: schedule, accrued interest, settlement rules.

mod accrued;
pub mod irregular;
pub mod schedule;
pub mod settlement;

pub use accrued::AccruedInterestCalculator;
pub use convex_core::cashflows::{
    principal_cashflows, settlement_date, BoundarySide, CashRounding, CashflowError,
    CouponCalculation, CouponPeriod, CouponPrincipal, CouponProfiles, CouponSchedule, DateOffset,
    DateRules, DatedProfile, DatedValue, ExCouponAccrual, ExCouponBoundary, ExCouponRule,
    ExDateAnchor, NegativeCouponPolicy, PaymentDateEntitlement, ProfileKind, ScheduleSpec,
};
pub use irregular::{IrregularPeriodHandler, IrregularStubType, ReferenceMethod};
pub use schedule::{Schedule, ScheduleConfig, StubType};
pub use settlement::{
    DayType, ExDivAccruedMethod, ExDividendRules, SettlementCalculator, SettlementRules,
    SettlementStatus,
};
