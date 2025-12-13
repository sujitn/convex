//! Yield calculation methods.
//!
//! This module provides various yield calculation methodologies:
//!
//! - **Street Convention**: Standard market yield quote
//! - **True Yield**: Accounts for actual settlement conventions
//! - **Current Yield**: Annual coupon / clean price
//! - **Simple Yield**: Simplified yield calculation
//! - **Money Market Yields**: Discount yield, BEY, CD equivalent, MMY
//! - **Configuration**: Yield calculator configuration and presets

mod config;
mod current;
mod money_market;
mod simple;
mod street;
mod true_yield;
mod yield_calculator;

pub use config::{
    YieldCalculatorConfig, YieldCalculatorConfigBuilder, DEFAULT_MAX_ITERATIONS, DEFAULT_TOLERANCE,
    MM_THRESHOLD_CAD, MM_THRESHOLD_US,
};
pub use current::*;
pub use money_market::{
    bond_equivalent_yield, cd_equivalent_yield, discount_yield, money_market_yield,
    money_market_yield_with_horizon, price_from_money_market_yield, solve_money_market_yield,
};
pub use simple::*;
pub use street::*;
pub use true_yield::*;
pub use yield_calculator::YieldCalculator;
