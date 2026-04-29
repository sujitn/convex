//! Thread-local last-error slot.
//!
//! Used by stateful constructors (which return `Handle`/`0`) so callers can
//! retrieve a diagnostic message via [`crate::convex_last_error`]. Stateless
//! analytics RPCs return errors inline in the JSON envelope and do not touch
//! this slot.

use std::cell::RefCell;
use std::ffi::CString;

use libc::c_char;

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

pub fn set_last_error(msg: impl Into<String>) {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = CString::new(msg.into()).ok();
    });
}

pub fn last_error_message() -> *const c_char {
    LAST_ERROR.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(std::ptr::null())
    })
}

pub fn clear_error() {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = None;
    });
}
