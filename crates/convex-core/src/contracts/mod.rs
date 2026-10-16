//! Validated, provider-neutral contracts for new analytics and application boundaries.
//!
//! Domain types are available in [`crate::ids`] and [`crate::types`]. Conversion
//! into this module is explicit and fallible. Exact values carry units on the wire;
//! model values are finite. No constructor consults a clock or an external service.
//!
//! Native units cannot be substituted even when their numbers happen to agree:
//! ```compile_fail,E0308
//! use convex_core::contracts::{PricePer100, DecimalRate};
//! fn price(value: PricePer100) {}
//! fn wrong(value: DecimalRate) { price(value); }
//! ```
//! ```compile_fail,E0308
//! use convex_core::contracts::{DecimalRate, BasisPoints};
//! fn rate(value: DecimalRate) {}
//! fn wrong(value: BasisPoints) { rate(value); }
//! ```
//! ```compile_fail,E0308
//! use convex_core::contracts::{FaceAmount, FuturesContracts};
//! fn face(value: FaceAmount) {}
//! fn wrong(value: FuturesContracts) { face(value); }
//! ```
//! ```compile_fail,E0308
//! use convex_core::contracts::{PricePer100, FaceAmount};
//! fn price(value: PricePer100) {}
//! fn wrong(value: FaceAmount) { price(value); }
//! ```
//! ```compile_fail,E0308
//! use convex_core::contracts::{DecimalRate, FaceAmount};
//! fn rate(value: DecimalRate) {}
//! fn wrong(value: FaceAmount) { rate(value); }
//! ```

#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

/// Bounded JSON parsing and numeric-token admission, shared by application adapters.
pub mod json;

/// Checked number-or-string Decimal admission for existing native wire inputs.
pub mod numeric_decimal;

mod identity;
mod results;
mod risk;
mod time;
mod units;

pub use identity::*;
pub use results::*;
pub use risk::*;
pub use time::*;
pub use units::*;

/// Generate JSON schemas from the same Rust wire contracts.
/// Cross-field invariants still require checked constructors/deserializers.
#[cfg(feature = "schemars")]
pub fn json_schemas() -> std::collections::BTreeMap<&'static str, schemars::Schema> {
    std::collections::BTreeMap::from([
        ("result", schemars::schema_for!(ResultEnvelope)),
        ("risk", schemars::schema_for!(RiskSensitivity)),
        ("quantity", schemars::schema_for!(PositionQuantity)),
        ("value", schemars::schema_for!(MeasureValue)),
        ("subject", schemars::schema_for!(ValuationSubject)),
        ("date", schemars::schema_for!(BusinessDate)),
        ("timestamp", schemars::schema_for!(UtcTimestamp)),
    ])
}

/// An invalid contract field. Submitted values are not copied into the error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Invalid {field}: {reason}")]
pub struct ContractError {
    /// Stable field name or path.
    pub field: &'static str,
    /// Explanation of the violated invariant.
    pub reason: &'static str,
}

impl ContractError {
    pub(crate) const fn new(field: &'static str, reason: &'static str) -> Self {
        Self { field, reason }
    }
}
