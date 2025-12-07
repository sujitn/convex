//! # Convex Bonds
//!
//! Bond pricing and analytics for the Convex fixed income analytics library.
//!
//! This crate provides:
//!
//! - **Instruments**: Fixed coupon bonds, zero coupon bonds, floating rate notes
//! - **Pricing**: Present value, clean/dirty price, yield-to-maturity
//! - **Cash Flows**: Coupon schedule generation with business day adjustments
//! - **Risk**: Duration, convexity, DV01, key rate durations
//!
//! ## Example
//!
//! ```rust,ignore
//! use convex_bonds::prelude::*;
//! use convex_core::types::{Date, Currency, Frequency};
//! use rust_decimal_macros::dec;
//!
//! // Create a fixed coupon bond
//! let bond = FixedBondBuilder::new()
//!     .isin("US912828Z229")
//!     .coupon_rate(dec!(2.5))
//!     .maturity(Date::from_ymd(2030, 5, 15).unwrap())
//!     .frequency(Frequency::SemiAnnual)
//!     .currency(Currency::USD)
//!     .build()
//!     .unwrap();
//!
//! // Calculate yield-to-maturity
//! let settlement = Date::from_ymd(2025, 1, 15).unwrap();
//! let price = Price::new(dec!(98.50), Currency::USD);
//! let ytm = bond.yield_to_maturity(price, settlement).unwrap();
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod cashflows;
pub mod conventions;
pub mod curve_instruments;
pub mod error;
pub mod indices;
pub mod instruments;
pub mod options;
pub mod pricing;
pub mod risk;
pub mod traits;
pub mod types;

/// Prelude module for convenient imports.
pub mod prelude {
    // Cash flows
    pub use crate::cashflows::{
        AccruedInterestCalculator, CashFlowGenerator, Schedule, ScheduleConfig, StubType,
    };

    // Conventions
    pub use crate::conventions::{BondConventions, BondConventionsBuilder};

    // Curve instruments
    pub use crate::curve_instruments::{
        GovernmentCouponBond, GovernmentZeroCoupon, MarketConvention, day_count_factor,
    };

    // Errors
    pub use crate::error::{BondError, BondResult, IdentifierError};

    // Indices (for FRN support)
    pub use crate::indices::{
        ArrearConvention, IndexConventions, IndexFixing, IndexFixingStore, IndexSource,
        OvernightCompounding, PublicationTime, ShiftType,
    };

    // Instruments
    pub use crate::instruments::{
        AccelerationOption, CallableBond, CallableBondBuilder, FixedBond, FixedBondBuilder,
        FixedRateBond, FixedRateBondBuilder, FloatingRateNote, FloatingRateNoteBuilder,
        SinkingFundBond, SinkingFundBondBuilder, SinkingFundPayment, SinkingFundSchedule,
        ZeroCouponBond,
    };

    // Pricing
    pub use crate::pricing::{
        current_yield, current_yield_from_bond, BondPricer, PriceResult, YieldResult, YieldSolver,
    };

    // Options (for OAS pricing)
    pub use crate::options::{BinomialTree, HullWhite, ModelError, ShortRateModel};

    // Risk
    pub use crate::risk::{DurationResult, RiskMetrics};

    // Traits
    pub use crate::traits::{
        AmortizingBond, Bond, BondAnalytics, BondCashFlow, CashFlowType, EmbeddedOptionBond,
        FixedCouponBond, FloatingCouponBond, InflationLinkedBond,
    };

    // Types
    pub use crate::types::{
        AccruedConvention, AmortizationEntry, AmortizationSchedule, AmortizationType,
        BondIdentifiers, BondType, CalendarId, CallEntry, CallSchedule, CallType, Cusip, Figi,
        InflationIndexType, Isin, PriceQuote, PriceQuoteConvention, PutEntry, PutSchedule, PutType,
        RateIndex, RoundingConvention, SOFRConvention, Sedol, Tenor, YieldConvention,
    };
}

pub use error::{BondError, BondResult};
pub use indices::{
    ArrearConvention, IndexConventions, IndexFixing, IndexFixingStore, IndexSource,
    OvernightCompounding, PublicationTime, ShiftType,
};
pub use instruments::{
    AccelerationOption, CallableBond, CallableBondBuilder, FixedBond, FixedBondBuilder,
    FixedRateBond, FixedRateBondBuilder, FloatingRateNote, FloatingRateNoteBuilder,
    SinkingFundBond, SinkingFundBondBuilder, SinkingFundPayment, SinkingFundSchedule,
};
