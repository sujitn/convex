//! Compounding method definitions for yield calculations.
//!
//! This module defines how interest compounds across different market conventions.
//! Different markets and instrument types use different compounding methodologies.

use serde::{Deserialize, Serialize};

/// Compounding method for yield calculations.
///
/// Different markets use different compounding conventions for yield calculations.
/// This enum captures all standard compounding methodologies.
///
/// # Example
///
/// ```rust
/// use convex_bonds::types::CompoundingMethod;
///
/// // US corporates use semi-annual compounding
/// let us_corp = CompoundingMethod::Periodic { frequency: 2 };
///
/// // Eurobonds use annual compounding
/// let eurobond = CompoundingMethod::Periodic { frequency: 1 };
///
/// // Money market instruments use simple interest
/// let money_market = CompoundingMethod::Simple;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CompoundingMethod {
    /// Periodic compounding: (1 + r/n)^(nt)
    ///
    /// Standard compounding used for most coupon-bearing bonds.
    /// - `frequency`: Number of compounding periods per year
    ///   - 1 = Annual (Eurobonds, German Bunds)
    ///   - 2 = Semi-annual (US Treasuries, Corporates)
    ///   - 4 = Quarterly (some FRNs)
    ///   - 12 = Monthly (mortgages)
    Periodic {
        /// Compounding periods per year
        frequency: u32,
    },

    /// Continuous compounding: e^(rt)
    ///
    /// Used in theoretical models, derivatives pricing, and some
    /// academic calculations. The discount factor is exp(-rt).
    Continuous,

    /// Simple interest: 1 + rt
    ///
    /// No compounding - used for:
    /// - Japanese government bonds (JGBs)
    /// - Money market instruments
    /// - Short-term discount instruments
    Simple,

    /// Discount basis: 1 / (1 + rt)
    ///
    /// Used for discount instruments like:
    /// - US Treasury Bills
    /// - Commercial paper
    /// - Bankers acceptances
    Discount,

    /// ISMA/ICMA actual period compounding.
    ///
    /// Uses actual period lengths for compounding, accounting for
    /// irregular periods. Standard for international bonds (Eurobonds).
    /// The discount factor depends on actual days in the accrual period.
    ActualPeriod {
        /// Base compounding frequency per year
        frequency: u32,
    },
}

impl CompoundingMethod {
    /// Returns the base compounding frequency, if applicable.
    ///
    /// Returns `None` for `Continuous`, `Simple`, and `Discount` methods.
    #[must_use]
    pub const fn frequency(&self) -> Option<u32> {
        match self {
            Self::Periodic { frequency } | Self::ActualPeriod { frequency } => Some(*frequency),
            Self::Continuous | Self::Simple | Self::Discount => None,
        }
    }

    /// Returns true if this is periodic compounding.
    #[must_use]
    pub const fn is_periodic(&self) -> bool {
        matches!(self, Self::Periodic { .. } | Self::ActualPeriod { .. })
    }

    /// Returns true if this uses simple interest (no compounding).
    #[must_use]
    pub const fn is_simple(&self) -> bool {
        matches!(self, Self::Simple | Self::Discount)
    }

    /// Creates a semi-annual periodic compounding method.
    #[must_use]
    pub const fn semi_annual() -> Self {
        Self::Periodic { frequency: 2 }
    }

    /// Creates an annual periodic compounding method.
    #[must_use]
    pub const fn annual() -> Self {
        Self::Periodic { frequency: 1 }
    }

    /// Creates a quarterly periodic compounding method.
    #[must_use]
    pub const fn quarterly() -> Self {
        Self::Periodic { frequency: 4 }
    }

    /// Creates a monthly periodic compounding method.
    #[must_use]
    pub const fn monthly() -> Self {
        Self::Periodic { frequency: 12 }
    }

    /// Calculate the discount factor for a given yield and time.
    ///
    /// # Arguments
    ///
    /// * `yield_rate` - Annual yield rate as decimal (e.g., 0.05 for 5%)
    /// * `time` - Time in years
    ///
    /// # Returns
    ///
    /// The discount factor to apply to future cash flows.
    #[must_use]
    pub fn discount_factor(&self, yield_rate: f64, time: f64) -> f64 {
        match self {
            Self::Periodic { frequency } => {
                let f = *frequency as f64;
                let rate_per_period = yield_rate / f;
                let periods = time * f;
                (1.0 + rate_per_period).powf(-periods)
            }
            Self::Continuous => (-yield_rate * time).exp(),
            Self::Simple => 1.0 / (1.0 + yield_rate * time),
            Self::Discount => 1.0 / (1.0 + yield_rate * time),
            Self::ActualPeriod { frequency } => {
                // For ActualPeriod, time should already reflect actual period fractions
                let f = *frequency as f64;
                let rate_per_period = yield_rate / f;
                let periods = time * f;
                (1.0 + rate_per_period).powf(-periods)
            }
        }
    }

    /// Calculate the derivative of the discount factor with respect to yield.
    ///
    /// Used for Newton-Raphson yield solving.
    ///
    /// # Arguments
    ///
    /// * `yield_rate` - Annual yield rate as decimal
    /// * `time` - Time in years
    ///
    /// # Returns
    ///
    /// The derivative d(DF)/d(yield).
    #[must_use]
    pub fn discount_factor_derivative(&self, yield_rate: f64, time: f64) -> f64 {
        match self {
            Self::Periodic { frequency } => {
                let f = *frequency as f64;
                let rate_per_period = yield_rate / f;
                let periods = time * f;
                let df = (1.0 + rate_per_period).powf(-periods);
                -time * df / (1.0 + rate_per_period)
            }
            Self::Continuous => {
                let df = (-yield_rate * time).exp();
                -time * df
            }
            Self::Simple | Self::Discount => {
                let denom = 1.0 + yield_rate * time;
                -time / (denom * denom)
            }
            Self::ActualPeriod { frequency } => {
                let f = *frequency as f64;
                let rate_per_period = yield_rate / f;
                let periods = time * f;
                let df = (1.0 + rate_per_period).powf(-periods);
                -time * df / (1.0 + rate_per_period)
            }
        }
    }
}

impl Default for CompoundingMethod {
    fn default() -> Self {
        Self::semi_annual()
    }
}

impl std::fmt::Display for CompoundingMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Periodic { frequency: 1 } => write!(f, "Annual"),
            Self::Periodic { frequency: 2 } => write!(f, "Semi-Annual"),
            Self::Periodic { frequency: 4 } => write!(f, "Quarterly"),
            Self::Periodic { frequency: 12 } => write!(f, "Monthly"),
            Self::Periodic { frequency } => write!(f, "Periodic ({frequency}x/year)"),
            Self::Continuous => write!(f, "Continuous"),
            Self::Simple => write!(f, "Simple Interest"),
            Self::Discount => write!(f, "Discount Basis"),
            Self::ActualPeriod { frequency: 1 } => write!(f, "ICMA Annual"),
            Self::ActualPeriod { frequency: 2 } => write!(f, "ICMA Semi-Annual"),
            Self::ActualPeriod { frequency } => write!(f, "ICMA ({frequency}x/year)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_compounding_method_frequency() {
        assert_eq!(CompoundingMethod::semi_annual().frequency(), Some(2));
        assert_eq!(CompoundingMethod::annual().frequency(), Some(1));
        assert_eq!(CompoundingMethod::Continuous.frequency(), None);
        assert_eq!(CompoundingMethod::Simple.frequency(), None);
    }

    #[test]
    fn test_discount_factor_periodic() {
        let method = CompoundingMethod::semi_annual();
        // 5% yield, 1 year: DF = (1 + 0.05/2)^(-2) = 0.951814
        let df = method.discount_factor(0.05, 1.0);
        assert_relative_eq!(df, 0.951814, epsilon = 0.0001);
    }

    #[test]
    fn test_discount_factor_continuous() {
        let method = CompoundingMethod::Continuous;
        // 5% yield, 1 year: DF = e^(-0.05) = 0.951229
        let df = method.discount_factor(0.05, 1.0);
        assert_relative_eq!(df, 0.951229, epsilon = 0.0001);
    }

    #[test]
    fn test_discount_factor_simple() {
        let method = CompoundingMethod::Simple;
        // 5% yield, 1 year: DF = 1/(1 + 0.05) = 0.952381
        let df = method.discount_factor(0.05, 1.0);
        assert_relative_eq!(df, 0.952381, epsilon = 0.0001);
    }

    #[test]
    fn test_is_periodic() {
        assert!(CompoundingMethod::semi_annual().is_periodic());
        assert!(CompoundingMethod::ActualPeriod { frequency: 2 }.is_periodic());
        assert!(!CompoundingMethod::Continuous.is_periodic());
        assert!(!CompoundingMethod::Simple.is_periodic());
    }

    #[test]
    fn test_display() {
        assert_eq!(
            format!("{}", CompoundingMethod::semi_annual()),
            "Semi-Annual"
        );
        assert_eq!(format!("{}", CompoundingMethod::annual()), "Annual");
        assert_eq!(format!("{}", CompoundingMethod::Continuous), "Continuous");
    }
}
