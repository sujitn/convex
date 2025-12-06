//! Japanese Government Bond (JGB) conventions.
//!
//! Provides conventions for:
//! - JGBs (coupon-bearing bonds)
//! - JGB inflation-linked bonds
//! - Japanese T-Bills (discount)
//!
//! # References
//!
//! - Ministry of Finance Japan: <https://www.mof.go.jp/>
//! - Japan Securities Dealers Association (JSDA)

use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::Frequency;

use super::BondConventions;
use crate::types::{AccruedConvention, CalendarId, PriceQuoteConvention, YieldConvention};

/// Returns conventions for Japanese Government Bonds (JGBs).
///
/// - Day count: Actual/365 (Japanese)
/// - Frequency: Semi-annual
/// - Settlement: T+2
/// - Yield: Simple yield (no compounding)
///
/// # Example
///
/// ```rust
/// use convex_bonds::conventions::japanese_jgb;
/// use convex_bonds::types::YieldConvention;
///
/// let conv = japanese_jgb::jgb();
/// assert_eq!(conv.yield_convention(), YieldConvention::SimpleYield);
/// ```
///
/// # Notes
///
/// JGBs use simple yield calculation (no compounding), which differs
/// from most other government bond markets.
#[must_use]
pub fn jgb() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Act365Fixed)
        .frequency(Frequency::SemiAnnual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::japan())
        .end_of_month(true)
        .yield_convention(YieldConvention::SimpleYield)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(50000)
        .description("Japanese Government Bond (JGB)")
        .build()
}

/// Returns conventions for JGB inflation-linked bonds.
///
/// - Day count: Actual/365 (Japanese)
/// - Frequency: Semi-annual
/// - Settlement: T+2
/// - Index: Japanese CPI
#[must_use]
pub fn jgb_inflation_linked() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Act365Fixed)
        .frequency(Frequency::SemiAnnual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::japan())
        .end_of_month(true)
        .yield_convention(YieldConvention::SimpleYield)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(50000)
        .description("JGB Inflation-Linked Bond")
        .build()
}

/// Returns conventions for JGB floating rate notes.
///
/// - Day count: Actual/365 (Japanese)
/// - Frequency: Quarterly
/// - Settlement: T+2
/// - Index: 10-year JGB rate
#[must_use]
pub fn jgb_frn() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Act365Fixed)
        .frequency(Frequency::Quarterly)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::japan())
        .end_of_month(false)
        .yield_convention(YieldConvention::SimpleYield)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(50000)
        .description("JGB Floating Rate Note")
        .build()
}

/// Returns conventions for Japanese Treasury Bills (T-Bills).
///
/// - Day count: Actual/365
/// - Frequency: Zero coupon (discount)
/// - Settlement: T+2
#[must_use]
pub fn t_bill() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Act365Fixed)
        .frequency(Frequency::Zero)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::japan())
        .end_of_month(false)
        .yield_convention(YieldConvention::DiscountYield)
        .accrued_convention(AccruedConvention::None)
        .price_quote(PriceQuoteConvention::Discount)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(50000)
        .description("Japanese Treasury Bill")
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jgb_conventions() {
        let conv = jgb();
        assert_eq!(conv.day_count(), DayCountConvention::Act365Fixed);
        assert_eq!(conv.frequency(), Frequency::SemiAnnual);
        assert_eq!(conv.settlement_days(), 2);
        assert_eq!(conv.yield_convention(), YieldConvention::SimpleYield);
        assert_eq!(conv.payments_per_year(), 2);
    }

    #[test]
    fn test_jgb_inflation_linked_conventions() {
        let conv = jgb_inflation_linked();
        assert_eq!(conv.day_count(), DayCountConvention::Act365Fixed);
        assert_eq!(conv.yield_convention(), YieldConvention::SimpleYield);
    }

    #[test]
    fn test_jgb_frn_conventions() {
        let conv = jgb_frn();
        assert_eq!(conv.frequency(), Frequency::Quarterly);
        assert_eq!(conv.payments_per_year(), 4);
    }

    #[test]
    fn test_t_bill_conventions() {
        let conv = t_bill();
        assert_eq!(conv.frequency(), Frequency::Zero);
        assert_eq!(conv.day_count(), DayCountConvention::Act365Fixed);
        assert_eq!(conv.price_quote(), PriceQuoteConvention::Discount);
    }
}
