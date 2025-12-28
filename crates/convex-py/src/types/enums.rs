//! Enum type wrappers for Python.

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Frequency};
use pyo3::prelude::*;

/// Currency codes.
///
/// Examples:
///     >>> from convex import Currency
///     >>> Currency.USD
///     Currency.USD
#[pyclass(name = "Currency", eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PyCurrency {
    USD,
    EUR,
    GBP,
    JPY,
    CHF,
    CAD,
    AUD,
    NZD,
}

#[pymethods]
impl PyCurrency {
    fn __repr__(&self) -> String {
        format!("Currency.{:?}", self)
    }

    fn __str__(&self) -> String {
        format!("{:?}", self)
    }
}

impl From<PyCurrency> for Currency {
    fn from(c: PyCurrency) -> Self {
        match c {
            PyCurrency::USD => Currency::USD,
            PyCurrency::EUR => Currency::EUR,
            PyCurrency::GBP => Currency::GBP,
            PyCurrency::JPY => Currency::JPY,
            PyCurrency::CHF => Currency::CHF,
            PyCurrency::CAD => Currency::CAD,
            PyCurrency::AUD => Currency::AUD,
            PyCurrency::NZD => Currency::NZD,
        }
    }
}

impl From<Currency> for PyCurrency {
    fn from(c: Currency) -> Self {
        match c {
            Currency::USD => PyCurrency::USD,
            Currency::EUR => PyCurrency::EUR,
            Currency::GBP => PyCurrency::GBP,
            Currency::JPY => PyCurrency::JPY,
            Currency::CHF => PyCurrency::CHF,
            Currency::CAD => PyCurrency::CAD,
            Currency::AUD => PyCurrency::AUD,
            Currency::NZD => PyCurrency::NZD,
            _ => PyCurrency::USD, // Default fallback for other currencies
        }
    }
}

/// Payment frequency for bonds.
///
/// Examples:
///     >>> from convex import Frequency
///     >>> Frequency.SEMI_ANNUAL
///     Frequency.SEMI_ANNUAL
///     >>> Frequency.SEMI_ANNUAL.periods_per_year()
///     2
#[pyclass(name = "Frequency", eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PyFrequency {
    ANNUAL,
    SEMI_ANNUAL,
    QUARTERLY,
    MONTHLY,
    ZERO,
}

#[pymethods]
impl PyFrequency {
    /// Number of payment periods per year.
    fn periods_per_year(&self) -> u32 {
        match self {
            PyFrequency::ANNUAL => 1,
            PyFrequency::SEMI_ANNUAL => 2,
            PyFrequency::QUARTERLY => 4,
            PyFrequency::MONTHLY => 12,
            PyFrequency::ZERO => 0,
        }
    }

    fn __repr__(&self) -> String {
        format!("Frequency.{:?}", self)
    }

    fn __str__(&self) -> String {
        match self {
            PyFrequency::ANNUAL => "Annual".to_string(),
            PyFrequency::SEMI_ANNUAL => "Semi-Annual".to_string(),
            PyFrequency::QUARTERLY => "Quarterly".to_string(),
            PyFrequency::MONTHLY => "Monthly".to_string(),
            PyFrequency::ZERO => "Zero".to_string(),
        }
    }
}

impl From<PyFrequency> for Frequency {
    fn from(f: PyFrequency) -> Self {
        match f {
            PyFrequency::ANNUAL => Frequency::Annual,
            PyFrequency::SEMI_ANNUAL => Frequency::SemiAnnual,
            PyFrequency::QUARTERLY => Frequency::Quarterly,
            PyFrequency::MONTHLY => Frequency::Monthly,
            PyFrequency::ZERO => Frequency::Zero,
        }
    }
}

impl From<Frequency> for PyFrequency {
    fn from(f: Frequency) -> Self {
        match f {
            Frequency::Annual => PyFrequency::ANNUAL,
            Frequency::SemiAnnual => PyFrequency::SEMI_ANNUAL,
            Frequency::Quarterly => PyFrequency::QUARTERLY,
            Frequency::Monthly => PyFrequency::MONTHLY,
            Frequency::Zero => PyFrequency::ZERO,
        }
    }
}

/// Day count conventions for interest calculations.
///
/// Examples:
///     >>> from convex import DayCount
///     >>> DayCount.ACT_365_FIXED
///     DayCount.ACT_365_FIXED
#[pyclass(name = "DayCount", eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PyDayCount {
    ACT_360,
    ACT_365_FIXED,
    ACT_ACT_ISDA,
    ACT_ACT_ICMA,
    THIRTY_360_US,
    THIRTY_360_EUROPEAN,
}

#[pymethods]
impl PyDayCount {
    fn __repr__(&self) -> String {
        format!("DayCount.{:?}", self)
    }

    fn __str__(&self) -> String {
        match self {
            PyDayCount::ACT_360 => "ACT/360".to_string(),
            PyDayCount::ACT_365_FIXED => "ACT/365 Fixed".to_string(),
            PyDayCount::ACT_ACT_ISDA => "ACT/ACT ISDA".to_string(),
            PyDayCount::ACT_ACT_ICMA => "ACT/ACT ICMA".to_string(),
            PyDayCount::THIRTY_360_US => "30/360 US".to_string(),
            PyDayCount::THIRTY_360_EUROPEAN => "30E/360".to_string(),
        }
    }
}

impl From<PyDayCount> for DayCountConvention {
    fn from(dc: PyDayCount) -> Self {
        match dc {
            PyDayCount::ACT_360 => DayCountConvention::Act360,
            PyDayCount::ACT_365_FIXED => DayCountConvention::Act365Fixed,
            PyDayCount::ACT_ACT_ISDA => DayCountConvention::ActActIsda,
            PyDayCount::ACT_ACT_ICMA => DayCountConvention::ActActIcma,
            PyDayCount::THIRTY_360_US => DayCountConvention::Thirty360US,
            PyDayCount::THIRTY_360_EUROPEAN => DayCountConvention::Thirty360E,
        }
    }
}

impl From<DayCountConvention> for PyDayCount {
    fn from(dc: DayCountConvention) -> Self {
        match dc {
            DayCountConvention::Act360 => PyDayCount::ACT_360,
            DayCountConvention::Act365Fixed => PyDayCount::ACT_365_FIXED,
            DayCountConvention::ActActIsda => PyDayCount::ACT_ACT_ISDA,
            DayCountConvention::ActActIcma => PyDayCount::ACT_ACT_ICMA,
            DayCountConvention::Thirty360US => PyDayCount::THIRTY_360_US,
            DayCountConvention::Thirty360E => PyDayCount::THIRTY_360_EUROPEAN,
            _ => PyDayCount::ACT_365_FIXED, // Default fallback for other conventions
        }
    }
}
