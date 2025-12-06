//! Asset Swap Spread (ASW) calculations.
//!
//! This module provides asset swap spread calculations commonly used in fixed income:
//!
//! - **Par-Par ASW**: Exchange bond at par, spread compensates for price difference
//! - **Proceeds ASW**: Swap notional equals bond market value
//! - **Market Value ASW**: Similar to proceeds with different conventions
//!
//! # Overview
//!
//! An asset swap packages a bond with an interest rate swap, converting fixed
//! coupons to floating rate payments. The asset swap spread (ASW) is the spread
//! over the floating rate that makes the package trade at par.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_spreads::asw::{ParParAssetSwap, ASWType};
//! use convex_curves::ZeroCurve;
//!
//! let asw_calc = ParParAssetSwap::new(&swap_curve);
//! let spread = asw_calc.calculate(&bond, clean_price, settlement)?;
//! println!("Par-Par ASW: {} bps", spread.as_bps());
//! ```

mod par_par;
mod proceeds;

pub use par_par::ParParAssetSwap;
pub use proceeds::ProceedsAssetSwap;

use serde::{Deserialize, Serialize};

/// Asset swap spread types.
///
/// Different conventions for calculating asset swap spreads, each with
/// different economics and use cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ASWType {
    /// Par-par asset swap: exchange bond at par, spread compensates.
    ///
    /// In a par-par swap:
    /// - Investor pays par (100) for the bond regardless of market price
    /// - Receives bond coupons
    /// - Pays fixed rate equal to bond coupon to swap counterparty
    /// - Receives floating + spread
    ///
    /// The spread compensates for the difference between par and market price.
    /// This is the most common convention for investment-grade bonds.
    ParPar,

    /// Market value asset swap: notional equals market value of bond.
    ///
    /// In a market value swap:
    /// - Investor pays market price for the bond
    /// - Swap notional equals the bond's market value
    /// - More accurate hedge but introduces basis risk
    MarketValue,

    /// Proceeds asset swap: swap notional equals bond proceeds.
    ///
    /// Similar to par-par but with adjustment for funding cost:
    /// - Spread is adjusted for the difference between par and dirty price
    /// - More commonly used in structured products
    Proceeds,
}

impl ASWType {
    /// Returns the description of this asset swap type.
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::ParPar => "Par-Par Asset Swap",
            Self::MarketValue => "Market Value Asset Swap",
            Self::Proceeds => "Proceeds Asset Swap",
        }
    }

    /// Returns true if this type uses par notional.
    #[must_use]
    pub fn uses_par_notional(&self) -> bool {
        matches!(self, Self::ParPar)
    }

    /// Returns true if this type uses market value notional.
    #[must_use]
    pub fn uses_market_notional(&self) -> bool {
        matches!(self, Self::MarketValue | Self::Proceeds)
    }
}

impl std::fmt::Display for ASWType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl Default for ASWType {
    fn default() -> Self {
        Self::ParPar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asw_type_description() {
        assert_eq!(ASWType::ParPar.description(), "Par-Par Asset Swap");
        assert_eq!(ASWType::MarketValue.description(), "Market Value Asset Swap");
        assert_eq!(ASWType::Proceeds.description(), "Proceeds Asset Swap");
    }

    #[test]
    fn test_asw_type_notional() {
        assert!(ASWType::ParPar.uses_par_notional());
        assert!(!ASWType::ParPar.uses_market_notional());

        assert!(!ASWType::MarketValue.uses_par_notional());
        assert!(ASWType::MarketValue.uses_market_notional());

        assert!(!ASWType::Proceeds.uses_par_notional());
        assert!(ASWType::Proceeds.uses_market_notional());
    }

    #[test]
    fn test_asw_type_display() {
        assert_eq!(format!("{}", ASWType::ParPar), "Par-Par Asset Swap");
    }

    #[test]
    fn test_asw_type_default() {
        assert_eq!(ASWType::default(), ASWType::ParPar);
    }
}
