//! # Convex Spreads
//!
//! Spread analytics for the Convex fixed income analytics library.
//!
//! This crate provides various spread calculations commonly used in fixed income:
//!
//! - **G-Spread**: Spread over interpolated government yield curve
//! - **I-Spread**: Spread over swap rate curve
//! - **Z-Spread**: Zero-volatility spread (constant spread over spot curve)
//! - **OAS**: Option-adjusted spread (for bonds with embedded options)
//! - **Asset Swap Spread**: Spread in asset swap transactions
//!
//! ## Example
//!
//! ```rust,ignore
//! use convex_spreads::{ZSpread, SpreadCalculator};
//! use convex_bonds::FixedBond;
//! use convex_curves::ZeroCurve;
//!
//! let bond = // ... create bond
//! let curve = // ... create curve
//! let market_price = dec!(98.50);
//!
//! let z_spread = SpreadCalculator::z_spread(&bond, &curve, market_price, settlement)?;
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod asw;
pub mod benchmark;
pub mod error;
pub mod government_curve;
pub mod gspread;
pub mod ispread;
pub mod sovereign;
pub mod zspread;

pub use asw::{ASWType, ParParAssetSwap, ProceedsAssetSwap};
pub use benchmark::{BenchmarkSpec, SecurityId};
pub use error::{SpreadError, SpreadResult};
pub use government_curve::{GovernmentBenchmark, GovernmentCurve};
pub use gspread::{BenchmarkInfo, GSpreadCalculator, GSpreadResult, TreasuryBenchmark};
pub use sovereign::{Sovereign, SupranationalIssuer};
pub use zspread::ZSpreadCalculator;

use rust_decimal::Decimal;

use convex_core::types::{Date, Price, Spread};
use convex_curves::curves::ZeroCurve;
use convex_bonds::instruments::FixedBond;

/// Spread calculator providing various spread calculations.
pub struct SpreadCalculator;

impl SpreadCalculator {
    /// Calculates Z-spread for a bond.
    ///
    /// The Z-spread is the constant spread that, when added to all points on the
    /// spot rate curve, makes the present value of cash flows equal to the market price.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond to calculate spread for
    /// * `curve` - The zero/spot rate curve
    /// * `market_price` - Market clean price
    /// * `settlement` - Settlement date
    ///
    /// # Errors
    ///
    /// Returns `SpreadError` if the calculation fails to converge.
    pub fn z_spread(
        bond: &FixedBond,
        curve: &ZeroCurve,
        market_price: Price,
        settlement: Date,
    ) -> SpreadResult<Spread> {
        zspread::calculate(bond, curve, market_price, settlement)
    }

    /// Calculates G-spread for a bond.
    ///
    /// The G-spread is the spread over the interpolated government bond yield curve.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond to calculate spread for
    /// * `govt_curve` - Government bond yield curve
    /// * `bond_yield` - The bond's yield to maturity
    /// * `settlement` - Settlement date
    pub fn g_spread(
        bond: &FixedBond,
        govt_curve: &ZeroCurve,
        bond_yield: Decimal,
        settlement: Date,
    ) -> SpreadResult<Spread> {
        gspread::calculate(bond, govt_curve, bond_yield, settlement)
    }

    /// Calculates I-spread for a bond.
    ///
    /// The I-spread is the spread over the swap rate curve (LIBOR/SOFR).
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond to calculate spread for
    /// * `swap_curve` - Swap rate curve
    /// * `bond_yield` - The bond's yield to maturity
    /// * `settlement` - Settlement date
    pub fn i_spread(
        bond: &FixedBond,
        swap_curve: &ZeroCurve,
        bond_yield: Decimal,
        settlement: Date,
    ) -> SpreadResult<Spread> {
        ispread::calculate(bond, swap_curve, bond_yield, settlement)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_placeholder() {
        // Placeholder test
        assert!(true);
    }
}
