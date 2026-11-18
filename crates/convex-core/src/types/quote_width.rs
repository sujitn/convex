//! A two-way quote's width, in the unit its level is quoted in, chosen by trade size.
//!
//! A width is the distance from bid to ask, and each side lies half of it from the mid. A
//! price falls to its bid and rises to its ask. A yield or spread rises to its bid and falls
//! to its ask, since a higher yield or spread is a lower price.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{Mark, Spread};
use crate::contracts::ExactDecimal;

/// The most size buckets a width schedule holds.
pub const MAX_SIZE_BUCKETS: usize = 16;

/// The unit of a width: the quoting unit of the level it widens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WidthUnit {
    /// Price points per 100 face, for a price quote.
    PricePoints,
    /// 32nds of a price point, as Treasuries are quoted: `0.5` is a "+", a 64th.
    ThirtySeconds,
    /// Basis points: of yield for a yield quote, of spread for a spread quote.
    BasisPoints,
}

impl WidthUnit {
    /// The widest width the unit takes: 20 price points, 640 32nds (20 points) or 1,000
    /// basis points.
    pub fn max_width(self) -> Decimal {
        match self {
            Self::PricePoints => dec!(20),
            Self::ThirtySeconds => dec!(640),
            Self::BasisPoints => dec!(1000),
        }
    }

    /// One unit of width, in the units of the level `mid` it widens: price points, a
    /// decimal yield, or basis points of spread. `None` when the unit does not quote that
    /// level.
    fn in_units_of(self, mid: &Mark) -> Option<Decimal> {
        match (self, mid) {
            (Self::PricePoints, Mark::Price { .. }) | (Self::BasisPoints, Mark::Spread { .. }) => {
                Some(Decimal::ONE)
            }
            (Self::ThirtySeconds, Mark::Price { .. }) => Some(dec!(0.03125)),
            (Self::BasisPoints, Mark::Yield { .. }) => Some(dec!(0.0001)),
            _ => None,
        }
    }
}

impl fmt::Display for WidthUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::PricePoints => "price points",
            Self::ThirtySeconds => "32nds",
            Self::BasisPoints => "basis points",
        })
    }
}

/// Why a width or two-way quote is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum QuoteWidthError {
    /// The schedule has no buckets, or more than [`MAX_SIZE_BUCKETS`].
    #[error("a width schedule holds 1 to {MAX_SIZE_BUCKETS} size buckets")]
    BucketCount,
    /// A bucket's bound is not positive or not above the one before it, or a bucket other
    /// than the last has no bound.
    #[error(
        "size bucket bounds are positive and ascending, and only the last bucket has no bound"
    )]
    BucketBounds,
    /// A width is negative or wider than its unit takes.
    #[error("a width in {unit} is from 0 to {max}")]
    WidthOutOfRange {
        /// The width's unit.
        unit: WidthUnit,
        /// The widest width the unit takes.
        max: Decimal,
    },
    /// The trade's face amount is zero or negative.
    #[error("a trade's size is a positive face amount")]
    FaceNotPositive,
    /// The trade is larger than the largest bucket's bound.
    #[error("the trade is larger than the width schedule's largest size")]
    FaceAboveSchedule,
    /// The width's unit is not the unit the level is quoted in.
    #[error("a width in {unit} does not widen a {level} level")]
    UnitMismatch {
        /// The width's unit.
        unit: WidthUnit,
        /// The kind of level: `price`, `yield` or `spread`.
        level: &'static str,
    },
    /// The width takes the bid price to zero or below.
    #[error("the bid price is not positive")]
    BidPriceNotPositive,
    /// A side is outside the decimal range.
    #[error("a side of the two-way quote is outside the decimal range")]
    Overflow,
}

/// A width in its unit: the distance from bid to ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuoteWidth {
    unit: WidthUnit,
    value: Decimal,
}

impl QuoteWidth {
    /// A width of `value` in `unit`, from zero to [`WidthUnit::max_width`].
    pub fn new(unit: WidthUnit, value: Decimal) -> Result<Self, QuoteWidthError> {
        if value < Decimal::ZERO || value > unit.max_width() {
            return Err(QuoteWidthError::WidthOutOfRange {
                unit,
                max: unit.max_width(),
            });
        }
        Ok(Self { unit, value })
    }

    /// The width's unit.
    pub fn unit(&self) -> WidthUnit {
        self.unit
    }

    /// The width, in its unit.
    pub fn value(&self) -> Decimal {
        self.value
    }

    /// The bid and ask this width gives around `mid`, each half the width from it, in the
    /// mid's own form: a price keeps its kind, a yield its frequency, and a spread its type
    /// and benchmark. Nothing is rounded to a tick.
    pub fn two_way(&self, mid: &Mark) -> Result<TwoWayMark, QuoteWidthError> {
        let unit = self
            .unit
            .in_units_of(mid)
            .ok_or(QuoteWidthError::UnitMismatch {
                unit: self.unit,
                level: match mid {
                    Mark::Price { .. } => "price",
                    Mark::Yield { .. } => "yield",
                    Mark::Spread { .. } => "spread",
                },
            })?;
        let half = self
            .value
            .checked_mul(unit)
            .and_then(|width| width.checked_div(Decimal::TWO))
            .ok_or(QuoteWidthError::Overflow)?;
        let below = |level: Decimal| level.checked_sub(half).ok_or(QuoteWidthError::Overflow);
        let above = |level: Decimal| level.checked_add(half).ok_or(QuoteWidthError::Overflow);
        let (bid, ask) = match mid {
            Mark::Price { value, kind } => {
                let bid = below(*value)?;
                if bid <= Decimal::ZERO {
                    return Err(QuoteWidthError::BidPriceNotPositive);
                }
                let price = |value| Mark::Price { value, kind: *kind };
                (price(bid), price(above(*value)?))
            }
            Mark::Yield { value, frequency } => {
                let yield_mark = |value| Mark::Yield {
                    value,
                    frequency: *frequency,
                };
                (yield_mark(above(*value)?), yield_mark(below(*value)?))
            }
            Mark::Spread { value, benchmark } => {
                let spread = |bps| Mark::Spread {
                    value: Spread::new(bps, value.spread_type()),
                    benchmark: benchmark.clone(),
                };
                let bps = value.as_bps();
                (spread(above(bps)?), spread(below(bps)?))
            }
        };
        Ok(TwoWayMark { bid, ask })
    }
}

/// The bid and ask a width gives around a mid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwoWayMark {
    /// The bid: a lower price, or a higher yield or spread, than the mid.
    pub bid: Mark,
    /// The ask: a higher price, or a lower yield or spread, than the mid.
    pub ask: Mark,
}

/// The width for trades up to a face amount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SizeBucket {
    /// The largest face amount the bucket holds, inclusive. None for the last bucket, which
    /// then holds every larger trade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_face: Option<ExactDecimal>,
    /// The width for a trade in the bucket, in the schedule's unit.
    pub width: ExactDecimal,
}

/// Widths by trade size, in one unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WidthSchedule {
    /// The unit of every bucket's width.
    pub unit: WidthUnit,
    /// The buckets, ascending by `max_face`; only the last may have no bound.
    pub buckets: Vec<SizeBucket>,
}

impl WidthSchedule {
    /// Validate the schedule: 1 to [`MAX_SIZE_BUCKETS`] buckets with positive ascending
    /// bounds, only the last unbounded, each width within its unit's range.
    pub fn validate(&self) -> Result<(), QuoteWidthError> {
        if self.buckets.is_empty() || self.buckets.len() > MAX_SIZE_BUCKETS {
            return Err(QuoteWidthError::BucketCount);
        }
        let last = self.buckets.len() - 1;
        let mut previous = Decimal::ZERO;
        for (index, bucket) in self.buckets.iter().enumerate() {
            QuoteWidth::new(self.unit, bucket.width.decimal())?;
            match bucket.max_face.map(ExactDecimal::decimal) {
                Some(max_face) if max_face > previous => previous = max_face,
                None if index == last => {}
                _ => return Err(QuoteWidthError::BucketBounds),
            }
        }
        Ok(())
    }

    /// The width for a trade of `face`: that of the first bucket that holds it. A quote
    /// with no size, such as a screen price, takes the first bucket's, the smallest size
    /// the schedule names.
    pub fn width_for(&self, face: Option<Decimal>) -> Result<QuoteWidth, QuoteWidthError> {
        self.validate()?;
        let bucket = match face {
            None => self.buckets.first(),
            Some(face) if face <= Decimal::ZERO => return Err(QuoteWidthError::FaceNotPositive),
            Some(face) => self.buckets.iter().find(|bucket| {
                bucket
                    .max_face
                    .is_none_or(|max_face| face <= max_face.decimal())
            }),
        }
        .ok_or(QuoteWidthError::FaceAboveSchedule)?;
        QuoteWidth::new(self.unit, bucket.width.decimal())
    }
}
