//! Asset Swap Spread (ASW) calculations.
//!
//! This module provides asset swap spread calculations commonly used in fixed income:
//!
//! - **Par-Par ASW**: Exchange bond at par, spread compensates for price difference
//! - **Proceeds ASW**: Swap notional equals bond market value
//! - **Market Value ASW**: Similar to proceeds with different conventions

mod par_par;
mod proceeds;

pub use par_par::ParParAssetSwap;
pub use proceeds::ProceedsAssetSwap;

use convex_bonds::traits::BondCashFlow;
use convex_core::daycounts::{ActActIcma, DayCount, DayCountConvention};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

/// Parses a bond's day-count convention string into a reusable day counter, ACT/ACT ICMA
/// at the bond's own `payments_per_year`.
///
/// Returns `None` for unrecognised conventions, in which case callers fall back
/// to the nominal coupon fraction. Parse once per bond and reuse across the
/// coupon loop rather than re-parsing the string on every cash flow.
pub(crate) fn day_counter(day_count: &str, payments_per_year: u32) -> Option<Box<dyn DayCount>> {
    match day_count.parse::<DayCountConvention>().ok()? {
        DayCountConvention::ActActIcma => Some(Box::new(ActActIcma::new(payments_per_year))),
        convention => Some(convention.to_day_count()),
    }
}

/// Year fraction for a single coupon period.
///
/// When the cash flow carries accrual boundaries and a `day_count` is available,
/// the year fraction is computed with the bond's own day-count convention;
/// otherwise it falls back to the nominal `1 / payments_per_year`. This prices
/// regular periods at the nominal fraction and stubs on their actual accrual
/// length, instead of guessing "regular vs stub" from a day-count threshold.
/// ACT/ACT ICMA counts a period against its reference period, the regular period
/// ending with the coupon: a regular coupon is exactly `1 / payments_per_year`, and a
/// short stub its share of that.
pub(crate) fn coupon_year_fraction(
    day_count: Option<&dyn DayCount>,
    cf: &BondCashFlow,
    payments_per_year: u32,
) -> f64 {
    let nominal = 1.0 / payments_per_year as f64;
    match (day_count, cf.accrual_start, cf.accrual_end) {
        (Some(dc), Some(start), Some(end)) => {
            let reference_start = i32::try_from(12 / payments_per_year.max(1))
                .ok()
                .and_then(|months| end.add_months(-months).ok())
                .unwrap_or(start);
            dc.period_year_fraction(start, end, reference_start, end)
                .to_f64()
                .filter(|yf| *yf > 0.0)
                .unwrap_or(nominal)
        }
        _ => nominal,
    }
}

/// Asset swap spread types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ASWType {
    /// Par-par asset swap: exchange bond at par, spread compensates.
    #[default]
    ParPar,

    /// Market value asset swap: notional equals market value of bond.
    MarketValue,

    /// Proceeds asset swap: swap notional equals bond proceeds.
    Proceeds,
}

impl ASWType {
    /// Returns the description of this asset swap type.
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::ParPar => "Par-Par Asset Swap",
            Self::MarketValue => "Market Value Asset Swap",
            Self::Proceeds => "Proceeds Asset Swap",
        }
    }

    /// Returns true if this type uses par notional.
    #[must_use]
    pub fn uses_par_notional(&self) -> bool {
        matches!(self, Self::ParPar)
    }

    /// Returns true if this type uses market value notional.
    #[must_use]
    pub fn uses_market_notional(&self) -> bool {
        matches!(self, Self::MarketValue | Self::Proceeds)
    }
}

impl std::fmt::Display for ASWType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coupon_year_fraction_uses_day_count_for_accrual_period() {
        use convex_bonds::traits::CashFlowType;
        use convex_core::types::Date;
        use rust_decimal::Decimal;

        // A coupon carrying explicit accrual boundaries must use the day-count
        // convention, not the nominal 1/frequency fallback the test fixtures hit.
        let start = Date::from_ymd(2024, 1, 15).unwrap();
        let end = Date::from_ymd(2024, 7, 17).unwrap();
        let cf = BondCashFlow {
            date: end,
            amount: Decimal::ONE,
            flow_type: CashFlowType::Coupon,
            accrual_start: Some(start),
            accrual_end: Some(end),
            factor: Decimal::ONE,
            reference_rate: None,
        };

        let dc = day_counter("ACT/360", 2);
        let yf = coupon_year_fraction(dc.as_deref(), &cf, 2);

        // ACT/360 == actual days / 360, and crucially not the nominal 0.5.
        let expected = start.days_between(&end) as f64 / 360.0;
        assert!(
            (yf - expected).abs() < 1e-9,
            "ACT/360 year fraction, got {yf}"
        );
        assert!(
            (yf - 0.5).abs() > 1e-6,
            "must not fall back to nominal 1/freq"
        );
    }

    fn coupon(start: (i32, u32, u32), end: (i32, u32, u32)) -> BondCashFlow {
        use convex_bonds::traits::CashFlowType;
        use convex_core::types::Date;
        let start = Date::from_ymd(start.0, start.1, start.2).unwrap();
        let end = Date::from_ymd(end.0, end.1, end.2).unwrap();
        BondCashFlow {
            date: end,
            amount: rust_decimal::Decimal::ONE,
            flow_type: CashFlowType::Coupon,
            accrual_start: Some(start),
            accrual_end: Some(end),
            factor: rust_decimal::Decimal::ONE,
            reference_rate: None,
        }
    }

    #[test]
    fn an_icma_coupon_is_its_share_of_the_regular_period_ending_with_it() {
        // Regular semiannual periods of 181 and 184 days are each exactly a half year,
        // not their ACT/ACT ISDA fractions (about 0.4959 and 0.5041).
        let semiannual = day_counter("ACT/ACT ICMA", 2);
        for (start, end) in [
            ((2054, 11, 15), (2055, 5, 15)),
            ((2055, 5, 15), (2055, 11, 15)),
        ] {
            let yf = coupon_year_fraction(semiannual.as_deref(), &coupon(start, end), 2);
            assert!((yf - 0.5).abs() < 1e-15, "{start:?} to {end:?}: {yf}");
        }
        // A quarterly bond's regular period is a quarter, at its own frequency.
        let quarterly = day_counter("ACT/ACT ICMA", 4);
        let yf = coupon_year_fraction(
            quarterly.as_deref(),
            &coupon((2026, 1, 15), (2026, 4, 15)),
            4,
        );
        assert!((yf - 0.25).abs() < 1e-15, "{yf}");
        // A short first coupon of 92 days in a 181-day regular period.
        let yf = coupon_year_fraction(
            semiannual.as_deref(),
            &coupon((2026, 2, 12), (2026, 5, 15)),
            2,
        );
        let expected = 92.0 / (2.0 * 181.0);
        assert!((yf - expected).abs() < 1e-15, "{yf} against {expected}");
    }

    #[test]
    fn test_asw_type_description() {
        assert_eq!(ASWType::ParPar.description(), "Par-Par Asset Swap");
        assert_eq!(
            ASWType::MarketValue.description(),
            "Market Value Asset Swap"
        );
        assert_eq!(ASWType::Proceeds.description(), "Proceeds Asset Swap");
    }

    #[test]
    fn test_asw_type_notional() {
        assert!(ASWType::ParPar.uses_par_notional());
        assert!(!ASWType::ParPar.uses_market_notional());

        assert!(!ASWType::MarketValue.uses_par_notional());
        assert!(ASWType::MarketValue.uses_market_notional());

        assert!(!ASWType::Proceeds.uses_par_notional());
        assert!(ASWType::Proceeds.uses_market_notional());
    }

    #[test]
    fn test_asw_type_display() {
        assert_eq!(format!("{}", ASWType::ParPar), "Par-Par Asset Swap");
    }

    #[test]
    fn test_asw_type_default() {
        assert_eq!(ASWType::default(), ASWType::ParPar);
    }
}
