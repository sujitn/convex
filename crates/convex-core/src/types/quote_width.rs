//! A two-way quote's width, in the unit its level is quoted in, chosen by trade size.
//!
//! A width is the distance from bid to ask, and each side lies half of it from the mid. A
//! price falls to its bid and rises to its ask. A yield or spread rises to its bid and falls
//! to its ask, since a higher yield or spread is a lower price.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt;

use convex_math::decimal::BoundedDecimal;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use super::{Mark, QuoteBasis};
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

    /// Whether a width in this unit widens a level quoted on `basis`: price points and
    /// 32nds a price, basis points a yield or a spread.
    pub fn quotes(self, basis: QuoteBasis) -> bool {
        self.level_per_unit(basis).is_some()
    }

    /// One unit of width in the units of a level quoted on `basis`: price points, a
    /// decimal yield, or basis points of spread.
    fn level_per_unit(self, basis: QuoteBasis) -> Option<Decimal> {
        match (self, basis) {
            (Self::PricePoints, QuoteBasis::Price) | (Self::BasisPoints, QuoteBasis::Spread) => {
                Some(Decimal::ONE)
            }
            (Self::ThirtySeconds, QuoteBasis::Price) => Some(dec!(0.03125)),
            (Self::BasisPoints, QuoteBasis::Yield) => Some(dec!(0.0001)),
            _ => None,
        }
    }
}

/// Why a width or two-way quote is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteWidthError {
    /// The schedule has no buckets.
    NoBuckets,
    /// The schedule has more than [`MAX_SIZE_BUCKETS`] buckets.
    TooManyBuckets,
    /// A bucket's bound is not positive or not above the one before it, or a bucket other
    /// than the last has no bound.
    BucketBounds,
    /// A width is negative or wider than [`WidthUnit::max_width`].
    WidthOutOfRange,
    /// The trade's face amount is zero or negative.
    FaceNotPositive,
    /// The trade is larger than the largest bucket's bound.
    FaceAboveSchedule,
    /// The width's unit does not quote the mid's basis.
    UnitMismatch,
    /// The width takes the bid price to zero or below.
    BidPriceNotPositive,
    /// A side needs more than a decimal's range or precision; it is never rounded.
    Unrepresentable,
}

impl QuoteWidthError {
    /// Why, in one fixed sentence.
    pub fn reason(self) -> &'static str {
        match self {
            Self::NoBuckets => "a width schedule holds at least one size bucket",
            Self::TooManyBuckets => "a width schedule holds at most 16 size buckets",
            Self::BucketBounds => {
                "size bucket bounds are positive and ascending, and only the last bucket has no bound"
            }
            Self::WidthOutOfRange => {
                "a width is from 0 to 20 price points, 640 32nds or 1,000 basis points"
            }
            Self::FaceNotPositive => "a trade's size is a positive face amount",
            Self::FaceAboveSchedule => "the trade is larger than the width schedule's largest size",
            Self::UnitMismatch => {
                "a width in price points or 32nds widens a price, and one in basis points a yield or a spread"
            }
            Self::BidPriceNotPositive => "the bid price is not positive",
            Self::Unrepresentable => {
                "a side of the two-way quote needs more than a decimal's range or precision"
            }
        }
    }
}

impl fmt::Display for QuoteWidthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason())
    }
}

impl std::error::Error for QuoteWidthError {}

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
            return Err(QuoteWidthError::WidthOutOfRange);
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
    /// mid's own form ([`Mark::at_level`]). Each side is exact, never rounded to a tick or
    /// to a decimal's precision.
    pub fn two_way(&self, mid: &Mark) -> Result<TwoWayMark, QuoteWidthError> {
        let basis = mid.quote_basis();
        let level_per_unit = self
            .unit
            .level_per_unit(basis)
            .ok_or(QuoteWidthError::UnitMismatch)?;
        let half = BoundedDecimal::decimal(self.value)
            .multiply(level_per_unit)
            .and_then(|width| width.multiply(dec!(0.5)))
            .and_then(|half| half.finish_exact())
            .map_err(|_| QuoteWidthError::Unrepresentable)?;
        let level = mid.level();
        let (lower, higher) = (exact_sum(level, -half)?, exact_sum(level, half)?);
        let (bid, ask) = match basis {
            QuoteBasis::Price if lower <= Decimal::ZERO => {
                return Err(QuoteWidthError::BidPriceNotPositive)
            }
            QuoteBasis::Price => (lower, higher),
            QuoteBasis::Yield | QuoteBasis::Spread => (higher, lower),
        };
        Ok(TwoWayMark {
            bid: mid.at_level(bid),
            ask: mid.at_level(ask),
        })
    }
}

/// `level + change`, exactly.
fn exact_sum(level: Decimal, change: Decimal) -> Result<Decimal, QuoteWidthError> {
    let mut sum = BoundedDecimal::decimal(level);
    sum.add(&BoundedDecimal::decimal(change))
        .and_then(|()| sum.finish_exact())
        .map_err(|_| QuoteWidthError::Unrepresentable)
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
        let Some(last) = self.buckets.len().checked_sub(1) else {
            return Err(QuoteWidthError::NoBuckets);
        };
        if self.buckets.len() > MAX_SIZE_BUCKETS {
            return Err(QuoteWidthError::TooManyBuckets);
        }
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
        // `validate` has checked every bucket's width.
        Ok(QuoteWidth {
            unit: self.unit,
            value: bucket.width.decimal(),
        })
    }
}
