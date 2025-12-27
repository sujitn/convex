//! Seniority/capital structure classification for fixed income securities.
//!
//! This module provides debt seniority classification:
//!
//! - [`Seniority`]: Capital structure position from senior secured to equity

use serde::{Deserialize, Serialize};

/// Normalized seniority for analytics.
///
/// Ordered from most senior (lowest risk) to most junior (highest risk).
/// This ordering affects recovery rate assumptions and spread expectations.
///
/// # Examples
///
/// ```
/// use convex_bonds::types::Seniority;
///
/// let senior = Seniority::SeniorSecured;
/// assert!(!senior.is_bailin_eligible());
/// assert_eq!(senior.typical_recovery(), 0.60);
///
/// let sub = Seniority::Subordinated;
/// assert!(sub.is_bailin_eligible());
/// assert!(sub > senior); // More junior
/// ```
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum Seniority {
    /// Secured by collateral
    SeniorSecured = 1,
    /// Unsecured senior debt
    #[default]
    SeniorUnsecured = 2,
    /// EU MREL / Senior non-preferred
    SeniorNonPreferred = 3,
    /// Subordinated debt (Tier 2)
    Subordinated = 4,
    /// Junior subordinated
    JuniorSubordinated = 5,
    /// AT1, CoCo, Preferred
    Hybrid = 6,
    /// Equity or equity-like
    Equity = 7,
}

impl Seniority {
    /// Returns all seniority levels in order (most senior first).
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::SeniorSecured,
            Self::SeniorUnsecured,
            Self::SeniorNonPreferred,
            Self::Subordinated,
            Self::JuniorSubordinated,
            Self::Hybrid,
            Self::Equity,
        ]
    }

    /// Returns a human-readable name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::SeniorSecured => "Senior Secured",
            Self::SeniorUnsecured => "Senior Unsecured",
            Self::SeniorNonPreferred => "Senior Non-Preferred",
            Self::Subordinated => "Subordinated",
            Self::JuniorSubordinated => "Junior Subordinated",
            Self::Hybrid => "Hybrid/AT1",
            Self::Equity => "Equity",
        }
    }

    /// Returns a short code for the seniority.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::SeniorSecured => "SR_SEC",
            Self::SeniorUnsecured => "SR_UNSEC",
            Self::SeniorNonPreferred => "SR_NP",
            Self::Subordinated => "SUB",
            Self::JuniorSubordinated => "JR_SUB",
            Self::Hybrid => "HYB",
            Self::Equity => "EQ",
        }
    }

    /// Returns typical recovery rate assumption.
    ///
    /// These are market-standard assumptions for loss-given-default calculations.
    /// Actual recovery rates vary significantly by jurisdiction and circumstances.
    #[must_use]
    pub fn typical_recovery(&self) -> f64 {
        match self {
            Self::SeniorSecured => 0.60,
            Self::SeniorUnsecured => 0.40,
            Self::SeniorNonPreferred => 0.35,
            Self::Subordinated => 0.20,
            Self::JuniorSubordinated => 0.10,
            Self::Hybrid => 0.05,
            Self::Equity => 0.0,
        }
    }

    /// Returns true if this is bail-inable under BRRD/TLAC frameworks.
    ///
    /// Under EU BRRD and global TLAC requirements, certain debt classes
    /// can be written down or converted to equity in resolution.
    #[must_use]
    pub fn is_bailin_eligible(&self) -> bool {
        matches!(
            self,
            Self::SeniorNonPreferred
                | Self::Subordinated
                | Self::JuniorSubordinated
                | Self::Hybrid
                | Self::Equity
        )
    }

    /// Returns true if this is a senior tranche (secured or unsecured).
    #[must_use]
    pub fn is_senior(&self) -> bool {
        matches!(
            self,
            Self::SeniorSecured | Self::SeniorUnsecured | Self::SeniorNonPreferred
        )
    }

    /// Returns true if this is subordinated (includes all sub-senior debt).
    #[must_use]
    pub fn is_subordinated(&self) -> bool {
        matches!(
            self,
            Self::Subordinated | Self::JuniorSubordinated | Self::Hybrid
        )
    }

    /// Returns true if this is a regulatory capital instrument (AT1/T2).
    #[must_use]
    pub fn is_capital_instrument(&self) -> bool {
        matches!(self, Self::Subordinated | Self::Hybrid)
    }

    /// Returns the typical spread premium over senior unsecured (in basis points).
    ///
    /// These are rough market conventions; actual spreads vary by issuer and market.
    #[must_use]
    pub fn typical_spread_premium_bps(&self) -> u32 {
        match self {
            Self::SeniorSecured => 0,   // Often tighter than unsecured
            Self::SeniorUnsecured => 0, // Base case
            Self::SeniorNonPreferred => 25,
            Self::Subordinated => 75,
            Self::JuniorSubordinated => 125,
            Self::Hybrid => 200,
            Self::Equity => 0, // N/A
        }
    }
}

impl std::fmt::Display for Seniority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seniority_ordering() {
        assert!(Seniority::SeniorSecured < Seniority::SeniorUnsecured);
        assert!(Seniority::SeniorUnsecured < Seniority::Subordinated);
        assert!(Seniority::Subordinated < Seniority::Hybrid);
        assert!(Seniority::Hybrid < Seniority::Equity);
    }

    #[test]
    fn test_seniority_bailin() {
        assert!(!Seniority::SeniorSecured.is_bailin_eligible());
        assert!(!Seniority::SeniorUnsecured.is_bailin_eligible());
        assert!(Seniority::SeniorNonPreferred.is_bailin_eligible());
        assert!(Seniority::Subordinated.is_bailin_eligible());
        assert!(Seniority::Hybrid.is_bailin_eligible());
    }

    #[test]
    fn test_seniority_recovery() {
        assert_eq!(Seniority::SeniorSecured.typical_recovery(), 0.60);
        assert_eq!(Seniority::SeniorUnsecured.typical_recovery(), 0.40);
        assert_eq!(Seniority::Subordinated.typical_recovery(), 0.20);
        assert_eq!(Seniority::Equity.typical_recovery(), 0.0);
    }

    #[test]
    fn test_seniority_name_and_code() {
        assert_eq!(Seniority::SeniorSecured.name(), "Senior Secured");
        assert_eq!(Seniority::SeniorSecured.code(), "SR_SEC");
        assert_eq!(Seniority::Hybrid.name(), "Hybrid/AT1");
        assert_eq!(Seniority::Hybrid.code(), "HYB");
    }

    #[test]
    fn test_seniority_is_senior() {
        assert!(Seniority::SeniorSecured.is_senior());
        assert!(Seniority::SeniorUnsecured.is_senior());
        assert!(Seniority::SeniorNonPreferred.is_senior());
        assert!(!Seniority::Subordinated.is_senior());
    }

    #[test]
    fn test_seniority_is_subordinated() {
        assert!(!Seniority::SeniorSecured.is_subordinated());
        assert!(Seniority::Subordinated.is_subordinated());
        assert!(Seniority::JuniorSubordinated.is_subordinated());
        assert!(Seniority::Hybrid.is_subordinated());
    }

    #[test]
    fn test_seniority_is_capital() {
        assert!(!Seniority::SeniorUnsecured.is_capital_instrument());
        assert!(Seniority::Subordinated.is_capital_instrument()); // Tier 2
        assert!(Seniority::Hybrid.is_capital_instrument()); // AT1
    }

    #[test]
    fn test_seniority_all() {
        let all = Seniority::all();
        assert_eq!(all.len(), 7);
        assert_eq!(all[0], Seniority::SeniorSecured);
        assert_eq!(all[6], Seniority::Equity);
    }

    #[test]
    fn test_seniority_default() {
        assert_eq!(Seniority::default(), Seniority::SeniorUnsecured);
    }

    #[test]
    fn test_spread_premium() {
        assert_eq!(Seniority::SeniorUnsecured.typical_spread_premium_bps(), 0);
        assert!(
            Seniority::Subordinated.typical_spread_premium_bps()
                > Seniority::SeniorNonPreferred.typical_spread_premium_bps()
        );
        assert!(
            Seniority::Hybrid.typical_spread_premium_bps()
                > Seniority::Subordinated.typical_spread_premium_bps()
        );
    }

    #[test]
    fn test_serde() {
        let seniority = Seniority::Subordinated;
        let json = serde_json::to_string(&seniority).unwrap();
        let parsed: Seniority = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, seniority);
    }
}
