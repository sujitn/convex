//! Bond instrument types.

mod callable;
mod fixed_rate;
mod floating_rate;
mod sinking_fund;
mod zero_coupon;

pub use callable::{CallableBond, CallableBondBuilder};
pub use fixed_rate::{FixedRateBond, FixedRateBondBuilder};
pub use floating_rate::{FloatingRateNote, FloatingRateNoteBuilder};
pub use sinking_fund::{
    AccelerationOption, SinkingFundBond, SinkingFundBondBuilder, SinkingFundPayment,
    SinkingFundSchedule,
};
pub use zero_coupon::{convert_yield, Compounding, ZeroCouponBond, ZeroCouponBondBuilder};

// Canonical `Bond` trait lives in `crate::traits`; the legacy local trait
// that used to shadow it was only referenced by the removed `FixedBond`.
pub use crate::traits::Bond;
