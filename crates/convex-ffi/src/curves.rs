//! FFI functions for yield curve operations.

use std::ffi::CStr;
use std::slice;

use libc::{c_char, c_double, c_int};

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Date, Frequency};
use convex_curves::calibration::{
    Deposit, GlobalFitter, InstrumentSet, Ois, PiecewiseBootstrapper, Swap,
};
use convex_curves::{DiscreteCurve, InterpolationMethod, RateCurve, ValueType};

use crate::error::set_last_error;
use crate::registry::{self, Handle, ObjectType, INVALID_HANDLE};

pub type StoredCurve = RateCurve<DiscreteCurve>;

fn interp_from_ffi(method: c_int) -> InterpolationMethod {
    match method {
        0 => InterpolationMethod::Linear,
        1 => InterpolationMethod::LogLinear,
        2 => InterpolationMethod::CubicSpline,
        _ => InterpolationMethod::MonotoneConvex,
    }
}

fn daycount_from_ffi(dc: c_int) -> DayCountConvention {
    match dc {
        0 => DayCountConvention::Act360,
        1 => DayCountConvention::Act365Fixed,
        2 => DayCountConvention::ActActIsda,
        3 => DayCountConvention::Thirty360US,
        4 => DayCountConvention::Thirty360E,
        _ => DayCountConvention::Act365Fixed,
    }
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_from_zero_rates(
    name: *const c_char,
    ref_year: c_int, ref_month: c_int, ref_day: c_int,
    tenors: *const c_double, rates: *const c_double, count: c_int,
    interpolation: c_int, day_count: c_int,
) -> Handle {
    if tenors.is_null() || rates.is_null() {
        set_last_error("Null pointer");
        return INVALID_HANDLE;
    }
    if count <= 0 {
        set_last_error("Count must be positive");
        return INVALID_HANDLE;
    }
    let ref_date = match Date::from_ymd(ref_year, ref_month as u32, ref_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid date: {}", e));
            return INVALID_HANDLE;
        }
    };
    let tenor_vec: Vec<f64> = slice::from_raw_parts(tenors, count as usize).to_vec();
    let rate_vec: Vec<f64> = slice::from_raw_parts(rates, count as usize).to_vec();
    let dc = daycount_from_ffi(day_count);
    let value_type = ValueType::ZeroRate {
        compounding: Compounding::Continuous,
        day_count: dc,
    };
    let discrete = match DiscreteCurve::new(
        ref_date, tenor_vec, rate_vec, value_type, interp_from_ffi(interpolation)
    ) {
        Ok(c) => c,
        Err(e) => {
            set_last_error(format!("Failed: {}", e));
            return INVALID_HANDLE;
        }
    };
    let curve = RateCurve::new(discrete);
    let curve_name = if !name.is_null() {
        CStr::from_ptr(name).to_str().ok().map(|s| s.to_string())
    } else {
        None
    };
    registry::register(curve, ObjectType::Curve, curve_name)
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_from_dfs(
    name: *const c_char,
    ref_year: c_int, ref_month: c_int, ref_day: c_int,
    tenors: *const c_double, dfs: *const c_double, count: c_int,
    interpolation: c_int, _day_count: c_int,
) -> Handle {
    if tenors.is_null() || dfs.is_null() {
        set_last_error("Null pointer");
        return INVALID_HANDLE;
    }
    if count <= 0 {
        set_last_error("Count must be positive");
        return INVALID_HANDLE;
    }
    let ref_date = match Date::from_ymd(ref_year, ref_month as u32, ref_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid date: {}", e));
            return INVALID_HANDLE;
        }
    };
    let tenor_vec: Vec<f64> = slice::from_raw_parts(tenors, count as usize).to_vec();
    let df_vec: Vec<f64> = slice::from_raw_parts(dfs, count as usize).to_vec();
    let discrete = match DiscreteCurve::new(
        ref_date, tenor_vec, df_vec, ValueType::DiscountFactor, interp_from_ffi(interpolation)
    ) {
        Ok(c) => c,
        Err(e) => {
            set_last_error(format!("Failed: {}", e));
            return INVALID_HANDLE;
        }
    };
    let curve = RateCurve::new(discrete);
    let curve_name = if !name.is_null() {
        CStr::from_ptr(name).to_str().ok().map(|s| s.to_string())
    } else {
        None
    };
    registry::register(curve, ObjectType::Curve, curve_name)
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_zero_rate(handle: Handle, tenor: c_double) -> c_double {
    registry::with_object::<StoredCurve, _, _>(handle, |curve| {
        curve.zero_rate_at_tenor(tenor, Compounding::Continuous).unwrap_or(f64::NAN)
    }).unwrap_or(f64::NAN)
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_df(handle: Handle, tenor: c_double) -> c_double {
    registry::with_object::<StoredCurve, _, _>(handle, |curve| {
        curve.discount_factor_at_tenor(tenor).unwrap_or(f64::NAN)
    }).unwrap_or(f64::NAN)
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_forward_rate(
    handle: Handle, start_tenor: c_double, end_tenor: c_double
) -> c_double {
    registry::with_object::<StoredCurve, _, _>(handle, |curve| {
        curve.forward_rate_at_tenors(start_tenor, end_tenor, Compounding::Continuous)
            .unwrap_or(f64::NAN)
    }).unwrap_or(f64::NAN)
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_ref_date(handle: Handle) -> c_int {
    registry::with_object::<StoredCurve, _, _>(handle, |curve| {
        let d = curve.reference_date();
        d.year() * 10000 + d.month() as i32 * 100 + d.day() as i32
    }).unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_tenor_count(handle: Handle) -> c_int {
    registry::with_object::<StoredCurve, _, _>(handle, |curve| {
        curve.inner().tenors().len() as c_int
    }).unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_get_tenor(handle: Handle, index: c_int) -> c_double {
    registry::with_object::<StoredCurve, _, _>(handle, |curve| {
        curve.inner().tenors().get(index as usize).copied().unwrap_or(f64::NAN)
    }).unwrap_or(f64::NAN)
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_get_rate(handle: Handle, index: c_int) -> c_double {
    registry::with_object::<StoredCurve, _, _>(handle, |curve| {
        curve.inner().values().get(index as usize).copied().unwrap_or(f64::NAN)
    }).unwrap_or(f64::NAN)
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_max_tenor(handle: Handle) -> c_double {
    registry::with_object::<StoredCurve, _, _>(handle, |curve| {
        curve.inner().tenors().last().copied().unwrap_or(f64::NAN)
    }).unwrap_or(f64::NAN)
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_shift(
    handle: Handle, basis_points: c_double, new_name: *const c_char
) -> Handle {
    let shifted = registry::with_object::<StoredCurve, _, _>(handle, |curve| {
        let shift = basis_points / 10000.0;
        let tenors = curve.inner().tenors().to_vec();
        let new_values: Vec<f64> = curve.inner().values().iter().map(|v| v + shift).collect();
        let value_type = ValueType::ZeroRate {
            compounding: Compounding::Continuous,
            day_count: DayCountConvention::Act365Fixed,
        };
        DiscreteCurve::new(
            curve.reference_date(), tenors, new_values, value_type, InterpolationMethod::Linear
        ).ok().map(RateCurve::new)
    });
    match shifted {
        Some(Some(curve)) => {
            let n = if !new_name.is_null() {
                CStr::from_ptr(new_name).to_str().ok().map(|s| s.to_string())
            } else { None };
            registry::register(curve, ObjectType::Curve, n)
        }
        _ => {
            set_last_error("Failed to shift");
            INVALID_HANDLE
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_twist(
    handle: Handle, short_bp: c_double, long_bp: c_double,
    pivot_tenor: c_double, new_name: *const c_char
) -> Handle {
    let twisted = registry::with_object::<StoredCurve, _, _>(handle, |curve| {
        let short_shift = short_bp / 10000.0;
        let long_shift = long_bp / 10000.0;
        let tenors = curve.inner().tenors().to_vec();
        let new_values: Vec<f64> = curve.inner().tenors().iter()
            .zip(curve.inner().values().iter())
            .map(|(t, v)| {
                let w = (*t / pivot_tenor).min(1.0);
                v + short_shift * (1.0 - w) + long_shift * w
            }).collect();
        let value_type = ValueType::ZeroRate {
            compounding: Compounding::Continuous,
            day_count: DayCountConvention::Act365Fixed,
        };
        DiscreteCurve::new(
            curve.reference_date(), tenors, new_values, value_type, InterpolationMethod::Linear
        ).ok().map(RateCurve::new)
    });
    match twisted {
        Some(Some(curve)) => {
            let n = if !new_name.is_null() {
                CStr::from_ptr(new_name).to_str().ok().map(|s| s.to_string())
            } else { None };
            registry::register(curve, ObjectType::Curve, n)
        }
        _ => {
            set_last_error("Failed to twist");
            INVALID_HANDLE
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn convex_curve_bump_tenor(
    handle: Handle, tenor: c_double, basis_points: c_double, new_name: *const c_char
) -> Handle {
    let bumped = registry::with_object::<StoredCurve, _, _>(handle, |curve| {
        let bump = basis_points / 10000.0;
        let tenors = curve.inner().tenors().to_vec();
        let new_values: Vec<f64> = curve.inner().tenors().iter()
            .zip(curve.inner().values().iter())
            .map(|(t, v)| if (*t - tenor).abs() < 0.01 { v + bump } else { *v })
            .collect();
        let value_type = ValueType::ZeroRate {
            compounding: Compounding::Continuous,
            day_count: DayCountConvention::Act365Fixed,
        };
        DiscreteCurve::new(
            curve.reference_date(), tenors, new_values, value_type, InterpolationMethod::Linear
        ).ok().map(RateCurve::new)
    });
    match bumped {
        Some(Some(curve)) => {
            let n = if !new_name.is_null() {
                CStr::from_ptr(new_name).to_str().ok().map(|s| s.to_string())
            } else { None };
            registry::register(curve, ObjectType::Curve, n)
        }
        _ => {
            set_last_error("Failed to bump");
            INVALID_HANDLE
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn convex_bootstrap_from_instruments(
    name: *const c_char, ref_year: c_int, ref_month: c_int, ref_day: c_int,
    deposit_tenors: *const c_double, deposit_rates: *const c_double, deposit_count: c_int,
    swap_tenors: *const c_double, swap_rates: *const c_double, swap_count: c_int,
    _interpolation: c_int, day_count: c_int,
) -> Handle {
    let ref_date = match Date::from_ymd(ref_year, ref_month as u32, ref_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid date: {}", e));
            return INVALID_HANDLE;
        }
    };
    let dc = daycount_from_ffi(day_count);
    let mut instruments = InstrumentSet::new();
    
    if deposit_count > 0 && !deposit_tenors.is_null() && !deposit_rates.is_null() {
        let t = slice::from_raw_parts(deposit_tenors, deposit_count as usize);
        let r = slice::from_raw_parts(deposit_rates, deposit_count as usize);
        for (tenor, rate) in t.iter().zip(r.iter()) {
            instruments.add(Deposit::from_tenor(ref_date, *tenor, *rate, dc));
        }
    }
    if swap_count > 0 && !swap_tenors.is_null() && !swap_rates.is_null() {
        let t = slice::from_raw_parts(swap_tenors, swap_count as usize);
        let r = slice::from_raw_parts(swap_rates, swap_count as usize);
        for (tenor, rate) in t.iter().zip(r.iter()) {
            instruments.add(Swap::from_tenor(
                ref_date, *tenor, *rate, Frequency::SemiAnnual, DayCountConvention::Thirty360US
            ));
        }
    }
    if instruments.is_empty() {
        set_last_error("No instruments");
        return INVALID_HANDLE;
    }
    let fitter = GlobalFitter::new();
    let result = match fitter.fit(ref_date, &instruments) {
        Ok(r) => r,
        Err(e) => {
            set_last_error(format!("Bootstrap failed: {}", e));
            return INVALID_HANDLE;
        }
    };
    let curve_name = if !name.is_null() {
        CStr::from_ptr(name).to_str().ok().map(|s| s.to_string())
    } else { None };
    let rate_curve = RateCurve::new(result.curve);
    registry::register(rate_curve, ObjectType::Curve, curve_name)
}

#[no_mangle]
pub unsafe extern "C" fn convex_bootstrap_ois(
    name: *const c_char, ref_year: c_int, ref_month: c_int, ref_day: c_int,
    tenors: *const c_double, rates: *const c_double, count: c_int,
    _interpolation: c_int, day_count: c_int,
) -> Handle {
    if tenors.is_null() || rates.is_null() || count <= 0 {
        set_last_error("Invalid OIS input");
        return INVALID_HANDLE;
    }
    let ref_date = match Date::from_ymd(ref_year, ref_month as u32, ref_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid date: {}", e));
            return INVALID_HANDLE;
        }
    };
    let dc = daycount_from_ffi(day_count);
    let t = slice::from_raw_parts(tenors, count as usize);
    let r = slice::from_raw_parts(rates, count as usize);
    let mut instruments = InstrumentSet::new();
    for (tenor, rate) in t.iter().zip(r.iter()) {
        instruments.add(Ois::from_tenor(ref_date, *tenor, *rate, dc));
    }
    let fitter = GlobalFitter::new();
    let result = match fitter.fit(ref_date, &instruments) {
        Ok(r) => r,
        Err(e) => {
            set_last_error(format!("OIS failed: {}", e));
            return INVALID_HANDLE;
        }
    };
    let curve_name = if !name.is_null() {
        CStr::from_ptr(name).to_str().ok().map(|s| s.to_string())
    } else { None };
    let rate_curve = RateCurve::new(result.curve);
    registry::register(rate_curve, ObjectType::Curve, curve_name)
}

#[no_mangle]
pub unsafe extern "C" fn convex_bootstrap_mixed(
    name: *const c_char, ref_year: c_int, ref_month: c_int, ref_day: c_int,
    instrument_types: *const c_int, tenors: *const c_double, rates: *const c_double, count: c_int,
    _interpolation: c_int, day_count: c_int,
) -> Handle {
    if instrument_types.is_null() || tenors.is_null() || rates.is_null() || count <= 0 {
        set_last_error("Invalid input");
        return INVALID_HANDLE;
    }
    let ref_date = match Date::from_ymd(ref_year, ref_month as u32, ref_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid date: {}", e));
            return INVALID_HANDLE;
        }
    };
    let dc = daycount_from_ffi(day_count);
    let types = slice::from_raw_parts(instrument_types, count as usize);
    let t = slice::from_raw_parts(tenors, count as usize);
    let r = slice::from_raw_parts(rates, count as usize);
    let mut instruments = InstrumentSet::new();
    for i in 0..count as usize {
        match types[i] {
            0 => instruments.add(Deposit::from_tenor(ref_date, t[i], r[i], dc)),
            2 => instruments.add(Swap::from_tenor(
                ref_date, t[i], r[i], Frequency::SemiAnnual, DayCountConvention::Thirty360US
            )),
            3 => instruments.add(Ois::from_tenor(ref_date, t[i], r[i], dc)),
            _ => instruments.add(Deposit::from_tenor(ref_date, t[i], r[i], dc)),
        }
    }
    let fitter = GlobalFitter::new();
    let result = match fitter.fit(ref_date, &instruments) {
        Ok(r) => r,
        Err(e) => {
            set_last_error(format!("Mixed failed: {}", e));
            return INVALID_HANDLE;
        }
    };
    let curve_name = if !name.is_null() {
        CStr::from_ptr(name).to_str().ok().map(|s| s.to_string())
    } else { None };
    let rate_curve = RateCurve::new(result.curve);
    registry::register(rate_curve, ObjectType::Curve, curve_name)
}

#[no_mangle]
pub unsafe extern "C" fn convex_bootstrap_piecewise(
    name: *const c_char, ref_year: c_int, ref_month: c_int, ref_day: c_int,
    deposit_tenors: *const c_double, deposit_rates: *const c_double, deposit_count: c_int,
    swap_tenors: *const c_double, swap_rates: *const c_double, swap_count: c_int,
    _interpolation: c_int, day_count: c_int,
) -> Handle {
    let ref_date = match Date::from_ymd(ref_year, ref_month as u32, ref_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid date: {}", e));
            return INVALID_HANDLE;
        }
    };
    let dc = daycount_from_ffi(day_count);
    let mut instruments = InstrumentSet::new();
    
    if deposit_count > 0 && !deposit_tenors.is_null() && !deposit_rates.is_null() {
        let t = slice::from_raw_parts(deposit_tenors, deposit_count as usize);
        let r = slice::from_raw_parts(deposit_rates, deposit_count as usize);
        for (tenor, rate) in t.iter().zip(r.iter()) {
            instruments.add(Deposit::from_tenor(ref_date, *tenor, *rate, dc));
        }
    }
    if swap_count > 0 && !swap_tenors.is_null() && !swap_rates.is_null() {
        let t = slice::from_raw_parts(swap_tenors, swap_count as usize);
        let r = slice::from_raw_parts(swap_rates, swap_count as usize);
        for (tenor, rate) in t.iter().zip(r.iter()) {
            instruments.add(Swap::from_tenor(
                ref_date, *tenor, *rate, Frequency::SemiAnnual, DayCountConvention::Thirty360US
            ));
        }
    }
    if instruments.is_empty() {
        set_last_error("No instruments");
        return INVALID_HANDLE;
    }
    let bootstrapper = PiecewiseBootstrapper::new();
    let result = match bootstrapper.bootstrap(ref_date, &instruments) {
        Ok(r) => r,
        Err(e) => {
            set_last_error(format!("Piecewise failed: {}", e));
            return INVALID_HANDLE;
        }
    };
    let curve_name = if !name.is_null() {
        CStr::from_ptr(name).to_str().ok().map(|s| s.to_string())
    } else { None };
    let rate_curve = RateCurve::new(result.curve);
    registry::register(rate_curve, ObjectType::Curve, curve_name)
}

#[no_mangle]
pub unsafe extern "C" fn convex_bootstrap_piecewise_mixed(
    name: *const c_char, ref_year: c_int, ref_month: c_int, ref_day: c_int,
    instrument_types: *const c_int, tenors: *const c_double, rates: *const c_double, count: c_int,
    _interpolation: c_int, day_count: c_int,
) -> Handle {
    if instrument_types.is_null() || tenors.is_null() || rates.is_null() || count <= 0 {
        set_last_error("Invalid input");
        return INVALID_HANDLE;
    }
    let ref_date = match Date::from_ymd(ref_year, ref_month as u32, ref_day as u32) {
        Ok(d) => d,
        Err(e) => {
            set_last_error(format!("Invalid date: {}", e));
            return INVALID_HANDLE;
        }
    };
    let dc = daycount_from_ffi(day_count);
    let types = slice::from_raw_parts(instrument_types, count as usize);
    let t = slice::from_raw_parts(tenors, count as usize);
    let r = slice::from_raw_parts(rates, count as usize);
    let mut instruments = InstrumentSet::new();
    for i in 0..count as usize {
        match types[i] {
            0 => instruments.add(Deposit::from_tenor(ref_date, t[i], r[i], dc)),
            2 => instruments.add(Swap::from_tenor(
                ref_date, t[i], r[i], Frequency::SemiAnnual, DayCountConvention::Thirty360US
            )),
            3 => instruments.add(Ois::from_tenor(ref_date, t[i], r[i], dc)),
            _ => instruments.add(Deposit::from_tenor(ref_date, t[i], r[i], dc)),
        }
    }
    let bootstrapper = PiecewiseBootstrapper::new();
    let result = match bootstrapper.bootstrap(ref_date, &instruments) {
        Ok(r) => r,
        Err(e) => {
            set_last_error(format!("Piecewise mixed failed: {}", e));
            return INVALID_HANDLE;
        }
    };
    let curve_name = if !name.is_null() {
        CStr::from_ptr(name).to_str().ok().map(|s| s.to_string())
    } else { None };
    let rate_curve = RateCurve::new(result.curve);
    registry::register(rate_curve, ObjectType::Curve, curve_name)
}
