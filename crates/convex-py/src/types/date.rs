//! Date type wrapper for Python.

use convex_core::types::Date;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// A date type for financial calculations.
///
/// Can be created from year/month/day integers or parsed from ISO 8601 strings.
/// Also accepts Python `datetime.date` objects.
///
/// Examples:
///     >>> from convex import Date
///     >>> d = Date(2025, 6, 15)
///     >>> d.year
///     2025
///     >>> d = Date.parse("2025-06-15")
///     >>> str(d)
///     '2025-06-15'
#[pyclass(name = "Date")]
#[derive(Clone, Debug)]
pub struct PyDate(pub(crate) Date);

#[pymethods]
impl PyDate {
    /// Create a new date from year, month, and day.
    ///
    /// Args:
    ///     year: The year (e.g., 2025)
    ///     month: The month (1-12)
    ///     day: The day of month (1-31)
    ///
    /// Returns:
    ///     A new Date object
    ///
    /// Raises:
    ///     ValueError: If the date is invalid
    #[new]
    #[pyo3(signature = (year, month, day))]
    fn new(year: i32, month: u32, day: u32) -> PyResult<Self> {
        Date::from_ymd(year, month, day)
            .map(PyDate)
            .map_err(|e| PyValueError::new_err(format!("Invalid date: {}", e)))
    }

    /// Parse a date from an ISO 8601 string (YYYY-MM-DD).
    ///
    /// Args:
    ///     s: Date string in YYYY-MM-DD format
    ///
    /// Returns:
    ///     A new Date object
    ///
    /// Raises:
    ///     ValueError: If the string cannot be parsed
    #[staticmethod]
    fn parse(s: &str) -> PyResult<Self> {
        // Parse YYYY-MM-DD format
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 3 {
            return Err(PyValueError::new_err(format!(
                "Invalid date format '{}'. Expected YYYY-MM-DD",
                s
            )));
        }

        let year: i32 = parts[0]
            .parse()
            .map_err(|_| PyValueError::new_err(format!("Invalid year in '{}'", s)))?;
        let month: u32 = parts[1]
            .parse()
            .map_err(|_| PyValueError::new_err(format!("Invalid month in '{}'", s)))?;
        let day: u32 = parts[2]
            .parse()
            .map_err(|_| PyValueError::new_err(format!("Invalid day in '{}'", s)))?;

        Self::new(year, month, day)
    }

    /// The year component.
    #[getter]
    pub fn year(&self) -> i32 {
        self.0.year()
    }

    /// The month component (1-12).
    #[getter]
    pub fn month(&self) -> u32 {
        self.0.month()
    }

    /// The day component (1-31).
    #[getter]
    pub fn day(&self) -> u32 {
        self.0.day()
    }

    fn __repr__(&self) -> String {
        format!("Date({}, {}, {})", self.year(), self.month(), self.day())
    }

    fn __str__(&self) -> String {
        self.to_string_repr()
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.0 == other.0
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.year().hash(&mut hasher);
        self.month().hash(&mut hasher);
        self.day().hash(&mut hasher);
        hasher.finish()
    }
}

impl PyDate {
    /// Get string representation (callable from Rust code).
    pub fn to_string_repr(&self) -> String {
        format!("{}-{:02}-{:02}", self.year(), self.month(), self.day())
    }
}

impl From<Date> for PyDate {
    fn from(d: Date) -> Self {
        PyDate(d)
    }
}

impl From<PyDate> for Date {
    fn from(d: PyDate) -> Self {
        d.0
    }
}

/// Extract a date from various Python inputs.
///
/// Accepts:
/// - PyDate objects
/// - datetime.date objects
/// - Strings in YYYY-MM-DD format
pub fn extract_date(ob: &Bound<'_, PyAny>) -> PyResult<Date> {
    // Try our PyDate first
    if let Ok(d) = ob.extract::<PyDate>() {
        return Ok(d.0);
    }

    // Try Python datetime.date (using getattr for abi3 compatibility)
    if let Ok(year) = ob.getattr("year") {
        if let (Ok(y), Ok(m), Ok(d)) = (
            year.extract::<i32>(),
            ob.getattr("month").and_then(|m| m.extract::<u32>()),
            ob.getattr("day").and_then(|d| d.extract::<u32>()),
        ) {
            return Date::from_ymd(y, m, d)
                .map_err(|e| PyValueError::new_err(format!("Invalid date: {}", e)));
        }
    }

    // Try string
    if let Ok(s) = ob.extract::<String>() {
        let py_date = PyDate::parse(&s)?;
        return Ok(py_date.0);
    }

    Err(PyValueError::new_err(
        "Expected Date, datetime.date, or string in YYYY-MM-DD format",
    ))
}
