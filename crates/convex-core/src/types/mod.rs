//! Domain types for fixed income analytics.
//!
//! This module provides type-safe representations of financial concepts:
//!
//! - [`Date`]: Calendar date for financial calculations
//! - [`Price`]: Bond price with currency
//! - [`Yield`]: Yield value with compounding convention
//! - [`Spread`]: Spread in basis points
//! - [`WidthSchedule`]: A two-way quote's width by trade size, in the quoting unit
//! - [`CashFlow`]: Dated cash flow amount
//! - [`Currency`]: ISO currency codes
//! - [`Frequency`]: Payment frequency
//! - [`Compounding`]: Interest compounding convention

mod cashflow;
mod currency;
mod date;
mod frequency;
mod mark;
mod price;
mod quote_display;
mod quote_width;
mod spread;
mod yield_type;

pub use cashflow::{CashFlow, CashFlowSchedule, CashFlowType};
pub use currency::Currency;
pub use date::Date;
pub use frequency::{Compounding, Frequency};
pub use mark::{Mark, PriceKind, QuoteBasis};
pub use price::Price;
pub use quote_width::{
    QuoteWidth, QuoteWidthError, SizeBucket, TwoWayMark, WidthFactor, WidthSchedule, WidthUnit,
    MAX_SIZE_BUCKETS, MAX_WIDTH_FACTOR,
};
pub use spread::{Spread, SpreadType};
pub use yield_type::Yield;
