//! German government bond (Bund) conventions.
//!
//! Provides conventions for:
//! - Bundesanleihen (Bunds) - 10-30 year bonds
//! - Bundesobligationen (Bobls) - 5 year bonds
//! - Bundesschatzanweisungen (Schätze) - 2 year notes
//! - Inflation-linked Bunds (Bundei)
//!
//! # References
//!
//! - Deutsche Finanzagentur: <https://www.deutsche-finanzagentur.de/>
//! - Eurex bond futures specifications

use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::Frequency;

use super::BondConventions;
use crate::types::{AccruedConvention, CalendarId, FirstPeriodDiscounting, PriceQuoteConvention, YieldMethod};

/// Returns conventions for German government bonds (Bunds).
///
/// - Day count: Actual/Actual (ICMA)
/// - Frequency: Annual
/// - Settlement: T+2
/// - Price quote: Decimal
/// - Yield: ISMA (annual compounding)
///
/// # Example
///
/// ```rust
/// use convex_bonds::conventions::german_bund;
/// use convex_core::types::Frequency;
///
/// let conv = german_bund::bund();
/// assert_eq!(conv.frequency(), Frequency::Annual);
/// assert_eq!(conv.settlement_days(), 2);
/// ```
#[must_use]
pub fn bund() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::Annual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Compound)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("German Bundesanleihe (Bund)")
        .build()
}

/// Returns conventions for German 5-year bonds (Bobls).
///
/// Same conventions as Bunds but for 5-year maturity.
#[must_use]
pub fn bobl() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::Annual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Compound)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("German Bundesobligation (Bobl)")
        .build()
}

/// Returns conventions for German 2-year notes (Schätze).
///
/// Same conventions as Bunds but for 2-year maturity.
#[must_use]
pub fn schatz() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::Annual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Compound)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("German Bundesschatzanweisung (Schatz)")
        .build()
}

/// Returns conventions for German inflation-linked bonds (Bundei).
///
/// - Day count: Actual/Actual (ICMA)
/// - Frequency: Annual
/// - Settlement: T+2
/// - Indexation: Eurozone HICP ex-Tobacco
/// - 3-month indexation lag
#[must_use]
pub fn bundei() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::Annual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(true)
        .yield_method(YieldMethod::Compounded)
        .first_period_discounting(FirstPeriodDiscounting::Compound)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("German Inflation-Linked Bund (Bundei)")
        .build()
}

/// Returns conventions for German discount paper (Bubills).
///
/// - Day count: Actual/360
/// - Frequency: Zero coupon (discount)
/// - Settlement: T+2
#[must_use]
pub fn bubill() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Act360)
        .frequency(Frequency::Zero)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(false)
        .yield_method(YieldMethod::Discount)
        .first_period_discounting(FirstPeriodDiscounting::Linear)
        .accrued_convention(AccruedConvention::None)
        .price_quote(PriceQuoteConvention::Discount)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("German Bubill (Discount Paper)")
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bund_conventions() {
        let conv = bund();
        assert_eq!(conv.day_count(), DayCountConvention::ActActIcma);
        assert_eq!(conv.frequency(), Frequency::Annual);
        assert_eq!(conv.settlement_days(), 2);
        assert_eq!(conv.yield_method(), YieldMethod::Compounded);
        assert_eq!(conv.first_period_discounting(), FirstPeriodDiscounting::Compound);
        assert_eq!(conv.payments_per_year(), 1);
    }

    #[test]
    fn test_bobl_conventions() {
        let conv = bobl();
        assert_eq!(conv.frequency(), Frequency::Annual);
        assert_eq!(conv.settlement_days(), 2);
    }

    #[test]
    fn test_schatz_conventions() {
        let conv = schatz();
        assert_eq!(conv.frequency(), Frequency::Annual);
        assert_eq!(conv.day_count(), DayCountConvention::ActActIcma);
    }

    #[test]
    fn test_bundei_conventions() {
        let conv = bundei();
        assert_eq!(conv.day_count(), DayCountConvention::ActActIcma);
        assert_eq!(conv.frequency(), Frequency::Annual);
    }

    #[test]
    fn test_bubill_conventions() {
        let conv = bubill();
        assert_eq!(conv.day_count(), DayCountConvention::Act360);
        assert_eq!(conv.frequency(), Frequency::Zero);
        assert_eq!(conv.price_quote(), PriceQuoteConvention::Discount);
    }
}
