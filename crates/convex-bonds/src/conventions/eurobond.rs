//! Eurobond and international bond conventions.
//!
//! Provides conventions for:
//! - Eurobonds (international bonds)
//! - Euro-denominated government bonds (non-German)
//! - Supranational bonds
//!
//! # References
//!
//! - ICMA (International Capital Market Association)
//! - Euroclear settlement conventions

use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::Frequency;

use super::BondConventions;
use crate::types::{AccruedConvention, CalendarId, PriceQuoteConvention, YieldConvention};

/// Returns conventions for standard Eurobonds.
///
/// - Day count: 30E/360 (Eurobond basis)
/// - Frequency: Annual
/// - Settlement: T+2
/// - Yield: ISMA (annual compounding)
///
/// # Example
///
/// ```rust
/// use convex_bonds::conventions::eurobond;
/// use convex_core::daycounts::DayCountConvention;
///
/// let conv = eurobond::standard();
/// assert_eq!(conv.day_count(), DayCountConvention::Thirty360E);
/// ```
#[must_use]
pub fn standard() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Thirty360E)
        .frequency(Frequency::Annual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(true)
        .yield_convention(YieldConvention::ISMA)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("Eurobond (Standard)")
        .build()
}

/// Returns conventions for Eurobonds using ICMA Actual/Actual.
///
/// Some Eurobonds use Actual/Actual ICMA instead of 30E/360.
/// This is increasingly common for sovereign issuers.
#[must_use]
pub fn actual_actual() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::Annual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(true)
        .yield_convention(YieldConvention::ISMA)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("Eurobond (Actual/Actual ICMA)")
        .build()
}

/// Returns conventions for French government bonds (OATs).
///
/// - Day count: Actual/Actual (ICMA)
/// - Frequency: Annual
/// - Settlement: T+2
#[must_use]
pub fn french_oat() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::Annual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(true)
        .yield_convention(YieldConvention::ISMA)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1)
        .description("French OAT")
        .build()
}

/// Returns conventions for French inflation-linked bonds (`OATi`, OAT€i).
///
/// - Day count: Actual/Actual (ICMA)
/// - Frequency: Annual
/// - Settlement: T+2
/// - Index: French CPI ex-Tobacco or Eurozone HICP ex-Tobacco
#[must_use]
pub fn french_oat_inflation() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::Annual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(true)
        .yield_convention(YieldConvention::ISMA)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1)
        .description("French OATi/OAT€i (Inflation-Linked)")
        .build()
}

/// Returns conventions for Italian government bonds (BTPs).
///
/// - Day count: Actual/Actual (ICMA)
/// - Frequency: Semi-annual
/// - Settlement: T+2
#[must_use]
pub fn italian_btp() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::SemiAnnual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(true)
        .yield_convention(YieldConvention::ISMA)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("Italian BTP")
        .build()
}

/// Returns conventions for Spanish government bonds (Bonos).
///
/// - Day count: Actual/Actual (ICMA)
/// - Frequency: Annual
/// - Settlement: T+2
#[must_use]
pub fn spanish_bono() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::ActActIcma)
        .frequency(Frequency::Annual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(true)
        .yield_convention(YieldConvention::ISMA)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("Spanish Bono")
        .build()
}

/// Returns conventions for supranational bonds.
///
/// - Day count: 30E/360 or Actual/Actual (depends on issue)
/// - Frequency: Annual (typically)
/// - Settlement: T+2
///
/// # Notes
///
/// Supranationals (World Bank, EIB, etc.) often follow Eurobond conventions
/// but may vary by issue.
#[must_use]
pub fn supranational() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Thirty360E)
        .frequency(Frequency::Annual)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(true)
        .yield_convention(YieldConvention::ISMA)
        .accrued_convention(AccruedConvention::Standard)
        .price_quote(PriceQuoteConvention::Decimal)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(1000)
        .description("Supranational Bond")
        .build()
}

/// Returns conventions for Euro commercial paper (ECP).
///
/// - Day count: Actual/360
/// - Frequency: Zero coupon (discount)
/// - Settlement: T+2
#[must_use]
pub fn commercial_paper() -> BondConventions {
    BondConventions::builder()
        .day_count(DayCountConvention::Act360)
        .frequency(Frequency::Zero)
        .settlement_days(2)
        .business_day_convention(BusinessDayConvention::Following)
        .calendar(CalendarId::target2())
        .end_of_month(false)
        .yield_convention(YieldConvention::DiscountYield)
        .accrued_convention(AccruedConvention::None)
        .price_quote(PriceQuoteConvention::Discount)
        .quote_clean(true)
        .face_denomination(100)
        .minimum_denomination(100_000)
        .description("Euro Commercial Paper")
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_eurobond_conventions() {
        let conv = standard();
        assert_eq!(conv.day_count(), DayCountConvention::Thirty360E);
        assert_eq!(conv.frequency(), Frequency::Annual);
        assert_eq!(conv.settlement_days(), 2);
        assert_eq!(conv.yield_convention(), YieldConvention::ISMA);
    }

    #[test]
    fn test_actual_actual_eurobond() {
        let conv = actual_actual();
        assert_eq!(conv.day_count(), DayCountConvention::ActActIcma);
        assert_eq!(conv.frequency(), Frequency::Annual);
    }

    #[test]
    fn test_french_oat_conventions() {
        let conv = french_oat();
        assert_eq!(conv.day_count(), DayCountConvention::ActActIcma);
        assert_eq!(conv.frequency(), Frequency::Annual);
    }

    #[test]
    fn test_italian_btp_conventions() {
        let conv = italian_btp();
        assert_eq!(conv.day_count(), DayCountConvention::ActActIcma);
        assert_eq!(conv.frequency(), Frequency::SemiAnnual);
        assert_eq!(conv.payments_per_year(), 2);
    }

    #[test]
    fn test_spanish_bono_conventions() {
        let conv = spanish_bono();
        assert_eq!(conv.frequency(), Frequency::Annual);
        assert_eq!(conv.payments_per_year(), 1);
    }

    #[test]
    fn test_supranational_conventions() {
        let conv = supranational();
        assert_eq!(conv.day_count(), DayCountConvention::Thirty360E);
    }

    #[test]
    fn test_commercial_paper_conventions() {
        let conv = commercial_paper();
        assert_eq!(conv.day_count(), DayCountConvention::Act360);
        assert_eq!(conv.frequency(), Frequency::Zero);
        assert_eq!(conv.price_quote(), PriceQuoteConvention::Discount);
        assert_eq!(conv.minimum_denomination(), Some(100000));
    }
}
