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

pub mod calendars;
pub mod daycounts;
pub mod error;
pub mod traits;
pub mod types;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::calendars::{BusinessDayConvention, Calendar};
    pub use crate::daycounts::DayCount;
    pub use crate::error::{ConvexError, ConvexResult};
    pub use crate::traits::{Discountable, PricingEngine, RiskCalculator, YieldCurve};
    pub use crate::types::{
        CashFlow, CashFlowSchedule, CashFlowType, Compounding, Currency, Date, Frequency, Price,
        Spread, SpreadType, Yield,
    };
}

// Re-export commonly used types at crate root
pub use error::{ConvexError, ConvexResult};
pub use types::{Currency, Date, Price, Yield};
