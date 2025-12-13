//! Yield and accrued interest conventions for bond analytics.
//!
//! This module provides:
//! - [`YieldMethod`]: Re-exported from `convex_core` - basic calculation methodology
//! - [`AccruedConvention`]: Accrued interest calculation conventions
//! - [`RoundingConvention`]: Rounding conventions for yield calculations

use serde::{Deserialize, Serialize};

// Re-export YieldMethod from convex-core for backwards compatibility
pub use convex_core::types::YieldMethod;

/// Accrued interest calculation convention.
///
/// Different markets handle accrued interest differently,
/// particularly around ex-dividend dates and record dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum AccruedConvention {
    /// Standard accrued interest calculation.
    ///
    /// Accrued = Coupon × (Days from last coupon / Days in period)
    #[default]
    Standard,

    /// No accrued interest (zero-coupon bonds).
    None,

    /// Ex-dividend convention (UK Gilts).
    ///
    /// During the ex-dividend period (typically 7 business days
    /// before the coupon date), accrued interest becomes negative.
    ExDividend,

    /// Record date convention.
    ///
    /// Similar to ex-dividend but based on a record date
    /// rather than a fixed number of days before payment.
    RecordDate,

    /// Cum-dividend until payment date.
    ///
    /// Buyer receives the full coupon if settling before
    /// payment date, regardless of accrued.
    CumDividend,
}

impl AccruedConvention {
    /// Returns true if this convention supports negative accrued.
    #[must_use]
    pub const fn can_be_negative(&self) -> bool {
        matches!(
            self,
            AccruedConvention::ExDividend | AccruedConvention::RecordDate
        )
    }

    /// Returns the typical ex-dividend period in business days.
    ///
    /// Only relevant for `ExDividend` convention.
    #[must_use]
    pub const fn ex_dividend_days(&self) -> Option<u32> {
        match self {
            AccruedConvention::ExDividend => Some(7), // UK standard
            _ => None,
        }
    }
}

impl std::fmt::Display for AccruedConvention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AccruedConvention::Standard => "Standard",
            AccruedConvention::None => "None (Zero Coupon)",
            AccruedConvention::ExDividend => "Ex-Dividend",
            AccruedConvention::RecordDate => "Record Date",
            AccruedConvention::CumDividend => "Cum-Dividend",
        };
        write!(f, "{s}")
    }
}

/// Rounding convention for yield calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum RoundingConvention {
    /// No rounding applied.
    #[default]
    None,

    /// Round to nearest basis point (0.01%).
    BasisPoint,

    /// Round to nearest half basis point (0.005%).
    HalfBasisPoint,

    /// Round to 3 decimal places (0.001%).
    ThreeDecimals,

    /// Truncate (always round down).
    Truncate,
}

impl RoundingConvention {
    /// Returns the number of decimal places for this convention.
    #[must_use]
    pub const fn decimal_places(&self) -> Option<u32> {
        match self {
            RoundingConvention::None => None,
            RoundingConvention::BasisPoint => Some(4),
            RoundingConvention::HalfBasisPoint => Some(5),
            RoundingConvention::ThreeDecimals => Some(5),
            RoundingConvention::Truncate => Some(4),
        }
    }
}

/// First-period discounting method for yield calculations.
///
/// This controls how the first (fractional) period is discounted
/// in yield calculations when using compounded methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum FirstPeriodDiscounting {
    /// Linear/simple discounting for first period (US Street Convention).
    ///
    /// DF = 1 / (1 + y × n / f)
    ///
    /// This is the SIFMA standard for US markets.
    #[default]
    Linear,

    /// Compound discounting for first period (ICMA/ISMA).
    ///
    /// DF = 1 / (1 + y/f)^n
    ///
    /// Used for Eurobonds and European government bonds.
    Compound,
}

impl FirstPeriodDiscounting {
    /// Returns true if this is the linear/simple method.
    #[must_use]
    pub const fn is_linear(&self) -> bool {
        matches!(self, FirstPeriodDiscounting::Linear)
    }

    /// Returns true if this is the compound method.
    #[must_use]
    pub const fn is_compound(&self) -> bool {
        matches!(self, FirstPeriodDiscounting::Compound)
    }
}

impl std::fmt::Display for FirstPeriodDiscounting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            FirstPeriodDiscounting::Linear => "Linear (Street)",
            FirstPeriodDiscounting::Compound => "Compound (ICMA)",
        };
        write!(f, "{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yield_method_default() {
        let method = YieldMethod::default();
        assert_eq!(method, YieldMethod::Compounded);
    }

    #[test]
    fn test_yield_method_is_simple() {
        assert!(!YieldMethod::Compounded.is_simple());
        assert!(YieldMethod::Simple.is_simple());
        assert!(YieldMethod::Discount.is_simple());
        assert!(!YieldMethod::AddOn.is_simple());
    }

    #[test]
    fn test_accrued_convention_default() {
        let conv = AccruedConvention::default();
        assert_eq!(conv, AccruedConvention::Standard);
    }

    #[test]
    fn test_accrued_convention_negative() {
        assert!(AccruedConvention::ExDividend.can_be_negative());
        assert!(AccruedConvention::RecordDate.can_be_negative());
        assert!(!AccruedConvention::Standard.can_be_negative());
        assert!(!AccruedConvention::None.can_be_negative());
    }

    #[test]
    fn test_accrued_convention_ex_dividend_days() {
        assert_eq!(AccruedConvention::ExDividend.ex_dividend_days(), Some(7));
        assert_eq!(AccruedConvention::Standard.ex_dividend_days(), None);
    }

    #[test]
    fn test_rounding_convention() {
        assert_eq!(RoundingConvention::None.decimal_places(), None);
        assert_eq!(RoundingConvention::BasisPoint.decimal_places(), Some(4));
        assert_eq!(RoundingConvention::ThreeDecimals.decimal_places(), Some(5));
    }

    #[test]
    fn test_first_period_discounting() {
        assert!(FirstPeriodDiscounting::Linear.is_linear());
        assert!(!FirstPeriodDiscounting::Linear.is_compound());
        assert!(!FirstPeriodDiscounting::Compound.is_linear());
        assert!(FirstPeriodDiscounting::Compound.is_compound());
    }
}
