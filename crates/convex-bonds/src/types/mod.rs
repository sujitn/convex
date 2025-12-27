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
//! - [`YieldCalculationRules`]: Complete yield calculation rules
//! - [`CompoundingMethod`]: Interest compounding methods
//! - [`StubPeriodRules`]: Irregular coupon period handling
//! - [`ExDividendRules`]: Ex-dividend period rules
//! - [`SettlementRules`]: Settlement date calculation rules
//! - [`CreditRating`], [`RatingBucket`]: Credit rating classifications
//! - [`Sector`]: Issuer sector classification
//! - [`Seniority`]: Capital structure position

mod amortization;
mod bond_type;
mod compounding;
mod ex_dividend;
mod identifiers;
mod options;
mod price_quote;
mod rate_index;
mod rating;
mod sector;
mod seniority;
mod settlement_rules;
mod stub_rules;
mod yield_convention;
mod yield_rules;

pub use amortization::{AmortizationEntry, AmortizationSchedule, AmortizationType};
pub use bond_type::BondType;
pub use compounding::CompoundingMethod;
pub use ex_dividend::{DayType, ExDivAccruedMethod, ExDividendRules, ExDividendStatus};
pub use identifiers::{BondIdentifiers, CalendarId, Cusip, Figi, Isin, Sedol};
pub use options::{CallEntry, CallSchedule, CallType, PutEntry, PutSchedule, PutType};
pub use price_quote::{PriceQuote, PriceQuoteConvention};
pub use rate_index::{InflationIndexType, RateIndex, SOFRConvention, Tenor};
pub use rating::{CreditRating, RatingBucket};
pub use sector::Sector;
pub use seniority::Seniority;
pub use settlement_rules::{SettlementAdjustment, SettlementRules, SettlementType};
pub use stub_rules::{ReferenceMethod, StubPeriodRules, StubPosition, StubType};
pub use yield_convention::{AccruedConvention, RoundingConvention, YieldConvention};
pub use yield_rules::YieldCalculationRules;
