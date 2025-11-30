//! Compounding conventions for interest rate calculations.
//!
//! This module provides the [`Compounding`] enum that specifies how interest
//! rates are compounded when converting between zero rates and discount factors.

use serde::{Deserialize, Serialize};

/// Compounding conventions for interest rates.
///
/// The compounding convention determines how an interest rate is applied
/// over time to compute a discount factor, or conversely, how to derive
/// an interest rate from a discount factor.
///
/// # Mathematical Relationships
///
/// Given a zero rate `r` and time `t`:
///
/// | Compounding | Discount Factor Formula |
/// |-------------|------------------------|
/// | Continuous | `DF = exp(-r * t)` |
/// | Annual | `DF = (1 + r)^(-t)` |
/// | SemiAnnual | `DF = (1 + r/2)^(-2t)` |
/// | Quarterly | `DF = (1 + r/4)^(-4t)` |
/// | Monthly | `DF = (1 + r/12)^(-12t)` |
/// | Simple | `DF = 1 / (1 + r * t)` |
///
/// # Example
///
/// ```rust
/// use convex_curves::Compounding;
///
/// let rate = 0.05; // 5% rate
/// let t = 2.0;     // 2 years
///
/// let df_continuous = Compounding::Continuous.discount_factor(rate, t);
/// let df_annual = Compounding::Annual.discount_factor(rate, t);
///
/// // Continuous compounding gives slightly lower DF
/// assert!(df_continuous < df_annual);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Compounding {
    /// Continuous compounding: DF = exp(-r * t)
    ///
    /// This is the most common convention for yield curve construction
    /// and derivative pricing due to its mathematical convenience.
    #[default]
    Continuous,

    /// Annual compounding: DF = (1 + r)^(-t)
    Annual,

    /// Semi-annual compounding: DF = (1 + r/2)^(-2t)
    ///
    /// Standard for US Treasury bonds and many corporate bonds.
    SemiAnnual,

    /// Quarterly compounding: DF = (1 + r/4)^(-4t)
    Quarterly,

    /// Monthly compounding: DF = (1 + r/12)^(-12t)
    Monthly,

    /// Simple (linear) interest: DF = 1 / (1 + r * t)
    ///
    /// Common for money market instruments with maturities under 1 year.
    Simple,
}

impl Compounding {
    /// Returns the number of compounding periods per year.
    ///
    /// Returns `None` for continuous and simple compounding.
    #[must_use]
    pub fn periods_per_year(&self) -> Option<u32> {
        match self {
            Self::Continuous | Self::Simple => None,
            Self::Annual => Some(1),
            Self::SemiAnnual => Some(2),
            Self::Quarterly => Some(4),
            Self::Monthly => Some(12),
        }
    }

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
    /// # Example
    ///
    /// ```rust
    /// use convex_curves::Compounding;
    ///
    /// // 5% continuously compounded for 1 year
    /// let df = Compounding::Continuous.discount_factor(0.05, 1.0);
    /// assert!((df - 0.9512).abs() < 0.001);
    /// ```
    #[must_use]
    pub fn discount_factor(&self, rate: f64, t: f64) -> f64 {
        if t <= 0.0 {
            return 1.0;
        }

        match self {
            Self::Continuous => (-rate * t).exp(),
            Self::Simple => 1.0 / (1.0 + rate * t),
            Self::Annual => (1.0 + rate).powf(-t),
            Self::SemiAnnual => (1.0 + rate / 2.0).powf(-2.0 * t),
            Self::Quarterly => (1.0 + rate / 4.0).powf(-4.0 * t),
            Self::Monthly => (1.0 + rate / 12.0).powf(-12.0 * t),
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
    ///
    /// # Example
    ///
    /// ```rust
    /// use convex_curves::Compounding;
    ///
    /// let df = 0.9512; // Discount factor
    /// let rate = Compounding::Continuous.zero_rate(df, 1.0);
    /// assert!((rate - 0.05).abs() < 0.001);
    /// ```
    #[must_use]
    pub fn zero_rate(&self, df: f64, t: f64) -> f64 {
        if t <= 0.0 || df <= 0.0 {
            return 0.0;
        }

        match self {
            Self::Continuous => -df.ln() / t,
            Self::Simple => (1.0 / df - 1.0) / t,
            Self::Annual => df.powf(-1.0 / t) - 1.0,
            Self::SemiAnnual => 2.0 * (df.powf(-1.0 / (2.0 * t)) - 1.0),
            Self::Quarterly => 4.0 * (df.powf(-1.0 / (4.0 * t)) - 1.0),
            Self::Monthly => 12.0 * (df.powf(-1.0 / (12.0 * t)) - 1.0),
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
    ///
    /// # Example
    ///
    /// ```rust
    /// use convex_curves::Compounding;
    ///
    /// // Convert 5% semi-annual to continuous
    /// let semi_rate = 0.05;
    /// let cont_rate = Compounding::SemiAnnual.convert_to(semi_rate, Compounding::Continuous, 1.0);
    /// assert!((cont_rate - 0.04939).abs() < 0.0001);
    /// ```
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

impl std::fmt::Display for Compounding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Continuous => write!(f, "Continuous"),
            Self::Annual => write!(f, "Annual"),
            Self::SemiAnnual => write!(f, "Semi-Annual"),
            Self::Quarterly => write!(f, "Quarterly"),
            Self::Monthly => write!(f, "Monthly"),
            Self::Simple => write!(f, "Simple"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_discount_factor_continuous() {
        let rate = 0.05;
        let t = 1.0;
        let df = Compounding::Continuous.discount_factor(rate, t);
        // DF = e^(-0.05) ≈ 0.9512
        assert_relative_eq!(df, (-0.05_f64).exp(), epsilon = 1e-10);
    }

    #[test]
    fn test_discount_factor_semi_annual() {
        let rate = 0.05;
        let t = 1.0;
        let df = Compounding::SemiAnnual.discount_factor(rate, t);
        // DF = (1 + 0.025)^(-2) ≈ 0.9518
        let expected = (1.0 + 0.025_f64).powf(-2.0);
        assert_relative_eq!(df, expected, epsilon = 1e-10);
    }

    #[test]
    fn test_discount_factor_simple() {
        let rate = 0.05;
        let t = 0.5;
        let df = Compounding::Simple.discount_factor(rate, t);
        // DF = 1 / (1 + 0.05 * 0.5) = 1/1.025 ≈ 0.9756
        assert_relative_eq!(df, 1.0 / 1.025, epsilon = 1e-10);
    }

    #[test]
    fn test_zero_rate_continuous() {
        let df = 0.9512294245;
        let t = 1.0;
        let rate = Compounding::Continuous.zero_rate(df, t);
        assert_relative_eq!(rate, 0.05, epsilon = 1e-4);
    }

    #[test]
    fn test_roundtrip_all_compounding() {
        let original_rate = 0.05;
        let t = 2.0;

        for compounding in [
            Compounding::Continuous,
            Compounding::Annual,
            Compounding::SemiAnnual,
            Compounding::Quarterly,
            Compounding::Monthly,
            Compounding::Simple,
        ] {
            let df = compounding.discount_factor(original_rate, t);
            let recovered_rate = compounding.zero_rate(df, t);
            assert_relative_eq!(
                recovered_rate,
                original_rate,
                epsilon = 1e-10,
                max_relative = 1e-10
            );
        }
    }

    #[test]
    fn test_convert_semi_annual_to_continuous() {
        let semi_rate = 0.05;
        let t = 1.0;

        let cont_rate = Compounding::SemiAnnual.convert_to(semi_rate, Compounding::Continuous, t);

        // Verify by computing DF both ways
        let df_semi = Compounding::SemiAnnual.discount_factor(semi_rate, t);
        let df_cont = Compounding::Continuous.discount_factor(cont_rate, t);

        assert_relative_eq!(df_semi, df_cont, epsilon = 1e-10);
    }

    #[test]
    fn test_convert_continuous_to_annual() {
        let cont_rate = 0.05;
        let t = 1.0;

        let annual_rate = Compounding::Continuous.convert_to(cont_rate, Compounding::Annual, t);

        // Continuous 5% ≈ Annual 5.127% (since e^0.05 - 1 ≈ 0.05127)
        let expected = (0.05_f64).exp() - 1.0;
        assert_relative_eq!(annual_rate, expected, epsilon = 1e-10);
    }

    #[test]
    fn test_periods_per_year() {
        assert_eq!(Compounding::Continuous.periods_per_year(), None);
        assert_eq!(Compounding::Simple.periods_per_year(), None);
        assert_eq!(Compounding::Annual.periods_per_year(), Some(1));
        assert_eq!(Compounding::SemiAnnual.periods_per_year(), Some(2));
        assert_eq!(Compounding::Quarterly.periods_per_year(), Some(4));
        assert_eq!(Compounding::Monthly.periods_per_year(), Some(12));
    }

    #[test]
    fn test_zero_time_returns_one() {
        for compounding in [
            Compounding::Continuous,
            Compounding::Annual,
            Compounding::SemiAnnual,
            Compounding::Simple,
        ] {
            assert_eq!(compounding.discount_factor(0.05, 0.0), 1.0);
        }
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Compounding::Continuous), "Continuous");
        assert_eq!(format!("{}", Compounding::SemiAnnual), "Semi-Annual");
    }
}
