//! Market conventions for bond analytics.
//!
//! This module provides market-specific conventions for different bond types
//! and markets. Conventions include:
//!
//! - Day count basis
//! - Coupon frequency
//! - Settlement days
//! - Business day adjustment rules
//! - Yield calculation method
//! - Price quoting conventions
//!
//! # Market Modules
//!
//! - [`us_treasury`]: US Treasury notes, bonds, bills, TIPS, FRNs
//! - [`us_corporate`]: US corporate bonds (investment grade, high yield)
//! - [`uk_gilt`]: UK gilts (conventional and index-linked)
//! - [`german_bund`]: German government bonds
//! - [`japanese_jgb`]: Japanese government bonds
//! - [`eurobond`]: Eurobonds and international bonds
//!
//! # Example
//!
//! ```rust
//! use convex_bonds::conventions::{BondConventions, us_treasury};
//!
//! // Get US Treasury note conventions
//! let conventions = us_treasury::note_bond();
//! assert_eq!(conventions.settlement_days(), 1);
//! ```

pub mod eurobond;
pub mod german_bund;
pub mod japanese_jgb;
pub mod uk_gilt;
pub mod us_corporate;
pub mod us_treasury;

pub use eurobond::*;
pub use german_bund::*;
pub use japanese_jgb::*;
pub use uk_gilt::*;
pub use us_corporate::*;
pub use us_treasury::*;

use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::Frequency;

use crate::types::{AccruedConvention, CalendarId, PriceQuoteConvention, YieldConvention};

/// Re-export DayCountConvention for convenience as DayCountBasis alias
pub type DayCountBasis = DayCountConvention;

/// Complete bond market conventions.
///
/// This struct encapsulates all the conventions needed to price and
/// analyze a bond according to its market standards.
///
/// # Performance
///
/// Convention lookup is designed to be < 10ns as conventions are
/// pre-computed static values.
///
/// # Example
///
/// ```rust
/// use convex_bonds::conventions::BondConventions;
/// use convex_core::daycounts::DayCountConvention;
/// use convex_core::types::Frequency;
///
/// let conventions = BondConventions::builder()
///     .day_count(DayCountConvention::ActActIcma)
///     .frequency(Frequency::SemiAnnual)
///     .settlement_days(1)
///     .build();
///
/// assert_eq!(conventions.day_count(), DayCountConvention::ActActIcma);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BondConventions {
    /// Day count basis for accrued interest and discounting.
    day_count: DayCountConvention,

    /// Coupon payment frequency.
    frequency: Frequency,

    /// Number of business days for settlement (T+n).
    settlement_days: u32,

    /// Business day adjustment convention.
    business_day_convention: BusinessDayConvention,

    /// Calendar for business day adjustments.
    calendar: CalendarId,

    /// End-of-month rule for schedule generation.
    end_of_month: bool,

    /// Yield calculation convention.
    yield_convention: YieldConvention,

    /// Accrued interest convention.
    accrued_convention: AccruedConvention,

    /// Price quoting convention.
    price_quote: PriceQuoteConvention,

    /// Whether prices are quoted clean (without accrued).
    quote_clean: bool,

    /// Face value denomination (e.g., 100 for most markets).
    face_denomination: u32,

    /// Minimum settlement amount if any.
    minimum_denomination: Option<u64>,

    /// Ex-dividend days (for markets with ex-dividend period).
    ex_dividend_days: Option<u32>,

    /// Description of the convention set.
    description: &'static str,
}

impl BondConventions {
    /// Creates a new `BondConventionsBuilder`.
    #[must_use]
    pub fn builder() -> BondConventionsBuilder {
        BondConventionsBuilder::default()
    }

    /// Returns the day count convention.
    #[must_use]
    pub fn day_count(&self) -> DayCountConvention {
        self.day_count
    }

    /// Returns the coupon frequency.
    #[must_use]
    pub const fn frequency(&self) -> Frequency {
        self.frequency
    }

    /// Returns the number of settlement days.
    #[must_use]
    pub const fn settlement_days(&self) -> u32 {
        self.settlement_days
    }

    /// Returns the business day convention.
    #[must_use]
    pub const fn business_day_convention(&self) -> BusinessDayConvention {
        self.business_day_convention
    }

    /// Returns the calendar ID.
    #[must_use]
    pub const fn calendar(&self) -> &CalendarId {
        &self.calendar
    }

    /// Returns whether end-of-month rule applies.
    #[must_use]
    pub const fn end_of_month(&self) -> bool {
        self.end_of_month
    }

    /// Returns the yield calculation convention.
    #[must_use]
    pub const fn yield_convention(&self) -> YieldConvention {
        self.yield_convention
    }

    /// Returns the accrued interest convention.
    #[must_use]
    pub const fn accrued_convention(&self) -> AccruedConvention {
        self.accrued_convention
    }

    /// Returns the price quote convention.
    #[must_use]
    pub const fn price_quote(&self) -> PriceQuoteConvention {
        self.price_quote
    }

    /// Returns whether prices are quoted clean.
    #[must_use]
    pub const fn quote_clean(&self) -> bool {
        self.quote_clean
    }

    /// Returns the face value denomination.
    #[must_use]
    pub const fn face_denomination(&self) -> u32 {
        self.face_denomination
    }

    /// Returns the minimum denomination if any.
    #[must_use]
    pub const fn minimum_denomination(&self) -> Option<u64> {
        self.minimum_denomination
    }

    /// Returns the ex-dividend days if applicable.
    #[must_use]
    pub const fn ex_dividend_days(&self) -> Option<u32> {
        self.ex_dividend_days
    }

    /// Returns the convention description.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        self.description
    }

    /// Returns the number of coupon payments per year.
    #[must_use]
    pub fn payments_per_year(&self) -> u32 {
        self.frequency.periods_per_year()
    }
}

impl Default for BondConventions {
    fn default() -> Self {
        // Default to generic international bond conventions
        Self {
            day_count: DayCountConvention::ActActIcma,
            frequency: Frequency::Annual,
            settlement_days: 2,
            business_day_convention: BusinessDayConvention::ModifiedFollowing,
            calendar: CalendarId::target2(),
            end_of_month: true,
            yield_convention: YieldConvention::ISMA,
            accrued_convention: AccruedConvention::Standard,
            price_quote: PriceQuoteConvention::Decimal,
            quote_clean: true,
            face_denomination: 100,
            minimum_denomination: None,
            ex_dividend_days: None,
            description: "Generic International Bond",
        }
    }
}

/// Builder for `BondConventions`.
#[derive(Debug, Clone, Default)]
pub struct BondConventionsBuilder {
    day_count: Option<DayCountConvention>,
    frequency: Option<Frequency>,
    settlement_days: Option<u32>,
    business_day_convention: Option<BusinessDayConvention>,
    calendar: Option<CalendarId>,
    end_of_month: Option<bool>,
    yield_convention: Option<YieldConvention>,
    accrued_convention: Option<AccruedConvention>,
    price_quote: Option<PriceQuoteConvention>,
    quote_clean: Option<bool>,
    face_denomination: Option<u32>,
    minimum_denomination: Option<u64>,
    ex_dividend_days: Option<u32>,
    description: Option<&'static str>,
}

impl BondConventionsBuilder {
    /// Sets the day count convention.
    #[must_use]
    pub fn day_count(mut self, day_count: DayCountConvention) -> Self {
        self.day_count = Some(day_count);
        self
    }

    /// Sets the coupon frequency.
    #[must_use]
    pub fn frequency(mut self, frequency: Frequency) -> Self {
        self.frequency = Some(frequency);
        self
    }

    /// Sets the settlement days.
    #[must_use]
    pub fn settlement_days(mut self, days: u32) -> Self {
        self.settlement_days = Some(days);
        self
    }

    /// Sets the business day convention.
    #[must_use]
    pub fn business_day_convention(mut self, convention: BusinessDayConvention) -> Self {
        self.business_day_convention = Some(convention);
        self
    }

    /// Sets the calendar.
    #[must_use]
    pub fn calendar(mut self, calendar: CalendarId) -> Self {
        self.calendar = Some(calendar);
        self
    }

    /// Sets the end-of-month rule.
    #[must_use]
    pub fn end_of_month(mut self, eom: bool) -> Self {
        self.end_of_month = Some(eom);
        self
    }

    /// Sets the yield convention.
    #[must_use]
    pub fn yield_convention(mut self, convention: YieldConvention) -> Self {
        self.yield_convention = Some(convention);
        self
    }

    /// Sets the accrued interest convention.
    #[must_use]
    pub fn accrued_convention(mut self, convention: AccruedConvention) -> Self {
        self.accrued_convention = Some(convention);
        self
    }

    /// Sets the price quote convention.
    #[must_use]
    pub fn price_quote(mut self, convention: PriceQuoteConvention) -> Self {
        self.price_quote = Some(convention);
        self
    }

    /// Sets whether prices are quoted clean.
    #[must_use]
    pub fn quote_clean(mut self, clean: bool) -> Self {
        self.quote_clean = Some(clean);
        self
    }

    /// Sets the face denomination.
    #[must_use]
    pub fn face_denomination(mut self, denom: u32) -> Self {
        self.face_denomination = Some(denom);
        self
    }

    /// Sets the minimum denomination.
    #[must_use]
    pub fn minimum_denomination(mut self, min: u64) -> Self {
        self.minimum_denomination = Some(min);
        self
    }

    /// Sets the ex-dividend days.
    #[must_use]
    pub fn ex_dividend_days(mut self, days: u32) -> Self {
        self.ex_dividend_days = Some(days);
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn description(mut self, desc: &'static str) -> Self {
        self.description = Some(desc);
        self
    }

    /// Builds the `BondConventions`.
    #[must_use]
    pub fn build(self) -> BondConventions {
        let default = BondConventions::default();

        BondConventions {
            day_count: self.day_count.unwrap_or(default.day_count),
            frequency: self.frequency.unwrap_or(default.frequency),
            settlement_days: self.settlement_days.unwrap_or(default.settlement_days),
            business_day_convention: self
                .business_day_convention
                .unwrap_or(default.business_day_convention),
            calendar: self.calendar.unwrap_or(default.calendar),
            end_of_month: self.end_of_month.unwrap_or(default.end_of_month),
            yield_convention: self.yield_convention.unwrap_or(default.yield_convention),
            accrued_convention: self.accrued_convention.unwrap_or(default.accrued_convention),
            price_quote: self.price_quote.unwrap_or(default.price_quote),
            quote_clean: self.quote_clean.unwrap_or(default.quote_clean),
            face_denomination: self.face_denomination.unwrap_or(default.face_denomination),
            minimum_denomination: self.minimum_denomination.or(default.minimum_denomination),
            ex_dividend_days: self.ex_dividend_days.or(default.ex_dividend_days),
            description: self.description.unwrap_or(default.description),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bond_conventions_default() {
        let conv = BondConventions::default();
        assert_eq!(conv.day_count(), DayCountConvention::ActActIcma);
        assert_eq!(conv.frequency(), Frequency::Annual);
        assert_eq!(conv.settlement_days(), 2);
    }

    #[test]
    fn test_bond_conventions_builder() {
        let conv = BondConventions::builder()
            .day_count(DayCountConvention::Thirty360US)
            .frequency(Frequency::SemiAnnual)
            .settlement_days(3)
            .yield_convention(YieldConvention::StreetConvention)
            .build();

        assert_eq!(conv.day_count(), DayCountConvention::Thirty360US);
        assert_eq!(conv.frequency(), Frequency::SemiAnnual);
        assert_eq!(conv.settlement_days(), 3);
        assert_eq!(conv.yield_convention(), YieldConvention::StreetConvention);
    }

    #[test]
    fn test_payments_per_year() {
        let annual = BondConventions::builder()
            .frequency(Frequency::Annual)
            .build();
        assert_eq!(annual.payments_per_year(), 1);

        let semi = BondConventions::builder()
            .frequency(Frequency::SemiAnnual)
            .build();
        assert_eq!(semi.payments_per_year(), 2);

        let quarterly = BondConventions::builder()
            .frequency(Frequency::Quarterly)
            .build();
        assert_eq!(quarterly.payments_per_year(), 4);
    }
}
