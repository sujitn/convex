//! Domain types for bond analytics.
//!
//! This module provides type-safe representations of bond-specific concepts:
//!
//! - [`BondType`]: Classification of bond types
//! - [`BondIdentifiers`]: Security identifiers (ISIN, CUSIP, etc.)
//! - [`Cusip`], [`Isin`], [`Figi`], [`Sedol`]: Validated identifier types
//! - [`CallSchedule`] / [`PutSchedule`]: Embedded option schedules
//! - [`AmortizationSchedule`]: Principal amortization
//! - [`RateIndex`]: Floating rate reference indices
//! - [`YieldConvention`], [`AccruedConvention`]: Yield calculation conventions
//! - [`PriceQuoteConvention`], [`PriceQuote`]: Price quote formats

mod amortization;
mod bond_type;
mod identifiers;
mod options;
mod price_quote;
mod rate_index;
mod yield_convention;

pub use amortization::{AmortizationEntry, AmortizationSchedule, AmortizationType};
pub use bond_type::BondType;
pub use identifiers::{BondIdentifiers, CalendarId, Cusip, Figi, Isin, Sedol};
pub use options::{CallEntry, CallSchedule, CallType, PutEntry, PutSchedule, PutType};
pub use price_quote::{PriceQuote, PriceQuoteConvention};
pub use rate_index::{InflationIndexType, RateIndex, Tenor};
pub use yield_convention::{AccruedConvention, RoundingConvention, YieldConvention};
