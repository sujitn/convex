//! Frequency and compounding types.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Payment frequency for coupon bonds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

    /// Periodic compounding for a periods-per-year count. Returns `None` for
    /// values outside the supported set; callers that have already validated
    /// the input can use [`Self::from_periods_per_year`] for the panicking
    /// flavour.
    #[must_use]
    pub fn try_from_periods_per_year(periods: u32) -> Option<Compounding> {
        Some(match periods {
            0 => Compounding::Continuous,
            1 => Compounding::Annual,
            2 => Compounding::SemiAnnual,
            4 => Compounding::Quarterly,
            12 => Compounding::Monthly,
            365 => Compounding::Daily,
            _ => return None,
        })
    }

    /// Like [`Self::try_from_periods_per_year`] but panics on unsupported
    /// counts. Use only when `periods` came from a closed source like
    /// `Frequency::periods_per_year`.
    #[must_use]
    pub fn from_periods_per_year(periods: u32) -> Compounding {
        Self::try_from_periods_per_year(periods)
            .unwrap_or_else(|| panic!("unsupported compounding period count: {periods}"))
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
