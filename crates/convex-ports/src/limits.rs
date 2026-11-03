//! Book limits (P14, D82–D84): what a limit bounds, and what its evaluation reports.
//!
//! A limit names one measure of one book, the member it bounds where the measure has
//! members (one instrument or group, or each of them), and a lower bound, an upper bound
//! or both, in the measure's unit. Amounts are in the book's reporting currency; there is
//! no FX. The evaluation lives in the engine (`convex_engine::limits`); these are the
//! types a limit set stores and a status reports.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::BTreeSet;

use convex_core::contracts::{ExactDecimal, FactorTenor, InstrumentId, ReferenceDataId};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::reference_repository::ReferenceError;

/// Limits in one book's set.
pub const MAX_BOOK_LIMITS: usize = 64;
/// Bytes in a group key that a limit names.
pub const MAX_GROUP_KEY_BYTES: usize = 128;
/// Instruments a reason names; its count is always complete.
pub const MAX_LISTED_INSTRUMENTS: usize = 16;
/// Bytes of an analyzer refusal a reason keeps.
pub const MAX_REASON_DETAIL_BYTES: usize = 256;

/// Identity of a limit, unique in its book's set.
pub type LimitId = ReferenceDataId;

/// What a group limit groups positions by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitGroup {
    /// The priced bond's `issuer_id`.
    Issuer,
    /// The priced bond's `country_of_risk`.
    Country,
    /// The priced bond's `sector`.
    Sector,
    /// The position's supplied rating, the only rating the server holds.
    Rating,
}

/// The members a limit bounds: each of them, or one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitMember<T> {
    /// Every member the book holds; the most extreme sets the status.
    Each,
    /// One named member. A member the book does not hold has zero exposure.
    One(T),
}

/// The measure a limit bounds, with its unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LimitMeasure {
    /// Signed face of an instrument, in face units. Needs no market data.
    Face {
        /// The instrument, or each position's.
        instrument: LimitMember<InstrumentId>,
    },
    /// Net dirty market value, in book currency.
    MarketValue {},
    /// Sum of the positions' absolute dirty market values, in book currency.
    GrossMarketValue {},
    /// Net DV01, in book currency per basis point.
    Dv01 {},
    /// Net CS01, in book currency per basis point.
    Cs01 {},
    /// Net key-rate DV01 at one dated tenor, in book currency per basis point.
    KeyRateDv01 {
        /// Dated tenor, as the runtime labels its key rates (`5Y`).
        tenor: FactorTenor,
    },
    /// Modified duration weighted by net market value, in years.
    ModifiedDuration {},
    /// Net market value of a group, in book currency.
    GroupMarketValue {
        /// What positions are grouped by.
        by: LimitGroup,
        /// The group, or each group.
        group: LimitMember<String>,
    },
    /// A group's net market value as a fraction of gross market value.
    GroupShare {
        /// What positions are grouped by.
        by: LimitGroup,
        /// The group, or each group.
        group: LimitMember<String>,
    },
}

impl LimitMeasure {
    /// Whether the measure reads runtime results; only `face` does not.
    #[must_use]
    pub fn needs_market_data(&self) -> bool {
        !matches!(self, Self::Face { .. })
    }
}

/// One limit in a book's set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitDefinition {
    /// Identity, unique in the set.
    pub limit_id: LimitId,
    /// What is bounded.
    pub measure: LimitMeasure,
    /// Lowest value within the limit, inclusive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lower: Option<ExactDecimal>,
    /// Highest value within the limit, inclusive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper: Option<ExactDecimal>,
    /// Fraction of each bound past which the status is `warning`. Every bound it applies
    /// to faces away from zero (an upper bound above 0, a lower bound below 0), so the
    /// band runs from the fraction of the bound to the bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning_fraction: Option<ExactDecimal>,
}

fn invalid(reason: &'static str) -> ReferenceError {
    ReferenceError::InvalidInput(reason)
}

fn group_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_GROUP_KEY_BYTES
        && key.trim() == key
        && !key.chars().any(char::is_control)
}

impl LimitDefinition {
    /// Validate one limit on its own: its member, its bounds and its warning band.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        match &self.measure {
            LimitMeasure::KeyRateDv01 { tenor }
                if matches!(tenor.as_str(), "spot" | "parallel") =>
            {
                return Err(invalid("a key-rate limit needs a dated tenor"));
            }
            LimitMeasure::GroupMarketValue {
                group: LimitMember::One(key),
                ..
            }
            | LimitMeasure::GroupShare {
                group: LimitMember::One(key),
                ..
            } if !group_key(key) => {
                return Err(invalid(
                    "a group key is 1 to 128 bytes, without control characters or outer spaces",
                ));
            }
            _ => {}
        }
        let lower = self.lower.map(ExactDecimal::decimal);
        let upper = self.upper.map(ExactDecimal::decimal);
        match (lower, upper) {
            (None, None) => return Err(invalid("a limit needs a lower or an upper bound")),
            (Some(lower), Some(upper)) if lower > upper => {
                return Err(invalid("a limit's lower bound exceeds its upper bound"));
            }
            _ => {}
        }
        if let Some(fraction) = self.warning_fraction.map(ExactDecimal::decimal) {
            if fraction <= Decimal::ZERO || fraction >= Decimal::ONE {
                return Err(invalid("a warning fraction lies strictly between 0 and 1"));
            }
            if upper.is_some_and(|u| u <= Decimal::ZERO)
                || lower.is_some_and(|l| l >= Decimal::ZERO)
            {
                return Err(invalid(
                    "a warning band needs each bound away from zero: upper above 0, lower below 0",
                ));
            }
        }
        Ok(())
    }
}

/// Validate a book's limit set: at most [`MAX_BOOK_LIMITS`] limits, each valid, with
/// unique ids. An empty set is valid.
pub fn validate_limits(limits: &[LimitDefinition]) -> Result<(), ReferenceError> {
    if limits.len() > MAX_BOOK_LIMITS {
        return Err(invalid("a book holds at most 64 limits"));
    }
    let mut ids = BTreeSet::new();
    for limit in limits {
        limit.validate()?;
        if !ids.insert(limit.limit_id.as_str()) {
            return Err(invalid("limit ids are unique in a set"));
        }
    }
    Ok(())
}

/// A limit's status, ordered by severity: `within` < `warning` < `unknown` < `breach`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitStatus {
    /// Within every bound, and outside every warning band.
    Within,
    /// Within every bound, but past a warning fraction.
    Warning,
    /// Not evaluated: an input is missing. Never counted as within.
    Unknown,
    /// Past a bound.
    Breach,
}

/// The instruments a reason names: the first [`MAX_LISTED_INSTRUMENTS`] in book order,
/// and how many there are in all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentList {
    /// Every instrument the reason applies to.
    pub count: u32,
    /// The first of them, in book order.
    pub first: Vec<InstrumentId>,
}

impl InstrumentList {
    /// Collect the instruments, keeping the first [`MAX_LISTED_INSTRUMENTS`]; `None` when
    /// there are none.
    pub fn collect<'a>(instruments: impl IntoIterator<Item = &'a InstrumentId>) -> Option<Self> {
        let mut list = Self {
            count: 0,
            first: Vec::new(),
        };
        for instrument in instruments {
            list.count = list.count.saturating_add(1);
            if list.first.len() < MAX_LISTED_INSTRUMENTS {
                list.first.push(instrument.clone());
            }
        }
        (list.count > 0).then_some(list)
    }
}

/// Why a limit is `unknown`. Book-level reasons are checked first, in this order, then
/// the measure's own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case", deny_unknown_fields)]
pub enum UnknownReason {
    /// The book names no reporting currency.
    BookCurrencyRequired {},
    /// Market measures take face quantities, and these positions are in units. A face
    /// limit on one of them is unknown for the same reason.
    UnsupportedQuantityUnit {
        /// The positions in units.
        instruments: InstrumentList,
    },
    /// These positions have no accepted runtime result.
    MissingResult {
        /// The positions without one.
        instruments: InstrumentList,
    },
    /// These positions' results are in another currency than the book's.
    ForeignCurrency {
        /// The positions in another currency.
        instruments: InstrumentList,
    },
    /// A result was refused as an analyzer quote, as analytics by book refuse it.
    QuoteRefused {
        /// The first position refused.
        instrument: InstrumentId,
        /// The analyzer's reason, at most [`MAX_REASON_DETAIL_BYTES`] bytes.
        detail: String,
    },
    /// These positions' results lack the measure the limit reads.
    MeasureNotComputed {
        /// The runtime measure, such as `cs01_mid` or `krd_5y_mid`.
        measure: String,
        /// The positions without it.
        instruments: InstrumentList,
    },
    /// These positions lack the classification the limit groups by. Any of them could
    /// belong to any group, so no group's value is known.
    MissingClassification {
        /// The classification.
        by: LimitGroup,
        /// The positions without it.
        instruments: InstrumentList,
    },
    /// A value weighted by net market value is undefined at zero net value.
    ZeroNetValue {},
    /// A share of gross market value is undefined at zero gross value.
    ZeroGrossValue {},
    /// Exact arithmetic passed its fixed bound.
    ArithmeticBound {},
}

/// One limit's evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitEvaluation {
    /// The limit evaluated.
    pub limit_id: LimitId,
    /// Its status.
    pub status: LimitStatus,
    /// The measured value, rounded half to even at Decimal precision; absent when
    /// unknown, or when a limit on each member finds none. The status reads the exact
    /// value, not this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ExactDecimal>,
    /// For a limit on each member, the member whose value set the status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// The value over each bound that faces away from zero (an upper bound above 0, a
    /// lower bound below 0), the larger of the two. It is rounded half to even at Decimal
    /// precision, then to eight places, so it is for display: the status never reads it.
    /// Absent when no bound faces away from zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilisation: Option<ExactDecimal>,
    /// Why the status is `unknown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<UnknownReason>,
}
