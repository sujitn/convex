//! Supplied exchange reference facts. No provider, repository, clock or pricing model.
//!
//! Struct decoding is strict; call `validate` before using supplied terms. Execution
//! input eligibility is a necessary data gate, never trading authority or valuation.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod eligibility;
mod terms;
mod validation;

pub use eligibility::*;
pub use terms::*;

use crate::contracts::ContractError;

/// Invalid reference economics, identity, units or bounded input.
pub type FuturesReferenceError = ContractError;

pub(crate) fn invalid(field: &'static str, reason: &'static str) -> FuturesReferenceError {
    ContractError { field, reason }
}
