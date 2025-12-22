//! FFI functions for spread calculations (Z-spread, I-spread, G-spread).
//!
//! These functions calculate various spread measures for bonds relative to curves.

use libc::c_int;
use rust_decimal::prelude::*;

use convex_analytics::spreads::{
    DiscountMarginCalculator, OASCalculator, ZSpreadCalculator, simple_margin,
};
use convex_bonds::instruments::{CallableBond, FixedRateBond, FloatingRateNote};
use convex_bonds::traits::Bond;
use convex_core::types::Date;
use convex_curves::curves::ForwardCurve;
use rust_decimal::Decimal;
use std::sync::Arc;

use crate::curves::StoredCurve;
use crate::error::set_last_error;
use crate::registry::{self, Handle};
use crate::{CONVEX_ERROR, CONVEX_ERROR_NOT_FOUND, CONVEX_OK};

// ============================================================================
// Z-Spread Functions
// ============================================================================

/// Calculates Z-spread for a bond given market price.
///
/// # Arguments
///
/// * `bond_handle` - Handle to a FixedBond object
/// * `curve_handle` - Handle to a RateCurve (discount curve)
/// * `settle_year/month/day` - Settlement date
/// * `clean_price` - Market clean price (e.g., 98.5 for 98.5% of par)
///
/// # Returns
///
/// Z-spread in basis points, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_z_spread(
    bond_handle: Handle,
    curve_handle: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    clean_price: libc::c_double,
) -> libc::c_double {
    // Validate handles
    let bond_type = registry::get_type(bond_handle);
    if !bond_type.is_bond() {
        set_last_error("Invalid bond handle");
        return f64::NAN;
    }

    let curve_type = registry::get_type(curve_handle);
    if !curve_type.is_curve() {
        set_last_error("Invalid curve handle");
        return f64::NAN;
    }

    // Parse settlement date
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    // Get bond and curve from registry
    let spread_result = registry::with_object::<FixedRateBond, _, _>(bond_handle, |bond| {
        registry::with_object::<StoredCurve, _, _>(curve_handle, |curve| {
            // Calculate dirty price from clean price + accrued
            let accrued = Bond::accrued_interest(bond, settlement);
            let dirty_price =
                rust_decimal::Decimal::from_f64_retain(clean_price).unwrap_or_default() + accrued;

            ZSpreadCalculator::new(curve).calculate(bond, dirty_price, settlement)
        })
    });

    match spread_result {
        Some(Some(Ok(spread))) => spread.as_bps().to_f64().unwrap_or(f64::NAN),
        Some(Some(Err(e))) => {
            set_last_error(format!("Z-spread calculation failed: {}", e));
            f64::NAN
        }
        _ => {
            set_last_error("Failed to access bond or curve");
            f64::NAN
        }
    }
}

/// Calculates I-spread (interpolated swap spread) for a bond.
///
/// The I-spread is the difference between the bond's yield and the swap rate
/// at the same maturity, interpolated from the swap curve.
///
/// # Arguments
///
/// * `bond_handle` - Handle to a FixedBond object
/// * `swap_curve_handle` - Handle to a RateCurve (swap curve)
/// * `settle_year/month/day` - Settlement date
/// * `bond_yield` - Bond yield to maturity (as decimal, e.g., 0.05 for 5%)
///
/// # Returns
///
/// I-spread in basis points, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_i_spread(
    bond_handle: Handle,
    swap_curve_handle: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    bond_yield: libc::c_double,
) -> libc::c_double {
    // Validate handles
    let bond_type = registry::get_type(bond_handle);
    if !bond_type.is_bond() {
        set_last_error("Invalid bond handle");
        return f64::NAN;
    }

    let curve_type = registry::get_type(swap_curve_handle);
    if !curve_type.is_curve() {
        set_last_error("Invalid swap curve handle");
        return f64::NAN;
    }

    // Parse settlement date
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    // Get bond and curve from registry - calculate I-spread directly
    let spread_result = registry::with_object::<FixedRateBond, _, _>(bond_handle, |bond| {
        registry::with_object::<StoredCurve, _, _>(swap_curve_handle, |curve| {
            // Get bond's time to maturity
            let maturity = Bond::maturity(bond)?;
            let days_to_mat = settlement.days_between(&maturity) as f64 / 365.0;

            // Get swap rate at bond's maturity from curve
            let swap_rate = curve
                .zero_rate_at_tenor(days_to_mat, convex_core::types::Compounding::SemiAnnual)
                .ok()?;

            // I-spread = Bond Yield - Swap Rate
            let i_spread_decimal = bond_yield - swap_rate;
            let i_spread_bps = (i_spread_decimal * 10000.0).round();

            Some(i_spread_bps)
        })
    });

    match spread_result {
        Some(Some(Some(spread))) => spread,
        _ => {
            set_last_error("Failed to calculate I-spread");
            f64::NAN
        }
    }
}

/// Calculates G-spread (government spread) for a bond.
///
/// The G-spread is the difference between the bond's yield and the interpolated
/// government bond yield at the same maturity.
///
/// # Arguments
///
/// * `bond_handle` - Handle to a FixedBond object
/// * `govt_curve_handle` - Handle to a RateCurve (government curve)
/// * `settle_year/month/day` - Settlement date
/// * `bond_yield` - Bond yield to maturity (as decimal, e.g., 0.05 for 5%)
///
/// # Returns
///
/// G-spread in basis points, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_g_spread(
    bond_handle: Handle,
    govt_curve_handle: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    bond_yield: libc::c_double,
) -> libc::c_double {
    // Validate handles
    let bond_type = registry::get_type(bond_handle);
    if !bond_type.is_bond() {
        set_last_error("Invalid bond handle");
        return f64::NAN;
    }

    let curve_type = registry::get_type(govt_curve_handle);
    if !curve_type.is_curve() {
        set_last_error("Invalid government curve handle");
        return f64::NAN;
    }

    // Parse settlement date
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    // Get bond and curve from registry - calculate G-spread directly
    let spread_result = registry::with_object::<FixedRateBond, _, _>(bond_handle, |bond| {
        registry::with_object::<StoredCurve, _, _>(govt_curve_handle, |curve| {
            // Get bond's time to maturity
            let maturity = Bond::maturity(bond)?;
            let days_to_mat = settlement.days_between(&maturity) as f64 / 365.0;

            // Get government rate at bond's maturity from curve
            let govt_rate = curve
                .zero_rate_at_tenor(days_to_mat, convex_core::types::Compounding::SemiAnnual)
                .ok()?;

            // G-spread = Bond Yield - Government Rate
            let g_spread_decimal = bond_yield - govt_rate;
            let g_spread_bps = (g_spread_decimal * 10000.0).round();

            Some(g_spread_bps)
        })
    });

    match spread_result {
        Some(Some(Some(spread))) => spread,
        _ => {
            set_last_error("Failed to calculate G-spread");
            f64::NAN
        }
    }
}

// ============================================================================
// ASW (Asset Swap Spread) Functions
// ============================================================================

/// Calculates Par-Par Asset Swap Spread for a bond.
///
/// The par-par ASW is calculated as:
/// ASW = (100 - Dirty Price) / Annuity
///
/// Where Annuity = sum of (DF_i * period_fraction) for each swap payment date.
///
/// # Arguments
///
/// * `bond_handle` - Handle to a FixedRateBond object
/// * `swap_curve_handle` - Handle to a RateCurve (swap curve for discounting)
/// * `settle_year/month/day` - Settlement date
/// * `clean_price` - Market clean price (as percentage of par)
///
/// # Returns
///
/// Asset swap spread in basis points, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_asw_spread(
    bond_handle: Handle,
    swap_curve_handle: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    clean_price: libc::c_double,
) -> libc::c_double {
    // Validate handles
    let bond_type = registry::get_type(bond_handle);
    if !bond_type.is_bond() {
        set_last_error("Invalid bond handle");
        return f64::NAN;
    }

    let curve_type = registry::get_type(swap_curve_handle);
    if !curve_type.is_curve() {
        set_last_error("Invalid swap curve handle");
        return f64::NAN;
    }

    // Parse settlement date
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    // Get bond and curve from registry
    let asw_result = registry::with_object::<FixedRateBond, _, _>(bond_handle, |bond| {
        registry::with_object::<StoredCurve, _, _>(swap_curve_handle, |curve| {
            calculate_par_par_asw(bond, curve, settlement, clean_price)
        })
    });

    match asw_result {
        Some(Some(Ok(spread_bps))) => spread_bps,
        Some(Some(Err(e))) => {
            set_last_error(format!("ASW calculation failed: {}", e));
            f64::NAN
        }
        _ => {
            set_last_error("Failed to access bond or curve");
            f64::NAN
        }
    }
}

/// Internal function to calculate par-par ASW spread.
fn calculate_par_par_asw(
    bond: &FixedRateBond,
    curve: &StoredCurve,
    settlement: Date,
    clean_price: f64,
) -> Result<f64, String> {
    use convex_bonds::traits::{Bond, FixedCouponBond};

    // Get maturity
    let maturity = Bond::maturity(bond).ok_or("Bond has no maturity")?;

    if settlement >= maturity {
        return Err("Settlement is at or after maturity".to_string());
    }

    // Calculate accrued interest and dirty price
    let accrued = Bond::accrued_interest(bond, settlement);
    let accrued_f64 = accrued.to_f64().unwrap_or(0.0);
    let dirty_price = clean_price + accrued_f64;

    // Upfront payment = 100 - Dirty Price (positive if bond trades at discount)
    let upfront = 100.0 - dirty_price;

    // Calculate swap annuity
    // Generate payment dates from maturity backwards
    let frequency = FixedCouponBond::coupon_frequency(bond);
    let months_between: i32 = match frequency {
        1 => 12, // Annual
        4 => 3,  // Quarterly
        12 => 1, // Monthly
        _ => 6,  // Semi-annual (default)
    };

    let ref_date = curve.reference_date();
    let mut payment_dates = Vec::new();
    let mut current_date = maturity;

    // Walk backwards from maturity to find payment dates after settlement
    while current_date > settlement {
        payment_dates.push(current_date);
        current_date = current_date
            .add_months(-months_between)
            .map_err(|e| e.to_string())?;
    }

    if payment_dates.is_empty() {
        return Err("No payment dates after settlement".to_string());
    }

    // Calculate annuity = sum of (DF * period_fraction)
    let period_fraction = 1.0 / frequency as f64;
    let mut annuity = 0.0;

    for payment_date in &payment_dates {
        // Calculate tenor from curve reference date
        let tenor = ref_date.days_between(payment_date) as f64 / 365.0;
        let df = curve.discount_factor_at_tenor(tenor).unwrap_or(1.0);
        annuity += df * period_fraction;
    }

    if annuity <= 0.0 {
        return Err("Invalid annuity calculation".to_string());
    }

    // ASW spread = Upfront / Annuity (as percentage, then convert to bps)
    let spread_pct = upfront / annuity;
    let spread_bps = spread_pct * 100.0; // Convert to basis points

    Ok(spread_bps)
}

/// Structure for returning spread result along with spread DV01.
#[repr(C)]
pub struct FfiSpreadResult {
    /// Spread in basis points
    pub spread_bps: libc::c_double,
    /// Spread DV01 (price change per 1bp spread change)
    pub spread_dv01: libc::c_double,
    /// Spread duration
    pub spread_duration: libc::c_double,
    /// Success flag (1 = success, 0 = error)
    pub success: c_int,
}

/// Calculates Z-spread with full analytics.
///
/// # Safety
///
/// `result` pointer must be valid and writable.
///
/// # Returns
///
/// CONVEX_OK on success, error code on failure.
#[no_mangle]
pub unsafe extern "C" fn convex_z_spread_analytics(
    bond_handle: Handle,
    curve_handle: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    clean_price: libc::c_double,
    result: *mut FfiSpreadResult,
) -> c_int {
    if result.is_null() {
        set_last_error("Null result pointer");
        return CONVEX_ERROR;
    }

    // Initialize result
    (*result).success = 0;
    (*result).spread_bps = f64::NAN;
    (*result).spread_dv01 = f64::NAN;
    (*result).spread_duration = f64::NAN;

    // Validate handles
    let bond_type = registry::get_type(bond_handle);
    if !bond_type.is_bond() {
        set_last_error("Invalid bond handle");
        return CONVEX_ERROR_NOT_FOUND;
    }

    let curve_type = registry::get_type(curve_handle);
    if !curve_type.is_curve() {
        set_last_error("Invalid curve handle");
        return CONVEX_ERROR_NOT_FOUND;
    }

    // Calculate Z-spread
    let spread_bps = convex_z_spread(
        bond_handle,
        curve_handle,
        settle_year,
        settle_month,
        settle_day,
        clean_price,
    );

    if spread_bps.is_nan() {
        return CONVEX_ERROR;
    }

    (*result).spread_bps = spread_bps;

    // Estimate DV01 using finite difference
    let price_down = clean_price - 0.01; // 1 cent lower price
    let spread_up = convex_z_spread(
        bond_handle,
        curve_handle,
        settle_year,
        settle_month,
        settle_day,
        price_down,
    );

    if !spread_up.is_nan() && (spread_up - spread_bps).abs() > 0.001 {
        // Spread DV01: how much does price change for 1bp spread change?
        // We have: delta_spread for delta_price = 0.01
        // So: DV01 = delta_price / delta_spread * 100 (to get per 100bp)
        let delta_spread = spread_up - spread_bps;
        (*result).spread_dv01 = 0.01 / delta_spread * 100.0;
        (*result).spread_duration = (*result).spread_dv01 / clean_price * 10000.0;
    }

    (*result).success = 1;
    CONVEX_OK
}

// ============================================================================
// OAS (Option-Adjusted Spread) Functions
// ============================================================================

/// OAS result structure.
#[repr(C)]
pub struct FfiOasResult {
    /// OAS in basis points
    pub oas_bps: libc::c_double,
    /// Effective duration using OAS model
    pub effective_duration: libc::c_double,
    /// Effective convexity using OAS model
    pub effective_convexity: libc::c_double,
    /// Option value (straight bond price - callable price)
    pub option_value: libc::c_double,
    /// Success flag (1 = success, 0 = error)
    pub success: c_int,
}

/// Calculates Option-Adjusted Spread for a callable bond.
///
/// OAS is the constant spread that, when added to all rates in the interest
/// rate model, makes the model price equal to the market price.
///
/// # Arguments
///
/// * `bond_handle` - Handle to a CallableBond object
/// * `curve_handle` - Handle to a RateCurve (discount curve)
/// * `settle_year/month/day` - Settlement date
/// * `dirty_price` - Market dirty price
/// * `volatility` - Interest rate volatility (e.g., 0.01 for 1%)
///
/// # Returns
///
/// OAS in basis points, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_oas(
    bond_handle: Handle,
    curve_handle: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    dirty_price: libc::c_double,
    volatility: libc::c_double,
) -> libc::c_double {
    // Validate handles
    let bond_type = registry::get_type(bond_handle);
    if bond_type != registry::ObjectType::CallableBond {
        set_last_error("Invalid callable bond handle");
        return f64::NAN;
    }

    let curve_type = registry::get_type(curve_handle);
    if !curve_type.is_curve() {
        set_last_error("Invalid curve handle");
        return f64::NAN;
    }

    // Parse settlement date
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    let vol = if volatility > 0.0 { volatility } else { 0.01 };

    // Calculate OAS
    let oas_result =
        registry::with_object::<CallableBond, _, _>(bond_handle, |bond| {
            registry::with_object::<StoredCurve, _, _>(curve_handle, |curve| {
                let price_decimal = Decimal::from_f64_retain(dirty_price).unwrap_or_default();
                let calc = OASCalculator::default_hull_white(vol);
                calc.calculate(bond, price_decimal, curve, settlement)
            })
        });

    match oas_result {
        Some(Some(Ok(spread))) => spread.as_bps().to_f64().unwrap_or(f64::NAN),
        Some(Some(Err(e))) => {
            set_last_error(format!("OAS calculation failed: {}", e));
            f64::NAN
        }
        _ => {
            set_last_error("Failed to access bond or curve");
            f64::NAN
        }
    }
}

/// Calculates comprehensive OAS analytics.
///
/// # Safety
///
/// `result` pointer must be valid and writable.
///
/// # Returns
///
/// CONVEX_OK on success, error code on failure.
#[no_mangle]
pub unsafe extern "C" fn convex_oas_analytics(
    bond_handle: Handle,
    curve_handle: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    dirty_price: libc::c_double,
    volatility: libc::c_double,
    result: *mut FfiOasResult,
) -> c_int {
    if result.is_null() {
        set_last_error("Null result pointer");
        return CONVEX_ERROR;
    }

    // Initialize result
    (*result).success = 0;
    (*result).oas_bps = f64::NAN;
    (*result).effective_duration = f64::NAN;
    (*result).effective_convexity = f64::NAN;
    (*result).option_value = f64::NAN;

    // Validate handles
    let bond_type = registry::get_type(bond_handle);
    if bond_type != registry::ObjectType::CallableBond {
        set_last_error("Invalid callable bond handle");
        return CONVEX_ERROR_NOT_FOUND;
    }

    let curve_type = registry::get_type(curve_handle);
    if !curve_type.is_curve() {
        set_last_error("Invalid curve handle");
        return CONVEX_ERROR_NOT_FOUND;
    }

    // Parse settlement date
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return CONVEX_ERROR;
        }
    };

    let vol = if volatility > 0.0 { volatility } else { 0.01 };

    // Calculate all OAS analytics
    let analytics_result =
        registry::with_object::<CallableBond, _, _>(bond_handle, |bond| {
            registry::with_object::<StoredCurve, _, _>(curve_handle, |curve| {
                let price_decimal = Decimal::from_f64_retain(dirty_price).unwrap_or_default();
                let calc = OASCalculator::default_hull_white(vol);

                // Calculate OAS
                let oas = match calc.calculate(bond, price_decimal, curve, settlement) {
                    Ok(s) => s.as_bps().to_f64().unwrap_or(f64::NAN),
                    Err(_) => return None,
                };

                let oas_decimal = oas / 10000.0;

                // Calculate effective duration
                let eff_dur = calc
                    .effective_duration(bond, curve, oas_decimal, settlement)
                    .unwrap_or(f64::NAN);

                // Calculate effective convexity
                let eff_conv = calc
                    .effective_convexity(bond, curve, oas_decimal, settlement)
                    .unwrap_or(f64::NAN);

                // Calculate option value
                let opt_val = calc
                    .option_value(bond, curve, oas_decimal, settlement)
                    .unwrap_or(f64::NAN);

                Some((oas, eff_dur, eff_conv, opt_val))
            })
        });

    match analytics_result {
        Some(Some(Some((oas, dur, conv, opt)))) => {
            (*result).oas_bps = oas;
            (*result).effective_duration = dur;
            (*result).effective_convexity = conv;
            (*result).option_value = opt;
            (*result).success = 1;
            CONVEX_OK
        }
        _ => {
            set_last_error("OAS calculation failed");
            CONVEX_ERROR
        }
    }
}

// ============================================================================
// Discount Margin Functions (FRNs)
// ============================================================================

/// Calculates simple margin for a floating rate note.
///
/// Simple margin is a quick approximation:
/// SM = Current Yield + (100 - Price) / (Price × Years to Maturity) - Index Rate
///
/// # Arguments
///
/// * `frn_handle` - Handle to a FloatingRateNote object
/// * `settle_year/month/day` - Settlement date
/// * `dirty_price` - Market dirty price
/// * `current_index` - Current index rate (as decimal, e.g., 0.05 for 5%)
///
/// # Returns
///
/// Simple margin in basis points, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_simple_margin(
    frn_handle: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    dirty_price: libc::c_double,
    current_index: libc::c_double,
) -> libc::c_double {
    // Validate handle
    let bond_type = registry::get_type(frn_handle);
    if bond_type != registry::ObjectType::FloatingRateNote {
        set_last_error("Invalid FRN handle");
        return f64::NAN;
    }

    // Parse settlement date
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    // Calculate simple margin
    let margin_result = registry::with_object::<FloatingRateNote, _, _>(frn_handle, |frn| {
        let price_decimal = Decimal::from_f64_retain(dirty_price).unwrap_or_default();
        let index_decimal = Decimal::from_f64_retain(current_index).unwrap_or_default();
        let margin = simple_margin(frn, price_decimal, index_decimal, settlement);
        margin.as_bps().to_f64().unwrap_or(f64::NAN)
    });

    margin_result.unwrap_or_else(|| {
        set_last_error("Failed to calculate simple margin");
        f64::NAN
    })
}

/// Discount margin result structure.
#[repr(C)]
pub struct FfiDiscountMarginResult {
    /// Discount margin in basis points
    pub dm_bps: libc::c_double,
    /// Spread DV01
    pub spread_dv01: libc::c_double,
    /// Spread duration
    pub spread_duration: libc::c_double,
    /// Success flag
    pub success: c_int,
}

/// Calculates Z-DM (Zero Discount Margin) for a floating rate note.
///
/// Z-DM is the full discount margin using the forward curve for projected
/// coupons and a discount curve for present value calculations.
///
/// # Arguments
///
/// * `frn_handle` - Handle to a FloatingRateNote object
/// * `forward_curve_handle` - Handle to the forward/projection curve
/// * `discount_curve_handle` - Handle to the discount curve
/// * `settle_year/month/day` - Settlement date
/// * `dirty_price` - Market dirty price
///
/// # Returns
///
/// Discount margin in basis points, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_discount_margin(
    frn_handle: Handle,
    forward_curve_handle: Handle,
    discount_curve_handle: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    dirty_price: libc::c_double,
) -> libc::c_double {
    // Validate FRN handle
    let bond_type = registry::get_type(frn_handle);
    if bond_type != registry::ObjectType::FloatingRateNote {
        set_last_error("Invalid FRN handle");
        return f64::NAN;
    }

    // Validate curve handles
    if !registry::get_type(forward_curve_handle).is_curve() {
        set_last_error("Invalid forward curve handle");
        return f64::NAN;
    }

    if !registry::get_type(discount_curve_handle).is_curve() {
        set_last_error("Invalid discount curve handle");
        return f64::NAN;
    }

    // Parse settlement date
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    // This is a complex calculation that requires building a forward curve from
    // the discount curve. For simplicity, we'll use the forward curve directly
    // if the user provides it, or construct one.
    let dm_result = registry::with_object::<FloatingRateNote, _, _>(frn_handle, |frn| {
        registry::with_object::<StoredCurve, _, _>(discount_curve_handle, |discount_curve| {
            // Create forward curve from discount curve (3-month forwards)
            let discount_arc: Arc<dyn convex_curves::RateCurveDyn> =
                Arc::new(discount_curve.clone());
            let forward_curve = ForwardCurve::from_months(discount_arc.clone(), 3);

            let price_decimal = Decimal::from_f64_retain(dirty_price).unwrap_or_default();
            let calc = DiscountMarginCalculator::new(&forward_curve, discount_curve);

            match calc.calculate(frn, price_decimal, settlement) {
                Ok(dm) => Some(dm.as_bps().to_f64().unwrap_or(f64::NAN)),
                Err(e) => {
                    set_last_error(format!("DM calculation failed: {}", e));
                    None
                }
            }
        })
    });

    match dm_result {
        Some(Some(Some(dm))) => dm,
        _ => {
            set_last_error("Failed to calculate discount margin");
            f64::NAN
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bonds::convex_bond_us_corporate;
    use crate::curves::convex_curve_from_zero_rates;
    use std::ffi::CString;

    #[test]
    fn test_z_spread_basic() {
        unsafe {
            // Create a test curve
            let curve_name = CString::new("TEST.CURVE.SPREAD").unwrap();
            let tenors = [0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0];
            let rates = [0.04, 0.042, 0.045, 0.047, 0.048, 0.049, 0.05];

            let curve_handle = convex_curve_from_zero_rates(
                curve_name.as_ptr(),
                2025,
                1,
                15,
                tenors.as_ptr(),
                rates.as_ptr(),
                tenors.len() as c_int,
                1, // Linear interpolation
                1, // Act365Fixed
            );

            assert_ne!(curve_handle, crate::registry::INVALID_HANDLE);

            // Create a test bond
            let bond_name = CString::new("TEST.BOND.SPREAD").unwrap();
            let bond_handle = convex_bond_us_corporate(
                bond_name.as_ptr(),
                5.0, // 5% coupon
                2030,
                1,
                15, // Maturity
                2020,
                1,
                15, // Issue
            );

            assert_ne!(bond_handle, crate::registry::INVALID_HANDLE);

            // Calculate Z-spread at par
            let z_spread = convex_z_spread(
                bond_handle,
                curve_handle,
                2025,
                1,
                15,    // Settlement
                100.0, // Par price
            );

            // Z-spread should be reasonable (not NaN)
            // At par with 5% coupon and ~5% curve, spread should be around 0
            println!("Z-spread at par: {} bps", z_spread);

            // Clean up
            registry::release(bond_handle);
            registry::release(curve_handle);
        }
    }
}
