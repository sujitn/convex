//! # Convex Spreads
//!
//! Spread analytics for the Convex fixed income analytics library.
//!
//! This crate provides spread calculations using generic `CashFlow` types:
//!
//! - **Z-Spread**: Zero-volatility spread (constant spread over spot curve)
//! - **Government Curve**: Government benchmark curve utilities
//! - **Benchmark**: Benchmark specification and security identification
//! - **Sovereign**: Sovereign and supranational issuer types
//!
//! ## Example
//!
//! ```rust,ignore
//! use convex_spreads::ZSpreadCalculator;
//! use convex_core::types::CashFlow;
//! use convex_curves::curves::ZeroCurve;
//!
//! let curve = // ... create curve
//! let cash_flows = // ... create cash flows from any source
//!
//! let calculator = ZSpreadCalculator::new(&curve);
//! let z_spread = calculator.calculate(&cash_flows, dec!(98.50), settlement)?;
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::similar_names)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::struct_field_names)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::derivable_impls)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::unused_self)]
#![allow(dead_code)]
#![allow(unused_variables)]

pub mod benchmark;
pub mod error;
pub mod government_curve;
pub mod oas;
pub mod sovereign;
pub mod zspread;

pub use benchmark::{BenchmarkSpec, SecurityId};
pub use error::{SpreadError, SpreadResult};
pub use government_curve::{GovernmentBenchmark, GovernmentCurve};
pub use oas::OASCalculator;
pub use sovereign::{Sovereign, SupranationalIssuer};
pub use zspread::ZSpreadCalculator;

#[cfg(test)]
mod tests {
    #[test]
    fn test_placeholder() {
        // Placeholder test
    }
}
