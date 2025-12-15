//! Yield calculation methods.
//!
//! This module provides various yield calculation methodologies:
//!
//! - **Street Convention**: Standard market yield quote
//! - **True Yield**: Accounts for actual settlement conventions
//! - **Current Yield**: Annual coupon / clean price
//! - **Simple Yield**: Simplified yield calculation
//! - **Money Market Yields**: Discount yield, BEY, CD equivalent, MMY
//!
//! # Unified Yield Engine
//!
//! For comprehensive yield calculations with full convention support,
//! use the unified [`YieldEngine`] and [`YieldCalculationRules`] from
//! `convex_bonds::pricing`:
//!
//! ```rust,ignore
//! use convex_bonds::pricing::{StandardYieldEngine, YieldEngine};
//! use convex_bonds::types::YieldCalculationRules;
//!
//! let engine = StandardYieldEngine::default();
//! let rules = YieldCalculationRules::us_treasury();
//! // Use engine.yield_from_price() or engine.price_from_yield()
//! ```
//!
//! The unified engine supports:
//! - All market conventions (US, UK, EUR, JP, etc.)
//! - Ex-dividend handling
//! - Irregular period calculations
//! - Short-dated bond methodologies

mod current;
mod money_market;
mod simple;
mod street;
mod true_yield;

pub use current::*;
pub use money_market::{
    bond_equivalent_yield, cd_equivalent_yield, discount_yield, money_market_yield,
    money_market_yield_with_horizon,
};
pub use simple::*;
pub use street::*;
pub use true_yield::*;

// Re-export unified yield calculation types from convex-bonds
// These provide comprehensive convention support for all markets
pub use convex_bonds::conventions::{ConventionKey, ConventionRegistry, InstrumentType, Market};
pub use convex_bonds::pricing::{
    RollForwardMethod, ShortDateCalculator, StandardYieldEngine, YieldEngine, YieldEngineResult,
};
pub use convex_bonds::types::{
    AccruedConvention, CompoundingMethod, ExDividendRules, RoundingConvention, SettlementRules,
    StubPeriodRules, YieldCalculationRules, YieldConvention,
};
