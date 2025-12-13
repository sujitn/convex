//! Frequency and compounding types.
//!
//! This module also provides rate conversion utilities for converting yields
//! between different compounding frequencies.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Payment frequency for coupon bonds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Frequency {
    /// Annual payments (1 per year)
    Annual,
    /// Semi-annual payments (2 per year) - most common for US bonds
    #[default]
    SemiAnnual,
    /// Quarterly payments (4 per year)
    Quarterly,
    /// Monthly payments (12 per year)
    Monthly,
    /// Zero coupon (no periodic payments)
    Zero,
}

impl Frequency {
    /// Returns the number of periods per year.
    #[must_use]
    pub fn periods_per_year(&self) -> u32 {
        match self {
            Frequency::Annual => 1,
            Frequency::SemiAnnual => 2,
            Frequency::Quarterly => 4,
            Frequency::Monthly => 12,
            Frequency::Zero => 0,
        }
    }

    /// Returns the number of months per period.
    #[must_use]
    pub fn months_per_period(&self) -> u32 {
        match self {
            Frequency::Annual => 12,
            Frequency::SemiAnnual => 6,
            Frequency::Quarterly => 3,
            Frequency::Monthly => 1,
            Frequency::Zero => 0,
        }
    }

    /// Returns true if this is a zero coupon (no periodic payments).
    #[must_use]
    pub fn is_zero(&self) -> bool {
        matches!(self, Frequency::Zero)
    }
}

impl fmt::Display for Frequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Frequency::Annual => "Annual",
            Frequency::SemiAnnual => "Semi-Annual",
            Frequency::Quarterly => "Quarterly",
            Frequency::Monthly => "Monthly",
            Frequency::Zero => "Zero Coupon",
        };
        write!(f, "{name}")
    }
}

/// Interest compounding convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Compounding {
    /// Simple interest (no compounding)
    Simple,
    /// Annual compounding (1x per year)
    Annual,
    /// Semi-annual compounding (2x per year)
    #[default]
    SemiAnnual,
    /// Quarterly compounding (4x per year)
    Quarterly,
    /// Monthly compounding (12x per year)
    Monthly,
    /// Daily compounding (365x per year)
    Daily,
    /// Continuous compounding
    Continuous,
}

impl Compounding {
    /// Returns the number of compounding periods per year.
    ///
    /// Returns 0 for Simple and a large number for Continuous.
    #[must_use]
    pub fn periods_per_year(&self) -> u32 {
        match self {
            Compounding::Simple => 0,
            Compounding::Annual => 1,
            Compounding::SemiAnnual => 2,
            Compounding::Quarterly => 4,
            Compounding::Monthly => 12,
            Compounding::Daily => 365,
            Compounding::Continuous => u32::MAX, // Conceptually infinite
        }
    }

    /// Returns true if this is continuous compounding.
    #[must_use]
    pub fn is_continuous(&self) -> bool {
        matches!(self, Compounding::Continuous)
    }

    /// Returns true if this is simple interest (no compounding).
    #[must_use]
    pub fn is_simple(&self) -> bool {
        matches!(self, Compounding::Simple)
    }

    /// Returns the number of compounding periods per year, or None for
    /// Simple and Continuous compounding.
    #[must_use]
    pub fn periods_per_year_opt(&self) -> Option<u32> {
        match self {
            Compounding::Simple | Compounding::Continuous => None,
            Compounding::Annual => Some(1),
            Compounding::SemiAnnual => Some(2),
            Compounding::Quarterly => Some(4),
            Compounding::Monthly => Some(12),
            Compounding::Daily => Some(365),
        }
    }

    // ========================================================================
    // f64-based methods for curve operations
    // ========================================================================

    /// Calculates the discount factor from a zero rate and time.
    ///
    /// # Arguments
    ///
    /// * `rate` - The zero rate (as a decimal, e.g., 0.05 for 5%)
    /// * `t` - Time in years
    ///
    /// # Returns
    ///
    /// The discount factor, a value typically between 0 and 1.
    ///
    /// # Formulas
    ///
    /// | Compounding | Discount Factor Formula |
    /// |-------------|------------------------|
    /// | Continuous | `DF = exp(-r * t)` |
    /// | Annual | `DF = (1 + r)^(-t)` |
    /// | SemiAnnual | `DF = (1 + r/2)^(-2t)` |
    /// | Simple | `DF = 1 / (1 + r * t)` |
    #[must_use]
    pub fn discount_factor(&self, rate: f64, t: f64) -> f64 {
        if t <= 0.0 {
            return 1.0;
        }

        match self {
            Compounding::Continuous => (-rate * t).exp(),
            Compounding::Simple => 1.0 / (1.0 + rate * t),
            Compounding::Annual => (1.0 + rate).powf(-t),
            Compounding::SemiAnnual => (1.0 + rate / 2.0).powf(-2.0 * t),
            Compounding::Quarterly => (1.0 + rate / 4.0).powf(-4.0 * t),
            Compounding::Monthly => (1.0 + rate / 12.0).powf(-12.0 * t),
            Compounding::Daily => (1.0 + rate / 365.0).powf(-365.0 * t),
        }
    }

    /// Calculates the zero rate from a discount factor and time.
    ///
    /// # Arguments
    ///
    /// * `df` - The discount factor (between 0 and 1)
    /// * `t` - Time in years
    ///
    /// # Returns
    ///
    /// The zero rate as a decimal.
    #[must_use]
    pub fn zero_rate(&self, df: f64, t: f64) -> f64 {
        if t <= 0.0 || df <= 0.0 {
            return 0.0;
        }

        match self {
            Compounding::Continuous => -df.ln() / t,
            Compounding::Simple => (1.0 / df - 1.0) / t,
            Compounding::Annual => df.powf(-1.0 / t) - 1.0,
            Compounding::SemiAnnual => 2.0 * (df.powf(-1.0 / (2.0 * t)) - 1.0),
            Compounding::Quarterly => 4.0 * (df.powf(-1.0 / (4.0 * t)) - 1.0),
            Compounding::Monthly => 12.0 * (df.powf(-1.0 / (12.0 * t)) - 1.0),
            Compounding::Daily => 365.0 * (df.powf(-1.0 / (365.0 * t)) - 1.0),
        }
    }

    /// Converts a rate from this compounding to another compounding.
    ///
    /// # Arguments
    ///
    /// * `rate` - The rate in this compounding convention
    /// * `to` - The target compounding convention
    /// * `t` - Time in years (needed for simple interest conversion)
    ///
    /// # Returns
    ///
    /// The equivalent rate in the target compounding convention.
    #[must_use]
    pub fn convert_to(&self, rate: f64, to: Compounding, t: f64) -> f64 {
        if *self == to {
            return rate;
        }

        // Convert via discount factor
        let df = self.discount_factor(rate, t);
        to.zero_rate(df, t)
    }
}

impl fmt::Display for Compounding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Compounding::Simple => "Simple",
            Compounding::Annual => "Annual",
            Compounding::SemiAnnual => "Semi-Annual",
            Compounding::Quarterly => "Quarterly",
            Compounding::Monthly => "Monthly",
            Compounding::Daily => "Daily",
            Compounding::Continuous => "Continuous",
        };
        write!(f, "{name}")
    }
}

impl From<Frequency> for Compounding {
    fn from(freq: Frequency) -> Self {
        match freq {
            Frequency::Annual => Compounding::Annual,
            Frequency::SemiAnnual => Compounding::SemiAnnual,
            Frequency::Quarterly => Compounding::Quarterly,
            Frequency::Monthly => Compounding::Monthly,
            Frequency::Zero => Compounding::Continuous, // Zero coupon typically uses continuous
        }
    }
}

// ============================================================================
// Rate Conversion Functions
// ============================================================================

/// Convert a rate from one compounding frequency to another.
///
/// This function converts an interest rate expressed with one compounding
/// convention to an equivalent rate expressed with a different convention.
///
/// # Formula
///
/// For converting from frequency `n` to frequency `m`:
/// ```text
/// r_m = m × [(1 + r_n/n)^(n/m) - 1]
/// ```
///
/// For continuous compounding:
/// - To continuous: r_c = n × ln(1 + r_n/n)
/// - From continuous: r_n = n × (e^(r_c/n) - 1)
///
/// # Arguments
///
/// * `rate` - The rate as a decimal (e.g., 0.05 for 5%)
/// * `from` - Source compounding convention
/// * `to` - Target compounding convention
///
/// # Returns
///
/// The equivalent rate under the target compounding convention
///
/// # Example
///
/// ```rust
/// use convex_core::types::{Compounding, convert_rate};
/// use rust_decimal_macros::dec;
///
/// // Convert 5% semi-annual to annual
/// let semi_annual = dec!(0.05);
/// let annual = convert_rate(semi_annual, Compounding::SemiAnnual, Compounding::Annual);
///
/// // The annual rate should be slightly higher
/// assert!(annual > semi_annual);
/// ```
#[must_use]
pub fn convert_rate(rate: Decimal, from: Compounding, to: Compounding) -> Decimal {
    if from == to {
        return rate;
    }

    // Handle simple interest
    if from == Compounding::Simple || to == Compounding::Simple {
        // Simple interest doesn't really convert - just return as-is
        // A more sophisticated implementation might need additional parameters
        return rate;
    }

    let r = rate.to_f64().unwrap_or(0.0);

    // Convert to continuous first (as intermediate step)
    let r_continuous = match from {
        Compounding::Continuous => r,
        Compounding::Simple => r, // Handled above
        _ => {
            let n = from.periods_per_year() as f64;
            n * (1.0 + r / n).ln()
        }
    };

    // Convert from continuous to target
    let r_target = match to {
        Compounding::Continuous => r_continuous,
        Compounding::Simple => r_continuous, // Handled above
        _ => {
            let m = to.periods_per_year() as f64;
            m * ((r_continuous / m).exp() - 1.0)
        }
    };

    Decimal::from_f64_retain(r_target).unwrap_or(Decimal::ZERO)
}

/// Calculate the effective annual rate (EAR) from a nominal rate.
///
/// The effective annual rate is the actual annual return accounting for
/// compounding. It's the rate that would give the same return if
/// compounding occurred annually.
///
/// # Formula
///
/// ```text
/// EAR = (1 + r/n)^n - 1
/// ```
///
/// For continuous compounding:
/// ```text
/// EAR = e^r - 1
/// ```
///
/// # Arguments
///
/// * `rate` - Nominal rate as a decimal (e.g., 0.05 for 5%)
/// * `compounding` - The compounding convention
///
/// # Returns
///
/// The effective annual rate as a decimal
///
/// # Example
///
/// ```rust
/// use convex_core::types::{Compounding, effective_annual_rate};
/// use rust_decimal_macros::dec;
///
/// // 5% compounded semi-annually
/// let ear = effective_annual_rate(dec!(0.05), Compounding::SemiAnnual);
///
/// // EAR = (1 + 0.05/2)^2 - 1 ≈ 5.0625%
/// ```
#[must_use]
pub fn effective_annual_rate(rate: Decimal, compounding: Compounding) -> Decimal {
    let r = rate.to_f64().unwrap_or(0.0);

    let ear = match compounding {
        Compounding::Simple => r, // Simple interest EAR = nominal rate
        Compounding::Annual => r, // Annual compounding EAR = nominal rate
        Compounding::Continuous => r.exp() - 1.0,
        _ => {
            let n = compounding.periods_per_year() as f64;
            (1.0 + r / n).powf(n) - 1.0
        }
    };

    Decimal::from_f64_retain(ear).unwrap_or(Decimal::ZERO)
}

/// Convert an effective annual rate back to a nominal rate.
///
/// This is the inverse of `effective_annual_rate`.
///
/// # Formula
///
/// ```text
/// r = n × [(1 + EAR)^(1/n) - 1]
/// ```
///
/// # Arguments
///
/// * `ear` - Effective annual rate as a decimal
/// * `compounding` - Target compounding convention
///
/// # Returns
///
/// The nominal rate under the target compounding convention
#[must_use]
pub fn nominal_rate_from_ear(ear: Decimal, compounding: Compounding) -> Decimal {
    let e = ear.to_f64().unwrap_or(0.0);

    let nominal = match compounding {
        Compounding::Simple => e, // Simple interest nominal = EAR
        Compounding::Annual => e, // Annual compounding nominal = EAR
        Compounding::Continuous => (1.0 + e).ln(),
        _ => {
            let n = compounding.periods_per_year() as f64;
            n * ((1.0 + e).powf(1.0 / n) - 1.0)
        }
    };

    Decimal::from_f64_retain(nominal).unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use rust_decimal_macros::dec;

    #[test]
    fn test_frequency_periods() {
        assert_eq!(Frequency::Annual.periods_per_year(), 1);
        assert_eq!(Frequency::SemiAnnual.periods_per_year(), 2);
        assert_eq!(Frequency::Quarterly.periods_per_year(), 4);
        assert_eq!(Frequency::Monthly.periods_per_year(), 12);
        assert_eq!(Frequency::Zero.periods_per_year(), 0);
    }

    #[test]
    fn test_compounding_periods() {
        assert_eq!(Compounding::Annual.periods_per_year(), 1);
        assert_eq!(Compounding::SemiAnnual.periods_per_year(), 2);
        assert_eq!(Compounding::Daily.periods_per_year(), 365);
    }

    #[test]
    fn test_frequency_to_compounding() {
        let comp: Compounding = Frequency::SemiAnnual.into();
        assert_eq!(comp, Compounding::SemiAnnual);
    }

    // ========================================================================
    // Rate Conversion Tests
    // ========================================================================

    #[test]
    fn test_convert_rate_same_compounding() {
        let rate = dec!(0.05);
        let result = convert_rate(rate, Compounding::SemiAnnual, Compounding::SemiAnnual);
        assert_eq!(result, rate);
    }

    #[test]
    fn test_convert_rate_semi_to_annual() {
        // 5% semi-annual ≈ 5.0625% annual
        let semi = dec!(0.05);
        let annual = convert_rate(semi, Compounding::SemiAnnual, Compounding::Annual);

        // (1 + 0.05/2)^2 - 1 = 0.050625
        assert_relative_eq!(annual.to_f64().unwrap(), 0.050625, epsilon = 0.0001);
    }

    #[test]
    fn test_convert_rate_annual_to_semi() {
        // 5% annual → semi-annual
        let annual = dec!(0.05);
        let semi = convert_rate(annual, Compounding::Annual, Compounding::SemiAnnual);

        // 2 × ((1.05)^(1/2) - 1) ≈ 0.04939
        assert_relative_eq!(semi.to_f64().unwrap(), 0.04939, epsilon = 0.0001);
    }

    #[test]
    fn test_convert_rate_to_continuous() {
        // 5% semi-annual → continuous
        let semi = dec!(0.05);
        let continuous = convert_rate(semi, Compounding::SemiAnnual, Compounding::Continuous);

        // 2 × ln(1 + 0.05/2) ≈ 0.04938
        assert_relative_eq!(continuous.to_f64().unwrap(), 0.04939, epsilon = 0.0001);
    }

    #[test]
    fn test_convert_rate_from_continuous() {
        // 5% continuous → semi-annual
        let continuous = dec!(0.05);
        let semi = convert_rate(continuous, Compounding::Continuous, Compounding::SemiAnnual);

        // 2 × (e^(0.05/2) - 1) ≈ 0.05063
        assert_relative_eq!(semi.to_f64().unwrap(), 0.05063, epsilon = 0.0001);
    }

    #[test]
    fn test_convert_rate_roundtrip() {
        let original = dec!(0.06);

        // Semi-annual → Quarterly → Semi-annual
        let quarterly = convert_rate(original, Compounding::SemiAnnual, Compounding::Quarterly);
        let recovered = convert_rate(quarterly, Compounding::Quarterly, Compounding::SemiAnnual);

        assert_relative_eq!(
            recovered.to_f64().unwrap(),
            original.to_f64().unwrap(),
            epsilon = 0.0001
        );
    }

    #[test]
    fn test_effective_annual_rate_semi() {
        // 5% semi-annual → EAR
        let rate = dec!(0.05);
        let ear = effective_annual_rate(rate, Compounding::SemiAnnual);

        // (1 + 0.05/2)^2 - 1 = 0.050625
        assert_relative_eq!(ear.to_f64().unwrap(), 0.050625, epsilon = 0.0001);
    }

    #[test]
    fn test_effective_annual_rate_quarterly() {
        // 5% quarterly → EAR
        let rate = dec!(0.05);
        let ear = effective_annual_rate(rate, Compounding::Quarterly);

        // (1 + 0.05/4)^4 - 1 ≈ 0.05095
        assert_relative_eq!(ear.to_f64().unwrap(), 0.05095, epsilon = 0.0001);
    }

    #[test]
    fn test_effective_annual_rate_continuous() {
        // 5% continuous → EAR
        let rate = dec!(0.05);
        let ear = effective_annual_rate(rate, Compounding::Continuous);

        // e^0.05 - 1 ≈ 0.05127
        assert_relative_eq!(ear.to_f64().unwrap(), 0.05127, epsilon = 0.0001);
    }

    #[test]
    fn test_effective_annual_rate_annual() {
        // Annual compounding: EAR = nominal rate
        let rate = dec!(0.05);
        let ear = effective_annual_rate(rate, Compounding::Annual);
        assert_relative_eq!(ear.to_f64().unwrap(), 0.05, epsilon = 0.0001);
    }

    #[test]
    fn test_nominal_rate_from_ear() {
        // EAR of 5.0625% → semi-annual nominal
        let ear = dec!(0.050625);
        let nominal = nominal_rate_from_ear(ear, Compounding::SemiAnnual);

        // Should get back 5%
        assert_relative_eq!(nominal.to_f64().unwrap(), 0.05, epsilon = 0.0001);
    }

    #[test]
    fn test_ear_nominal_roundtrip() {
        let original = dec!(0.06);

        // Nominal → EAR → Nominal
        let ear = effective_annual_rate(original, Compounding::Quarterly);
        let recovered = nominal_rate_from_ear(ear, Compounding::Quarterly);

        assert_relative_eq!(
            recovered.to_f64().unwrap(),
            original.to_f64().unwrap(),
            epsilon = 0.0001
        );
    }

    // ========================================================================
    // f64-based Compounding method tests
    // ========================================================================

    #[test]
    fn test_compounding_discount_factor_continuous() {
        let rate = 0.05;
        let t = 1.0;
        let df = Compounding::Continuous.discount_factor(rate, t);
        // DF = e^(-0.05) ≈ 0.9512
        assert_relative_eq!(df, (-0.05_f64).exp(), epsilon = 1e-10);
    }

    #[test]
    fn test_compounding_discount_factor_semi_annual() {
        let rate = 0.05;
        let t = 1.0;
        let df = Compounding::SemiAnnual.discount_factor(rate, t);
        // DF = (1 + 0.025)^(-2) ≈ 0.9518
        let expected = (1.0 + 0.025_f64).powf(-2.0);
        assert_relative_eq!(df, expected, epsilon = 1e-10);
    }

    #[test]
    fn test_compounding_discount_factor_simple() {
        let rate = 0.05;
        let t = 0.5;
        let df = Compounding::Simple.discount_factor(rate, t);
        // DF = 1 / (1 + 0.05 * 0.5) = 1/1.025 ≈ 0.9756
        assert_relative_eq!(df, 1.0 / 1.025, epsilon = 1e-10);
    }

    #[test]
    fn test_compounding_zero_rate_continuous() {
        let df = 0.9512294245;
        let t = 1.0;
        let rate = Compounding::Continuous.zero_rate(df, t);
        assert_relative_eq!(rate, 0.05, epsilon = 1e-4);
    }

    #[test]
    fn test_compounding_roundtrip_all() {
        let original_rate = 0.05;
        let t = 2.0;

        for compounding in [
            Compounding::Continuous,
            Compounding::Annual,
            Compounding::SemiAnnual,
            Compounding::Quarterly,
            Compounding::Monthly,
            Compounding::Daily,
            Compounding::Simple,
        ] {
            let df = compounding.discount_factor(original_rate, t);
            let recovered_rate = compounding.zero_rate(df, t);
            assert_relative_eq!(recovered_rate, original_rate, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_compounding_convert_to_semi_to_continuous() {
        let semi_rate = 0.05;
        let t = 1.0;

        let cont_rate = Compounding::SemiAnnual.convert_to(semi_rate, Compounding::Continuous, t);

        // Verify by computing DF both ways
        let df_semi = Compounding::SemiAnnual.discount_factor(semi_rate, t);
        let df_cont = Compounding::Continuous.discount_factor(cont_rate, t);

        assert_relative_eq!(df_semi, df_cont, epsilon = 1e-10);
    }

    #[test]
    fn test_compounding_periods_per_year_opt() {
        assert_eq!(Compounding::Continuous.periods_per_year_opt(), None);
        assert_eq!(Compounding::Simple.periods_per_year_opt(), None);
        assert_eq!(Compounding::Annual.periods_per_year_opt(), Some(1));
        assert_eq!(Compounding::SemiAnnual.periods_per_year_opt(), Some(2));
        assert_eq!(Compounding::Quarterly.periods_per_year_opt(), Some(4));
        assert_eq!(Compounding::Monthly.periods_per_year_opt(), Some(12));
        assert_eq!(Compounding::Daily.periods_per_year_opt(), Some(365));
    }

    #[test]
    fn test_compounding_zero_time_returns_one() {
        for compounding in [
            Compounding::Continuous,
            Compounding::Annual,
            Compounding::SemiAnnual,
            Compounding::Simple,
        ] {
            assert_eq!(compounding.discount_factor(0.05, 0.0), 1.0);
        }
    }
}
