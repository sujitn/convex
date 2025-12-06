//! Core bond traits and extensions.
//!
//! This module defines the trait hierarchy for bonds:
//!
//! - [`Bond`]: Base trait for all bonds
//! - [`FixedCouponBond`]: Extension for fixed rate bonds
//! - [`FloatingCouponBond`]: Extension for floating rate bonds
//! - [`EmbeddedOptionBond`]: Extension for callable/puttable bonds
//! - [`AmortizingBond`]: Extension for amortizing bonds
//! - [`InflationLinkedBond`]: Extension for inflation-linked bonds

mod bond;
mod extensions;

pub use bond::{Bond, BondCashFlow, CashFlowType};
pub use extensions::{
    AmortizingBond, EmbeddedOptionBond, FixedCouponBond, FloatingCouponBond, InflationLinkedBond,
};
