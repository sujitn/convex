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
use convex_core::daycounts::{DayCount, DayCountConvention};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

/// Parses a bond's day-count convention string into a reusable day counter.
///
/// Returns `None` for unrecognised conventions, in which case callers fall back
/// to the nominal coupon fraction. Parse once per bond and reuse across the
/// coupon loop rather than re-parsing the string on every cash flow.
pub(crate) fn day_counter(day_count: &str) -> Option<Box<dyn DayCount>> {
    day_count
        .parse::<DayCountConvention>()
        .ok()
        .map(|conv| conv.to_day_count())
}

/// Year fraction for a single coupon period.
///
/// When the cash flow carries accrual boundaries and a `day_count` is available,
/// the year fraction is computed with the bond's own day-count convention;
/// otherwise it falls back to the nominal `1 / payments_per_year`. This prices
/// regular periods at the nominal fraction and stubs on their actual accrual
/// length, instead of guessing "regular vs stub" from a day-count threshold.
pub(crate) fn coupon_year_fraction(
    day_count: Option<&dyn DayCount>,
    cf: &BondCashFlow,
    payments_per_year: u32,
) -> f64 {
    let nominal = 1.0 / payments_per_year as f64;
    match (day_count, cf.accrual_start, cf.accrual_end) {
        (Some(dc), Some(start), Some(end)) => dc
            .year_fraction(start, end)
            .to_f64()
            .filter(|yf| *yf > 0.0)
            .unwrap_or(nominal),
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
