//! Benchmark specification for spread calculations.
//!
//! This module defines how government benchmarks are specified for spread
//! calculations: by interpolated yield, specific tenor, nearest on-the-run,
//! specific security, or explicit yield.

use convex_core::types::Yield;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifier for a specific security (treasury benchmark).
///
/// Supports CUSIP (US), ISIN (international), and FIGI (Bloomberg).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SecurityId {
    /// CUSIP identifier (9 characters, primarily US)
    Cusip(String),
    /// ISIN identifier (12 characters, international)
    Isin(String),
    /// FIGI/Bloomberg Global Identifier (12 characters)
    Figi(String),
}

impl SecurityId {
    /// Creates a `SecurityId` from a CUSIP string.
    #[must_use]
    pub fn cusip(s: &str) -> Self {
        Self::Cusip(s.to_string())
    }

    /// Creates a `SecurityId` from an ISIN string.
    #[must_use]
    pub fn isin(s: &str) -> Self {
        Self::Isin(s.to_string())
    }

    /// Creates a `SecurityId` from a FIGI string.
    #[must_use]
    pub fn figi(s: &str) -> Self {
        Self::Figi(s.to_string())
    }

    /// Returns the identifier string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Cusip(s) | Self::Isin(s) | Self::Figi(s) => s,
        }
    }
}

impl fmt::Display for SecurityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cusip(s) => write!(f, "CUSIP:{s}"),
            Self::Isin(s) => write!(f, "ISIN:{s}"),
            Self::Figi(s) => write!(f, "FIGI:{s}"),
        }
    }
}

/// Tenor specification for benchmark lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tenor {
    /// Number of months.
    months: u32,
}

impl Tenor {
    /// Creates a new tenor from months.
    #[must_use]
    pub const fn from_months(months: u32) -> Self {
        Self { months }
    }

    /// Creates a new tenor from years.
    #[must_use]
    pub const fn from_years(years: u32) -> Self {
        Self { months: years * 12 }
    }

    /// Returns the tenor in months.
    #[must_use]
    pub const fn months(&self) -> u32 {
        self.months
    }

    /// Returns the tenor in years (approximate).
    #[must_use]
    pub const fn years(&self) -> u32 {
        self.months / 12
    }

    /// 1-year tenor.
    pub const Y1: Self = Self::from_years(1);
    /// 2-year tenor.
    pub const Y2: Self = Self::from_years(2);
    /// 3-year tenor.
    pub const Y3: Self = Self::from_years(3);
    /// 5-year tenor.
    pub const Y5: Self = Self::from_years(5);
    /// 7-year tenor.
    pub const Y7: Self = Self::from_years(7);
    /// 10-year tenor.
    pub const Y10: Self = Self::from_years(10);
    /// 20-year tenor.
    pub const Y20: Self = Self::from_years(20);
    /// 30-year tenor.
    pub const Y30: Self = Self::from_years(30);
}

impl fmt::Display for Tenor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.months >= 12 && self.months % 12 == 0 {
            write!(f, "{}Y", self.months / 12)
        } else {
            write!(f, "{}M", self.months)
        }
    }
}

/// Specifies how to determine the benchmark for spread calculation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BenchmarkSpec {
    /// Interpolate yield from benchmark curve at bond's maturity.
    ///
    /// This is the standard approach for calculating G-spread, where the
    /// benchmark yield is obtained by interpolating the government yield
    /// curve at the exact maturity of the corporate bond.
    Interpolated,

    /// Use a specific benchmark tenor (e.g., 5Y, 10Y).
    ///
    /// The benchmark yield will be taken from the on-the-run security
    /// of the specified tenor, or interpolated if not exactly available.
    Tenor(Tenor),

    /// Use the nearest on-the-run benchmark to the bond's maturity.
    ///
    /// Selects the benchmark with maturity closest to the bond's maturity.
    NearestOnTheRun,

    /// Use a specific security as benchmark.
    ///
    /// Allows specifying an exact treasury security by its identifier.
    Security(SecurityId),

    /// Use an explicit benchmark yield (no curve lookup needed).
    ///
    /// Useful when the benchmark yield is already known or for
    /// scenario analysis with hypothetical benchmark levels.
    Explicit(Yield),
}

impl Default for BenchmarkSpec {
    fn default() -> Self {
        Self::Interpolated
    }
}

impl fmt::Display for BenchmarkSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interpolated => write!(f, "Interpolated"),
            Self::Tenor(t) => write!(f, "Tenor({t})"),
            Self::NearestOnTheRun => write!(f, "NearestOnTheRun"),
            Self::Security(id) => write!(f, "Security({id})"),
            Self::Explicit(y) => write!(f, "Explicit({y})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_id_creation() {
        let cusip = SecurityId::cusip("912828ZT0");
        assert!(matches!(cusip, SecurityId::Cusip(_)));

        let isin = SecurityId::isin("US912828ZT00");
        assert!(matches!(isin, SecurityId::Isin(_)));

        let figi = SecurityId::figi("BBG000BVPV84");
        assert!(matches!(figi, SecurityId::Figi(_)));
    }

    #[test]
    fn test_security_id_display() {
        let cusip = SecurityId::cusip("912828ZT0");
        assert_eq!(format!("{cusip}"), "CUSIP:912828ZT0");
    }

    #[test]
    fn test_tenor_creation() {
        let t1 = Tenor::from_years(5);
        assert_eq!(t1.years(), 5);
        assert_eq!(t1.months(), 60);

        let t2 = Tenor::from_months(18);
        assert_eq!(t2.months(), 18);
    }

    #[test]
    fn test_tenor_display() {
        assert_eq!(format!("{}", Tenor::Y5), "5Y");
        assert_eq!(format!("{}", Tenor::from_months(18)), "18M");
    }

    #[test]
    fn test_benchmark_spec_default() {
        let spec = BenchmarkSpec::default();
        assert!(matches!(spec, BenchmarkSpec::Interpolated));
    }
}
