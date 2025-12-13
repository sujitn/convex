//! # Convex Core
//!
//! Core types, traits, and abstractions for the Convex fixed income analytics library.
//!
//! This crate provides the foundational building blocks used throughout Convex:
//!
//! - **Types**: Domain-specific types like `Date`, `Price`, `Yield`, `Currency`
//! - **Day Count Conventions**: Industry-standard day count fraction calculations
//! - **Business Day Calendars**: Holiday calendars for different markets
//! - **Traits**: Core abstractions for curves, pricing engines, and risk calculators
//!
//! ## Design Philosophy
//!
//! - **Type Safety**: Newtypes prevent mixing incompatible values
//! - **Zero-Cost Abstractions**: Trait-based design with no runtime overhead
//! - **Explicit Over Implicit**: Clear, self-documenting APIs
//!
//! ## Example
//!
//! ```rust
//! use convex_core::prelude::*;
//! use rust_decimal_macros::dec;
//!
//! // Create domain types with compile-time safety
//! let price = Price::new(dec!(98.50), Currency::USD);
//! let yield_val = Yield::new(dec!(0.05), Compounding::SemiAnnual);
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::manual_div_ceil)]
#![allow(clippy::missing_fields_in_debug)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::struct_field_names)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::similar_names)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::if_not_else)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::single_match)]
#![allow(clippy::unused_self)]
#![allow(clippy::trivially_copy_pass_by_ref)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::unnecessary_map_or)]
#![allow(clippy::cast_possible_truncation)]

pub mod calendars;
pub mod daycounts;
pub mod error;
pub mod traits;
pub mod types;

#[cfg(test)]
mod validation_tests;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::calendars::{BusinessDayConvention, Calendar};
    pub use crate::daycounts::DayCount;
    pub use crate::error::{ConvexError, ConvexResult};
    pub use crate::traits::{Discountable, PricingEngine, RiskCalculator, YieldCurve};
    pub use crate::types::{
        CashFlow, CashFlowSchedule, CashFlowType, Compounding, Currency, Date, Frequency,
        MarketConvention, Price, Spread, SpreadType, Yield, YieldMethod,
    };
}

// Re-export commonly used types at crate root
pub use error::{ConvexError, ConvexResult};
pub use types::{Currency, Date, MarketConvention, Price, Yield, YieldMethod};
