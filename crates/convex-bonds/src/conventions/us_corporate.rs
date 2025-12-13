//! US Corporate bond conventions.
//!
//! Provides conventions for:
//! - Investment grade corporate bonds
//! - High yield (junk) bonds
//! - Municipal bonds
//!
//! # References
//!
//! - SIFMA: US Bond Market Association standards
//! - FINRA TRACE: Trade Reporting and Compliance Engine

use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::Frequency;

use super::BondConventions;
use crate::types::{AccruedConvention, CalendarId, FirstPeriodDiscounting, PriceQuoteConvention, YieldMethod};

/// Returns conventions for investment grade corporate bonds.
///
/// - Day count: 30/360 (US)
/// - Frequency: Semi-annual
/// - Settlement: T+2
/// - Price quote: Decimal
/// - Yield: Street Convention
///
/// # Example
///
/// ```rust
/// use convex_bonds::conventions::us_corporate;
/// use convex_core::daycounts::DayCountConvention;
///
/// let conv = us_corporate::investment_grade();
/// assert_eq!(conv.day_count(), DayCountConvention::Thirty360US);
/// assert_eq!(conv.settlement_days(), 2);
/// ```
#[must_use]
pub fn investment_grade() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Thirty360US)
        .frequency(Frequency::SemiAnnual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::sifma())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Linear)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("US Investment Grade Corporate Bond")
        .build()
}

/// Returns conventions for high yield (junk) corporate bonds.
///
/// Same as investment grade but often with higher minimum denominations
/// and different trading conventions.
///
/// - Day count: 30/360 (US)
/// - Frequency: Semi-annual
/// - Settlement: T+2
/// - Price quote: Decimal
#[must_use]
pub fn high_yield() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Thirty360US)
        .frequency(Frequency::SemiAnnual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::sifma())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Linear)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("US High Yield Corporate Bond")
        .build()
}

/// Returns conventions for US municipal bonds.
///
/// - Day count: 30/360 (US)
/// - Frequency: Semi-annual
/// - Settlement: T+2
/// - Price quote: Decimal
/// - Yield: Municipal (for tax-equivalent comparisons)
///
/// # Notes
///
/// Municipal bonds are often tax-exempt, so yield comparisons
/// require adjustment for the investor's tax bracket.
#[must_use]
pub fn municipal() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Thirty360US)
        .frequency(Frequency::SemiAnnual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::sifma())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Linear)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(5000)
        .description("US Municipal Bond")
        .build()
}

/// Returns conventions for US agency bonds (Fannie Mae, Freddie Mac, etc.).
///
/// - Day count: 30/360 (US)
/// - Frequency: Semi-annual
/// - Settlement: T+1
/// - Price quote: Decimal
#[must_use]
pub fn agency() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Thirty360US)
        .frequency(Frequency::SemiAnnual)
        .settlement_days(1)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::sifma())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Linear)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("US Agency Bond")
        .build()
}

/// Returns conventions for mortgage-backed securities (MBS).
///
/// - Day count: 30/360 (US)
/// - Frequency: Monthly
/// - Settlement: T+2 (varies by TBA vs specified pool)
/// - Price quote: 128ths
///
/// # Notes
///
/// MBS have principal prepayment risk and typically trade
/// with a delay in payment (payment delay days).
#[must_use]
pub fn mbs() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Thirty360US)
        .frequency(Frequency::Monthly)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::sifma())
        .end_of_month(false)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Linear)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::OneHundredTwentyEighths)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("US Mortgage-Backed Security")
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_investment_grade_conventions() {
        let conv = investment_grade();
        assert_eq!(conv.day_count(), DayCountConvention::Thirty360US);
        assert_eq!(conv.frequency(), Frequency::SemiAnnual);
        assert_eq!(conv.settlement_days(), 2);
        assert_eq!(conv.price_quote(), PriceQuoteConvention::Decimal);
        assert_eq!(conv.yield_method(), YieldMethod::Compounded);
        assert_eq!(conv.first_period_discounting(), FirstPeriodDiscounting::Linear);
    }

    #[test]
    fn test_high_yield_conventions() {
        let conv = high_yield();
        assert_eq!(conv.day_count(), DayCountConvention::Thirty360US);
        assert_eq!(conv.settlement_days(), 2);
    }

    #[test]
    fn test_municipal_conventions() {
        let conv = municipal();
        assert_eq!(conv.yield_method(), YieldMethod::Compounded);
        assert_eq!(conv.minimum_denomination(), Some(5000));
    }

    #[test]
    fn test_agency_conventions() {
        let conv = agency();
        assert_eq!(conv.settlement_days(), 1);
        assert_eq!(conv.day_count(), DayCountConvention::Thirty360US);
    }

    #[test]
    fn test_mbs_conventions() {
        let conv = mbs();
        assert_eq!(conv.frequency(), Frequency::Monthly);
        assert_eq!(conv.payments_per_year(), 12);
        assert_eq!(
            conv.price_quote(),
            PriceQuoteConvention::OneHundredTwentyEighths
        );
    }
}
