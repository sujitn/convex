//! Curve instruments for yield curve bootstrap.
//!
//! This module provides various financial instruments used to construct
//! yield curves through bootstrapping. Each instrument implements the
//! [`CurveInstrument`] trait, which provides a unified interface for
//! curve construction.
//!
//! # Available Instruments
//!
//! ## Money Market
//! - [`Deposit`]: Money market deposits (O/N, T/N, 1W, 1M, 3M, 6M, 12M)
//! - [`FRA`]: Forward Rate Agreements
//! - [`RateFuture`]: SOFR/Eurodollar futures
//!
//! ## Swaps
//! - [`Swap`]: Interest Rate Swaps (IRS)
//! - [`OIS`]: Overnight Index Swaps
//! - [`BasisSwap`]: Tenor and cross-currency basis swaps
//!
//! ## Government Securities
//! - [`TreasuryBill`]: Discount instruments (T-Bills)
//! - [`TreasuryBond`]: Coupon instruments (T-Notes, T-Bonds)
//!
//! # Generic Bootstrap
//!
//! The bootstrapper is generic and works with any mix of instruments:
//!
//! ```rust,ignore
//! let curve = CurveBuilder::new(settlement)
//!     .add(Deposit::new("3M", 0.0525))
//!     .add(TreasuryBill::new("6M", 99.50))
//!     .add(Swap::new("5Y", 0.0450))
//!     .add(TreasuryBond::new("10Y", 0.0410, 98.00))
//!     .bootstrap()?;
//! ```

mod basis_swap;
mod deposit;
mod fra;
mod future;
mod ois;
pub mod quotes;
mod swap;
mod tbill;
mod tbond;

pub use basis_swap::BasisSwap;
pub use deposit::Deposit;
pub use fra::FRA;
pub use future::{imm_date, next_imm_dates, FutureType, RateFuture};
pub use ois::OIS;
pub use quotes::{
    BondQuoteType, MarketQuote, QuoteType, QuoteValidationConfig, RateQuoteType,
    futures_price_to_rate, rate_to_futures_price, validate_market_data, validate_quote,
};
pub use swap::Swap;
pub use tbill::TreasuryBill;
pub use tbond::{CashFlow, TreasuryBond};

use convex_core::Date;

use crate::error::CurveResult;
use crate::traits::Curve;

/// Instrument type for categorization and sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InstrumentType {
    /// Money market deposit (shortest maturity first)
    Deposit = 0,
    /// Forward Rate Agreement
    FRA = 1,
    /// Rate future (SOFR, Eurodollar)
    Future = 2,
    /// Interest Rate Swap
    Swap = 3,
    /// Overnight Index Swap
    OIS = 4,
    /// Basis swap (tenor or cross-currency)
    BasisSwap = 5,
    /// Treasury Bill (discount instrument) - US-specific
    TreasuryBill = 6,
    /// Treasury Note/Bond (coupon instrument) - US-specific
    TreasuryBond = 7,
    /// Generic zero-coupon government bond (any market)
    GovernmentZeroCoupon = 8,
    /// Generic coupon government bond (any market: Gilts, Bunds, JGBs, etc.)
    GovernmentCouponBond = 9,
}

impl std::fmt::Display for InstrumentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deposit => write!(f, "Deposit"),
            Self::FRA => write!(f, "FRA"),
            Self::Future => write!(f, "Future"),
            Self::Swap => write!(f, "Swap"),
            Self::OIS => write!(f, "OIS"),
            Self::BasisSwap => write!(f, "BasisSwap"),
            Self::TreasuryBill => write!(f, "T-Bill"),
            Self::TreasuryBond => write!(f, "T-Bond"),
            Self::GovernmentZeroCoupon => write!(f, "Gov Zero"),
            Self::GovernmentCouponBond => write!(f, "Gov Bond"),
        }
    }
}

/// A rate index reference (e.g., SOFR, EURIBOR).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RateIndex {
    /// Index name (e.g., "SOFR", "EURIBOR")
    pub name: String,
    /// Tenor in months (e.g., 3 for 3M SOFR)
    pub tenor_months: u32,
    /// Day count convention name
    pub day_count: String,
}

impl RateIndex {
    /// Creates a new rate index.
    #[must_use]
    pub fn new(name: impl Into<String>, tenor_months: u32, day_count: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tenor_months,
            day_count: day_count.into(),
        }
    }

    /// Creates SOFR overnight index.
    #[must_use]
    pub fn sofr() -> Self {
        Self::new("SOFR", 0, "ACT/360")
    }

    /// Creates 1M SOFR index.
    #[must_use]
    pub fn sofr_1m() -> Self {
        Self::new("SOFR", 1, "ACT/360")
    }

    /// Creates 3M SOFR index.
    #[must_use]
    pub fn sofr_3m() -> Self {
        Self::new("SOFR", 3, "ACT/360")
    }

    /// Creates 3M EURIBOR index.
    #[must_use]
    pub fn euribor_3m() -> Self {
        Self::new("EURIBOR", 3, "ACT/360")
    }

    /// Creates 6M EURIBOR index.
    #[must_use]
    pub fn euribor_6m() -> Self {
        Self::new("EURIBOR", 6, "ACT/360")
    }

    /// Creates SONIA overnight index.
    #[must_use]
    pub fn sonia() -> Self {
        Self::new("SONIA", 0, "ACT/365F")
    }

    /// Creates ESTR overnight index.
    #[must_use]
    pub fn estr() -> Self {
        Self::new("ESTR", 0, "ACT/360")
    }

    /// Returns the tenor in years.
    #[must_use]
    pub fn tenor_years(&self) -> f64 {
        self.tenor_months as f64 / 12.0
    }
}

impl std::fmt::Display for RateIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.tenor_months == 0 {
            write!(f, "{}", self.name)
        } else {
            write!(f, "{} {}M", self.name, self.tenor_months)
        }
    }
}

/// Trait for curve instruments used in bootstrap.
///
/// All instruments that can be used to construct a yield curve implement
/// this trait. The trait provides methods for:
/// - Determining pillar dates for the curve
/// - Calculating present value given a curve
/// - Computing implied discount factors for sequential bootstrap
///
/// # Implementation Notes
///
/// - `maturity()` returns the final cash flow date
/// - `pillar_date()` returns the date where the discount factor is solved
/// - `pv()` should return ~0 when the curve is correctly calibrated
/// - `implied_df()` is used by sequential bootstrap to solve for DF at pillar
///
/// # Example
///
/// ```rust,ignore
/// struct MyInstrument { /* ... */ }
///
/// impl CurveInstrument for MyInstrument {
///     fn maturity(&self) -> Date { /* ... */ }
///     fn pillar_date(&self) -> Date { self.maturity() }
///     fn pv(&self, curve: &dyn Curve) -> CurveResult<f64> { /* ... */ }
///     fn implied_df(&self, curve: &dyn Curve, target_pv: f64) -> CurveResult<f64> { /* ... */ }
///     fn instrument_type(&self) -> InstrumentType { InstrumentType::Swap }
/// }
/// ```
pub trait CurveInstrument: Send + Sync {
    /// Returns the maturity date of the instrument.
    ///
    /// This is the final cash flow date.
    fn maturity(&self) -> Date;

    /// Returns the pillar date for curve construction.
    ///
    /// This is the date at which the discount factor will be solved.
    /// For most instruments, this equals `maturity()`.
    fn pillar_date(&self) -> Date {
        self.maturity()
    }

    /// Calculates the present value given a discount curve.
    ///
    /// For correctly calibrated curves, this should return approximately 0.
    ///
    /// # Arguments
    ///
    /// * `curve` - The discount curve to use for valuation
    ///
    /// # Returns
    ///
    /// The present value of the instrument (positive means asset value).
    fn pv(&self, curve: &dyn Curve) -> CurveResult<f64>;

    /// Computes the implied discount factor at the pillar date.
    ///
    /// Used by sequential bootstrap to solve for the unknown discount factor.
    ///
    /// # Arguments
    ///
    /// * `curve` - The partially-built curve (with known DFs up to this pillar)
    /// * `target_pv` - Target present value (usually 0)
    ///
    /// # Returns
    ///
    /// The discount factor at `pillar_date()` that makes `pv() = target_pv`.
    fn implied_df(&self, curve: &dyn Curve, target_pv: f64) -> CurveResult<f64>;

    /// Returns the instrument type for categorization and sorting.
    fn instrument_type(&self) -> InstrumentType;

    /// Returns a description string for debugging.
    fn description(&self) -> String {
        format!("{} maturing {}", self.instrument_type(), self.maturity())
    }
}

/// Helper function to calculate year fraction using ACT/365 Fixed.
pub fn year_fraction_act365(start: Date, end: Date) -> f64 {
    start.days_between(&end) as f64 / 365.0
}

/// Helper function to calculate year fraction using ACT/360.
pub fn year_fraction_act360(start: Date, end: Date) -> f64 {
    start.days_between(&end) as f64 / 360.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instrument_type_ordering() {
        // Deposits should sort before FRAs, which sort before Swaps
        assert!(InstrumentType::Deposit < InstrumentType::FRA);
        assert!(InstrumentType::FRA < InstrumentType::Swap);
        assert!(InstrumentType::Swap < InstrumentType::TreasuryBond);
    }

    #[test]
    fn test_rate_index() {
        let sofr = RateIndex::sofr_3m();
        assert_eq!(sofr.name, "SOFR");
        assert_eq!(sofr.tenor_months, 3);
        assert_eq!(format!("{}", sofr), "SOFR 3M");
    }

    #[test]
    fn test_rate_index_overnight() {
        let sofr = RateIndex::sofr();
        assert_eq!(sofr.tenor_months, 0);
        assert_eq!(format!("{}", sofr), "SOFR");
    }
}
