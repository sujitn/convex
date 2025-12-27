//! Sector classification for fixed income securities.
//!
//! This module provides issuer sector classification:
//!
//! - [`Sector`]: Normalized sector categories for fixed income markets

use serde::{Deserialize, Serialize};

/// Normalized sector for analytics.
///
/// These sectors cover the primary fixed income market segments without
/// requiring any data license. For detailed hierarchical classifications
/// (BICS, GICS, ICB), see the portfolio module's `SectorInfo` type.
///
/// # Examples
///
/// ```
/// use convex_bonds::types::Sector;
///
/// let sector = Sector::Government;
/// assert!(sector.is_government_related());
/// assert!(!sector.is_securitized());
///
/// let mbs = Sector::MortgageBacked;
/// assert!(mbs.is_securitized());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Sector {
    /// Sovereign government bonds
    Government,
    /// Government agency and GSE bonds
    Agency,
    /// Corporate bonds (non-financial)
    Corporate,
    /// Financial institution bonds
    Financial,
    /// Utility company bonds
    Utility,
    /// Municipal bonds
    Municipal,
    /// Supranational issuer bonds (World Bank, etc.)
    Supranational,
    /// Asset-backed securities
    AssetBacked,
    /// Mortgage-backed securities
    MortgageBacked,
    /// Covered bonds
    CoveredBond,
    /// Other or unclassified
    #[default]
    Other,
}

impl Sector {
    /// Returns all sectors in a standard order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::Government,
            Self::Agency,
            Self::Corporate,
            Self::Financial,
            Self::Utility,
            Self::Municipal,
            Self::Supranational,
            Self::AssetBacked,
            Self::MortgageBacked,
            Self::CoveredBond,
            Self::Other,
        ]
    }

    /// Returns a human-readable name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Government => "Government",
            Self::Agency => "Agency",
            Self::Corporate => "Corporate",
            Self::Financial => "Financial",
            Self::Utility => "Utility",
            Self::Municipal => "Municipal",
            Self::Supranational => "Supranational",
            Self::AssetBacked => "Asset-Backed",
            Self::MortgageBacked => "Mortgage-Backed",
            Self::CoveredBond => "Covered Bond",
            Self::Other => "Other",
        }
    }

    /// Returns a short code for the sector.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Government => "GOVT",
            Self::Agency => "AGCY",
            Self::Corporate => "CORP",
            Self::Financial => "FIN",
            Self::Utility => "UTIL",
            Self::Municipal => "MUNI",
            Self::Supranational => "SUPRA",
            Self::AssetBacked => "ABS",
            Self::MortgageBacked => "MBS",
            Self::CoveredBond => "CVB",
            Self::Other => "OTH",
        }
    }

    /// Returns true if this is a government or quasi-government sector.
    #[must_use]
    pub fn is_government_related(&self) -> bool {
        matches!(self, Self::Government | Self::Agency | Self::Supranational)
    }

    /// Returns true if this is a securitized sector.
    #[must_use]
    pub fn is_securitized(&self) -> bool {
        matches!(
            self,
            Self::AssetBacked | Self::MortgageBacked | Self::CoveredBond
        )
    }

    /// Returns true if this is a credit sector (has credit spread).
    #[must_use]
    pub fn is_credit(&self) -> bool {
        matches!(
            self,
            Self::Corporate | Self::Financial | Self::Utility | Self::Municipal
        )
    }

    /// Returns typical spread volatility category (for risk purposes).
    ///
    /// Lower values indicate more stable spreads.
    #[must_use]
    pub fn spread_volatility_rank(&self) -> u8 {
        match self {
            Self::Government => 1,
            Self::Agency | Self::Supranational => 2,
            Self::CoveredBond => 3,
            Self::Municipal => 4,
            Self::MortgageBacked => 5,
            Self::Utility => 6,
            Self::AssetBacked => 7,
            Self::Corporate => 8,
            Self::Financial => 9,
            Self::Other => 10,
        }
    }
}

impl std::fmt::Display for Sector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sector_basics() {
        assert_eq!(Sector::Government.name(), "Government");
        assert_eq!(Sector::Government.code(), "GOVT");
        assert!(Sector::Government.is_government_related());
        assert!(!Sector::Corporate.is_government_related());
    }

    #[test]
    fn test_sector_securitized() {
        assert!(Sector::MortgageBacked.is_securitized());
        assert!(Sector::AssetBacked.is_securitized());
        assert!(Sector::CoveredBond.is_securitized());
        assert!(!Sector::Corporate.is_securitized());
    }

    #[test]
    fn test_sector_credit() {
        assert!(Sector::Corporate.is_credit());
        assert!(Sector::Financial.is_credit());
        assert!(!Sector::Government.is_credit());
        assert!(!Sector::MortgageBacked.is_credit());
    }

    #[test]
    fn test_sector_all() {
        let all = Sector::all();
        assert_eq!(all.len(), 11);
        assert_eq!(all[0], Sector::Government);
        assert_eq!(all[10], Sector::Other);
    }

    #[test]
    fn test_sector_display() {
        assert_eq!(format!("{}", Sector::Government), "Government");
        assert_eq!(format!("{}", Sector::MortgageBacked), "Mortgage-Backed");
    }

    #[test]
    fn test_sector_default() {
        assert_eq!(Sector::default(), Sector::Other);
    }

    #[test]
    fn test_spread_volatility_rank() {
        assert!(
            Sector::Government.spread_volatility_rank()
                < Sector::Corporate.spread_volatility_rank()
        );
        assert!(
            Sector::Agency.spread_volatility_rank() < Sector::Financial.spread_volatility_rank()
        );
    }

    #[test]
    fn test_serde() {
        let sector = Sector::Financial;
        let json = serde_json::to_string(&sector).unwrap();
        let parsed: Sector = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, sector);
    }
}
