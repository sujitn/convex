//! Benchmark specification for spread calculations.
//!
//! This module defines how government benchmarks are specified for spread
//! calculations: by interpolated yield, specific tenor, nearest on-the-run,
//! specific security, or explicit yield.

use convex_bonds::error::IdentifierError;
use convex_bonds::types::{Cusip, Figi, Isin, Tenor};
use convex_core::types::Yield;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifier for a specific security (treasury benchmark).
///
/// Supports CUSIP (US), ISIN (international), and FIGI (Bloomberg).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SecurityId {
    /// CUSIP identifier (9 characters, primarily US)
    Cusip(Cusip),
    /// ISIN identifier (12 characters, international)
    Isin(Isin),
    /// FIGI/Bloomberg Global Identifier (12 characters)
    Figi(Figi),
}

impl SecurityId {
    /// Creates a `SecurityId` from a CUSIP string.
    ///
    /// # Errors
    ///
    /// Returns `IdentifierError` if the CUSIP is invalid.
    pub fn cusip(s: &str) -> Result<Self, IdentifierError> {
        Ok(Self::Cusip(Cusip::new(s)?))
    }

    /// Creates a `SecurityId` from an ISIN string.
    ///
    /// # Errors
    ///
    /// Returns `IdentifierError` if the ISIN is invalid.
    pub fn isin(s: &str) -> Result<Self, IdentifierError> {
        Ok(Self::Isin(Isin::new(s)?))
    }

    /// Creates a `SecurityId` from a FIGI string.
    ///
    /// # Errors
    ///
    /// Returns `IdentifierError` if the FIGI is invalid.
    pub fn figi(s: &str) -> Result<Self, IdentifierError> {
        Ok(Self::Figi(Figi::new(s)?))
    }

    /// Creates a `SecurityId` from a CUSIP without validation.
    #[must_use]
    pub fn cusip_unchecked(s: &str) -> Self {
        Self::Cusip(Cusip::new_unchecked(s))
    }

    /// Creates a `SecurityId` from an ISIN without validation.
    #[must_use]
    pub fn isin_unchecked(s: &str) -> Self {
        Self::Isin(Isin::new_unchecked(s))
    }

    /// Creates a `SecurityId` from a FIGI without validation.
    #[must_use]
    pub fn figi_unchecked(s: &str) -> Self {
        Self::Figi(Figi::new_unchecked(s))
    }

    /// Returns the identifier as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Cusip(c) => c.as_str(),
            Self::Isin(i) => i.as_str(),
            Self::Figi(f) => f.as_str(),
        }
    }

    /// Returns the type of identifier.
    #[must_use]
    pub fn id_type(&self) -> &'static str {
        match self {
            Self::Cusip(_) => "CUSIP",
            Self::Isin(_) => "ISIN",
            Self::Figi(_) => "FIGI",
        }
    }
}

impl From<Cusip> for SecurityId {
    fn from(c: Cusip) -> Self {
        Self::Cusip(c)
    }
}

impl From<Isin> for SecurityId {
    fn from(i: Isin) -> Self {
        Self::Isin(i)
    }
}

impl From<Figi> for SecurityId {
    fn from(f: Figi) -> Self {
        Self::Figi(f)
    }
}

impl fmt::Display for SecurityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cusip(c) => write!(f, "{c}"),
            Self::Isin(i) => write!(f, "{i}"),
            Self::Figi(g) => write!(f, "{g}"),
        }
    }
}

/// Specification for how to determine the government benchmark yield.
///
/// This enum provides flexibility in how the benchmark for G-spread
/// calculation is determined.
#[derive(Debug, Clone, Default)]
pub enum BenchmarkSpec {
    /// Interpolate treasury curve at bond's exact maturity (most common).
    #[default]
    Interpolated,

    /// Use a specific on-the-run tenor, regardless of bond maturity.
    OnTheRunTenor(Tenor),

    /// Use the on-the-run benchmark closest to bond's maturity.
    NearestOnTheRun,

    /// Use a specific treasury identified by CUSIP, ISIN, or FIGI.
    SpecificSecurity(SecurityId),

    /// Use an explicitly provided benchmark yield.
    ExplicitYield(Yield),
}

impl BenchmarkSpec {
    /// Creates an interpolated benchmark spec (most common).
    #[must_use]
    pub fn interpolated() -> Self {
        Self::Interpolated
    }

    /// Creates a benchmark spec for 2-year Treasury.
    #[must_use]
    pub fn two_year() -> Self {
        Self::OnTheRunTenor(Tenor::Y2)
    }

    /// Creates a benchmark spec for 3-year Treasury.
    #[must_use]
    pub fn three_year() -> Self {
        Self::OnTheRunTenor(Tenor::Y3)
    }

    /// Creates a benchmark spec for 5-year Treasury.
    #[must_use]
    pub fn five_year() -> Self {
        Self::OnTheRunTenor(Tenor::Y5)
    }

    /// Creates a benchmark spec for 7-year Treasury.
    #[must_use]
    pub fn seven_year() -> Self {
        Self::OnTheRunTenor(Tenor::Y7)
    }

    /// Creates a benchmark spec for 10-year Treasury.
    #[must_use]
    pub fn ten_year() -> Self {
        Self::OnTheRunTenor(Tenor::Y10)
    }

    /// Creates a benchmark spec for 20-year Treasury.
    #[must_use]
    pub fn twenty_year() -> Self {
        Self::OnTheRunTenor(Tenor::Y20)
    }

    /// Creates a benchmark spec for 30-year Treasury.
    #[must_use]
    pub fn thirty_year() -> Self {
        Self::OnTheRunTenor(Tenor::Y30)
    }

    /// Creates a benchmark spec for nearest on-the-run.
    #[must_use]
    pub fn nearest() -> Self {
        Self::NearestOnTheRun
    }

    /// Creates a benchmark spec with explicit yield.
    #[must_use]
    pub fn explicit(y: Yield) -> Self {
        Self::ExplicitYield(y)
    }

    /// Creates a benchmark spec for a specific tenor.
    #[must_use]
    pub fn tenor(t: Tenor) -> Self {
        Self::OnTheRunTenor(t)
    }

    /// Creates a benchmark spec for specific security by CUSIP.
    ///
    /// # Errors
    ///
    /// Returns `IdentifierError` if the CUSIP is invalid.
    pub fn cusip(c: &str) -> Result<Self, IdentifierError> {
        Ok(Self::SpecificSecurity(SecurityId::cusip(c)?))
    }

    /// Creates a benchmark spec for specific security by ISIN.
    ///
    /// # Errors
    ///
    /// Returns `IdentifierError` if the ISIN is invalid.
    pub fn isin(i: &str) -> Result<Self, IdentifierError> {
        Ok(Self::SpecificSecurity(SecurityId::isin(i)?))
    }

    /// Creates a benchmark spec for specific security by FIGI.
    ///
    /// # Errors
    ///
    /// Returns `IdentifierError` if the FIGI is invalid.
    pub fn figi(f: &str) -> Result<Self, IdentifierError> {
        Ok(Self::SpecificSecurity(SecurityId::figi(f)?))
    }

    /// Creates a benchmark spec for specific security by any identifier.
    #[must_use]
    pub fn security(id: impl Into<SecurityId>) -> Self {
        Self::SpecificSecurity(id.into())
    }

    /// Returns a description of this benchmark spec.
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::Interpolated => "Interpolated".to_string(),
            Self::OnTheRunTenor(t) => format!("{t} On-the-Run"),
            Self::NearestOnTheRun => "Nearest On-the-Run".to_string(),
            Self::SpecificSecurity(id) => format!("Security {id}"),
            Self::ExplicitYield(y) => format!("Explicit {:.2}%", y.as_percentage()),
        }
    }

    /// Returns true if this is an interpolated benchmark.
    #[must_use]
    pub fn is_interpolated(&self) -> bool {
        matches!(self, Self::Interpolated)
    }

    /// Returns true if this is an explicit yield.
    #[must_use]
    pub fn is_explicit(&self) -> bool {
        matches!(self, Self::ExplicitYield(_))
    }

    /// Returns true if this references a specific security.
    #[must_use]
    pub fn is_specific_security(&self) -> bool {
        matches!(self, Self::SpecificSecurity(_))
    }
}

impl fmt::Display for BenchmarkSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::types::Compounding;
    use rust_decimal_macros::dec;

    #[test]
    fn test_security_id_cusip() {
        let id = SecurityId::cusip_unchecked("91282CJN6");
        assert_eq!(id.id_type(), "CUSIP");
        assert_eq!(id.as_str(), "91282CJN6");
    }

    #[test]
    fn test_security_id_isin() {
        let id = SecurityId::isin_unchecked("US91282CJN65");
        assert_eq!(id.id_type(), "ISIN");
        assert!(id.as_str().starts_with("US"));
    }

    #[test]
    fn test_security_id_display() {
        let id = SecurityId::cusip_unchecked("91282CJN6");
        assert_eq!(format!("{id}"), "91282CJN6");
    }

    #[test]
    fn test_security_id_from_cusip() {
        let cusip = Cusip::new_unchecked("91282CJN6");
        let id: SecurityId = cusip.into();
        assert!(matches!(id, SecurityId::Cusip(_)));
    }

    #[test]
    fn test_benchmark_spec_interpolated() {
        let spec = BenchmarkSpec::interpolated();
        assert!(spec.is_interpolated());
        assert!(!spec.is_explicit());
        assert_eq!(spec.description(), "Interpolated");
    }

    #[test]
    fn test_benchmark_spec_tenors() {
        let spec = BenchmarkSpec::ten_year();
        assert!(matches!(spec, BenchmarkSpec::OnTheRunTenor(Tenor::Y10)));
        assert!(spec.description().contains("10"));

        let spec = BenchmarkSpec::five_year();
        assert!(matches!(spec, BenchmarkSpec::OnTheRunTenor(Tenor::Y5)));
    }

    #[test]
    fn test_benchmark_spec_explicit() {
        let y = Yield::new(dec!(0.0425), Compounding::SemiAnnual);
        let spec = BenchmarkSpec::explicit(y);
        assert!(spec.is_explicit());
        assert!(spec.description().contains("4.25"));
    }

    #[test]
    fn test_benchmark_spec_security() {
        let id = SecurityId::cusip_unchecked("91282CJN6");
        let spec = BenchmarkSpec::security(id);
        assert!(spec.is_specific_security());
        assert!(spec.description().contains("91282CJN6"));
    }

    #[test]
    fn test_benchmark_spec_default() {
        let spec = BenchmarkSpec::default();
        assert!(spec.is_interpolated());
    }

    #[test]
    fn test_benchmark_spec_display() {
        let spec = BenchmarkSpec::interpolated();
        assert_eq!(format!("{spec}"), "Interpolated");

        let spec = BenchmarkSpec::nearest();
        assert_eq!(format!("{spec}"), "Nearest On-the-Run");
    }
}
