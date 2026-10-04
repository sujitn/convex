//! Schema 1 normalized bond quote events. Sequences are per source/instrument/session.
//! Times are UTC Unix milliseconds; decimals are lossless strings on this wire.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use convex_core::ids::InstrumentId;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{market_data::StreamError, TraitError};

/// Maximum fields in one image or delta.
pub const MAX_QUOTE_FIELDS: usize = 12;

/// Ordering and source-selection key. Sources are never silently composited.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteKey {
    /// Internal instrument identity.
    pub instrument_id: InstrumentId,
    /// Provider-neutral source identity.
    pub source: String,
}

/// Source classification, independent of recovery and freshness state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuoteQuality {
    /// Real-time observation.
    Live,
    /// Delayed observation.
    Delayed,
    /// Non-executable indicative observation.
    Indicative,
    /// Operator-supplied observation.
    Manual,
}

/// Explicit field units; no implicit price/yield/spread conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuoteUnit {
    /// Bond clean price per 100 face.
    PricePer100,
    /// Yield as a decimal, e.g. 0.04.
    YieldDecimal,
    /// Spread in basis points, e.g. 125.
    SpreadBasisPoints,
    /// Size in face amount, not lots.
    FaceAmount,
}

/// Supported bond quote fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuoteField {
    /// Bid clean price.
    BidPrice,
    /// Ask clean price.
    AskPrice,
    /// Supplied mid clean price.
    MidPrice,
    /// Last clean price.
    LastPrice,
    /// Bid yield.
    BidYield,
    /// Ask yield.
    AskYield,
    /// Supplied mid yield.
    MidYield,
    /// Bid spread.
    BidSpread,
    /// Ask spread.
    AskSpread,
    /// Supplied mid spread.
    MidSpread,
    /// Bid face amount.
    BidSize,
    /// Ask face amount.
    AskSize,
}

impl QuoteField {
    /// Only unit admitted for this schema's field.
    pub fn unit(self) -> QuoteUnit {
        match self {
            Self::BidPrice | Self::AskPrice | Self::MidPrice | Self::LastPrice => {
                QuoteUnit::PricePer100
            }
            Self::BidYield | Self::AskYield | Self::MidYield => QuoteUnit::YieldDecimal,
            Self::BidSpread | Self::AskSpread | Self::MidSpread => QuoteUnit::SpreadBasisPoints,
            Self::BidSize | Self::AskSize => QuoteUnit::FaceAmount,
        }
    }
}

/// A value at declared contract precision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteValue {
    /// Exact value. Numeric JSON tokens are rejected.
    #[serde(with = "decimal_string")]
    pub value: Decimal,
    /// Declared unit.
    pub unit: QuoteUnit,
}

/// A partial update distinguishes omission from deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "action",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FieldUpdate {
    /// Retain the prior field and its original timestamp.
    Unchanged,
    /// Replace the field with a supplied value.
    Set(QuoteValue),
    /// Remove only this field.
    Clear,
}

/// One field instruction. Duplicate fields in an event are invalid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteChange {
    /// Field to change.
    pub field: QuoteField,
    /// Explicit operation.
    pub update: FieldUpdate,
}

/// Observable vendor-neutral control and data messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuoteEventKind {
    /// Complete image; omitted fields are absent. Only Set instructions are allowed.
    Image {
        /// Image fields.
        fields: Vec<QuoteChange>,
    },
    /// Delta; omitted and Unchanged fields retain their original observations.
    Delta {
        /// Field instructions.
        fields: Vec<QuoteChange>,
    },
    /// Explicit session change; requires a new image before any fresh output.
    Reset,
    /// Liveness only. Must repeat the current data sequence; never refreshes fields.
    Heartbeat,
    /// Provider explicitly reports loss; an image is required.
    Gap,
    /// Source ends delivery for this key.
    Closed,
    /// Provider reports a bounded error code; an image is required for recovery.
    Error {
        /// Provider-neutral diagnostic code.
        code: String,
    },
}

/// Normalized schema 1 envelope. Data sequence starts at 1; reset uses 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteEvent {
    /// Contract version, currently 1.
    pub schema_version: u32,
    /// Ordering/source key.
    pub key: QuoteKey,
    /// Source connection/session identity.
    pub session: String,
    /// Per-key data sequence; controls carry the last observed sequence.
    pub sequence: u64,
    /// Original source observation time.
    pub source_time_ms: i64,
    /// Recorded receipt time, preserved in replay.
    pub receive_time_ms: i64,
    /// Delivery classification.
    pub quality: QuoteQuality,
    /// Whether consumption is permitted by the supplying adapter.
    pub entitled: bool,
    /// Opaque entitlement labels, retained for downstream policy.
    pub entitlement_tags: Vec<String>,
    /// Optional permitted hash/reference to raw evidence, never raw licensed payload.
    pub evidence_reference: Option<String>,
    /// Data/control content.
    pub event: QuoteEventKind,
}

impl QuoteEvent {
    /// Validate structure, resource bounds and field units. Temporal policy is applied by the assembler.
    pub fn validate(&self) -> Result<(), TraitError> {
        if self.schema_version != 1 {
            return Err(TraitError::InvalidInput("unsupported quote event schema"));
        }
        for value in [
            self.key.instrument_id.as_str(),
            &self.key.source,
            &self.session,
        ] {
            validate_text(value, 128)?;
        }
        if self.source_time_ms < 0 || self.receive_time_ms < 0 {
            return Err(TraitError::InvalidInput("negative market timestamp"));
        }
        if self.entitlement_tags.len() > 16 {
            return Err(TraitError::CapacityExceeded);
        }
        for tag in &self.entitlement_tags {
            validate_text(tag, 128)?;
        }
        if let Some(reference) = &self.evidence_reference {
            validate_text(reference, 512)?;
        }
        match &self.event {
            QuoteEventKind::Image { fields } | QuoteEventKind::Delta { fields } => {
                if self.sequence == 0 || fields.len() > MAX_QUOTE_FIELDS {
                    return Err(TraitError::InvalidInput(
                        "invalid quote sequence or field count",
                    ));
                }
                let mut seen = std::collections::BTreeSet::new();
                for change in fields {
                    if !seen.insert(change.field) {
                        return Err(TraitError::InvalidInput("duplicate quote field"));
                    }
                    if matches!(self.event, QuoteEventKind::Image { .. })
                        && !matches!(change.update, FieldUpdate::Set(_))
                    {
                        return Err(TraitError::InvalidInput("image requires Set fields"));
                    }
                    if let FieldUpdate::Set(value) = &change.update {
                        if value.unit != change.field.unit()
                            || (value.unit == QuoteUnit::FaceAmount && value.value < Decimal::ZERO)
                        {
                            return Err(TraitError::InvalidInput(
                                "wrong quote unit or negative size",
                            ));
                        }
                    }
                }
            }
            QuoteEventKind::Reset if self.sequence != 0 => {
                return Err(TraitError::InvalidInput("reset sequence must be zero"))
            }
            QuoteEventKind::Error { code } => validate_text(code, 128)?,
            _ => {}
        }
        Ok(())
    }
}

/// Validate bounded identifiers and diagnostic text.
pub fn validate_text(value: &str, maximum: usize) -> Result<(), TraitError> {
    if value.is_empty()
        || value.len() > maximum
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(TraitError::InvalidInput("invalid market identifier/text"));
    }
    Ok(())
}

/// Pull delivery for deterministic replay. EOF is Closed; invalid records are Failure.
#[async_trait]
pub trait QuoteEventSource: Send {
    /// Read one event; no hidden task, buffering queue or wall-clock scheduling.
    async fn next_event(&mut self) -> Result<QuoteEvent, StreamError>;
}

mod decimal_string {
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &Decimal, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Decimal, D::Error> {
        let value = String::deserialize(deserializer)?;
        Decimal::from_str_exact(&value).map_err(serde::de::Error::custom)
    }
}
