//! Error handling and Python exception mapping.

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::PyErr;

/// Convert a Convex error to a Python exception.
pub fn to_py_err<E: std::error::Error>(err: E) -> PyErr {
    let msg = err.to_string();

    // Map specific error patterns to appropriate Python exceptions
    if msg.contains("invalid") || msg.contains("Invalid") {
        PyValueError::new_err(msg)
    } else if msg.contains("matured") || msg.contains("expired") {
        PyValueError::new_err(msg)
    } else {
        PyRuntimeError::new_err(msg)
    }
}

/// Extension trait for converting Results to PyResult.
pub trait IntoPyResult<T> {
    fn into_py_result(self) -> pyo3::PyResult<T>;
}

impl<T, E: std::error::Error> IntoPyResult<T> for Result<T, E> {
    fn into_py_result(self) -> pyo3::PyResult<T> {
        self.map_err(to_py_err)
    }
}
