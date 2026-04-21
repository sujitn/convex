//! Bond analytics as free functions.
//!
//! Each of these is a thin adapter over [`convex_bonds::traits::BondAnalytics`],
//! which owns the canonical math. The wrappers exist for callers that prefer a
//! functional style (`macaulay_duration(bond, ...)` instead of
//! `bond.macaulay_duration(...)`) and handle the `BondError` → `AnalyticsError`
//! translation.

use std::str::FromStr;

use rust_decimal::Decimal;

use convex_bonds::traits::{Bond, BondAnalytics};
use convex_bonds::types::YieldConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Date, Frequency};

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::yields::YieldResult;

fn bond_err<E: std::fmt::Display>(reason: E) -> AnalyticsError {
    AnalyticsError::CalculationFailed(reason.to_string())
}

// ============================================================================
// YIELD
// ============================================================================

/// Yield-to-maturity from clean price, using the street convention.
pub fn yield_to_maturity(
    bond: &dyn Bond,
    settlement: Date,
    clean_price: Decimal,
    frequency: Frequency,
) -> AnalyticsResult<YieldResult> {
    bond.yield_to_maturity(settlement, clean_price, frequency)
        .map_err(bond_err)
}

/// Yield-to-maturity with an explicit yield convention (street / true / continuous / ...).
pub fn yield_to_maturity_with_convention(
    bond: &dyn Bond,
    settlement: Date,
    clean_price: Decimal,
    frequency: Frequency,
    convention: YieldConvention,
) -> AnalyticsResult<YieldResult> {
    bond.yield_to_maturity_with_convention(settlement, clean_price, frequency, convention)
        .map_err(bond_err)
}

// ============================================================================
// PRICE
// ============================================================================

/// Dirty price per 100 face for a given yield.
pub fn dirty_price_from_yield(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    bond.dirty_price_from_yield(settlement, ytm, frequency)
        .map_err(bond_err)
}

/// Clean price per 100 face for a given yield.
pub fn clean_price_from_yield(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    bond.clean_price_from_yield(settlement, ytm, frequency)
        .map_err(bond_err)
}

// ============================================================================
// DURATION
// ============================================================================

/// Analytical Macaulay duration (years).
pub fn macaulay_duration(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    bond.macaulay_duration(settlement, ytm, frequency)
        .map_err(bond_err)
}

/// Modified duration = Macaulay / (1 + y/f).
pub fn modified_duration(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    bond.modified_duration(settlement, ytm, frequency)
        .map_err(bond_err)
}

/// Effective duration by central-difference bumping.
pub fn effective_duration(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
    bump_bps: f64,
) -> AnalyticsResult<f64> {
    bond.effective_duration(settlement, ytm, frequency, bump_bps)
        .map_err(bond_err)
}

// ============================================================================
// CONVEXITY
// ============================================================================

/// Analytical convexity (years²).
pub fn convexity(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    bond.convexity(settlement, ytm, frequency).map_err(bond_err)
}

/// Effective convexity by second-difference bumping.
pub fn effective_convexity(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
    bump_bps: f64,
) -> AnalyticsResult<f64> {
    bond.effective_convexity(settlement, ytm, frequency, bump_bps)
        .map_err(bond_err)
}

// ============================================================================
// DV01
// ============================================================================

/// DV01 per 100 face.
pub fn dv01(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    dirty_price: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    bond.dv01(settlement, ytm, dirty_price, frequency)
        .map_err(bond_err)
}

/// DV01 scaled to a notional amount.
pub fn dv01_notional(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    dirty_price: f64,
    notional: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    bond.dv01_notional(settlement, ytm, dirty_price, notional, frequency)
        .map_err(bond_err)
}

// ============================================================================
// PRICE-CHANGE APPROXIMATION
// ============================================================================

/// Estimated price change ΔP ≈ −D·ΔyP + ½·C·Δy²·P.
pub fn estimate_price_change(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    dirty_price: f64,
    yield_change: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    bond.estimate_price_change(settlement, ytm, dirty_price, yield_change, frequency)
        .map_err(bond_err)
}

// ============================================================================
// HELPER
// ============================================================================

/// Parses a day count convention string. Delegates to
/// [`DayCountConvention::from_str`]; kept in the analytics public surface so
/// callers don't need to import `std::str::FromStr` or the core type directly.
pub fn parse_day_count(dcc_str: &str) -> AnalyticsResult<DayCountConvention> {
    DayCountConvention::from_str(dcc_str).map_err(|e| AnalyticsError::DayCountError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_bonds::instruments::FixedRateBond;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_test_bond() -> FixedRateBond {
        FixedRateBond::builder()
            .issue_date(date(2020, 6, 15))
            .maturity(date(2025, 6, 15))
            .coupon_rate(dec!(0.075))
            .face_value(dec!(100))
            .frequency(Frequency::SemiAnnual)
            .day_count(DayCountConvention::Thirty360US)
            .cusip_unchecked("097023AH7")
            .build()
            .unwrap()
    }

    #[test]
    fn test_ytm_at_par() {
        let bond = create_test_bond();
        let result =
            yield_to_maturity(&bond, date(2020, 6, 15), dec!(100), Frequency::SemiAnnual).unwrap();
        assert!((result.yield_value - 0.075).abs() < 0.001);
    }

    #[test]
    fn test_ytm_price_roundtrip() {
        let bond = create_test_bond();
        let settlement = date(2021, 1, 15);
        let ytm =
            yield_to_maturity(&bond, settlement, dec!(105), Frequency::SemiAnnual).unwrap();
        let back = clean_price_from_yield(&bond, settlement, ytm.yield_value, Frequency::SemiAnnual)
            .unwrap();
        assert!((back - 105.0).abs() < 0.001);
    }

    #[test]
    fn test_modified_duration_range() {
        let bond = create_test_bond();
        let dur = modified_duration(&bond, date(2020, 6, 15), 0.075, Frequency::SemiAnnual).unwrap();
        assert!(dur > 3.5 && dur < 5.0);
    }

    #[test]
    fn test_convexity_positive() {
        let bond = create_test_bond();
        let c = convexity(&bond, date(2020, 6, 15), 0.075, Frequency::SemiAnnual).unwrap();
        assert!(c > 10.0 && c < 30.0);
    }

    #[test]
    fn test_dv01_range() {
        let bond = create_test_bond();
        let d = dv01(&bond, date(2020, 6, 15), 0.075, 100.0, Frequency::SemiAnnual).unwrap();
        assert!(d > 0.03 && d < 0.06);
    }

    #[test]
    fn test_effective_matches_modified() {
        let bond = create_test_bond();
        let settle = date(2020, 6, 15);
        let m = modified_duration(&bond, settle, 0.075, Frequency::SemiAnnual).unwrap();
        let e = effective_duration(&bond, settle, 0.075, Frequency::SemiAnnual, 10.0).unwrap();
        assert!((m - e).abs() < 0.1);
    }

    #[test]
    fn test_price_change_drops_on_yield_rise() {
        let bond = create_test_bond();
        let settle = date(2020, 6, 15);
        let dp =
            estimate_price_change(&bond, settle, 0.075, 100.0, 0.01, Frequency::SemiAnnual).unwrap();
        assert!(dp > -5.0 && dp < -3.0);
    }

    #[test]
    fn test_parse_day_count_known_and_unknown() {
        assert_eq!(parse_day_count("ACT/360").unwrap(), DayCountConvention::Act360);
        assert_eq!(
            parse_day_count("30/360").unwrap(),
            DayCountConvention::Thirty360US
        );
        assert_eq!(
            parse_day_count("ACT/ACT ISDA").unwrap(),
            DayCountConvention::ActActIsda
        );
        assert!(parse_day_count("INVALID").is_err());
    }
}
