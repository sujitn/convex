//! Thread-local error handling for FFI.

use std::cell::RefCell;
use std::ffi::CString;

use libc::c_char;

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// Sets the last error message.
pub fn set_last_error(msg: impl Into<String>) {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = CString::new(msg.into()).ok();
    });
}

/// Gets the last error message as a C string pointer.
///
/// Returns a null pointer if no error has been set.
pub fn last_error_message() -> *const c_char {
    LAST_ERROR.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(std::ptr::null())
    })
}

/// Clears the last error.
pub fn clear_error() {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn test_error_set_and_get() {
        set_last_error("Test error message");

        let ptr = last_error_message();
        assert!(!ptr.is_null());

        unsafe {
            let msg = CStr::from_ptr(ptr).to_string_lossy();
            assert_eq!(msg, "Test error message");
        }
    }

    #[test]
    fn test_clear_error() {
        set_last_error("Error");
        clear_error();

        let ptr = last_error_message();
        assert!(ptr.is_null());
    }
}
