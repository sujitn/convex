//! Domain types for fixed income analytics.
//!
//! This module provides type-safe representations of financial concepts:
//!
//! - [`Date`]: Calendar date for financial calculations
//! - [`Price`]: Bond price with currency
//! - [`Yield`]: Yield value with compounding convention
//! - [`YieldMethod`]: Yield calculation methodology
//! - [`Spread`]: Spread in basis points
//! - [`CashFlow`]: Dated cash flow amount
//! - [`Currency`]: ISO currency codes
//! - [`Frequency`]: Payment frequency
//! - [`Compounding`]: Interest compounding convention
//! - [`MarketConvention`]: Market-specific bond conventions

mod cashflow;
mod currency;
mod date;
mod frequency;
mod market_convention;
mod price;
mod spread;
mod yield_method;
mod yield_type;

pub use cashflow::{CashFlow, CashFlowSchedule, CashFlowType};
pub use currency::Currency;
pub use date::Date;
pub use frequency::{
    convert_rate, effective_annual_rate, nominal_rate_from_ear, Compounding, Frequency,
};
pub use market_convention::MarketConvention;
pub use price::Price;
pub use spread::{Spread, SpreadType};
pub use yield_method::YieldMethod;
pub use yield_type::Yield;
