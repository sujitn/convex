//! Cash flow primitives.
//!
//! - Schedule generation with stub handling
//! - Accrued interest calculation (`AccruedInterestCalculator`)
//! - Irregular-period handling for bonds with odd first/last coupons
//! - Settlement calculations
//! - Cash entitled between two settlement dates (`entitled_cash`)

mod accrued;
mod entitled;
pub mod irregular;
mod schedule;
pub mod settlement;

pub use accrued::AccruedInterestCalculator;
pub use entitled::entitled_cash;
pub(crate) use entitled::per_100_face;
pub use irregular::IrregularPeriodHandler;
pub use schedule::{Schedule, ScheduleConfig, StubType};
// Shared native building blocks retain one implementation below product crates.
pub use convex_core::cashflows::{
    principal_cashflows, settlement_date, BoundarySide, CashRounding, CashflowError,
    CouponCalculation, CouponPeriod, CouponPrincipal, CouponProfiles, CouponSchedule, DateOffset,
    DateRules, DatedProfile, DatedValue, ExCouponAccrual, ExCouponBoundary, ExCouponRule,
    ExDateAnchor, NegativeCouponPolicy, PaymentDateEntitlement, ProfileKind, ScheduleSpec,
};
pub use settlement::{SettlementCalculator, SettlementStatus};
