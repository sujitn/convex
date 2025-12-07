//! Core bond traits and extensions.
//!
//! This module defines the trait hierarchy for bonds:
//!
//! - [`Bond`]: Base trait for all bonds
//! - [`BondAnalytics`]: Blanket implementations for common analytics
//! - [`FixedCouponBond`]: Extension for fixed rate bonds
//! - [`FloatingCouponBond`]: Extension for floating rate bonds
//! - [`EmbeddedOptionBond`]: Extension for callable/puttable bonds
//! - [`AmortizingBond`]: Extension for amortizing bonds
//! - [`InflationLinkedBond`]: Extension for inflation-linked bonds

mod analytics;
mod bond;
mod extensions;

pub use analytics::BondAnalytics;
pub use bond::{Bond, BondCashFlow, CashFlowType};
pub use extensions::{
    AmortizingBond, EmbeddedOptionBond, FixedCouponBond, FloatingCouponBond, InflationLinkedBond,
};
