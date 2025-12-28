//! Cash flow wrapper for Python.

use convex_bonds::prelude::BondCashFlow;
use pyo3::prelude::*;

use crate::types::PyDate;

/// A single cash flow from a bond.
///
/// Attributes:
///     date: The payment date
///     amount: The cash flow amount
///     flow_type: Type of cash flow (Coupon, Principal, CouponAndPrincipal)
#[pyclass(name = "CashFlow")]
#[derive(Clone, Debug)]
pub struct PyCashFlow {
    date: PyDate,
    amount: f64,
    flow_type: String,
}

#[pymethods]
impl PyCashFlow {
    /// The payment date.
    #[getter]
    fn date(&self) -> PyDate {
        self.date.clone()
    }

    /// The cash flow amount.
    #[getter]
    fn amount(&self) -> f64 {
        self.amount
    }

    /// The type of cash flow (Coupon, Principal, CouponAndPrincipal).
    #[getter]
    fn flow_type(&self) -> &str {
        &self.flow_type
    }

    fn __repr__(&self) -> String {
        format!(
            "CashFlow(date={}, amount={:.4}, type={})",
            self.date.to_string_repr(),
            self.amount,
            self.flow_type
        )
    }

    fn __str__(&self) -> String {
        format!(
            "{}: ${:.2} ({})",
            self.date.to_string_repr(),
            self.amount,
            self.flow_type
        )
    }
}

impl From<BondCashFlow> for PyCashFlow {
    fn from(cf: BondCashFlow) -> Self {
        let amount: f64 = cf.amount.try_into().unwrap_or(0.0);
        let flow_type = format!("{:?}", cf.flow_type);

        PyCashFlow {
            date: PyDate::from(cf.date),
            amount,
            flow_type,
        }
    }
}
