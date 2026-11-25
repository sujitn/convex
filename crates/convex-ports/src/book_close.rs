//! A desk close of a book: each position of one book version and its value at one
//! settlement date, recorded for a UTC date.
//!
//! A book's closes are one versioned [`BookCloseDefinition`] (`MasterKey::BookClose`) per
//! book in the reference repository: each version is one close, at most one a UTC date,
//! and the history keeps them all. A close is the desk's display basis (`desk_close`), not
//! an official result.
//!
//! A valued position records the clean price and accrued interest of the accepted Live
//! result it read, both per 100 face as quoted, and the instrument version that result
//! priced. The snapshot ids pin the information set those results consumed, so the levels
//! in force are those snapshots' quotes. A position the close could not value records why.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::BTreeSet;

use convex_core::contracts::{
    BusinessDate, ExactDecimal, InstrumentId, SnapshotId, UtcTimestamp, Version,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::limits::{subject, MAX_REASON_DETAIL_BYTES};
use crate::reference_master::{
    BookId, EffectiveInterval, QuantityUnit, ReferenceProvenance, MAX_BOOK_POSITIONS,
};
use crate::reference_repository::ReferenceError;

/// What a close is: the desk's display basis, not an official result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseBasis {
    /// The desk's close.
    DeskClose,
}

/// Who took a close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CloseTaker {
    /// A subject on the desk, through the close route.
    Desk {
        /// The recorded subject.
        subject: String,
    },
    /// The weekday schedule.
    Schedule {},
}

/// Why a position has no value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case", deny_unknown_fields)]
pub enum UnvaluedReason {
    /// The quantity is in units; a value takes face.
    UnsupportedQuantityUnit {},
    /// No accepted Live result.
    MissingResult {},
    /// The result is in another currency than the book's.
    ForeignCurrency {},
    /// The result was refused as an analyzer quote, as analytics by book refuse it.
    QuoteRefused {
        /// The analyzer's reason, at most [`MAX_REASON_DETAIL_BYTES`] bytes.
        detail: String,
    },
}

/// A position's value from its accepted Live result, or why it has none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum PositionValue {
    /// Valued from an accepted Live result.
    Valued {
        /// The instrument version the result priced.
        instrument_version: Version,
        /// Clean price mid per 100 face.
        clean_price_mid: ExactDecimal,
        /// Accrued interest per 100 face.
        accrued_interest: ExactDecimal,
    },
    /// Not valued.
    Unvalued {
        /// Why.
        reason: UnvaluedReason,
    },
}

/// One position of the closed book version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosedPosition {
    /// The instrument.
    pub instrument_id: InstrumentId,
    /// Signed quantity, as the book version holds it.
    pub quantity: ExactDecimal,
    /// Its denomination.
    pub unit: QuantityUnit,
    /// Its value at the close.
    pub value: PositionValue,
}

/// A book's closes, one versioned record per book; each version is one close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookCloseDefinition {
    /// The book.
    pub book_id: BookId,
    /// The UTC date the close is for; a later version is for a later date.
    pub close_date: BusinessDate,
    /// Always `desk_close`.
    pub basis: CloseBasis,
    /// Who took it.
    pub taken_by: CloseTaker,
    /// The book version closed.
    pub book_version: Version,
    /// The book's reporting currency, which every value is in.
    pub currency: String,
    /// The settlement date every value is at.
    pub settlement_date: BusinessDate,
    /// The snapshots whose results the close read, sorted and unique.
    pub snapshot_ids: Vec<SnapshotId>,
    /// Each position of the book version, in book order; at least one valued.
    pub positions: Vec<ClosedPosition>,
    /// From the commit; the record is never retired.
    pub effective: EffectiveInterval,
    /// The request, or the schedule, that wrote this version.
    pub provenance: ReferenceProvenance,
}

impl BookCloseDefinition {
    /// When the close was taken: the start of its interval.
    #[must_use]
    pub fn taken_at(&self) -> UtcTimestamp {
        self.effective.from
    }

    /// Validate the record on its own: bounded unique positions, at least one valued, a
    /// non-negative dirty price for each, the currency, the subject and the snapshots.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        if self.positions.len() > MAX_BOOK_POSITIONS {
            return Err(ReferenceError::CapacityExceeded);
        }
        if self.effective.until.is_some() {
            return Err(invalid("a close is not retired"));
        }
        if convex_core::Currency::from_code(&self.currency).is_none() {
            return Err(invalid("a close's currency is a known currency code"));
        }
        if let CloseTaker::Desk { subject: taker } = &self.taken_by {
            subject(taker)?;
        }
        let mut instruments = BTreeSet::new();
        let mut is_any_valued = false;
        for position in &self.positions {
            if !instruments.insert(position.instrument_id.as_str()) {
                return Err(invalid("a close holds each instrument once"));
            }
            match &position.value {
                PositionValue::Valued {
                    clean_price_mid,
                    accrued_interest,
                    ..
                } => {
                    let clean = clean_price_mid.decimal();
                    if clean < Decimal::ZERO
                        || clean
                            .checked_add(accrued_interest.decimal())
                            .is_some_and(|dirty| dirty < Decimal::ZERO)
                    {
                        return Err(invalid("a closing price is not negative"));
                    }
                    is_any_valued = true;
                }
                PositionValue::Unvalued {
                    reason: UnvaluedReason::QuoteRefused { detail },
                } if detail.len() > MAX_REASON_DETAIL_BYTES => {
                    return Err(invalid("a refusal's detail is at most 256 bytes"));
                }
                PositionValue::Unvalued { .. } => {}
            }
        }
        if !is_any_valued {
            return Err(invalid("a close values at least one position"));
        }
        if self.snapshot_ids.is_empty()
            || self.snapshot_ids.len() > MAX_BOOK_POSITIONS
            || self.snapshot_ids.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(invalid(
                "a close names 1 to 1,000 snapshots, sorted and unique",
            ));
        }
        Ok(())
    }
}

fn invalid(reason: &'static str) -> ReferenceError {
    ReferenceError::InvalidInput(reason)
}
