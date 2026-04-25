//! # Convex Bonds
//!
//! Bond types, cashflow / schedule primitives, yield solving, and short-rate
//! option models. Pricing / risk analytics live on the `Bond` and
//! `BondAnalytics` traits (see `convex-analytics` for the functional wrappers).

#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::similar_names)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::if_not_else)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::struct_field_names)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(clippy::trivially_copy_pass_by_ref)]
#![allow(clippy::useless_conversion)]
#![allow(clippy::unused_self)]
#![allow(clippy::missing_fields_in_debug)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::single_match)]
#![allow(clippy::unnecessary_map_or)]
#![allow(clippy::float_cmp)]
#![allow(clippy::while_let_loop)]
#![allow(clippy::used_underscore_items)]
#![allow(clippy::borrowed_box)]
#![allow(dead_code)]

pub mod cashflows;
pub mod conventions;
pub mod error;
pub mod instruments;
pub mod options;
pub mod pricing;
pub mod traits;
pub mod types;

pub mod prelude {
    pub use crate::cashflows::{AccruedInterestCalculator, Schedule, ScheduleConfig, StubType};
    pub use crate::conventions::{BondConventions, BondConventionsBuilder};
    pub use crate::error::{BondError, BondResult, IdentifierError};
    pub use crate::instruments::{
        AccelerationOption, CallableBond, CallableBondBuilder, FixedRateBond, FixedRateBondBuilder,
        FloatingRateNote, FloatingRateNoteBuilder, SinkingFundBond, SinkingFundBondBuilder,
        SinkingFundPayment, SinkingFundSchedule, ZeroCouponBond,
    };
    pub use crate::options::{BinomialTree, HullWhite, ModelError, ShortRateModel};
    pub use crate::pricing::{current_yield, current_yield_from_bond, YieldResult, YieldSolver};
    pub use crate::traits::{
        AmortizingBond, Bond, BondAnalytics, BondCashFlow, CashFlowType, EmbeddedOptionBond,
        FixedCouponBond, FloatingCouponBond, InflationLinkedBond,
    };
    pub use crate::types::{
        AccruedConvention, AmortizationEntry, AmortizationSchedule, AmortizationType,
        BondIdentifiers, BondType, CalendarId, CallEntry, CallSchedule, CallType, Cusip, Figi,
        InflationIndexType, Isin, PriceQuote, PriceQuoteConvention, PutEntry, PutSchedule, PutType,
        RateIndex, RoundingConvention, Sedol, Tenor, YieldConvention,
    };
}

pub use error::{BondError, BondResult};
pub use instruments::{
    AccelerationOption, CallableBond, CallableBondBuilder, FixedRateBond, FixedRateBondBuilder,
    FloatingRateNote, FloatingRateNoteBuilder, SinkingFundBond, SinkingFundBondBuilder,
    SinkingFundPayment, SinkingFundSchedule,
};
