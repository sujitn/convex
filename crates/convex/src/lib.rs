//! # Convex
//!
//! Facade crate re-exporting the public API of:
//!
//! - [`convex_core`]    — domain types (`Date`, `Mark`, `Currency`, `Frequency`, `DayCountConvention`, …)
//! - [`convex_curves`]  — yield curves and bootstrapping
//! - [`convex_bonds`]   — bond instruments
//! - [`convex_analytics`] — pricing, spreads, risk
//!
//! Most consumers should depend on `convex` and import from this crate.
//! The internal split exists for compile-time parallelism and dependency
//! discipline; the facade collapses it for ergonomics.
//!
//! ## Example
//!
//! ```ignore
//! use convex::{Mark, PriceKind, FixedRateBond, price_from_mark, Frequency};
//! use rust_decimal_macros::dec;
//!
//! let bond = FixedRateBond::builder()
//!     .cusip_unchecked("AAPL10Y")
//!     .coupon_rate(dec!(0.0465))
//!     // ...
//!     .build()?;
//!
//! let mark = Mark::Price { value: dec!(99.5), kind: PriceKind::Clean };
//! let result = price_from_mark(&bond, settlement, &mark, None, Frequency::SemiAnnual)?;
//! ```

#![warn(missing_docs)]

// Re-export the underlying crates verbatim for callers that want to be explicit.
pub use convex_analytics;
pub use convex_bonds;
pub use convex_core;
pub use convex_curves;

// Flat re-exports for ergonomic usage.

pub use convex_core::daycounts::DayCountConvention;
pub use convex_core::error::{ConvexError, ConvexResult};
pub use convex_core::types::{
    CashFlow, CashFlowSchedule, CashFlowType, Compounding, Currency, Date, Frequency, Mark, Price,
    PriceKind, Spread, SpreadType, Yield,
};

pub use convex_curves::{
    CalibrationResult, CurveError, CurveResult, Deposit, DiscountCurve, DiscreteCurve, Fra,
    GlobalFitter, InstrumentSet, InstrumentType, InterpolationMethod, Ois, RateCurve, RateCurveDyn,
    Swap, ValueType, ZeroCurve,
};

pub use convex_bonds::instruments::{
    CallableBond, FixedRateBond, FloatingRateNote, ZeroCouponBond,
};
pub use convex_bonds::traits::{Bond, BondCashFlow, FixedCouponBond};
pub use convex_bonds::{BondError, BondResult};

pub use convex_analytics::error::{AnalyticsError, AnalyticsResult};
pub use convex_analytics::functions::{
    clean_price_from_yield, convexity, dirty_price_from_yield, dv01, macaulay_duration,
    modified_duration, yield_to_maturity,
};
pub use convex_analytics::pricing::{price_from_mark, PricingResult};
pub use convex_analytics::spreads::{
    GSpreadCalculator, ISpreadCalculator, OASCalculator, ZSpreadCalculator,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_imports_compile() {
        // Smoke test: every facade-exposed name is reachable.
        let _: Currency = Currency::USD;
        let _: Frequency = Frequency::SemiAnnual;
        let _: DayCountConvention = DayCountConvention::Thirty360US;
        let _ = PriceKind::Clean;
        let _: ZSpreadCalculator;
    }
}
