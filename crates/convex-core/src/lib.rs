// The crate documentation is the README, so docs.rs and crates.io show the
// same text and its examples run as doctests.
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![warn(clippy::all)]
#![forbid(unsafe_code)]
#![deny(unused_must_use)]

pub mod calendars;
pub mod cashflows;
pub mod contracts;
pub mod daycounts;
pub mod error;
pub mod fixings;
pub mod futures;
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
        CashFlow, CashFlowSchedule, CashFlowType, Compounding, Currency, Date, Frequency, Mark,
        Price, PriceKind, Spread, SpreadType, Yield,
    };
}

// Re-export commonly used types at crate root
pub use error::{ConvexError, ConvexResult};
pub use types::{Currency, Date, Price, Yield};
pub mod ids;
