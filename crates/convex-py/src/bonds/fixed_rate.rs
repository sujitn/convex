//! Fixed rate bond wrapper for Python.

use std::sync::Arc;

use convex_bonds::traits::Bond;
use convex_bonds::types::BondIdentifiers;
use convex_bonds::FixedRateBond;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use rust_decimal::Decimal;

use super::PyCashFlow;
use crate::error::IntoPyResult;
use crate::types::{extract_date, PyCurrency, PyDayCount, PyDate, PyFrequency};

/// A fixed rate bond.
///
/// Fixed rate bonds pay a fixed coupon rate at regular intervals until maturity,
/// when the principal is returned.
///
/// Examples:
///     >>> from datetime import date
///     >>> from convex import FixedRateBond, Frequency
///     >>>
///     >>> # Create a bond with explicit parameters
///     >>> bond = FixedRateBond(
///     ...     coupon=0.05,
///     ...     maturity=date(2030, 1, 15),
///     ...     issue_date=date(2020, 1, 15),
///     ...     frequency=Frequency.SEMI_ANNUAL,
///     ... )
///     >>>
///     >>> # Or use a convenience constructor
///     >>> bond = FixedRateBond.us_corporate(0.05, date(2030, 1, 15), date(2020, 1, 15))
///     >>>
///     >>> bond.coupon_rate
///     0.05
///     >>> bond.accrued_interest(date(2025, 6, 15))
///     2.0833...
#[pyclass(name = "FixedRateBond")]
#[derive(Clone)]
pub struct PyFixedRateBond {
    inner: Arc<FixedRateBond>,
}

#[pymethods]
impl PyFixedRateBond {
    /// Create a new fixed rate bond.
    ///
    /// Args:
    ///     coupon: Annual coupon rate as a decimal (e.g., 0.05 for 5%)
    ///     maturity: Maturity date
    ///     issue_date: Issue date
    ///     frequency: Payment frequency (default: SEMI_ANNUAL)
    ///     day_count: Day count convention (default: ACT_365_FIXED)
    ///     currency: Currency (default: USD)
    ///     face_value: Face value per bond (default: 100.0)
    ///     isin: ISIN identifier (optional)
    ///     cusip: CUSIP identifier (optional)
    ///
    /// Returns:
    ///     A new FixedRateBond object
    ///
    /// Raises:
    ///     ValueError: If parameters are invalid
    #[new]
    #[pyo3(signature = (
        coupon,
        maturity,
        issue_date,
        *,
        frequency = None,
        day_count = None,
        currency = None,
        face_value = None,
        isin = None,
        cusip = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        coupon: f64,
        maturity: &Bound<'_, PyAny>,
        issue_date: &Bound<'_, PyAny>,
        frequency: Option<PyFrequency>,
        day_count: Option<PyDayCount>,
        currency: Option<PyCurrency>,
        face_value: Option<f64>,
        isin: Option<&str>,
        cusip: Option<&str>,
    ) -> PyResult<Self> {
        let maturity_date = extract_date(maturity)?;
        let issue = extract_date(issue_date)?;

        let coupon_decimal = Decimal::from_f64_retain(coupon)
            .ok_or_else(|| PyValueError::new_err("Invalid coupon rate"))?;
        let face_decimal = Decimal::from_f64_retain(face_value.unwrap_or(100.0))
            .ok_or_else(|| PyValueError::new_err("Invalid face value"))?;

        let mut identifiers = BondIdentifiers::new();
        if let Some(id) = isin {
            identifiers = identifiers
                .with_isin_str(id)
                .map_err(|e| PyValueError::new_err(format!("Invalid ISIN: {}", e)))?;
        }
        if let Some(id) = cusip {
            identifiers = identifiers
                .with_cusip_str(id)
                .map_err(|e| PyValueError::new_err(format!("Invalid CUSIP: {}", e)))?;
        }

        let mut builder = FixedRateBond::builder()
            .identifiers(identifiers)
            .coupon_rate(coupon_decimal)
            .maturity(maturity_date)
            .issue_date(issue)
            .face_value(face_decimal);

        if let Some(freq) = frequency {
            builder = builder.frequency(freq.into());
        }
        if let Some(dc) = day_count {
            builder = builder.day_count(dc.into());
        }
        if let Some(ccy) = currency {
            builder = builder.currency(ccy.into());
        }

        let bond = builder.build().into_py_result()?;

        Ok(Self {
            inner: Arc::new(bond),
        })
    }

    /// Create a US corporate bond with standard conventions.
    ///
    /// US corporate bonds use:
    /// - 30/360 US day count
    /// - Semi-annual payments
    /// - T+2 settlement
    /// - USD currency
    ///
    /// Args:
    ///     coupon: Annual coupon rate as a decimal
    ///     maturity: Maturity date
    ///     issue_date: Issue date
    ///     isin: ISIN identifier (optional)
    ///
    /// Returns:
    ///     A new FixedRateBond with US corporate conventions
    #[staticmethod]
    #[pyo3(signature = (coupon, maturity, issue_date, isin = None))]
    fn us_corporate(
        coupon: f64,
        maturity: &Bound<'_, PyAny>,
        issue_date: &Bound<'_, PyAny>,
        isin: Option<&str>,
    ) -> PyResult<Self> {
        let maturity_date = extract_date(maturity)?;
        let issue = extract_date(issue_date)?;

        let coupon_decimal = Decimal::from_f64_retain(coupon)
            .ok_or_else(|| PyValueError::new_err("Invalid coupon rate"))?;

        let mut identifiers = BondIdentifiers::new();
        if let Some(id) = isin {
            identifiers = identifiers
                .with_isin_str(id)
                .map_err(|e| PyValueError::new_err(format!("Invalid ISIN: {}", e)))?;
        }

        let bond = FixedRateBond::builder()
            .identifiers(identifiers)
            .coupon_rate(coupon_decimal)
            .maturity(maturity_date)
            .issue_date(issue)
            .us_corporate()
            .build()
            .into_py_result()?;

        Ok(Self {
            inner: Arc::new(bond),
        })
    }

    /// Create a US Treasury bond with standard conventions.
    ///
    /// US Treasury bonds use:
    /// - ACT/ACT ICMA day count
    /// - Semi-annual payments
    /// - T+1 settlement
    /// - USD currency
    ///
    /// Args:
    ///     coupon: Annual coupon rate as a decimal
    ///     maturity: Maturity date
    ///     issue_date: Issue date
    ///     cusip: CUSIP identifier (optional)
    ///
    /// Returns:
    ///     A new FixedRateBond with US Treasury conventions
    #[staticmethod]
    #[pyo3(signature = (coupon, maturity, issue_date, cusip = None))]
    fn us_treasury(
        coupon: f64,
        maturity: &Bound<'_, PyAny>,
        issue_date: &Bound<'_, PyAny>,
        cusip: Option<&str>,
    ) -> PyResult<Self> {
        let maturity_date = extract_date(maturity)?;
        let issue = extract_date(issue_date)?;

        let coupon_decimal = Decimal::from_f64_retain(coupon)
            .ok_or_else(|| PyValueError::new_err("Invalid coupon rate"))?;

        let mut identifiers = BondIdentifiers::new();
        if let Some(id) = cusip {
            identifiers = identifiers
                .with_cusip_str(id)
                .map_err(|e| PyValueError::new_err(format!("Invalid CUSIP: {}", e)))?;
        }

        let bond = FixedRateBond::builder()
            .identifiers(identifiers)
            .coupon_rate(coupon_decimal)
            .maturity(maturity_date)
            .issue_date(issue)
            .us_treasury()
            .build()
            .into_py_result()?;

        Ok(Self {
            inner: Arc::new(bond),
        })
    }

    /// The annual coupon rate as a decimal (e.g., 0.05 for 5%).
    #[getter]
    fn coupon_rate(&self) -> f64 {
        self.inner.coupon_rate_decimal().try_into().unwrap_or(0.0)
    }

    /// The face value per bond.
    #[getter]
    fn face_value(&self) -> f64 {
        self.inner.face_value().try_into().unwrap_or(100.0)
    }

    /// The maturity date.
    #[getter]
    fn maturity(&self) -> Option<PyDate> {
        self.inner.maturity().map(PyDate::from)
    }

    /// The issue date.
    #[getter]
    fn issue_date(&self) -> PyDate {
        PyDate::from(self.inner.issue_date())
    }

    /// The payment frequency.
    #[getter]
    fn frequency(&self) -> PyFrequency {
        PyFrequency::from(self.inner.frequency())
    }

    /// The currency.
    #[getter]
    fn currency(&self) -> PyCurrency {
        PyCurrency::from(self.inner.currency())
    }

    /// Calculate accrued interest at a settlement date.
    ///
    /// Args:
    ///     settlement: The settlement date
    ///
    /// Returns:
    ///     Accrued interest per 100 face value
    fn accrued_interest(&self, settlement: &Bound<'_, PyAny>) -> PyResult<f64> {
        let settle_date = extract_date(settlement)?;
        let accrued = self.inner.accrued_interest(settle_date);
        Ok(accrued.try_into().unwrap_or(0.0))
    }

    /// Get all future cash flows from a given date.
    ///
    /// Args:
    ///     from_date: The date from which to list cash flows
    ///
    /// Returns:
    ///     A list of CashFlow objects
    fn cash_flows(&self, from_date: &Bound<'_, PyAny>) -> PyResult<Vec<PyCashFlow>> {
        let from = extract_date(from_date)?;
        let cfs = self.inner.cash_flows(from);
        Ok(cfs.into_iter().map(PyCashFlow::from).collect())
    }

    /// Get the next coupon date after a given date.
    ///
    /// Args:
    ///     after: The reference date
    ///
    /// Returns:
    ///     The next coupon date, or None if bond has matured
    fn next_coupon_date(&self, after: &Bound<'_, PyAny>) -> PyResult<Option<PyDate>> {
        let after_date = extract_date(after)?;
        Ok(self.inner.next_coupon_date(after_date).map(PyDate::from))
    }

    /// Check if the bond has matured as of a given date.
    ///
    /// Args:
    ///     as_of: The reference date
    ///
    /// Returns:
    ///     True if the bond has matured
    fn has_matured(&self, as_of: &Bound<'_, PyAny>) -> PyResult<bool> {
        let date = extract_date(as_of)?;
        Ok(self.inner.has_matured(date))
    }

    /// Years to maturity from a given date.
    ///
    /// Args:
    ///     from_date: The reference date
    ///
    /// Returns:
    ///     Years to maturity as a float, or None if bond has matured
    fn years_to_maturity(&self, from_date: &Bound<'_, PyAny>) -> PyResult<Option<f64>> {
        let from = extract_date(from_date)?;
        Ok(self.inner.years_to_maturity(from))
    }

    fn __repr__(&self) -> String {
        let coupon_pct = self.coupon_rate() * 100.0;
        let mat = self
            .maturity()
            .map(|d| d.to_string_repr())
            .unwrap_or_else(|| "N/A".to_string());
        format!("FixedRateBond(coupon={:.2}%, maturity={})", coupon_pct, mat)
    }

    fn __str__(&self) -> String {
        self.__repr__()
    }
}

impl PyFixedRateBond {
    /// Get a reference to the inner bond for use in analytics.
    pub fn inner(&self) -> &FixedRateBond {
        &self.inner
    }
}
