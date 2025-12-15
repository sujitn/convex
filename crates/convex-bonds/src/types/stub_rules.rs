//! Stub period rules for irregular coupon handling.
//!
//! This module defines how irregular (stub) periods are handled in yield
//! calculations. Different markets and conventions treat odd first/last
//! coupon periods differently.

use serde::{Deserialize, Serialize};

/// Rules for handling irregular (stub) coupon periods.
///
/// When a bond has an irregular first or last coupon period (e.g., a bond
/// issued mid-month), different conventions apply for calculating yields
/// and accrued interest.
///
/// # Example
///
/// ```rust
/// use convex_bonds::types::{StubPeriodRules, StubType, ReferenceMethod};
///
/// // ICMA convention for Eurobonds
/// let icma_rules = StubPeriodRules {
///     first_period: StubType::ShortStub,
///     last_period: StubType::None,
///     reference_method: ReferenceMethod::ICMA,
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StubPeriodRules {
    /// How to handle the first (possibly irregular) period.
    pub first_period: StubType,
    /// How to handle the last (possibly irregular) period.
    pub last_period: StubType,
    /// Method for calculating the reference period.
    pub reference_method: ReferenceMethod,
}

impl StubPeriodRules {
    /// Creates rules for a regular bond (no stubs).
    #[must_use]
    pub const fn regular() -> Self {
        Self {
            first_period: StubType::None,
            last_period: StubType::None,
            reference_method: ReferenceMethod::ICMA,
        }
    }

    /// Creates ICMA standard rules (short first stub).
    #[must_use]
    pub const fn icma() -> Self {
        Self {
            first_period: StubType::ShortStub,
            last_period: StubType::None,
            reference_method: ReferenceMethod::ICMA,
        }
    }

    /// Creates ISDA standard rules.
    #[must_use]
    pub const fn isda() -> Self {
        Self {
            first_period: StubType::ShortStub,
            last_period: StubType::None,
            reference_method: ReferenceMethod::ISDA,
        }
    }

    /// Creates Bloomberg standard rules.
    #[must_use]
    pub const fn bloomberg() -> Self {
        Self {
            first_period: StubType::ShortStub,
            last_period: StubType::None,
            reference_method: ReferenceMethod::Bloomberg,
        }
    }

    /// Creates rules for a long first coupon period.
    #[must_use]
    pub const fn long_first() -> Self {
        Self {
            first_period: StubType::LongStub,
            last_period: StubType::None,
            reference_method: ReferenceMethod::ICMA,
        }
    }

    /// Creates rules for a short last coupon period.
    #[must_use]
    pub const fn short_last() -> Self {
        Self {
            first_period: StubType::None,
            last_period: StubType::ShortStub,
            reference_method: ReferenceMethod::ICMA,
        }
    }

    /// Returns true if the bond has a short first period.
    #[must_use]
    pub const fn has_short_first(&self) -> bool {
        matches!(self.first_period, StubType::ShortStub)
    }

    /// Returns true if the bond has a long first period.
    #[must_use]
    pub const fn has_long_first(&self) -> bool {
        matches!(self.first_period, StubType::LongStub)
    }

    /// Returns true if any irregular period handling is needed.
    #[must_use]
    pub const fn has_irregular_period(&self) -> bool {
        !matches!(self.first_period, StubType::None)
            || !matches!(self.last_period, StubType::None)
    }
}

impl Default for StubPeriodRules {
    fn default() -> Self {
        Self::icma()
    }
}

/// Type of stub (irregular) period.
///
/// Defines how the day count fraction is calculated for periods
/// shorter or longer than the regular coupon period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum StubType {
    /// Short stub: actual days in stub / reference period days.
    ///
    /// Used when the first/last period is shorter than normal.
    /// Example: Bond issued on Jan 15 with Jun/Dec coupons has
    /// a short first period from Jan 15 to Jun 15.
    ShortStub,

    /// Long stub: actual days / actual days in extended period.
    ///
    /// Used when the first/last period is longer than normal.
    /// The coupon accrues over multiple notional periods.
    LongStub,

    /// Linear interpolation between regular periods.
    ///
    /// Some markets interpolate the discount factor for stub periods
    /// rather than using a modified day count fraction.
    Interpolated,

    /// No special handling (regular period).
    #[default]
    None,
}

impl StubType {
    /// Returns true if this is a short stub.
    #[must_use]
    pub const fn is_short(&self) -> bool {
        matches!(self, Self::ShortStub)
    }

    /// Returns true if this is a long stub.
    #[must_use]
    pub const fn is_long(&self) -> bool {
        matches!(self, Self::LongStub)
    }

    /// Returns true if this is a regular (non-stub) period.
    #[must_use]
    pub const fn is_regular(&self) -> bool {
        matches!(self, Self::None)
    }
}

impl std::fmt::Display for StubType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShortStub => write!(f, "Short Stub"),
            Self::LongStub => write!(f, "Long Stub"),
            Self::Interpolated => write!(f, "Interpolated"),
            Self::None => write!(f, "Regular"),
        }
    }
}

/// Method for calculating the reference period for stub handling.
///
/// Different standards define the notional regular period differently
/// when calculating day count fractions for irregular periods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ReferenceMethod {
    /// ICMA standard: Use notional regular period.
    ///
    /// The reference period is a hypothetical regular period that would
    /// exist if the bond had regular coupons. For a short first stub,
    /// this is the period from the notional previous coupon date to
    /// the first actual coupon date.
    ///
    /// Standard for Eurobonds and most international markets.
    #[default]
    ICMA,

    /// ISDA standard: Use preceding/following regular period.
    ///
    /// The reference period is the actual preceding or following
    /// regular coupon period. Used in swap calculations.
    ISDA,

    /// Bloomberg methodology: Specific stub handling rules.
    ///
    /// Bloomberg uses proprietary rules that may differ from ICMA/ISDA
    /// in edge cases. Used for Bloomberg YAS replication.
    Bloomberg,

    /// US Municipal: 30/360 with specific stub rules.
    ///
    /// US municipals use 30/360 day count with market-specific
    /// rules for irregular periods.
    USMunicipal,

    /// Japanese convention: Simple interest for stub periods.
    ///
    /// JGBs and some Asian bonds use simple interest calculations
    /// for stub periods rather than compounded day fractions.
    Japanese,
}

impl ReferenceMethod {
    /// Returns the name of this reference method.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::ICMA => "ICMA",
            Self::ISDA => "ISDA",
            Self::Bloomberg => "Bloomberg",
            Self::USMunicipal => "US Municipal",
            Self::Japanese => "Japanese",
        }
    }
}

impl std::fmt::Display for ReferenceMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Position of the stub period in the bond's life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StubPosition {
    /// Stub at the beginning (first coupon period).
    First,
    /// Stub at the end (last coupon period).
    Last,
    /// Both first and last periods are irregular.
    Both,
    /// No stub periods (regular bond).
    None,
}

impl StubPosition {
    /// Returns true if there's a first period stub.
    #[must_use]
    pub const fn has_first(&self) -> bool {
        matches!(self, Self::First | Self::Both)
    }

    /// Returns true if there's a last period stub.
    #[must_use]
    pub const fn has_last(&self) -> bool {
        matches!(self, Self::Last | Self::Both)
    }

    /// Returns true if the bond is regular (no stubs).
    #[must_use]
    pub const fn is_regular(&self) -> bool {
        matches!(self, Self::None)
    }
}

impl Default for StubPosition {
    fn default() -> Self {
        Self::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_period_rules_default() {
        let rules = StubPeriodRules::default();
        assert_eq!(rules.first_period, StubType::ShortStub);
        assert_eq!(rules.last_period, StubType::None);
        assert_eq!(rules.reference_method, ReferenceMethod::ICMA);
    }

    #[test]
    fn test_stub_period_rules_regular() {
        let rules = StubPeriodRules::regular();
        assert!(!rules.has_irregular_period());
    }

    #[test]
    fn test_stub_period_rules_icma() {
        let rules = StubPeriodRules::icma();
        assert!(rules.has_short_first());
        assert!(!rules.has_long_first());
        assert!(rules.has_irregular_period());
    }

    #[test]
    fn test_stub_period_rules_long_first() {
        let rules = StubPeriodRules::long_first();
        assert!(rules.has_long_first());
        assert!(!rules.has_short_first());
    }

    #[test]
    fn test_stub_type_display() {
        assert_eq!(format!("{}", StubType::ShortStub), "Short Stub");
        assert_eq!(format!("{}", StubType::LongStub), "Long Stub");
        assert_eq!(format!("{}", StubType::None), "Regular");
    }

    #[test]
    fn test_reference_method_default() {
        assert_eq!(ReferenceMethod::default(), ReferenceMethod::ICMA);
    }

    #[test]
    fn test_stub_position() {
        assert!(StubPosition::First.has_first());
        assert!(!StubPosition::First.has_last());
        assert!(StubPosition::Both.has_first());
        assert!(StubPosition::Both.has_last());
        assert!(StubPosition::None.is_regular());
    }
}
