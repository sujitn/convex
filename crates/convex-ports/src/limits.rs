//! Book limits (P14, D82–D85): what a limit bounds, how a book's set of them is changed,
//! and what an evaluation reports.
//!
//! A limit names one measure of one book, the member it bounds where the measure has
//! members (one instrument or group, or each of them), and a lower bound, an upper bound
//! or both, in the measure's unit. Amounts are in the book's reporting currency; there is
//! no FX. The evaluation lives in the engine (`convex_engine::limits`); these are the
//! types a limit set stores and a status reports.
//!
//! A book's limits are one versioned [`LimitSetDefinition`] (`MasterKey::LimitSet`) in the
//! reference repository. One subject proposes a change ([`LimitProposalDefinition`],
//! `MasterKey::LimitProposal`); a different subject approves it, which commits the set and
//! closes the proposal in one transaction (D85).
//!
//! A book's acknowledgements of breached or unknown limits are one versioned
//! [`LimitAcknowledgementsDefinition`] (`MasterKey::LimitAcknowledgements`) per book (P15c,
//! D94, D95): one subject may make one, and it changes no bound.
//!
//! A limit may carry a temporary excess ([`LimitExcess`], P15d, D96): wider bounds until a
//! time, changed through the set proposal as any change to a limit is.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::BTreeSet;

use convex_core::contracts::{
    ExactDecimal, FactorTenor, InstrumentId, ReferenceDataId, UtcTimestamp, Version,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::reference_master::{BookId, EffectiveInterval, ReferenceProvenance};
use crate::reference_repository::ReferenceError;

/// Limits in one book's set.
pub const MAX_BOOK_LIMITS: usize = 64;
/// Bytes in a group key that a limit names.
pub const MAX_GROUP_KEY_BYTES: usize = 128;
/// Instruments a reason names; its count is always complete.
pub const MAX_LISTED_INSTRUMENTS: usize = 16;
/// Bytes of an analyzer refusal a reason keeps.
pub const MAX_REASON_DETAIL_BYTES: usize = 256;
/// Bytes of a recorded subject: `access:` and a credential digest, or `local-development`.
pub const MAX_SUBJECT_BYTES: usize = 128;

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
    /// Wider bounds until an expiry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excess: Option<LimitExcess>,
}

/// Seconds after its proposal that an excess may run: thirty days (D96).
pub const MAX_EXCESS_SECONDS: i64 = 30 * 24 * 60 * 60;
/// Bytes in an excess's reason.
pub const MAX_EXCESS_REASON_BYTES: usize = 1024;

/// A temporary excess on a limit (P15d, D96): wider bounds until a time.
///
/// Before its expiry the limit is judged against these bounds, with its warning band on
/// them, and a bound the excess does not name stays the limit's own. From its expiry the
/// limit is judged against its own bounds again. An expired excess stays in the set,
/// inert, until a later version removes it. It is changed through the set proposal, so it
/// takes two subjects (D85).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitExcess {
    /// A lower bound strictly below the limit's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lower: Option<ExactDecimal>,
    /// An upper bound strictly above the limit's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper: Option<ExactDecimal>,
    /// When the limit's own bounds apply again.
    pub expires_at: UtcTimestamp,
    /// Why, 1 to 1,024 bytes.
    pub reason: String,
}

impl LimitExcess {
    /// Validate the excess against the limit's own bounds: it names a bound, each bound it
    /// names widens the limit's on that side, and its reason is well formed.
    fn validate(
        &self,
        lower: Option<Decimal>,
        upper: Option<Decimal>,
    ) -> Result<(), ReferenceError> {
        if self.lower.is_none() && self.upper.is_none() {
            return Err(invalid("an excess needs a lower or an upper bound"));
        }
        let below = match (self.lower.map(ExactDecimal::decimal), lower) {
            (None, _) => true,
            (Some(wider), Some(own)) => wider < own,
            (Some(_), None) => false,
        };
        let above = match (self.upper.map(ExactDecimal::decimal), upper) {
            (None, _) => true,
            (Some(wider), Some(own)) => wider > own,
            (Some(_), None) => false,
        };
        if !below || !above {
            return Err(invalid(
                "each excess bound widens a bound the limit has: a lower one below it, an upper one above it",
            ));
        }
        if !free_text(&self.reason, MAX_EXCESS_REASON_BYTES) {
            return Err(invalid(
                "an excess's reason is 1 to 1,024 bytes, not blank, without control characters other than line feeds",
            ));
        }
        Ok(())
    }
}

/// Validate an excess's expiry against the time of its proposal: after it, and at most
/// thirty days after it (D96).
pub fn excess_expiry(at: UtcTimestamp, expires_at: UtcTimestamp) -> Result<(), ReferenceError> {
    if lasts_at_most(at, expires_at, MAX_EXCESS_SECONDS) {
        Ok(())
    } else {
        Err(invalid(
            "an excess expires after it is proposed, and at most thirty days after",
        ))
    }
}

/// Whether `expires_at` is after `at`, and at most `seconds` after it.
fn lasts_at_most(at: UtcTimestamp, expires_at: UtcTimestamp, seconds: i64) -> bool {
    matches!(
        (expires_at.datetime() - at.datetime()).num_nanoseconds(),
        Some(span) if span > 0 && span <= seconds * 1_000_000_000
    )
}

/// Whether `text` is 1 to `max` bytes, not blank, without control characters other than
/// line feeds.
fn free_text(text: &str, max: usize) -> bool {
    !text.trim().is_empty()
        && text.len() <= max
        && !text.chars().any(|c| c.is_control() && c != '\n')
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
        // An excess only moves a bound away from zero, so a band stays valid on it.
        if let Some(excess) = &self.excess {
            excess.validate(lower, upper)?;
        }
        Ok(())
    }

    /// The excess that applies at `at`: one not yet expired. An excess applies strictly
    /// before its expiry, as an acknowledgement does.
    #[must_use]
    pub fn excess_at(&self, at: UtcTimestamp) -> Option<&LimitExcess> {
        self.excess.as_ref().filter(|excess| at < excess.expires_at)
    }

    /// The lower and upper bound `evaluation` of this limit was judged against: the
    /// excess's, on each side it names, when the evaluation applied one, and otherwise the
    /// limit's own.
    #[must_use]
    pub fn judged_bounds(
        &self,
        evaluation: &LimitEvaluation,
    ) -> (Option<ExactDecimal>, Option<ExactDecimal>) {
        match (&evaluation.excess, &self.excess) {
            (Some(_), Some(excess)) => (excess.lower.or(self.lower), excess.upper.or(self.upper)),
            _ => (self.lower, self.upper),
        }
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

fn subject(text: &str) -> Result<(), ReferenceError> {
    if text.is_empty()
        || text.len() > MAX_SUBJECT_BYTES
        || !text.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(invalid(
            "a recorded subject is 1 to 128 visible ASCII bytes, without spaces",
        ));
    }
    Ok(())
}

/// A book's approved limits, one versioned record per book (D82). Each version names who
/// proposed it and who approved it, as configuration records an actor: `access:` and the
/// credential digest, or `local-development`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitSetDefinition {
    /// The book the limits bound.
    pub book_id: BookId,
    /// The limits, at most [`MAX_BOOK_LIMITS`], with unique ids. An empty set has none.
    pub limits: Vec<LimitDefinition>,
    /// Who proposed this version.
    pub proposed_by: String,
    /// Who approved it: the proposer only where a change commits in one step (local
    /// development, D85).
    pub approved_by: String,
    /// The proposal version approved; absent where a change commits in one step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_version: Option<Version>,
    /// From the commit; a limit set is never retired, and an empty set clears it.
    pub effective: EffectiveInterval,
    /// The request that committed it.
    pub provenance: ReferenceProvenance,
}

impl LimitSetDefinition {
    /// Validate the set on its own: its limits, its recorded subjects, and who approved
    /// it. A set approved from a proposal names a different subject from its proposer
    /// (D85); one committed in one step names the same subject twice.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        validate_limits(&self.limits)?;
        subject(&self.proposed_by)?;
        subject(&self.approved_by)?;
        if self.effective.until.is_some() {
            return Err(invalid(
                "a limit set is not retired; an empty set clears it",
            ));
        }
        let same = self.approved_by == self.proposed_by;
        match self.proposal_version {
            Some(_) if same => Err(invalid(
                "a proposal is approved by a different subject from its proposer",
            )),
            None if !same => Err(invalid(
                "a limit set committed in one step is approved by its proposer",
            )),
            _ => Ok(()),
        }
    }
}

/// How a proposal was closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum LimitProposalClosure {
    /// A second subject approved it, committing this limit set version.
    Approved {
        /// The approver.
        by: String,
        /// The limit set version the approval committed.
        limit_set_version: Version,
    },
    /// Closed without committing, by its proposer or another subject.
    Withdrawn {
        /// Who closed it.
        by: String,
    },
}

/// A proposed change to a book's limit set, one versioned record per book (D85). A new
/// proposal replaces an open one; the history keeps every version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitProposalDefinition {
    /// The book whose limits would change.
    pub book_id: BookId,
    /// The limit set version the change replaces; absent proposes the book's first set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_version: Option<Version>,
    /// The complete proposed limits.
    pub limits: Vec<LimitDefinition>,
    /// Who proposed it.
    pub proposed_by: String,
    /// How it was closed; absent while open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<LimitProposalClosure>,
    /// From the proposal; it ends when the proposal closes.
    pub effective: EffectiveInterval,
    /// The request that wrote this version.
    pub provenance: ReferenceProvenance,
}

impl LimitProposalDefinition {
    /// Validate the proposal on its own: its limits, its subjects, that a different
    /// subject approved it (D85), and that it ends exactly when it is closed.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        validate_limits(&self.limits)?;
        subject(&self.proposed_by)?;
        match &self.closed {
            Some(LimitProposalClosure::Approved { by, .. }) => {
                subject(by)?;
                if *by == self.proposed_by {
                    return Err(invalid(
                        "a proposal is approved by a different subject from its proposer",
                    ));
                }
            }
            Some(LimitProposalClosure::Withdrawn { by }) => subject(by)?,
            None => {}
        }
        if self.closed.is_some() != self.effective.until.is_some() {
            return Err(invalid("a proposal ends exactly when it is closed"));
        }
        Ok(())
    }

    /// Whether the proposal awaits approval.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.closed.is_none()
    }
}

/// Seconds an acknowledgement may last: seven days (D95).
pub const MAX_ACKNOWLEDGEMENT_SECONDS: i64 = 7 * 24 * 60 * 60;
/// Bytes in an acknowledgement's note.
pub const MAX_ACKNOWLEDGEMENT_NOTE_BYTES: usize = 1024;

/// One limit's acknowledgement (P15c; D94, D95): someone has seen its breach or unknown
/// status, until a time. It changes no bound.
///
/// It is in force until it expires, unless it is withdrawn or the book's current set no
/// longer holds the limit exactly as acknowledged ([`LimitAcknowledgement::in_force`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitAcknowledgement {
    /// The limit as acknowledged: a set version that changes or drops it ends this.
    pub limit: LimitDefinition,
    /// The limit set version acknowledged.
    pub limit_set_version: Version,
    /// Who acknowledged it.
    pub acknowledged_by: String,
    /// When.
    pub acknowledged_at: UtcTimestamp,
    /// When it ends, at most seven days after `acknowledged_at`.
    pub expires_at: UtcTimestamp,
    /// Why, 1 to 1,024 bytes.
    pub note: String,
    /// The recorded status change it answers, as evidence only: a record of this book and
    /// limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<u64>,
}

impl LimitAcknowledgement {
    /// Whether it covers the limit at `at` under the book's current `limits`: it has not
    /// expired, and they hold the limit exactly as acknowledged.
    #[must_use]
    pub fn in_force(&self, at: UtcTimestamp, limits: &[LimitDefinition]) -> bool {
        at < self.expires_at && limits.contains(&self.limit)
    }

    /// Validate the entry on its own: its limit, its subject, its note and its expiry.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        self.limit.validate()?;
        subject(&self.acknowledged_by)?;
        acknowledgement_note(&self.note)?;
        acknowledgement_expiry(self.acknowledged_at, self.expires_at)?;
        if self.record_id == Some(0) {
            return Err(invalid("a record id is at least 1"));
        }
        Ok(())
    }
}

/// Validate an acknowledgement's note: 1 to 1,024 bytes, not blank, without control
/// characters other than line feeds.
pub fn acknowledgement_note(note: &str) -> Result<(), ReferenceError> {
    if !free_text(note, MAX_ACKNOWLEDGEMENT_NOTE_BYTES) {
        return Err(invalid(
            "a note is 1 to 1,024 bytes, not blank, without control characters other than line feeds",
        ));
    }
    Ok(())
}

/// Validate an expiry: after `at`, and at most seven days after it (D95).
pub fn acknowledgement_expiry(
    at: UtcTimestamp,
    expires_at: UtcTimestamp,
) -> Result<(), ReferenceError> {
    if lasts_at_most(at, expires_at, MAX_ACKNOWLEDGEMENT_SECONDS) {
        Ok(())
    } else {
        Err(invalid(
            "an acknowledgement expires after it is made, and at most seven days after",
        ))
    }
}

/// What one version of a book's acknowledgements changed: one limit's entry, made or
/// withdrawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum LimitAcknowledgementChange {
    /// The limit's entry was made, or replaced: its `acknowledged_by` made it.
    Acknowledged {
        /// The limit.
        limit_id: LimitId,
    },
    /// The limit's entry was withdrawn, and is in the previous version only.
    Withdrawn {
        /// The limit.
        limit_id: LimitId,
        /// Who withdrew it.
        by: String,
    },
}

/// A book's limit acknowledgements, one versioned record per book (D94). Each version
/// changes one limit's entry; entries no longer in force are dropped at the next
/// version, and the history keeps them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitAcknowledgementsDefinition {
    /// The book.
    pub book_id: BookId,
    /// At most one entry a limit, so at most [`MAX_BOOK_LIMITS`].
    pub acknowledgements: Vec<LimitAcknowledgement>,
    /// What this version changed.
    pub change: LimitAcknowledgementChange,
    /// From the commit; the record is never retired, and an empty one holds none.
    pub effective: EffectiveInterval,
    /// The request that wrote this version.
    pub provenance: ReferenceProvenance,
}

impl LimitAcknowledgementsDefinition {
    /// Validate the record on its own: each entry, one a limit, and a change that names
    /// the entry it made, or a limit it no longer holds.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        if self.acknowledgements.len() > MAX_BOOK_LIMITS {
            return Err(invalid("a book holds at most 64 acknowledgements"));
        }
        let mut ids = BTreeSet::new();
        for entry in &self.acknowledgements {
            entry.validate()?;
            if !ids.insert(entry.limit.limit_id.as_str()) {
                return Err(invalid("a book holds one acknowledgement a limit"));
            }
        }
        if self.effective.until.is_some() {
            return Err(invalid("acknowledgements are not retired"));
        }
        match &self.change {
            LimitAcknowledgementChange::Acknowledged { limit_id }
                if !ids.contains(limit_id.as_str()) =>
            {
                Err(invalid(
                    "an acknowledgement's version holds the entry it made",
                ))
            }
            LimitAcknowledgementChange::Withdrawn { limit_id, by } => {
                subject(by)?;
                if ids.contains(limit_id.as_str()) {
                    return Err(invalid(
                        "a withdrawal's version no longer holds the entry it withdrew",
                    ));
                }
                Ok(())
            }
            LimitAcknowledgementChange::Acknowledged { .. } => Ok(()),
        }
    }

    /// The entry for `limit_id`, in force or not.
    #[must_use]
    pub fn entry(&self, limit_id: &LimitId) -> Option<&LimitAcknowledgement> {
        self.acknowledgements
            .iter()
            .find(|entry| entry.limit.limit_id == *limit_id)
    }
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

/// The temporary excess an evaluation applied (P15d, D96): the limit's status and
/// utilisation read the excess bounds, and these are what apply again at its expiry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExcessApplied {
    /// The limit's own lower bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_lower: Option<ExactDecimal>,
    /// The limit's own upper bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_upper: Option<ExactDecimal>,
    /// When the limit's own bounds apply again.
    pub expires_at: UtcTimestamp,
    /// Why the excess was approved.
    pub reason: String,
}

/// One limit's evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitEvaluation {
    /// The limit evaluated.
    pub limit_id: LimitId,
    /// Its status.
    pub status: LimitStatus,
    /// The measured value, rounded half to even at Decimal precision, without trailing
    /// zeros; absent when unknown, or when a limit on each member finds none. The status
    /// reads the exact value, not this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ExactDecimal>,
    /// For a limit on each member, the member whose value set the status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// The value over each bound judged against that faces away from zero (an upper bound
    /// above 0, a lower bound below 0), the larger of the two. It is rounded half to even at Decimal
    /// precision, then to eight places, without trailing zeros, so it is for display: the
    /// status never reads it.
    /// Absent when no bound faces away from zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilisation: Option<ExactDecimal>,
    /// Why the status is `unknown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<UnknownReason>,
    /// The temporary excess in force when evaluated, whatever the status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excess: Option<ExcessApplied>,
}
