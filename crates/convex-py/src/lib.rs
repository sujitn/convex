//! Python bindings for the Convex fixed income analytics library.
//!
//! This crate provides PyO3-based bindings exposing Convex's bond pricing,
//! curve construction, and risk analytics to Python.

use pyo3::prelude::*;

mod bonds;
mod error;
mod types;

use bonds::{PyCashFlow, PyFixedRateBond};
use types::{PyCurrency, PyDayCount, PyDate, PyFrequency};

/// Convex fixed income analytics library.
///
/// A high-performance library for bond pricing, yield curve construction,
/// and risk analytics.
#[pymodule]
fn _convex(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Register types
    m.add_class::<PyDate>()?;
    m.add_class::<PyCurrency>()?;
    m.add_class::<PyFrequency>()?;
    m.add_class::<PyDayCount>()?;

    // Register bond types
    m.add_class::<PyFixedRateBond>()?;
    m.add_class::<PyCashFlow>()?;

    // Add version info
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;

    Ok(())
}
