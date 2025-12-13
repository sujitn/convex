//! US Treasury bond conventions.
//!
//! Provides conventions for:
//! - Treasury notes and bonds (coupon-bearing)
//! - Treasury bills (discount instruments)
//! - Treasury Inflation-Protected Securities (TIPS)
//! - Floating Rate Notes (FRNs)
//!
//! # References
//!
//! - Treasury Direct: <https://www.treasurydirect.gov/>
//! - SIFMA: US Bond Market Association standards

use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::Frequency;

use super::BondConventions;
use crate::types::{AccruedConvention, CalendarId, FirstPeriodDiscounting, PriceQuoteConvention, YieldMethod};

/// Returns conventions for US Treasury notes and bonds.
///
/// - Day count: Actual/Actual (ICMA)
/// - Frequency: Semi-annual
/// - Settlement: T+1
/// - Price quote: 32nds
/// - Yield: Street Convention
///
/// # Example
///
/// ```rust
/// use convex_bonds::conventions::us_treasury;
///
/// let conv = us_treasury::note_bond();
/// assert_eq!(conv.settlement_days(), 1);
/// ```
#[must_use]
pub fn note_bond() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::SemiAnnual)
        .settlement_days(1)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::us_government())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Linear)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::ThirtySeconds)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(100)
        .description("US Treasury Note/Bond")
        .build()
}

/// Returns conventions for US Treasury bills.
///
/// - Day count: Actual/360 (discount)
/// - Frequency: Zero coupon
/// - Settlement: T+1
/// - Price quote: Discount rate
///
/// # Example
///
/// ```rust
/// use convex_bonds::conventions::us_treasury;
/// use convex_bonds::types::PriceQuoteConvention;
///
/// let conv = us_treasury::bill();
/// assert_eq!(conv.price_quote(), PriceQuoteConvention::Discount);
/// ```
#[must_use]
pub fn bill() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Act360)
        .frequency(Frequency::Zero)
        .settlement_days(1)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::us_government())
        .end_of_month(false)
        .yield_method(YieldMethod::Discount)
        .first_period_discounting(FirstPeriodDiscounting::Linear)
        .accrued_convention(AccruedConvention::None)
        .price_quote(PriceQuoteConvention::Discount)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(100)
        .description("US Treasury Bill")
        .build()
}

/// Returns conventions for Treasury Inflation-Protected Securities (TIPS).
///
/// - Day count: Actual/Actual (ICMA)
/// - Frequency: Semi-annual
/// - Settlement: T+1
/// - Price quote: 32nds (real price)
/// - Principal adjusted by CPI ratio
///
/// # Notes
///
/// TIPS prices are quoted in real terms. The settlement price is the
/// quoted price multiplied by the index ratio.
#[must_use]
pub fn tips() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::SemiAnnual)
        .settlement_days(1)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::us_government())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Linear)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::ThirtySeconds)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(100)
        .description("US Treasury Inflation-Protected Security (TIPS)")
        .build()
}

/// Returns conventions for Treasury Floating Rate Notes (FRNs).
///
/// - Day count: Actual/360
/// - Reset frequency: Quarterly
/// - Settlement: T+1
/// - Index: 13-week T-bill high rate
/// - Spread quoted in basis points
///
/// # Notes
///
/// Treasury FRNs reset quarterly based on the most recent 13-week
/// T-bill auction high rate.
#[must_use]
pub fn frn() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Act360)
        .frequency(Frequency::Quarterly)
        .settlement_days(1)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::us_government())
        .end_of_month(false)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Linear)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(100)
        .description("US Treasury Floating Rate Note (FRN)")
        .build()
}

/// Returns conventions for Treasury STRIPS (Separate Trading of
/// Registered Interest and Principal Securities).
///
/// - Day count: Actual/Actual (ICMA)
/// - Frequency: Zero coupon
/// - Settlement: T+1
/// - Price quote: 64ths
#[must_use]
pub fn strips() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::Zero)
        .settlement_days(1)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::us_government())
        .end_of_month(false)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Linear)
        .accrued_convention(AccruedConvention::None)
        .price_quote(PriceQuoteConvention::SixtyFourths)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(100)
        .description("US Treasury STRIPS")
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_note_bond_conventions() {
        let conv = note_bond();
        assert_eq!(conv.day_count(), DayCountConvention::ActActIcma);
        assert_eq!(conv.frequency(), Frequency::SemiAnnual);
        assert_eq!(conv.settlement_days(), 1);
        assert_eq!(conv.price_quote(), PriceQuoteConvention::ThirtySeconds);
        assert_eq!(conv.yield_method(), YieldMethod::Compounded);
        assert_eq!(conv.first_period_discounting(), FirstPeriodDiscounting::Linear);
        assert_eq!(conv.payments_per_year(), 2);
    }

    #[test]
    fn test_bill_conventions() {
        let conv = bill();
        assert_eq!(conv.day_count(), DayCountConvention::Act360);
        assert_eq!(conv.frequency(), Frequency::Zero);
        assert_eq!(conv.price_quote(), PriceQuoteConvention::Discount);
        assert_eq!(conv.yield_method(), YieldMethod::Discount);
        assert_eq!(conv.accrued_convention(), AccruedConvention::None);
    }

    #[test]
    fn test_tips_conventions() {
        let conv = tips();
        assert_eq!(conv.day_count(), DayCountConvention::ActActIcma);
        assert_eq!(conv.frequency(), Frequency::SemiAnnual);
        assert_eq!(conv.settlement_days(), 1);
    }

    #[test]
    fn test_frn_conventions() {
        let conv = frn();
        assert_eq!(conv.day_count(), DayCountConvention::Act360);
        assert_eq!(conv.frequency(), Frequency::Quarterly);
        assert_eq!(conv.payments_per_year(), 4);
    }

    #[test]
    fn test_strips_conventions() {
        let conv = strips();
        assert_eq!(conv.frequency(), Frequency::Zero);
        assert_eq!(conv.price_quote(), PriceQuoteConvention::SixtyFourths);
        assert_eq!(conv.accrued_convention(), AccruedConvention::None);
    }
}
