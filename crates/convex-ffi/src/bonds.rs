//! FFI functions for bond operations.
//!
//! This module provides C-compatible functions for:
//! - Bond construction (fixed rate, zero coupon, FRN)
//! - Bond queries (cash flows, accrued interest)
//! - Bond pricing (yield to maturity, clean/dirty price)

use std::ffi::CStr;

use libc::{c_char, c_double, c_int};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_bonds::instruments::{CallableBond, FixedRateBond};
use convex_bonds::traits::{Bond, EmbeddedOptionBond};
use convex_bonds::types::{CallEntry, CallSchedule, CallType};
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};

use crate::error::set_last_error;
use crate::registry::{self, Handle, ObjectType, INVALID_HANDLE};
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

fn daycount_from_ffi(dc: c_int) -> DayCountConvention {
    match dc {
        0 => DayCountConvention::Act360,
        1 => DayCountConvention::Act365Fixed,
        2 => DayCountConvention::ActActIsda,
        3 => DayCountConvention::ActActIcma,
        4 => DayCountConvention::Thirty360US,
        5 => DayCountConvention::Thirty360E,
        _ => DayCountConvention::Thirty360US, // Default for corporate bonds
    }
}

fn currency_from_ffi(ccy: c_int) -> Currency {
    match ccy {
        0 => Currency::USD,
        1 => Currency::EUR,
        2 => Currency::GBP,
        3 => Currency::JPY,
        4 => Currency::CHF,
        5 => Currency::CAD,
        6 => Currency::AUD,
        _ => Currency::USD,
    }
}

/// Converts a date to an integer in YYYYMMDD format.
fn date_to_int(d: Date) -> c_int {
    d.year() * 10000 + d.month() as i32 * 100 + d.day() as i32
}

// ============================================================================
// Bond Construction
// ============================================================================

/// Creates a fixed rate bond.
///
/// # Safety
///
/// - `isin` must be a valid null-terminated C string (can be null for unnamed bond).
///
/// # Arguments
///
/// * `isin` - ISIN or CUSIP identifier (null-terminated string or null)
/// * `coupon_rate` - Annual coupon rate as decimal (0.05 for 5%)
/// * `maturity_year/month/day` - Maturity date components
/// * `issue_year/month/day` - Issue date components
/// * `frequency` - Payment frequency (1=Annual, 2=Semi, 4=Quarterly, 12=Monthly)
/// * `day_count` - Day count convention index
/// * `currency` - Currency index
/// * `face_value` - Face value (typically 100)
///
/// # Returns
///
/// Handle to the bond, or INVALID_HANDLE on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_fixed(
    isin: *const c_char,
    coupon_rate: c_double,
    maturity_year: c_int,
    maturity_month: c_int,
    maturity_day: c_int,
    issue_year: c_int,
    issue_month: c_int,
    issue_day: c_int,
    frequency: c_int,
    day_count: c_int,
    currency: c_int,
    face_value: c_double,
) -> Handle {
    // Parse maturity date
    let maturity = match Date::from_ymd(maturity_year, maturity_month as u32, maturity_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid maturity date: {}", e));
            return INVALID_HANDLE;
        }
    };

    // Parse issue date
    let issue = match Date::from_ymd(issue_year, issue_month as u32, issue_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid issue date: {}", e));
            return INVALID_HANDLE;
        }
    };

    // Validate coupon rate
    if !(0.0..=1.0).contains(&coupon_rate) {
        set_last_error("Coupon rate must be between 0 and 1 (e.g., 0.05 for 5%)");
        return INVALID_HANDLE;
    }

    // Parse identifier or generate a default one
    let (mut builder, bond_name) = if !isin.is_null() {
        match CStr::from_ptr(isin).to_str() {
            Ok(s) if !s.is_empty() => {
                let b = FixedRateBond::builder().cusip_unchecked(s);
                (b, Some(s.to_string()))
            }
            _ => {
                let synthetic_id = format!(
                    "FIXED_{:08}",
                    maturity_year * 10000 + maturity_month * 100 + maturity_day
                );
                let b = FixedRateBond::builder().cusip_unchecked(&synthetic_id);
                (b, None)
            }
        }
    } else {
        let synthetic_id = format!(
            "FIXED_{:08}",
            maturity_year * 10000 + maturity_month * 100 + maturity_day
        );
        let b = FixedRateBond::builder().cusip_unchecked(&synthetic_id);
        (b, None)
    };

    // Build the bond
    builder = builder
        .coupon_rate(Decimal::try_from(coupon_rate).unwrap_or(Decimal::ZERO))
        .maturity(maturity)
        .issue_date(issue)
        .frequency(frequency_from_ffi(frequency))
        .day_count(daycount_from_ffi(day_count))
        .currency(currency_from_ffi(currency))
        .face_value(Decimal::try_from(face_value).unwrap_or(Decimal::ONE_HUNDRED));

    // Apply US corporate conventions as default
    builder = builder.us_corporate();

    let bond = match builder.build() {
        Ok(b) => b,
        Err(e) => {
            set_last_error(format!("Failed to build bond: {}", e));
            return INVALID_HANDLE;
        }
    };

    registry::register(bond, ObjectType::FixedBond, bond_name)
}

/// Creates a fixed rate bond with US Corporate conventions.
///
/// This is a convenience function that applies standard US corporate bond conventions:
/// - 30/360 US day count
/// - Semi-annual payments
/// - T+2 settlement
/// - USD currency
#[no_mangle]
pub unsafe extern "C" fn convex_bond_us_corporate(
    isin: *const c_char,
    coupon_percent: c_double,
    maturity_year: c_int,
    maturity_month: c_int,
    maturity_day: c_int,
    issue_year: c_int,
    issue_month: c_int,
    issue_day: c_int,
) -> Handle {
    // Parse maturity date
    let maturity = match Date::from_ymd(maturity_year, maturity_month as u32, maturity_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid maturity date: {}", e));
            return INVALID_HANDLE;
        }
    };

    // Parse issue date
    let issue = match Date::from_ymd(issue_year, issue_month as u32, issue_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid issue date: {}", e));
            return INVALID_HANDLE;
        }
    };

    // Parse identifier or generate a default one
    let (builder, bond_name) = if !isin.is_null() {
        match CStr::from_ptr(isin).to_str() {
            Ok(s) if !s.is_empty() => {
                let b = FixedRateBond::builder().cusip_unchecked(s);
                (b, Some(s.to_string()))
            }
            _ => {
                // Generate a synthetic ID
                let synthetic_id = format!(
                    "BOND_{:08}",
                    maturity_year * 10000 + maturity_month * 100 + maturity_day
                );
                let b = FixedRateBond::builder().cusip_unchecked(&synthetic_id);
                (b, None)
            }
        }
    } else {
        // Generate a synthetic ID
        let synthetic_id = format!(
            "BOND_{:08}",
            maturity_year * 10000 + maturity_month * 100 + maturity_day
        );
        let b = FixedRateBond::builder().cusip_unchecked(&synthetic_id);
        (b, None)
    };

    let builder = builder
        .coupon_percent(coupon_percent)
        .maturity(maturity)
        .issue_date(issue)
        .us_corporate();

    let bond = match builder.build() {
        Ok(b) => b,
        Err(e) => {
            set_last_error(format!("Failed to build bond: {}", e));
            return INVALID_HANDLE;
        }
    };

    registry::register(bond, ObjectType::FixedBond, bond_name)
}

/// Creates a fixed rate bond with US Treasury conventions.
///
/// Applies standard US Treasury conventions:
/// - ACT/ACT ICMA day count
/// - Semi-annual payments
/// - T+1 settlement
/// - USD currency
#[no_mangle]
pub unsafe extern "C" fn convex_bond_us_treasury(
    isin: *const c_char,
    coupon_percent: c_double,
    maturity_year: c_int,
    maturity_month: c_int,
    maturity_day: c_int,
    issue_year: c_int,
    issue_month: c_int,
    issue_day: c_int,
) -> Handle {
    // Parse maturity date
    let maturity = match Date::from_ymd(maturity_year, maturity_month as u32, maturity_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid maturity date: {}", e));
            return INVALID_HANDLE;
        }
    };

    // Parse issue date
    let issue = match Date::from_ymd(issue_year, issue_month as u32, issue_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid issue date: {}", e));
            return INVALID_HANDLE;
        }
    };

    // Parse identifier or generate a default one
    let (builder, bond_name) = if !isin.is_null() {
        match CStr::from_ptr(isin).to_str() {
            Ok(s) if !s.is_empty() => {
                let b = FixedRateBond::builder().cusip_unchecked(s);
                (b, Some(s.to_string()))
            }
            _ => {
                let synthetic_id = format!(
                    "TSY_{:08}",
                    maturity_year * 10000 + maturity_month * 100 + maturity_day
                );
                let b = FixedRateBond::builder().cusip_unchecked(&synthetic_id);
                (b, None)
            }
        }
    } else {
        let synthetic_id = format!(
            "TSY_{:08}",
            maturity_year * 10000 + maturity_month * 100 + maturity_day
        );
        let b = FixedRateBond::builder().cusip_unchecked(&synthetic_id);
        (b, None)
    };

    let builder = builder
        .coupon_percent(coupon_percent)
        .maturity(maturity)
        .issue_date(issue)
        .us_treasury();

    let bond = match builder.build() {
        Ok(b) => b,
        Err(e) => {
            set_last_error(format!("Failed to build bond: {}", e));
            return INVALID_HANDLE;
        }
    };

    registry::register(bond, ObjectType::FixedBond, bond_name)
}

// ============================================================================
// Bond Queries
// ============================================================================

/// Gets the accrued interest for a bond at a settlement date.
///
/// # Returns
///
/// Accrued interest per 100 face value, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_accrued(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
) -> c_double {
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return f64::NAN;
        }
    };

    registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        b.accrued_interest(settlement).to_f64().unwrap_or(f64::NAN)
    })
    .unwrap_or_else(|| {
        set_last_error("Invalid bond handle");
        f64::NAN
    })
}

/// Gets the number of remaining cash flows for a bond.
///
/// # Returns
///
/// Number of cash flows, or -1 on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_cashflow_count(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
) -> c_int {
    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(_) => return -1,
    };

    registry::with_object::<FixedRateBond, _, _>(bond, |b| b.cash_flows(settlement).len() as c_int)
        .unwrap_or(-1)
}

/// Gets a specific cash flow for a bond.
///
/// # Safety
///
/// - `date_out` must be a valid pointer to write the date (Excel serial number).
/// - `amount_out` must be a valid pointer to write the amount.
///
/// # Returns
///
/// CONVEX_OK on success, error code on failure.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_cashflow_get(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    index: c_int,
    date_out: *mut c_int,
    amount_out: *mut c_double,
) -> c_int {
    if date_out.is_null() || amount_out.is_null() {
        set_last_error("Null pointer for output");
        return CONVEX_ERROR_NULL_PTR;
    }

    let settlement = match Date::from_ymd(settle_year, settle_month as u32, settle_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid settlement date: {}", e));
            return CONVEX_ERROR_INVALID_ARG;
        }
    };

    let result = registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        let flows = b.cash_flows(settlement);
        if index < 0 || index as usize >= flows.len() {
            return Err("Index out of bounds");
        }
        let cf = &flows[index as usize];
        Ok((date_to_int(cf.date), cf.amount.to_f64().unwrap_or(0.0)))
    });

    match result {
        Some(Ok((date, amount))) => {
            *date_out = date;
            *amount_out = amount;
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

/// Gets the maturity date of a bond.
///
/// # Returns
///
/// Maturity date as integer in YYYYMMDD format, or 0 on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_maturity(bond: Handle) -> c_int {
    registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        b.maturity().map(date_to_int).unwrap_or(0)
    })
    .unwrap_or(0)
}

/// Gets the coupon rate of a bond.
///
/// # Returns
///
/// Coupon rate as decimal (e.g., 0.05 for 5%), or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_coupon_rate(bond: Handle) -> c_double {
    registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        b.coupon_rate_decimal().to_f64().unwrap_or(f64::NAN)
    })
    .unwrap_or(f64::NAN)
}

/// Gets the face value of a bond.
///
/// # Returns
///
/// Face value, or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_face_value(bond: Handle) -> c_double {
    registry::with_object::<FixedRateBond, _, _>(bond, |b| b.face_value().to_f64().unwrap_or(100.0))
        .unwrap_or(f64::NAN)
}

/// Gets the payment frequency of a bond.
///
/// # Returns
///
/// Periods per year (1=Annual, 2=Semi, 4=Quarterly, 12=Monthly), or 0 on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_frequency(bond: Handle) -> c_int {
    registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        b.frequency().periods_per_year() as c_int
    })
    .unwrap_or(0)
}

/// Gets the day count convention of a bond as a string.
///
/// # Safety
///
/// Returns a pointer to a static string. Do not free.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_day_count_name(bond: Handle) -> *const c_char {
    static UNKNOWN: &[u8] = b"UNKNOWN\0";

    registry::with_object::<FixedRateBond, _, _>(bond, |b| {
        let name = b.day_count_convention();
        // We need to return a static string, so match on known conventions
        match name {
            "ACT/360" => b"ACT/360\0".as_ptr() as *const c_char,
            "ACT/365F" => b"ACT/365F\0".as_ptr() as *const c_char,
            "ACT/ACT ISDA" => b"ACT/ACT ISDA\0".as_ptr() as *const c_char,
            "ACT/ACT ICMA" => b"ACT/ACT ICMA\0".as_ptr() as *const c_char,
            "30/360 US" => b"30/360 US\0".as_ptr() as *const c_char,
            "30E/360" => b"30E/360\0".as_ptr() as *const c_char,
            _ => UNKNOWN.as_ptr() as *const c_char,
        }
    })
    .unwrap_or(UNKNOWN.as_ptr() as *const c_char)
}

// ============================================================================
// Callable Bond Functions
// ============================================================================

/// Creates a callable bond with a single call date.
///
/// This is a simplified callable bond constructor for the common case
/// of a bond with one call date and one call price.
///
/// # Arguments
///
/// * `isin` - Bond identifier (can be NULL for unnamed bond)
/// * `coupon_percent` - Annual coupon rate as percentage (e.g., 5.0 for 5%)
/// * `frequency` - Coupon frequency (1=Annual, 2=Semi, 4=Quarterly)
/// * `maturity_year/month/day` - Maturity date
/// * `issue_year/month/day` - Issue date
/// * `call_year/month/day` - First call date
/// * `call_price` - Call price as percentage of par (e.g., 102.0)
/// * `day_count` - Day count convention (0-5)
///
/// # Returns
///
/// Handle to the created callable bond, or INVALID_HANDLE on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_callable(
    isin: *const c_char,
    coupon_percent: c_double,
    frequency: c_int,
    maturity_year: c_int,
    maturity_month: c_int,
    maturity_day: c_int,
    issue_year: c_int,
    issue_month: c_int,
    issue_day: c_int,
    call_year: c_int,
    call_month: c_int,
    call_day: c_int,
    call_price: c_double,
    day_count: c_int,
) -> Handle {
    // Validate dates
    let maturity = match Date::from_ymd(maturity_year, maturity_month as u32, maturity_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid maturity date: {}", e));
            return INVALID_HANDLE;
        }
    };

    let issue = match Date::from_ymd(issue_year, issue_month as u32, issue_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid issue date: {}", e));
            return INVALID_HANDLE;
        }
    };

    let call_date = match Date::from_ymd(call_year, call_month as u32, call_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid call date: {}", e));
            return INVALID_HANDLE;
        }
    };

    // Validate parameters
    if !(0.0..=100.0).contains(&coupon_percent) {
        set_last_error("Coupon percent must be between 0 and 100");
        return INVALID_HANDLE;
    }

    if call_price <= 0.0 {
        set_last_error("Call price must be positive");
        return INVALID_HANDLE;
    }

    // Get identifier
    let name = if isin.is_null() {
        None
    } else {
        CStr::from_ptr(isin).to_str().ok().map(String::from)
    };

    let freq = frequency_from_ffi(frequency);
    let dc = daycount_from_ffi(day_count);

    // Build the base bond
    let mut builder = FixedRateBond::builder()
        .coupon_percent(coupon_percent)
        .frequency(freq)
        .maturity(maturity)
        .issue_date(issue)
        .day_count(dc)
        .currency(Currency::USD)
        .face_value(Decimal::ONE_HUNDRED);

    if let Some(ref id) = name {
        builder = builder.cusip_unchecked(id);
    }

    let base_bond = match builder.build() {
        Ok(b) => b,
        Err(e) => {
            set_last_error(format!("Failed to build base bond: {}", e));
            return INVALID_HANDLE;
        }
    };

    // Create call schedule with a single entry
    let call_schedule =
        CallSchedule::new(CallType::American).with_entry(CallEntry::new(call_date, call_price));

    // Create callable bond
    let callable = CallableBond::new(base_bond, call_schedule);

    // Register the callable bond
    registry::register(callable, ObjectType::CallableBond, name)
}

/// Creates a callable bond with multiple call dates.
///
/// # Arguments
///
/// * `isin` - Bond identifier (can be NULL)
/// * `coupon_percent` - Annual coupon rate as percentage
/// * `frequency` - Coupon frequency (1=Annual, 2=Semi, 4=Quarterly)
/// * `maturity_year/month/day` - Maturity date
/// * `issue_year/month/day` - Issue date
/// * `call_dates` - Array of call dates as YYYYMMDD integers
/// * `call_prices` - Array of call prices (percentage of par)
/// * `call_count` - Number of call entries
/// * `day_count` - Day count convention
///
/// # Returns
///
/// Handle to the callable bond, or INVALID_HANDLE on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_callable_schedule(
    isin: *const c_char,
    coupon_percent: c_double,
    frequency: c_int,
    maturity_year: c_int,
    maturity_month: c_int,
    maturity_day: c_int,
    issue_year: c_int,
    issue_month: c_int,
    issue_day: c_int,
    call_dates: *const c_int,
    call_prices: *const c_double,
    call_count: c_int,
    day_count: c_int,
) -> Handle {
    if call_count <= 0 {
        set_last_error("Call count must be positive");
        return INVALID_HANDLE;
    }

    if call_dates.is_null() || call_prices.is_null() {
        set_last_error("Call dates and prices arrays cannot be null");
        return INVALID_HANDLE;
    }

    // Parse call entries
    let dates_slice = std::slice::from_raw_parts(call_dates, call_count as usize);
    let prices_slice = std::slice::from_raw_parts(call_prices, call_count as usize);

    let mut entries = Vec::with_capacity(call_count as usize);
    for i in 0..call_count as usize {
        let date_int = dates_slice[i];
        let year = date_int / 10000;
        let month = (date_int / 100) % 100;
        let day = date_int % 100;

        let call_date = match Date::from_ymd(year, month as u32, day as u32) {
            Ok(d) => d,
            Err(e) => {
                set_last_error(format!("Invalid call date at index {}: {}", i, e));
                return INVALID_HANDLE;
            }
        };

        entries.push(CallEntry::new(call_date, prices_slice[i]));
    }

    // Validate dates
    let maturity = match Date::from_ymd(maturity_year, maturity_month as u32, maturity_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid maturity date: {}", e));
            return INVALID_HANDLE;
        }
    };

    let issue = match Date::from_ymd(issue_year, issue_month as u32, issue_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid issue date: {}", e));
            return INVALID_HANDLE;
        }
    };

    // Get identifier
    let name = if isin.is_null() {
        None
    } else {
        CStr::from_ptr(isin).to_str().ok().map(String::from)
    };

    let freq = frequency_from_ffi(frequency);
    let dc = daycount_from_ffi(day_count);

    // Build the base bond
    let mut builder = FixedRateBond::builder()
        .coupon_percent(coupon_percent)
        .frequency(freq)
        .maturity(maturity)
        .issue_date(issue)
        .day_count(dc)
        .currency(Currency::USD)
        .face_value(Decimal::ONE_HUNDRED);

    if let Some(ref id) = name {
        builder = builder.cusip_unchecked(id);
    }

    let base_bond = match builder.build() {
        Ok(b) => b,
        Err(e) => {
            set_last_error(format!("Failed to build base bond: {}", e));
            return INVALID_HANDLE;
        }
    };

    // Create call schedule
    let mut call_schedule = CallSchedule::new(CallType::American);
    for entry in entries {
        call_schedule = call_schedule.with_entry(entry);
    }

    // Create callable bond
    let callable = CallableBond::new(base_bond, call_schedule);

    // Register the callable bond
    registry::register(callable, ObjectType::CallableBond, name)
}

/// Calculates yield to first call for a callable bond.
///
/// # Arguments
///
/// * `bond` - Callable bond handle
/// * `settle_year/month/day` - Settlement date
/// * `clean_price` - Clean price per 100 face value
///
/// # Returns
///
/// Yield to first call as decimal (e.g., 0.05 for 5%), or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_yield_to_call(
    bond: Handle,
    settle_year: c_int,
    settle_month: c_int,
    settle_day: c_int,
    clean_price: c_double,
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

    let price_decimal = Decimal::try_from(clean_price).unwrap_or(Decimal::ZERO);

    registry::with_object::<CallableBond, _, _>(bond, |b| {
        match b.yield_to_first_call(price_decimal, settlement) {
            Ok(ytc) => ytc.to_f64().unwrap_or(f64::NAN),
            Err(e) => {
                set_last_error(format!("Yield to call calculation failed: {}", e));
                f64::NAN
            }
        }
    })
    .unwrap_or_else(|| {
        set_last_error("Invalid callable bond handle");
        f64::NAN
    })
}

/// Gets the first call date of a callable bond.
///
/// # Returns
///
/// Call date as YYYYMMDD integer, or 0 on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_first_call_date(bond: Handle) -> c_int {
    registry::with_object::<CallableBond, _, _>(bond, |b| {
        b.first_call_date().map(date_to_int).unwrap_or(0)
    })
    .unwrap_or(0)
}

/// Gets the first call price of a callable bond.
///
/// # Returns
///
/// Call price as percentage of par (e.g., 102.0), or NaN on error.
#[no_mangle]
pub unsafe extern "C" fn convex_bond_first_call_price(bond: Handle) -> c_double {
    registry::with_object::<CallableBond, _, _>(bond, |b| {
        b.first_call_date()
            .and_then(|d| b.call_price_on(d))
            .unwrap_or(f64::NAN)
    })
    .unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::ptr;

    #[test]
    fn test_create_fixed_bond() {
        unsafe {
            let cusip = CString::new("097023AH7").unwrap();

            let handle = convex_bond_fixed(
                cusip.as_ptr(),
                0.075, // 7.5%
                2025,
                6,
                15, // Maturity
                2005,
                5,
                31, // Issue
                2,  // Semi-annual
                4,  // 30/360 US
                0,  // USD
                100.0,
            );

            assert_ne!(handle, INVALID_HANDLE);

            // Check bond properties
            let coupon = convex_bond_coupon_rate(handle);
            assert!((coupon - 0.075).abs() < 0.001);

            let face = convex_bond_face_value(handle);
            assert!((face - 100.0).abs() < 0.001);

            let freq = convex_bond_frequency(handle);
            assert_eq!(freq, 2);

            // Calculate accrued interest
            let accrued = convex_bond_accrued(handle, 2020, 4, 29);
            assert!(!accrued.is_nan());
            // Boeing bond: 134 days accrued at 7.5% semi-annual on $100
            // Accrued = 100 * 0.075 * (134/360) = $2.79
            assert!(accrued > 2.0 && accrued < 3.5, "Accrued: {}", accrued);

            registry::release(handle);
        }
    }

    #[test]
    fn test_create_us_corporate() {
        unsafe {
            let handle = convex_bond_us_corporate(
                ptr::null(),
                5.0, // 5%
                2030,
                1,
                15,
                2020,
                1,
                15,
            );

            assert_ne!(handle, INVALID_HANDLE);

            let coupon = convex_bond_coupon_rate(handle);
            assert!((coupon - 0.05).abs() < 0.001);

            registry::release(handle);
        }
    }

    #[test]
    fn test_bond_cash_flows() {
        unsafe {
            let handle = convex_bond_us_corporate(ptr::null(), 5.0, 2025, 1, 15, 2020, 1, 15);

            // Get cash flow count as of 2023-01-15
            let count = convex_bond_cashflow_count(handle, 2023, 1, 15);
            assert!(count > 0, "Should have remaining cash flows");

            // Get first cash flow
            let mut date: c_int = 0;
            let mut amount: c_double = 0.0;
            let result = convex_bond_cashflow_get(handle, 2023, 1, 15, 0, &mut date, &mut amount);

            assert_eq!(result, CONVEX_OK);
            assert!(date > 0);
            assert!(amount > 0.0);

            registry::release(handle);
        }
    }

    #[test]
    fn test_invalid_bond_handle() {
        unsafe {
            let accrued = convex_bond_accrued(999999, 2020, 1, 15);
            assert!(accrued.is_nan());

            let coupon = convex_bond_coupon_rate(999999);
            assert!(coupon.is_nan());
        }
    }
}
