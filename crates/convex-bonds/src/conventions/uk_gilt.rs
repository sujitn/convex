//! UK Gilt conventions.
//!
//! Provides conventions for:
//! - Conventional gilts (fixed coupon)
//! - Index-linked gilts (inflation-linked)
//!
//! # References
//!
//! - UK Debt Management Office (DMO): <https://www.dmo.gov.uk/>
//! - Bank of England gilt market conventions

use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::Frequency;

use super::BondConventions;
use crate::types::{
    AccruedConvention, CalendarId, FirstPeriodDiscounting, PriceQuoteConvention, YieldMethod,
};

/// Returns conventions for UK conventional gilts.
///
/// - Day count: Actual/Actual (ICMA)
/// - Frequency: Semi-annual
/// - Settlement: T+1
/// - Price quote: Decimal
/// - Ex-dividend: 7 business days before coupon
///
/// # Example
///
/// ```rust
/// use convex_bonds::conventions::uk_gilt;
///
/// let conv = uk_gilt::conventional();
/// assert_eq!(conv.settlement_days(), 1);
/// assert_eq!(conv.ex_dividend_days(), Some(7));
/// ```
#[must_use]
pub fn conventional() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::SemiAnnual)
        .settlement_days(1)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::uk())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Compound)
        .accrued_convention(AccruedConvention::ExDividend)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1)
        .ex_dividend_days(7)
        .description("UK Conventional Gilt")
        .build()
}

/// Returns conventions for UK index-linked gilts (old style, 8-month lag).
///
/// - Day count: Actual/Actual (ICMA)
/// - Frequency: Semi-annual
/// - Settlement: T+1
/// - Indexation lag: 8 months
/// - Ex-dividend: 7 business days before coupon
///
/// # Notes
///
/// Old-style linkers use an 8-month indexation lag and RPI.
/// They are being phased out in favor of the 3-month lag convention.
#[must_use]
pub fn index_linked_old() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::SemiAnnual)
        .settlement_days(1)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::uk())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Compound)
        .accrued_convention(AccruedConvention::ExDividend)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1)
        .ex_dividend_days(7)
        .description("UK Index-Linked Gilt (8-month lag)")
        .build()
}

/// Returns conventions for UK index-linked gilts (new style, 3-month lag).
///
/// - Day count: Actual/Actual (ICMA)
/// - Frequency: Semi-annual
/// - Settlement: T+1
/// - Indexation lag: 3 months
/// - Ex-dividend: 7 business days before coupon
///
/// # Notes
///
/// New-style linkers use a 3-month indexation lag, similar to TIPS.
#[must_use]
pub fn index_linked_new() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::SemiAnnual)
        .settlement_days(1)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::uk())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Compound)
        .accrued_convention(AccruedConvention::ExDividend)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1)
        .ex_dividend_days(7)
        .description("UK Index-Linked Gilt (3-month lag)")
        .build()
}

/// Alias for the current UK index-linked gilt convention (3-month lag).
#[must_use]
pub fn index_linked() -> BondConventions {
    index_linked_new()
}

/// Returns conventions for UK Treasury Bills.
///
/// - Day count: Actual/365
/// - Frequency: Zero coupon (discount)
/// - Settlement: T+1
/// - Price quote: Discount rate
#[must_use]
pub fn treasury_bill() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Act365Fixed)
        .frequency(Frequency::Zero)
        .settlement_days(1)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::uk())
        .end_of_month(false)
        .yield_method(YieldMethod::Discount)
        .first_period_discounting(FirstPeriodDiscounting::Linear)
        .accrued_convention(AccruedConvention::None)
        .price_quote(PriceQuoteConvention::Discount)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1)
        .description("UK Treasury Bill")
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conventional_gilt_conventions() {
        let conv = conventional();
        assert_eq!(conv.day_count(), DayCountConvention::ActActIcma);
        assert_eq!(conv.frequency(), Frequency::SemiAnnual);
        assert_eq!(conv.settlement_days(), 1);
        assert_eq!(conv.ex_dividend_days(), Some(7));
        assert_eq!(conv.accrued_convention(), AccruedConvention::ExDividend);
        assert_eq!(conv.yield_method(), YieldMethod::Compounded);
        assert_eq!(
            conv.first_period_discounting(),
            FirstPeriodDiscounting::Compound
        );
    }

    #[test]
    fn test_index_linked_conventions() {
        let old = index_linked_old();
        let new = index_linked_new();

        // Both use same basic conventions
        assert_eq!(old.day_count(), new.day_count());
        assert_eq!(old.frequency(), new.frequency());
        assert_eq!(old.ex_dividend_days(), new.ex_dividend_days());

        // Description differs
        assert_ne!(old.description(), new.description());
    }

    #[test]
    fn test_treasury_bill_conventions() {
        let conv = treasury_bill();
        assert_eq!(conv.day_count(), DayCountConvention::Act365Fixed);
        assert_eq!(conv.frequency(), Frequency::Zero);
        assert_eq!(conv.price_quote(), PriceQuoteConvention::Discount);
        assert_eq!(conv.accrued_convention(), AccruedConvention::None);
    }
}
