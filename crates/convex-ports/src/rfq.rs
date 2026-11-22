//! A client's request for quote on a stored bond: what the source sends, the states it
//! passes through, and the closed table of who may move it from one to the next.
//!
//! A request arrives from a source (a venue adapter or a simulator) and is priced by the
//! server. A trader sends a quote or passes; the source reports the client's outcome or a
//! cancellation; the server expires it at its deadline, or at a restart. Each state but the
//! three open ones is final.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt;

use convex_core::contracts::{BusinessDate, ExactDecimal, InstrumentId, SourceId, UtcTimestamp};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::quote_event::validate_text;
use crate::reference_master::{BookId, ExternalIdentifier};

/// The earliest deadline after receipt, in milliseconds.
pub const MIN_RFQ_DEADLINE_MS: i64 = 1_000;
/// The latest deadline after receipt, in milliseconds.
pub const MAX_RFQ_DEADLINE_MS: i64 = 300_000;
/// The most requests one [`RfqBatch`] carries.
pub const MAX_RFQ_BATCH: usize = 64;
/// The largest [`RfqBatch`] on the wire, in bytes: 64 of the largest requests fit.
pub const MAX_RFQ_BODY_BYTES: usize = 128 * 1024;
/// The longest RFQ id or client id, in bytes.
pub const MAX_RFQ_ID_BYTES: usize = 128;
/// The longest client tier, in bytes.
pub const MAX_CLIENT_TIER_BYTES: usize = 32;

/// The client's side of the trade it asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientSide {
    /// The client buys from the desk.
    Buy,
    /// The client sells to the desk.
    Sell,
}

/// The side of the desk's two-way quote a client trades on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeskSide {
    /// The desk's bid: the client sells to it.
    Bid,
    /// The desk's ask: the client buys at it.
    Ask,
}

impl ClientSide {
    /// The side of the desk's quote the client trades on: a client buy lifts the desk's
    /// ask, and a client sell hits its bid.
    pub fn desk_side(self) -> DeskSide {
        match self {
            Self::Buy => DeskSide::Ask,
            Self::Sell => DeskSide::Bid,
        }
    }

    /// The change in face a done trade of `face` makes to the desk's position: a client
    /// buy shortens it, a client sell lengthens it.
    pub fn position_change(self, face: Decimal) -> Decimal {
        match self {
            Self::Buy => -face,
            Self::Sell => face,
        }
    }
}

/// A request as its source sends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RfqRequest {
    /// The source that sent it; with `rfq_id`, its identity.
    pub source_id: SourceId,
    /// The source's id for it, unique within the source.
    pub rfq_id: String,
    /// The client asking.
    pub client_id: String,
    /// The client's tier, which scales the desk's width.
    pub client_tier: String,
    /// The bond, by its internal id; or `identifier`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument_id: Option<InstrumentId>,
    /// The bond, by its ISIN, CUSIP or a provider's id, through the identifier mappings; an
    /// identifier that maps to no bond is refused when it is resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<ExternalIdentifier>,
    /// The client's side.
    pub side: ClientSide,
    /// The size asked for, in face amount.
    pub face: ExactDecimal,
    /// The trade's settlement date.
    pub settlement_date: BusinessDate,
    /// The desk's book the trade would go to.
    pub book_id: BookId,
    /// When the client stops taking answers.
    pub deadline: UtcTimestamp,
}

impl RfqRequest {
    /// Check the request as received at `received_at`: its names are bounded, without
    /// surrounding spaces or control characters, it names its bond once by a valid
    /// identifier, its size is positive and its deadline is from [`MIN_RFQ_DEADLINE_MS`]
    /// to [`MAX_RFQ_DEADLINE_MS`] after receipt.
    pub fn validate(&self, received_at: UtcTimestamp) -> Result<(), RfqRefusal> {
        validate_text(&self.rfq_id, MAX_RFQ_ID_BYTES).map_err(|_| RfqRefusal::RfqId)?;
        validate_text(&self.client_id, MAX_RFQ_ID_BYTES).map_err(|_| RfqRefusal::ClientId)?;
        validate_text(&self.client_tier, MAX_CLIENT_TIER_BYTES)
            .map_err(|_| RfqRefusal::ClientTier)?;
        match (&self.instrument_id, &self.identifier) {
            (Some(_), None) => {}
            (None, Some(identifier)) if identifier.validate().is_ok() => {}
            _ => return Err(RfqRefusal::Instrument),
        }
        if self.face.decimal() <= Decimal::ZERO {
            return Err(RfqRefusal::Face);
        }
        let ahead_ns = (self.deadline.datetime() - received_at.datetime()).num_nanoseconds();
        let allowed_ns = MIN_RFQ_DEADLINE_MS * 1_000_000..=MAX_RFQ_DEADLINE_MS * 1_000_000;
        if !ahead_ns.is_some_and(|ahead_ns| allowed_ns.contains(&ahead_ns)) {
            return Err(RfqRefusal::Deadline);
        }
        Ok(())
    }
}

/// The requests one ingest carries. Ingest refuses more than [`MAX_RFQ_BATCH`], or a body
/// over [`MAX_RFQ_BODY_BYTES`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RfqBatch {
    /// The requests, each answered on its own.
    pub rfqs: Vec<RfqRequest>,
}

/// Why a request is refused at ingest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RfqRefusal {
    /// The RFQ id is empty, too long, or holds surrounding spaces or a control character.
    RfqId,
    /// The client id is empty, too long, or holds surrounding spaces or a control character.
    ClientId,
    /// The client tier is empty, too long, or holds surrounding spaces or a control
    /// character.
    ClientTier,
    /// The bond is named twice, not at all, or by an invalid identifier.
    Instrument,
    /// The size is not a positive face amount.
    Face,
    /// The deadline is too soon or too far after receipt.
    Deadline,
}

impl RfqRefusal {
    /// A stable code.
    pub fn code(self) -> &'static str {
        match self {
            Self::RfqId => "RFQ_ID",
            Self::ClientId => "RFQ_CLIENT_ID",
            Self::ClientTier => "RFQ_CLIENT_TIER",
            Self::Instrument => "RFQ_INSTRUMENT",
            Self::Face => "RFQ_FACE",
            Self::Deadline => "RFQ_DEADLINE",
        }
    }

    /// Why, in one fixed sentence.
    pub fn reason(self) -> &'static str {
        match self {
            Self::RfqId => {
                "an RFQ id is 1 to 128 bytes, without surrounding spaces or control characters"
            }
            Self::ClientId => {
                "a client id is 1 to 128 bytes, without surrounding spaces or control characters"
            }
            Self::ClientTier => {
                "a client tier is 1 to 32 bytes, without surrounding spaces or control characters"
            }
            Self::Instrument => {
                "an RFQ names its bond once, by instrument_id or by a valid identifier"
            }
            Self::Face => "an RFQ's size is a positive face amount",
            Self::Deadline => "an RFQ's deadline is 1 to 300 seconds after it is received",
        }
    }
}

impl fmt::Display for RfqRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason())
    }
}

impl std::error::Error for RfqRefusal {}

/// Where a request stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RfqState {
    /// Taken in, not yet priced.
    Received,
    /// Priced by the server, awaiting the trader.
    Priced,
    /// The trader's quote is with the client.
    Quoted,
    /// The client traded on the desk's quote.
    Done,
    /// The client traded with another dealer.
    TradedAway,
    /// The client traded with another dealer, and the desk's quote was the next best.
    Covered,
    /// The trader declined to quote.
    Passed,
    /// The deadline passed, or the server restarted, before an outcome.
    Expired,
    /// The client withdrew the request.
    Cancelled,
}

impl RfqState {
    /// Every state, in the order a request passes through them.
    pub const ALL: [Self; 9] = [
        Self::Received,
        Self::Priced,
        Self::Quoted,
        Self::Done,
        Self::TradedAway,
        Self::Covered,
        Self::Passed,
        Self::Expired,
        Self::Cancelled,
    ];

    /// Whether the request is final: nothing moves it again.
    pub fn is_final(self) -> bool {
        !matches!(self, Self::Received | Self::Priced | Self::Quoted)
    }

    /// Its name on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::Priced => "priced",
            Self::Quoted => "quoted",
            Self::Done => "done",
            Self::TradedAway => "traded_away",
            Self::Covered => "covered",
            Self::Passed => "passed",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for RfqState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who moves a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RfqActor {
    /// The server: it prices a request and expires it.
    Server,
    /// The trader answering for the desk.
    Trader,
    /// The source the request came from.
    Source,
}

/// Why the server expired a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpiryReason {
    /// Its deadline passed.
    Deadline,
    /// The server restarted while it was open; no quote is sent again.
    Restart,
}

/// What moves a request, each by one actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RfqEvent {
    /// The server priced it.
    Priced,
    /// The trader sent a quote, as priced or adjusted.
    Quoted,
    /// The trader declined to quote.
    Passed,
    /// The client traded on the desk's quote.
    Done,
    /// The client traded with another dealer.
    TradedAway,
    /// The client traded with another dealer, the desk's quote next best.
    Covered,
    /// The server expired it.
    Expired {
        /// Why.
        reason: ExpiryReason,
    },
    /// The client withdrew it.
    Cancelled,
}

impl RfqEvent {
    /// Every event, each expiry reason included.
    pub const ALL: [Self; 9] = [
        Self::Priced,
        Self::Quoted,
        Self::Passed,
        Self::Done,
        Self::TradedAway,
        Self::Covered,
        Self::Expired {
            reason: ExpiryReason::Deadline,
        },
        Self::Expired {
            reason: ExpiryReason::Restart,
        },
        Self::Cancelled,
    ];

    /// The one actor that may send this event.
    pub fn actor(self) -> RfqActor {
        match self {
            Self::Priced | Self::Expired { .. } => RfqActor::Server,
            Self::Quoted | Self::Passed => RfqActor::Trader,
            Self::Done | Self::TradedAway | Self::Covered | Self::Cancelled => RfqActor::Source,
        }
    }
}

/// What a source reports of a request: the client's outcome, or its withdrawal. These are
/// the only events a source sends, each named on the wire as its [`RfqEvent`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceOutcome {
    /// The client traded on the desk's quote.
    Done,
    /// The client traded with another dealer.
    TradedAway,
    /// The client traded with another dealer, the desk's quote next best.
    Covered,
    /// The client withdrew it.
    Cancelled,
}

impl From<SourceOutcome> for RfqEvent {
    fn from(outcome: SourceOutcome) -> Self {
        match outcome {
            SourceOutcome::Done => Self::Done,
            SourceOutcome::TradedAway => Self::TradedAway,
            SourceOutcome::Covered => Self::Covered,
            SourceOutcome::Cancelled => Self::Cancelled,
        }
    }
}

/// Why an event cannot move a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RfqTransitionError {
    /// The request is final.
    Final(RfqState),
    /// The event does not apply in this state.
    NotAllowed(RfqState),
}

impl RfqTransitionError {
    /// A stable code.
    pub fn code(self) -> &'static str {
        match self {
            Self::Final(_) => "RFQ_FINAL",
            Self::NotAllowed(_) => "RFQ_TRANSITION",
        }
    }
}

impl fmt::Display for RfqTransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Final(state) => write!(f, "the RFQ is final: {state}"),
            Self::NotAllowed(state) => write!(f, "the event does not apply to an RFQ {state}"),
        }
    }
}

impl std::error::Error for RfqTransitionError {}

/// The state `event` moves a request in `state` to:
///
/// | From | Event | To |
/// |---|---|---|
/// | received | priced | priced |
/// | priced | quoted | quoted |
/// | priced | passed | passed |
/// | quoted | done, traded away, covered | done, traded away, covered |
/// | received, priced, quoted | expired | expired |
/// | received, priced, quoted | cancelled | cancelled |
///
/// A final request refuses every event, a source's outcome after the server expired it
/// included.
pub fn transition(state: RfqState, event: RfqEvent) -> Result<RfqState, RfqTransitionError> {
    use RfqEvent as E;
    use RfqState as S;
    if state.is_final() {
        return Err(RfqTransitionError::Final(state));
    }
    match (state, event) {
        (S::Received, E::Priced) => Ok(S::Priced),
        (S::Priced, E::Quoted) => Ok(S::Quoted),
        (S::Priced, E::Passed) => Ok(S::Passed),
        (S::Quoted, E::Done) => Ok(S::Done),
        (S::Quoted, E::TradedAway) => Ok(S::TradedAway),
        (S::Quoted, E::Covered) => Ok(S::Covered),
        (_, E::Expired { .. }) => Ok(S::Expired),
        (_, E::Cancelled) => Ok(S::Cancelled),
        _ => Err(RfqTransitionError::NotAllowed(state)),
    }
}
