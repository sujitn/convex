//! Bond instrument types.
//!
//! This module provides various bond types:
//!
//! - [`FixedBond`]: Simple fixed coupon bonds
//! - [`FixedRateBond`]: Full-featured fixed rate bonds with conventions
//! - [`ZeroCouponBond`]: Zero coupon (discount) bonds
//! - [`FloatingRateNote`]: Floating rate notes (FRNs) with SOFR, SONIA, €STR support
//! - [`CallableBond`]: Callable bonds with YTC/YTW calculations
//! - [`SinkingFundBond`]: Sinking fund bonds with average life calculations

mod callable;
mod fixed;
mod fixed_rate;
mod floating_rate;
mod sinking_fund;
mod zero_coupon;

pub use callable::{CallableBond, CallableBondBuilder};
pub use fixed::{FixedBond, FixedBondBuilder};
pub use fixed_rate::{FixedRateBond, FixedRateBondBuilder};
pub use floating_rate::{FloatingRateNote, FloatingRateNoteBuilder};
pub use sinking_fund::{
    AccelerationOption, SinkingFundBond, SinkingFundBondBuilder, SinkingFundPayment,
    SinkingFundSchedule,
};
pub use zero_coupon::{convert_yield, Compounding, ZeroCouponBond, ZeroCouponBondBuilder};

use convex_core::types::{Currency, Date, Frequency};
use rust_decimal::Decimal;

/// Common bond attributes.
pub trait Bond {
    /// Returns the ISIN or other identifier.
    fn identifier(&self) -> &str;

    /// Returns the maturity date.
    fn maturity(&self) -> Date;

    /// Returns the currency.
    fn currency(&self) -> Currency;

    /// Returns the face value (typically 100).
    fn face_value(&self) -> Decimal;

    /// Returns the coupon frequency.
    fn frequency(&self) -> Frequency;

    /// Returns true if this is a zero coupon bond.
    fn is_zero_coupon(&self) -> bool {
        self.frequency().is_zero()
    }
}
