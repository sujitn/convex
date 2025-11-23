//! # Convex FFI
//!
//! C-compatible Foreign Function Interface for the Convex fixed income analytics library.
//!
//! This crate provides C-compatible bindings for use from other languages including:
//! - C/C++
//! - Python (via ctypes/cffi)
//! - Java (via JNI)
//! - C# (via P/Invoke)
//!
//! ## Safety
//!
//! All public functions in this crate are `unsafe` as they deal with raw pointers
//! and assume correct usage from the caller. The caller is responsible for:
//!
//! - Ensuring pointers are valid and non-null
//! - Properly freeing allocated memory using the provided free functions
//! - Not using objects after they have been freed
//!
//! ## Memory Management
//!
//! Objects created by this library must be freed using the corresponding
//! `convex_*_free` functions. Failure to do so will result in memory leaks.
//!
//! ## Error Handling
//!
//! Functions return error codes (0 = success, non-zero = error).
//! Error messages can be retrieved using `convex_last_error_message`.

#![allow(clippy::missing_safety_doc)]

use std::ffi::CStr;

use libc::{c_char, c_int};

use convex_core::types::Date;

mod error;

use error::set_last_error;

/// Result code for successful operations.
pub const CONVEX_OK: c_int = 0;

/// Result code for general errors.
pub const CONVEX_ERROR: c_int = -1;

/// Result code for invalid arguments.
pub const CONVEX_ERROR_INVALID_ARG: c_int = -2;

/// Result code for null pointer errors.
pub const CONVEX_ERROR_NULL_PTR: c_int = -3;

// ============================================================================
// Date Functions
// ============================================================================

/// Opaque handle to a Date object.
pub struct ConvexDate {
    inner: Date,
}

/// Creates a new date from year, month, day.
///
/// # Safety
///
/// The `out` pointer must be valid and writable.
#[no_mangle]
pub unsafe extern "C" fn convex_date_new(
    year: c_int,
    month: c_int,
    day: c_int,
    out: *mut *mut ConvexDate,
) -> c_int {
    if out.is_null() {
        set_last_error("Output pointer is null");
        return CONVEX_ERROR_NULL_PTR;
    }

    match Date::from_ymd(year, month as u32, day as u32) {
        Ok(date) => {
            let boxed = Box::new(ConvexDate { inner: date });
            *out = Box::into_raw(boxed);
            CONVEX_OK
        }
        Err(e) => {
            set_last_error(format!("Invalid date: {}", e));
            CONVEX_ERROR_INVALID_ARG
        }
    }
}

/// Parses a date from an ISO 8601 string (YYYY-MM-DD).
///
/// # Safety
///
/// - `date_str` must be a valid null-terminated C string.
/// - `out` pointer must be valid and writable.
#[no_mangle]
pub unsafe extern "C" fn convex_date_parse(
    date_str: *const c_char,
    out: *mut *mut ConvexDate,
) -> c_int {
    if date_str.is_null() || out.is_null() {
        set_last_error("Null pointer argument");
        return CONVEX_ERROR_NULL_PTR;
    }

    let c_str = CStr::from_ptr(date_str);
    let str_slice = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("Invalid UTF-8 string");
            return CONVEX_ERROR_INVALID_ARG;
        }
    };

    match Date::parse(str_slice) {
        Ok(date) => {
            let boxed = Box::new(ConvexDate { inner: date });
            *out = Box::into_raw(boxed);
            CONVEX_OK
        }
        Err(e) => {
            set_last_error(format!("Failed to parse date: {}", e));
            CONVEX_ERROR_INVALID_ARG
        }
    }
}

/// Gets the year component of a date.
///
/// # Safety
///
/// `date` must be a valid pointer created by `convex_date_new` or `convex_date_parse`.
#[no_mangle]
pub unsafe extern "C" fn convex_date_year(date: *const ConvexDate) -> c_int {
    if date.is_null() {
        return 0;
    }
    (*date).inner.year()
}

/// Gets the month component of a date (1-12).
///
/// # Safety
///
/// `date` must be a valid pointer created by `convex_date_new` or `convex_date_parse`.
#[no_mangle]
pub unsafe extern "C" fn convex_date_month(date: *const ConvexDate) -> c_int {
    if date.is_null() {
        return 0;
    }
    (*date).inner.month() as c_int
}

/// Gets the day component of a date (1-31).
///
/// # Safety
///
/// `date` must be a valid pointer created by `convex_date_new` or `convex_date_parse`.
#[no_mangle]
pub unsafe extern "C" fn convex_date_day(date: *const ConvexDate) -> c_int {
    if date.is_null() {
        return 0;
    }
    (*date).inner.day() as c_int
}

/// Frees a date object.
///
/// # Safety
///
/// `date` must be a valid pointer created by `convex_date_new` or `convex_date_parse`,
/// or null (in which case this is a no-op).
#[no_mangle]
pub unsafe extern "C" fn convex_date_free(date: *mut ConvexDate) {
    if !date.is_null() {
        drop(Box::from_raw(date));
    }
}

// ============================================================================
// Error Handling
// ============================================================================

/// Gets the last error message.
///
/// # Safety
///
/// The returned string is valid until the next call to any convex function.
/// The caller must not free the returned string.
#[no_mangle]
pub unsafe extern "C" fn convex_last_error_message() -> *const c_char {
    error::last_error_message()
}

/// Clears the last error message.
#[no_mangle]
pub extern "C" fn convex_clear_error() {
    error::clear_error();
}

// ============================================================================
// Version Information
// ============================================================================

/// Returns the library version string.
///
/// # Safety
///
/// The returned string is statically allocated and valid for the lifetime of the program.
#[no_mangle]
pub extern "C" fn convex_version() -> *const c_char {
    static VERSION: &[u8] = b"0.1.0\0";
    VERSION.as_ptr() as *const c_char
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn test_date_creation() {
        unsafe {
            let mut date_ptr: *mut ConvexDate = ptr::null_mut();
            let result = convex_date_new(2025, 6, 15, &mut date_ptr);

            assert_eq!(result, CONVEX_OK);
            assert!(!date_ptr.is_null());
            assert_eq!(convex_date_year(date_ptr), 2025);
            assert_eq!(convex_date_month(date_ptr), 6);
            assert_eq!(convex_date_day(date_ptr), 15);

            convex_date_free(date_ptr);
        }
    }

    #[test]
    fn test_invalid_date() {
        unsafe {
            let mut date_ptr: *mut ConvexDate = ptr::null_mut();
            let result = convex_date_new(2025, 2, 30, &mut date_ptr);

            assert_eq!(result, CONVEX_ERROR_INVALID_ARG);
            assert!(date_ptr.is_null());
        }
    }
}
