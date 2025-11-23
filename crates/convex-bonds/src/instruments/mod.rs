//! Bond instrument types.
//!
//! This module provides various bond types:
//!
//! - [`FixedBond`]: Fixed coupon bonds
//! - [`ZeroCouponBond`]: Zero coupon (discount) bonds

mod fixed;
mod zero_coupon;

pub use fixed::{FixedBond, FixedBondBuilder};
pub use zero_coupon::ZeroCouponBond;

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
