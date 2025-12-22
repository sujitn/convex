//! FFI functions for bond pricing and risk analytics.
//!
//! This module provides C-compatible functions for:
//! - Yield to maturity calculations
//! - Price from yield calculations
//! - Duration and convexity
//! - DV01 and risk metrics

use libc::{c_double, c_int};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_analytics::functions::{
    clean_price_from_yield, convexity, dirty_price_from_yield, dv01, macaulay_duration,
    modified_duration, yield_to_maturity, yield_to_maturity_with_convention,
};
use convex_analytics::risk::{
    effective_duration as compute_effective_duration, key_rate_duration_at_tenor as compute_krd,
    STANDARD_KEY_RATE_TENORS,
};
use convex_bonds::instruments::FixedRateBond;
use convex_bonds::traits::Bond;
use convex_bonds::types::YieldConvention;
use convex_core::types::{Date, Frequency};

use crate::error::set_last_error;
use crate::registry::{self, Handle};
use crate::{CONVEX_ERROR_INVALID_ARG, CONVEX_ERROR_NULL_PTR, CONVEX_OK};

// ============================================================================
// Conversion Helpers
// ============================================================================

fn frequency_from_ffi(freq: c_int) -> Frequency {
    match freq {
        1 => Frequency::Annual,
        2 => Frequency::SemiAnnual,
        4 => Frequency::Quarterly,
        12 => Frequency::Monthly,
        _ => Frequency::SemiAnnual,
    }
}

fn yield_convention_from_ffi(convention: c_int) -> YieldConvention {
    match convention {
        0 => YieldConvention::StreetConvention,
        1 => YieldConvention::TrueYield,
        2 => YieldConvention::ISMA,
        3 => YieldConvention::SimpleYield,
        4 => YieldConvention::DiscountYield,
        5 => YieldConvention::BondEquivalentYield,
        6 => YieldConvention::MunicipalYield,
        7 => YieldConvention::Continuous,
        _ => YieldConvention::StreetConvention,
    }
}

// ============================================================================
// Yield Calculations
// ============================================================================

/// Calculates yield to maturity from clean price.
///
/// # Arguments
///
/// * `bond` - Bond handle
/// * `settle_year/month/day` - Settlement date
/// * `clean_price` - Clean price per 100 face value
/// * `frequency` - Compounding frequency (1=Annual, 2=Semi, 4=Quarterly)
///
/// # Returns
///
/// Yield to maturity as decimal (e.g., 0.05 for 5%), or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_yield(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    clean_price: c_double,
    frequency: c_int,
) -> c_double {
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    if clean_price <= 0.0 {
        set_last_error("Price must be positive");
        return f64::NAN;
    }

    let freq = frequency_from_ffi(frequency);
    let price_decimal = Decimal::try_from(clean_price).unwrap_or(Decimal::ZERO);

    registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        match yield_to_maturity(b, settlement, price_decimal, freq) {
            Ok(result) => result.yield_value,
            Err(e) => {
                set_last_error(format!("Yield calculation failed: {}", e));
                f64::NAN
            }
        }
    })
    .unwrap_or_else(|| {
        set_last_error("Invalid bond handle");
        f64::NAN
    })
}

/// Calculates yield to maturity with a specific convention.
///
/// # Arguments
///
/// * `bond` - Bond handle
/// * `settle_year/month/day` - Settlement date
/// * `clean_price` - Clean price per 100 face value
/// * `frequency` - Compounding frequency (1=Annual, 2=Semi, 4=Quarterly)
/// * `convention` - Yield convention:
///   - 0 = Street Convention (default)
///   - 1 = True Yield
///   - 2 = ISMA/ICMA
///   - 3 = Simple Yield (Japanese)
///   - 4 = Discount Yield
///   - 5 = Bond Equivalent Yield
///   - 6 = Municipal Yield
///   - 7 = Continuous
///
/// # Returns
///
/// Yield to maturity as decimal (e.g., 0.05 for 5%), or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_yield_with_convention(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    clean_price: c_double,
    frequency: c_int,
    convention: c_int,
) -> c_double {
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    if clean_price <= 0.0 {
        set_last_error("Price must be positive");
        return f64::NAN;
    }

    let freq = frequency_from_ffi(frequency);
    let yield_conv = yield_convention_from_ffi(convention);
    let price_decimal = Decimal::try_from(clean_price).unwrap_or(Decimal::ZERO);

    registry::with_object::<FixedRateBond, _, _>(bond, |b| match yield_to_maturity_with_convention(
        b,
        settlement,
        price_decimal,
        freq,
        yield_conv,
    ) {
        Ok(result) => result.yield_value,
        Err(e) => {
            set_last_error(format!("Yield calculation failed: {}", e));
            f64::NAN
        }
    })
    .unwrap_or_else(|| {
        set_last_error("Invalid bond handle");
        f64::NAN
    })
}

// ============================================================================
// Price Calculations
// ============================================================================

/// Calculates clean price from yield.
///
/// # Arguments
///
/// * `bond` - Bond handle
/// * `settle_year/month/day` - Settlement date
/// * `ytm` - Yield to maturity as decimal (e.g., 0.05 for 5%)
/// * `frequency` - Compounding frequency
///
/// # Returns
///
/// Clean price per 100 face value, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_price(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    ytm: c_double,
    frequency: c_int,
) -> c_double {
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    let freq = frequency_from_ffi(frequency);

    registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        match clean_price_from_yield(b, settlement, ytm, freq) {
            Ok(price) => price,
            Err(e) => {
                set_last_error(format!("Price calculation failed: {}", e));
                f64::NAN
            }
        }
    })
    .unwrap_or_else(|| {
        set_last_error("Invalid bond handle");
        f64::NAN
    })
}

/// Calculates dirty (full) price from yield.
///
/// # Returns
///
/// Dirty price per 100 face value, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_dirty_price(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    ytm: c_double,
    frequency: c_int,
) -> c_double {
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    let freq = frequency_from_ffi(frequency);

    registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        match dirty_price_from_yield(b, settlement, ytm, freq) {
            Ok(price) => price,
            Err(e) => {
                set_last_error(format!("Dirty price calculation failed: {}", e));
                f64::NAN
            }
        }
    })
    .unwrap_or_else(|| {
        set_last_error("Invalid bond handle");
        f64::NAN
    })
}

// ============================================================================
// Duration Calculations
// ============================================================================

/// Calculates modified duration.
///
/// # Arguments
///
/// * `bond` - Bond handle
/// * `settle_year/month/day` - Settlement date
/// * `ytm` - Yield to maturity as decimal
/// * `frequency` - Compounding frequency
///
/// # Returns
///
/// Modified duration in years, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_duration(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    ytm: c_double,
    frequency: c_int,
) -> c_double {
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    let freq = frequency_from_ffi(frequency);

    registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        match modified_duration(b, settlement, ytm, freq) {
            Ok(dur) => dur,
            Err(e) => {
                set_last_error(format!("Duration calculation failed: {}", e));
                f64::NAN
            }
        }
    })
    .unwrap_or_else(|| {
        set_last_error("Invalid bond handle");
        f64::NAN
    })
}

/// Calculates Macaulay duration.
///
/// # Returns
///
/// Macaulay duration in years, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_duration_macaulay(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    ytm: c_double,
    frequency: c_int,
) -> c_double {
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    let freq = frequency_from_ffi(frequency);

    registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        match macaulay_duration(b, settlement, ytm, freq) {
            Ok(dur) => dur,
            Err(e) => {
                set_last_error(format!("Macaulay duration calculation failed: {}", e));
                f64::NAN
            }
        }
    })
    .unwrap_or_else(|| {
        set_last_error("Invalid bond handle");
        f64::NAN
    })
}

// ============================================================================
// Convexity Calculations
// ============================================================================

/// Calculates convexity.
///
/// # Returns
///
/// Convexity (in years squared), or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_convexity(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    ytm: c_double,
    frequency: c_int,
) -> c_double {
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    let freq = frequency_from_ffi(frequency);

    registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        match convexity(b, settlement, ytm, freq) {
            Ok(conv) => conv,
            Err(e) => {
                set_last_error(format!("Convexity calculation failed: {}", e));
                f64::NAN
            }
        }
    })
    .unwrap_or_else(|| {
        set_last_error("Invalid bond handle");
        f64::NAN
    })
}

// ============================================================================
// DV01 Calculations
// ============================================================================

/// Calculates DV01 (dollar value of one basis point).
///
/// DV01 = Modified Duration × Dirty Price × 0.0001
///
/// # Arguments
///
/// * `bond` - Bond handle
/// * `settle_year/month/day` - Settlement date
/// * `ytm` - Yield to maturity as decimal
/// * `dirty_price` - Dirty price per 100 face value
/// * `frequency` - Compounding frequency
///
/// # Returns
///
/// DV01 per 100 face value, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_dv01(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    ytm: c_double,
    dirty_price: c_double,
    frequency: c_int,
) -> c_double {
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    let freq = frequency_from_ffi(frequency);

    registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        match dv01(b, settlement, ytm, dirty_price, freq) {
            Ok(dv01_val) => dv01_val,
            Err(e) => {
                set_last_error(format!("DV01 calculation failed: {}", e));
                f64::NAN
            }
        }
    })
    .unwrap_or_else(|| {
        set_last_error("Invalid bond handle");
        f64::NAN
    })
}

// ============================================================================
// Combined Analytics
// ============================================================================

/// Bond analytics result structure for FFI.
#[repr(C)]
pub struct FfiBondAnalytics {
    pub clean_price: c_double,
    pub dirty_price: c_double,
    pub accrued: c_double,
    pub yield_to_maturity: c_double,
    pub modified_duration: c_double,
    pub macaulay_duration: c_double,
    pub convexity: c_double,
    pub dv01: c_double,
}

/// Calculates comprehensive bond analytics.
///
/// This function performs all major calculations in a single call for efficiency.
///
/// # Safety
///
/// `result_out` must point to a valid `FfiBondAnalytics` structure.
///
/// # Arguments
///
/// * `bond` - Bond handle
/// * `settle_year/month/day` - Settlement date
/// * `clean_price` - Clean price per 100 face value (input for yield calculation)
/// * `frequency` - Compounding frequency
/// * `result_out` - Pointer to output structure
///
/// # Returns
///
/// CONVEX_OK on success, error code on failure.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_analytics(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    clean_price: c_double,
    frequency: c_int,
    result_out: *mut FfiBondAnalytics,
) -> c_int {
    if result_out.is_null() {
        set_last_error("Null pointer for result");
        return CONVEX_ERROR_NULL_PTR;
    }

    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return CONVEX_ERROR_INVALID_ARG;
        }
    };

    let freq = frequency_from_ffi(frequency);
    let price_decimal = Decimal::try_from(clean_price).unwrap_or(Decimal::ZERO);

    let result = registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        // Calculate accrued interest
        let accrued = b.accrued_interest(settlement).to_f64().unwrap_or(0.0);
        let dirty = clean_price + accrued;

        // Calculate YTM
        let ytm = match yield_to_maturity(b, settlement, price_decimal, freq) {
            Ok(r) => r.yield_value,
            Err(_) => return Err("Yield calculation failed"),
        };

        // Calculate durations
        let mod_dur = modified_duration(b, settlement, ytm, freq).unwrap_or(f64::NAN);
        let mac_dur = macaulay_duration(b, settlement, ytm, freq).unwrap_or(f64::NAN);

        // Calculate convexity
        let conv = convexity(b, settlement, ytm, freq).unwrap_or(f64::NAN);

        // Calculate DV01
        let dv01_val = dv01(b, settlement, ytm, dirty, freq).unwrap_or(f64::NAN);

        Ok(FfiBondAnalytics {
            clean_price,
            dirty_price: dirty,
            accrued,
            yield_to_maturity: ytm,
            modified_duration: mod_dur,
            macaulay_duration: mac_dur,
            convexity: conv,
            dv01: dv01_val,
        })
    });

    match result {
        Some(Ok(analytics)) => {
            *result_out = analytics;
            CONVEX_OK
        }
        Some(Err(e)) => {
            set_last_error(e);
            CONVEX_ERROR_INVALID_ARG
        }
        None => {
            set_last_error("Invalid bond handle");
            CONVEX_ERROR_INVALID_ARG
        }
    }
}

// ============================================================================
// Effective Duration (Finite Difference)
// ============================================================================

/// Calculates effective duration using finite differences.
///
/// Effective duration measures price sensitivity by bumping yields up and down,
/// making it suitable for bonds with embedded options.
///
/// D_eff = (P- - P+) / (2 × P0 × Δy)
///
/// # Arguments
///
/// * `price_up` - Price when yield increases by bump_bps
/// * `price_down` - Price when yield decreases by bump_bps
/// * `price_base` - Current base price
/// * `bump_bps` - Yield bump size in basis points (e.g., 10 for 10bp)
///
/// # Returns
///
/// Effective duration, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_effective_duration(
    price_up: c_double,
    price_down: c_double,
    price_base: c_double,
    bump_bps: c_double,
) -> c_double {
    if price_base.abs() < 1e-10 {
        set_last_error("Base price is zero");
        return f64::NAN;
    }

    if bump_bps.abs() < 0.001 {
        set_last_error("Bump size too small");
        return f64::NAN;
    }

    // Convert bps to decimal
    let bump_decimal = bump_bps / 10000.0;

    match compute_effective_duration(price_up, price_down, price_base, bump_decimal) {
        Ok(dur) => dur.as_f64(),
        Err(e) => {
            set_last_error(format!("Effective duration failed: {}", e));
            f64::NAN
        }
    }
}

/// Calculates effective convexity using finite differences.
///
/// C_eff = (P- + P+ - 2×P0) / (P0 × Δy²)
///
/// # Arguments
///
/// * `price_up` - Price when yield increases by bump_bps
/// * `price_down` - Price when yield decreases by bump_bps
/// * `price_base` - Current base price
/// * `bump_bps` - Yield bump size in basis points
///
/// # Returns
///
/// Effective convexity, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_effective_convexity(
    price_up: c_double,
    price_down: c_double,
    price_base: c_double,
    bump_bps: c_double,
) -> c_double {
    if price_base.abs() < 1e-10 {
        set_last_error("Base price is zero");
        return f64::NAN;
    }

    if bump_bps.abs() < 0.001 {
        set_last_error("Bump size too small");
        return f64::NAN;
    }

    // Convert bps to decimal
    let bump_decimal = bump_bps / 10000.0;

    // C_eff = (P- + P+ - 2×P0) / (P0 × Δy²)
    (price_down + price_up - 2.0 * price_base) / (price_base * bump_decimal * bump_decimal)
}

// ============================================================================
// Key Rate Duration
// ============================================================================

/// Calculates key rate duration at a specific tenor.
///
/// Key rate duration measures sensitivity to a bump at a specific curve point.
///
/// # Arguments
///
/// * `price_up` - Price when rate at tenor increases by bump
/// * `price_down` - Price when rate at tenor decreases by bump
/// * `price_base` - Current base price
/// * `bump_bps` - Rate bump size in basis points
/// * `tenor` - The tenor point in years (e.g., 2.0 for 2-year)
///
/// # Returns
///
/// Key rate duration at the specified tenor, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_key_rate_duration(
    price_up: c_double,
    price_down: c_double,
    price_base: c_double,
    bump_bps: c_double,
    tenor: c_double,
) -> c_double {
    if price_base.abs() < 1e-10 {
        set_last_error("Base price is zero");
        return f64::NAN;
    }

    if bump_bps.abs() < 0.001 {
        set_last_error("Bump size too small");
        return f64::NAN;
    }

    // Convert bps to decimal
    let bump_decimal = bump_bps / 10000.0;

    match compute_krd(price_up, price_down, price_base, bump_decimal, tenor) {
        Ok(krd) => krd.duration.as_f64(),
        Err(e) => {
            set_last_error(format!("Key rate duration failed: {}", e));
            f64::NAN
        }
    }
}

/// Key rate duration result for a single tenor.
#[repr(C)]
#[allow(dead_code)]
pub struct FfiKeyRateDuration {
    pub tenor: c_double,
    pub duration: c_double,
}

/// Key rate durations result structure.
#[repr(C)]
#[allow(dead_code)]
pub struct FfiKeyRateDurations {
    /// Array of key rate durations (caller must provide buffer of at least 10 elements)
    pub count: c_int,
    /// Total duration (sum of all KRDs)
    pub total_duration: c_double,
}

/// Gets the standard key rate tenors used for KRD calculations.
///
/// # Arguments
///
/// * `tenors_out` - Output array (must have space for at least 10 doubles)
/// * `max_count` - Maximum number of tenors to return
///
/// # Returns
///
/// Number of standard tenors (typically 10).
#[no_mangle]
pub unsafe extern "C" fn convex_standard_key_rate_tenors(
    tenors_out: *mut c_double,
    max_count: c_int,
) -> c_int {
    if tenors_out.is_null() {
        return 0;
    }

    let count = std::cmp::min(max_count as usize, STANDARD_KEY_RATE_TENORS.len());

    for (i, &tenor) in STANDARD_KEY_RATE_TENORS.iter().enumerate().take(count) {
        *tenors_out.add(i) = tenor;
    }

    count as c_int
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bonds::convex_bond_us_corporate;
    use crate::registry;
    use std::ptr;

    fn create_test_bond() -> Handle {
        unsafe {
            convex_bond_us_corporate(
                ptr::null(),
                7.5, // 7.5% coupon
                2025,
                6,
                15, // Maturity
                2005,
                5,
                31, // Issue
            )
        }
    }

    #[test]
    fn test_yield_calculation() {
        unsafe {
            let bond = create_test_bond();
            assert_ne!(bond, 0);

            // Calculate yield from price
            let ytm = convex_bond_yield(bond, 2020, 4, 29, 110.503, 2);
            assert!(!ytm.is_nan(), "YTM should not be NaN");
            assert!(
                ytm > 0.0 && ytm < 0.10,
                "YTM {} out of reasonable range",
                ytm
            );

            registry::release(bond);
        }
    }

    #[test]
    fn test_price_yield_roundtrip() {
        unsafe {
            let bond = create_test_bond();

            let original_price = 105.0;
            let ytm = convex_bond_yield(bond, 2020, 4, 29, original_price, 2);

            assert!(!ytm.is_nan());

            let calculated_price = convex_bond_price(bond, 2020, 4, 29, ytm, 2);
            assert!(!calculated_price.is_nan());

            let diff = (calculated_price - original_price).abs();
            assert!(diff < 0.01, "Price roundtrip error: {}", diff);

            registry::release(bond);
        }
    }

    #[test]
    fn test_duration() {
        unsafe {
            let bond = create_test_bond();

            let ytm = 0.05; // 5%
            let duration = convex_bond_duration(bond, 2020, 4, 29, ytm, 2);

            assert!(!duration.is_nan());
            // 5-year bond should have duration around 4-5 years
            assert!(
                duration > 3.0 && duration < 6.0,
                "Duration {} out of range",
                duration
            );

            let mac_duration = convex_bond_duration_macaulay(bond, 2020, 4, 29, ytm, 2);
            assert!(!mac_duration.is_nan());
            // Macaulay should be slightly higher than modified
            assert!(mac_duration >= duration);

            registry::release(bond);
        }
    }

    #[test]
    fn test_convexity() {
        unsafe {
            let bond = create_test_bond();

            let ytm = 0.05;
            let conv = convex_bond_convexity(bond, 2020, 4, 29, ytm, 2);

            assert!(!conv.is_nan());
            assert!(conv > 0.0, "Convexity should be positive");

            registry::release(bond);
        }
    }

    #[test]
    fn test_dv01() {
        unsafe {
            let bond = create_test_bond();

            let ytm = 0.05;
            let dirty_price = 105.0;
            let dv01_val = convex_bond_dv01(bond, 2020, 4, 29, ytm, dirty_price, 2);

            assert!(!dv01_val.is_nan());
            assert!(dv01_val > 0.0, "DV01 should be positive");

            registry::release(bond);
        }
    }

    #[test]
    fn test_full_analytics() {
        unsafe {
            let bond = create_test_bond();

            let mut result: FfiBondAnalytics = std::mem::zeroed();
            let status = convex_bond_analytics(bond, 2020, 4, 29, 110.503, 2, &mut result);

            assert_eq!(status, CONVEX_OK);
            assert!(!result.yield_to_maturity.is_nan());
            assert!(!result.modified_duration.is_nan());
            assert!(!result.convexity.is_nan());
            assert!(!result.dv01.is_nan());
            assert!(result.accrued > 0.0);
            assert!(result.dirty_price > result.clean_price);

            registry::release(bond);
        }
    }
}
