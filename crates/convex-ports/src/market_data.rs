//! Live quote source port.
//!
//! [`QuoteSource`] supplies bond quotes to the Live runtime, by snapshot or
//! subscription. Curve inputs, fixings and other market inputs reach valuation
//! through stored recipes, the fixing repository and snapshots, not through
//! source traits.

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::TraitError;
pub use crate::quote_event::*;
use convex_core::ids::*;

/// Source type for market data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceType {
    /// Real-time streaming (Bloomberg B-PIPE, Refinitiv Elektron)
    Streaming,
    /// Snapshot/request-response (Bloomberg SAPI, REST APIs)
    Snapshot,
    /// File-based (CSV, JSON, Parquet)
    File,
    /// Database (for historical/EOD)
    Database,
    /// Manual entry
    Manual,
}

/// Explicit delivery failures. Loss never looks like end-of-stream.
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    /// Consumer missed messages; stop fresh output and request a validated image.
    #[error("stream lagged by {missed} messages")]
    Lagged {
        /// Number discarded by the bounded transport.
        missed: u64,
    },
    /// No further messages will arrive after buffered messages drain.
    #[error("stream closed")]
    Closed,
    /// Adapter failure, including malformed replay evidence.
    #[error("stream failure: {0}")]
    Failure(#[from] TraitError),
}

/// One bounded broadcast receiver used by all streaming ports.
/// This is transient delivery; it provides neither durable checkpoints nor catch-up.
pub struct StreamReceiver<T> {
    rx: tokio::sync::broadcast::Receiver<T>,
}

impl<T: Clone> StreamReceiver<T> {
    /// Wrap an adapter-owned bounded channel without hiding loss.
    pub fn new(rx: tokio::sync::broadcast::Receiver<T>) -> Self {
        Self { rx }
    }

    /// Receive data, reported lag, or terminal closure.
    pub async fn recv(&mut self) -> Result<T, StreamError> {
        self.rx.recv().await.map_err(|error| match error {
            tokio::sync::broadcast::error::RecvError::Lagged(missed) => {
                StreamError::Lagged { missed }
            }
            tokio::sync::broadcast::error::RecvError::Closed => StreamError::Closed,
        })
    }
}

// =============================================================================
// QUOTE SOURCE
// =============================================================================

/// Raw quote from market data source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawQuote {
    /// Instrument identifier
    pub instrument_id: InstrumentId,
    /// Bid price
    pub bid_price: Option<Decimal>,
    /// Ask price
    pub ask_price: Option<Decimal>,
    /// Mid price
    pub mid_price: Option<Decimal>,
    /// Last traded price
    pub last_price: Option<Decimal>,
    /// Bid yield
    pub bid_yield: Option<Decimal>,
    /// Ask yield
    pub ask_yield: Option<Decimal>,
    /// Bid size
    pub bid_size: Option<Decimal>,
    /// Ask size
    pub ask_size: Option<Decimal>,
    /// Timestamp of the quote
    pub timestamp: i64,
    /// Source of the quote
    pub source: String,
    /// Trading venue
    pub venue: Option<String>,
}

/// Trait for quote providers (streaming or snapshot).
#[async_trait]
pub trait QuoteSource: Send + Sync {
    /// Source type.
    fn source_type(&self) -> SourceType;

    /// Get current quote (snapshot).
    async fn get_quote(&self, instrument_id: &InstrumentId)
        -> Result<Option<RawQuote>, TraitError>;

    /// Get quotes for multiple instruments.
    async fn get_quotes(
        &self,
        instrument_ids: &[InstrumentId],
    ) -> Result<Vec<RawQuote>, TraitError>;

    /// Subscribe to quote updates (streaming sources only).
    async fn subscribe(
        &self,
        instrument_ids: &[InstrumentId],
    ) -> Result<StreamReceiver<QuoteEvent>, TraitError>;

    /// Unsubscribe from quote updates.
    async fn unsubscribe(&self, instrument_ids: &[InstrumentId]) -> Result<(), TraitError>;
}
