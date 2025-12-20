//! Value type definitions for term structures.
//!
//! This module defines `ValueType`, which describes what a term structure's
//! values represent. This enables semantic conversion between different
//! curve representations (e.g., discount factors ↔ zero rates).

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, SpreadType};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Describes what a term structure's values represent.
///
/// A term structure can store values in different representations:
/// - Discount factors: P(t) where P(0) = 1
/// - Zero rates: r(t) such that P(t) = exp(-r*t) or (1+r/n)^(-nt)
/// - Forward rates: f(t, t+Δ) for a specific forward period
/// - Survival probabilities: Q(t) = P(default > t)
/// - Hazard rates: h(t) instantaneous default intensity
/// - Credit spreads: additional spread over risk-free rate
/// - Inflation ratios: I(t) / I(0)
/// - FX forward points
///
/// The `ValueType` enables automatic conversion between compatible types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum ValueType {
    /// Discount factor: P(t) where P(0) = 1.
    ///
    /// Values should be between 0 and 1, monotonically decreasing.
    #[default]
    DiscountFactor,

    /// Zero rate with specified compounding and day count convention.
    ///
    /// The relationship to discount factors depends on compounding:
    /// - Continuous: P(t) = exp(-r * t)
    /// - Periodic: P(t) = (1 + r/n)^(-n * t)
    /// - Simple: P(t) = 1 / (1 + r * t)
    ZeroRate {
        /// How the rate is compounded.
        compounding: Compounding,
        /// Day count convention for year fraction calculation.
        day_count: DayCountConvention,
    },

    /// Forward rate for a specific tenor period.
    ///
    /// f(t, t+Δ) = forward rate from time t to t+Δ.
    ForwardRate {
        /// Forward period length in years (e.g., 0.25 for 3M).
        tenor: f64,
        /// Compounding convention for the rate.
        compounding: Compounding,
    },

    /// Instantaneous forward rate: f(t) = -d/dt ln(P(t)).
    ///
    /// This is the limiting case of forward rates as tenor → 0.
    InstantaneousForward,

    /// Survival probability: Q(t) = P(default time τ > t).
    ///
    /// Values should be between 0 and 1, monotonically decreasing.
    SurvivalProbability,

    /// Instantaneous hazard rate: h(t) = -d/dt ln(Q(t)).
    ///
    /// Also known as default intensity. Relates to survival as:
    /// Q(t) = exp(-∫₀ᵗ h(s) ds)
    HazardRate,

    /// Credit spread over a risk-free curve.
    ///
    /// The spread type determines how it's applied to the base curve.
    CreditSpread {
        /// Type of spread (Z-spread, OAS, etc.).
        spread_type: SpreadType,
        /// Recovery rate assumption (typically 0.40).
        recovery: f64,
    },

    /// Inflation index ratio: I(t) / I(base).
    ///
    /// Used for inflation-linked bonds. Values > 1 indicate inflation.
    InflationIndexRatio,

    /// FX forward points in pips.
    ///
    /// Forward = Spot + Points / 10000
    FxForwardPoints,

    /// Par swap rate at each tenor.
    ///
    /// Used for swap curve representation.
    ParSwapRate {
        /// Payment frequency for the fixed leg.
        frequency: convex_core::types::Frequency,
        /// Day count for the fixed leg.
        day_count: DayCountConvention,
    },
}

impl ValueType {
    /// Returns true if this value type can be directly converted to discount factors.
    #[must_use]
    pub fn can_convert_to_discount_factor(&self) -> bool {
        matches!(
            self,
            ValueType::DiscountFactor | ValueType::ZeroRate { .. } | ValueType::SurvivalProbability
        )
    }

    /// Returns true if this is a rate-based value type.
    #[must_use]
    pub fn is_rate_type(&self) -> bool {
        matches!(
            self,
            ValueType::ZeroRate { .. }
                | ValueType::ForwardRate { .. }
                | ValueType::InstantaneousForward
                | ValueType::HazardRate
                | ValueType::ParSwapRate { .. }
        )
    }

    /// Returns true if this is a probability-based value type.
    #[must_use]
    pub fn is_probability_type(&self) -> bool {
        matches!(
            self,
            ValueType::DiscountFactor | ValueType::SurvivalProbability
        )
    }

    /// Returns true if this is a credit-related value type.
    #[must_use]
    pub fn is_credit_type(&self) -> bool {
        matches!(
            self,
            ValueType::SurvivalProbability | ValueType::HazardRate | ValueType::CreditSpread { .. }
        )
    }

    /// Returns a short name for display purposes.
    #[must_use]
    pub fn short_name(&self) -> &'static str {
        match self {
            ValueType::DiscountFactor => "DF",
            ValueType::ZeroRate { .. } => "Zero",
            ValueType::ForwardRate { .. } => "Fwd",
            ValueType::InstantaneousForward => "InstFwd",
            ValueType::SurvivalProbability => "Surv",
            ValueType::HazardRate => "Hazard",
            ValueType::CreditSpread { .. } => "CrSprd",
            ValueType::InflationIndexRatio => "Infl",
            ValueType::FxForwardPoints => "FxPts",
            ValueType::ParSwapRate { .. } => "ParSwap",
        }
    }

    /// Creates a zero rate value type with continuous compounding.
    #[must_use]
    pub fn continuous_zero(day_count: DayCountConvention) -> Self {
        ValueType::ZeroRate {
            compounding: Compounding::Continuous,
            day_count,
        }
    }

    /// Creates a zero rate value type with annual compounding.
    #[must_use]
    pub fn annual_zero(day_count: DayCountConvention) -> Self {
        ValueType::ZeroRate {
            compounding: Compounding::Annual,
            day_count,
        }
    }

    /// Creates a zero rate value type with semi-annual compounding.
    #[must_use]
    pub fn semi_annual_zero(day_count: DayCountConvention) -> Self {
        ValueType::ZeroRate {
            compounding: Compounding::SemiAnnual,
            day_count,
        }
    }

    /// Creates a 3-month forward rate value type.
    #[must_use]
    pub fn forward_3m(compounding: Compounding) -> Self {
        ValueType::ForwardRate {
            tenor: 0.25,
            compounding,
        }
    }

    /// Creates a 6-month forward rate value type.
    #[must_use]
    pub fn forward_6m(compounding: Compounding) -> Self {
        ValueType::ForwardRate {
            tenor: 0.5,
            compounding,
        }
    }

    /// Creates a credit spread value type with standard 40% recovery.
    #[must_use]
    pub fn credit_spread(spread_type: SpreadType) -> Self {
        ValueType::CreditSpread {
            spread_type,
            recovery: 0.40,
        }
    }

    /// Creates a zero rate value type with the given compounding convention.
    /// Uses Act/365 Fixed day count by default.
    #[must_use]
    pub fn zero_rate(compounding: Compounding) -> Self {
        ValueType::ZeroRate {
            compounding,
            day_count: DayCountConvention::Act365Fixed,
        }
    }

    /// Creates a forward rate value type with the given tenor and compounding.
    #[must_use]
    pub fn forward_rate(tenor: f64) -> Self {
        ValueType::ForwardRate {
            tenor,
            compounding: Compounding::Continuous,
        }
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueType::DiscountFactor => write!(f, "Discount Factor"),
            ValueType::ZeroRate {
                compounding,
                day_count,
            } => {
                write!(f, "Zero Rate ({compounding}, {day_count})")
            }
            ValueType::ForwardRate { tenor, compounding } => {
                if (*tenor - 0.25).abs() < 0.001 {
                    write!(f, "3M Forward Rate ({compounding})")
                } else if (*tenor - 0.5).abs() < 0.001 {
                    write!(f, "6M Forward Rate ({compounding})")
                } else {
                    write!(f, "{tenor}Y Forward Rate ({compounding})")
                }
            }
            ValueType::InstantaneousForward => write!(f, "Instantaneous Forward"),
            ValueType::SurvivalProbability => write!(f, "Survival Probability"),
            ValueType::HazardRate => write!(f, "Hazard Rate"),
            ValueType::CreditSpread {
                spread_type,
                recovery,
            } => {
                write!(f, "{} (R={:.0}%)", spread_type, recovery * 100.0)
            }
            ValueType::InflationIndexRatio => write!(f, "Inflation Index Ratio"),
            ValueType::FxForwardPoints => write!(f, "FX Forward Points"),
            ValueType::ParSwapRate {
                frequency,
                day_count,
            } => {
                write!(f, "Par Swap Rate ({frequency}, {day_count})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_type_display() {
        let df = ValueType::DiscountFactor;
        assert_eq!(format!("{}", df), "Discount Factor");

        let zero = ValueType::ZeroRate {
            compounding: Compounding::Continuous,
            day_count: DayCountConvention::Act365Fixed,
        };
        let display = format!("{}", zero);
        assert!(display.contains("Zero Rate"));
        assert!(display.contains("Continuous"));
    }

    #[test]
    fn test_value_type_predicates() {
        let df = ValueType::DiscountFactor;
        assert!(df.can_convert_to_discount_factor());
        assert!(df.is_probability_type());
        assert!(!df.is_rate_type());

        let zero = ValueType::ZeroRate {
            compounding: Compounding::Annual,
            day_count: DayCountConvention::Act360,
        };
        assert!(zero.can_convert_to_discount_factor());
        assert!(zero.is_rate_type());
        assert!(!zero.is_probability_type());

        let hazard = ValueType::HazardRate;
        assert!(hazard.is_credit_type());
        assert!(hazard.is_rate_type());
    }

    #[test]
    fn test_short_names() {
        assert_eq!(ValueType::DiscountFactor.short_name(), "DF");
        assert_eq!(ValueType::SurvivalProbability.short_name(), "Surv");
        assert_eq!(ValueType::HazardRate.short_name(), "Hazard");
    }

    #[test]
    fn test_convenience_constructors() {
        let zero = ValueType::continuous_zero(DayCountConvention::Act365Fixed);
        match zero {
            ValueType::ZeroRate { compounding, .. } => {
                assert_eq!(compounding, Compounding::Continuous);
            }
            _ => panic!("Expected ZeroRate"),
        }

        let fwd = ValueType::forward_3m(Compounding::Simple);
        match fwd {
            ValueType::ForwardRate { tenor, .. } => {
                assert!((tenor - 0.25).abs() < 1e-10);
            }
            _ => panic!("Expected ForwardRate"),
        }
    }

    #[test]
    fn test_serde() {
        let value_type = ValueType::ZeroRate {
            compounding: Compounding::SemiAnnual,
            day_count: DayCountConvention::Thirty360US,
        };
        let json = serde_json::to_string(&value_type).unwrap();
        let parsed: ValueType = serde_json::from_str(&json).unwrap();
        assert_eq!(value_type, parsed);
    }
}
