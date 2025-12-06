//! Yield and accrued interest conventions for bond analytics.
//!
//! This module defines the various conventions used for calculating yields
//! and accrued interest across different bond markets.

use serde::{Deserialize, Serialize};

/// Yield calculation convention.
///
/// Different markets and bond types use different conventions for
/// computing yields. This affects how cash flows are discounted and
/// how the yield is annualized.
///
/// # Example
///
/// ```rust
/// use convex_bonds::types::YieldConvention;
///
/// let convention = YieldConvention::StreetConvention;
/// assert!(convention.compounds_semi_annually());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum YieldConvention {
    /// US Street Convention (SIFMA).
    ///
    /// Standard US market convention:
    /// - Semi-annual compounding
    /// - Actual/Actual day count for Treasuries
    /// - 30/360 for corporates
    /// - Assumes coupon reinvestment at yield
    #[default]
    StreetConvention,

    /// True Yield (Academic/Theoretical).
    ///
    /// Uses actual cash flow timing without assuming
    /// periodic compounding. More accurate for bonds
    /// with irregular cash flows.
    TrueYield,

    /// ISMA (International Securities Market Association).
    ///
    /// Now ICMA (International Capital Market Association):
    /// - Annual compounding for most markets
    /// - Actual/Actual ICMA day count
    /// - Standard for Eurobonds
    ISMA,

    /// Simple Yield (Japanese convention).
    ///
    /// No compounding - simple interest calculation:
    /// ```text
    /// Simple Yield = (Annual Coupon + (100 - Price) / Years) / Price
    /// ```
    /// Used for JGBs and some Asian markets.
    SimpleYield,

    /// Discount Yield (Money market).
    ///
    /// Used for Treasury Bills and other discount instruments:
    /// ```text
    /// Discount Yield = (Face - Price) / Face * (360 / Days)
    /// ```
    DiscountYield,

    /// Bond Equivalent Yield.
    ///
    /// Converts discount yield to a semi-annual bond basis
    /// for comparison with coupon-bearing securities.
    BondEquivalentYield,

    /// Municipal Yield (Tax-equivalent).
    ///
    /// Adjusts yield for tax-exempt status:
    /// ```text
    /// Tax-Equivalent Yield = Municipal Yield / (1 - Tax Rate)
    /// ```
    MunicipalYield,

    /// Moosmüller Yield.
    ///
    /// German convention that differs from ISMA in the
    /// treatment of broken periods.
    Moosmuller,

    /// Braess-Fangmeyer Yield.
    ///
    /// Another German convention used for certain bond types.
    BraessFangmeyer,

    /// Annual Yield.
    ///
    /// Simple annual compounding convention.
    Annual,

    /// Continuous Yield.
    ///
    /// Continuous compounding (e^(rt)).
    /// Used in derivatives pricing and theoretical models.
    Continuous,
}

impl YieldConvention {
    /// Returns true if this convention uses semi-annual compounding.
    #[must_use]
    pub const fn compounds_semi_annually(&self) -> bool {
        matches!(
            self,
            YieldConvention::StreetConvention | YieldConvention::BondEquivalentYield
        )
    }

    /// Returns true if this convention uses annual compounding.
    #[must_use]
    pub const fn compounds_annually(&self) -> bool {
        matches!(
            self,
            YieldConvention::ISMA
                | YieldConvention::Moosmuller
                | YieldConvention::BraessFangmeyer
                | YieldConvention::Annual
        )
    }

    /// Returns true if this is a simple (non-compounding) yield.
    #[must_use]
    pub const fn is_simple(&self) -> bool {
        matches!(
            self,
            YieldConvention::SimpleYield | YieldConvention::DiscountYield
        )
    }

    /// Returns the compounding frequency per year.
    ///
    /// Returns `None` for simple yields and continuous compounding.
    #[must_use]
    pub const fn compounding_frequency(&self) -> Option<u32> {
        match self {
            YieldConvention::StreetConvention | YieldConvention::BondEquivalentYield => Some(2),
            YieldConvention::ISMA
            | YieldConvention::Moosmuller
            | YieldConvention::BraessFangmeyer
            | YieldConvention::Annual
            | YieldConvention::MunicipalYield => Some(1),
            YieldConvention::TrueYield => Some(2), // Default to semi-annual
            YieldConvention::SimpleYield
            | YieldConvention::DiscountYield
            | YieldConvention::Continuous => None,
        }
    }

    /// Returns the typical day count basis used with this convention.
    ///
    /// This is a hint; actual day count may vary by bond type.
    #[must_use]
    pub const fn typical_day_count(&self) -> &'static str {
        match self {
            YieldConvention::StreetConvention => "30/360 or Act/Act",
            YieldConvention::TrueYield => "Actual/Actual",
            YieldConvention::ISMA => "Actual/Actual ICMA",
            YieldConvention::SimpleYield => "Actual/365",
            YieldConvention::DiscountYield => "Actual/360",
            YieldConvention::BondEquivalentYield => "Actual/365",
            YieldConvention::MunicipalYield => "30/360",
            YieldConvention::Moosmuller => "Actual/Actual German",
            YieldConvention::BraessFangmeyer => "Actual/Actual German",
            YieldConvention::Annual => "Actual/365",
            YieldConvention::Continuous => "Actual/365",
        }
    }

    /// Returns the standard convention for a given market.
    #[must_use]
    pub const fn for_market(market: &str) -> Self {
        // Match on first 2 chars for efficiency
        match market.as_bytes() {
            [b'U', b'S', ..] => YieldConvention::StreetConvention,
            [b'U', b'K', ..] | [b'G', b'B', ..] => YieldConvention::ISMA,
            [b'D', b'E', ..] => YieldConvention::Moosmuller,
            [b'J', b'P', ..] => YieldConvention::SimpleYield,
            [b'E', b'U', ..] => YieldConvention::ISMA, // Eurobond
            _ => YieldConvention::ISMA,               // International default
        }
    }
}

impl std::fmt::Display for YieldConvention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            YieldConvention::StreetConvention => "Street Convention",
            YieldConvention::TrueYield => "True Yield",
            YieldConvention::ISMA => "ISMA/ICMA",
            YieldConvention::SimpleYield => "Simple Yield",
            YieldConvention::DiscountYield => "Discount Yield",
            YieldConvention::BondEquivalentYield => "Bond Equivalent Yield",
            YieldConvention::MunicipalYield => "Municipal Yield",
            YieldConvention::Moosmuller => "Moosmüller",
            YieldConvention::BraessFangmeyer => "Braess-Fangmeyer",
            YieldConvention::Annual => "Annual",
            YieldConvention::Continuous => "Continuous",
        };
        write!(f, "{}", s)
    }
}

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
    /// Only relevant for ExDividend convention.
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
        write!(f, "{}", s)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yield_convention_default() {
        let conv: YieldConvention = Default::default();
        assert_eq!(conv, YieldConvention::StreetConvention);
    }

    #[test]
    fn test_yield_convention_compounding() {
        assert!(YieldConvention::StreetConvention.compounds_semi_annually());
        assert!(!YieldConvention::StreetConvention.compounds_annually());

        assert!(YieldConvention::ISMA.compounds_annually());
        assert!(!YieldConvention::ISMA.compounds_semi_annually());

        assert!(YieldConvention::SimpleYield.is_simple());
        assert!(YieldConvention::DiscountYield.is_simple());
        assert!(!YieldConvention::StreetConvention.is_simple());
    }

    #[test]
    fn test_yield_convention_frequency() {
        assert_eq!(
            YieldConvention::StreetConvention.compounding_frequency(),
            Some(2)
        );
        assert_eq!(YieldConvention::ISMA.compounding_frequency(), Some(1));
        assert_eq!(YieldConvention::SimpleYield.compounding_frequency(), None);
        assert_eq!(YieldConvention::Continuous.compounding_frequency(), None);
    }

    #[test]
    fn test_yield_convention_for_market() {
        assert_eq!(
            YieldConvention::for_market("US"),
            YieldConvention::StreetConvention
        );
        assert_eq!(YieldConvention::for_market("UK"), YieldConvention::ISMA);
        assert_eq!(YieldConvention::for_market("GB"), YieldConvention::ISMA);
        assert_eq!(
            YieldConvention::for_market("DE"),
            YieldConvention::Moosmuller
        );
        assert_eq!(
            YieldConvention::for_market("JP"),
            YieldConvention::SimpleYield
        );
        assert_eq!(YieldConvention::for_market("EU"), YieldConvention::ISMA);
        assert_eq!(YieldConvention::for_market("FR"), YieldConvention::ISMA);
    }

    #[test]
    fn test_yield_convention_display() {
        assert_eq!(
            format!("{}", YieldConvention::StreetConvention),
            "Street Convention"
        );
        assert_eq!(format!("{}", YieldConvention::ISMA), "ISMA/ICMA");
        assert_eq!(format!("{}", YieldConvention::SimpleYield), "Simple Yield");
    }

    #[test]
    fn test_accrued_convention_default() {
        let conv: AccruedConvention = Default::default();
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
}
