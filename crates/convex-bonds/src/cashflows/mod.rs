//! Cash flow primitives.
//!
//! - Schedule generation with stub handling
//! - Accrued interest calculation (`AccruedInterestCalculator`)
//! - Irregular-period handling for bonds with odd first/last coupons
//! - Settlement calculations

mod accrued;
pub mod irregular;
mod schedule;
pub mod settlement;

pub use accrued::AccruedInterestCalculator;
pub use irregular::IrregularPeriodHandler;
pub use schedule::{Schedule, ScheduleConfig, StubType};
pub use settlement::{SettlementCalculator, SettlementStatus};
