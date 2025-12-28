//! Streaming infrastructure for real-time market data and analytics.
//!
//! This module provides:
//!
//! - **BondQuote**: Real-time quote representation
//! - **StreamPublisher**: Fan-out publisher for real-time updates
//! - **StreamSubscriber**: Subscription management for consumers
//!
//! # Architecture
//!
//! ```text
//! Market Data Feed
//!       │
//!       ▼
//! ┌─────────────┐
//! │  Publisher  │────► Channel ────► Subscriber 1
//! │             │────► Channel ────► Subscriber 2
//! │             │────► Channel ────► Subscriber 3
//! └─────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_engine::streaming::{StreamPublisher, BondQuote, QuoteSide};
//!
//! let publisher = StreamPublisher::new(1000);
//!
//! // Subscribe to updates
//! let mut subscriber = publisher.subscribe();
//!
//! // Publish a quote
//! let quote = BondQuote::new("US912828Z229")
//!     .with_bid(99.50, 1_000_000)
//!     .with_ask(99.75, 500_000);
//! publisher.publish(quote);
//!
//! // Receive updates
//! while let Ok(quote) = subscriber.recv().await {
//!     println!("Received: {:?}", quote);
//! }
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use crossbeam::channel::{self, Receiver, Sender};
use dashmap::DashMap;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

// =============================================================================
// BOND QUOTE
// =============================================================================

/// Side of a quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuoteSide {
    /// Bid (buy) side.
    Bid,
    /// Ask (offer) side.
    Ask,
}

/// Condition/status of a quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum QuoteCondition {
    /// Normal trading.
    #[default]
    Normal,
    /// Indicative (not firm).
    Indicative,
    /// Stale (not updated recently).
    Stale,
    /// Halted.
    Halted,
    /// Auction.
    Auction,
}

/// Source of the quote.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum QuoteSource {
    /// Direct from exchange/trading venue.
    Exchange(String),
    /// From a dealer.
    Dealer(String),
    /// Composite (aggregated from multiple sources).
    Composite,
    /// Internal (calculated).
    Internal,
    /// Manual override.
    Override,
    /// Unknown source.
    #[default]
    Unknown,
}

/// Real-time bond quote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondQuote {
    /// Instrument identifier (CUSIP, ISIN, etc.).
    pub instrument_id: String,
    /// Bid price.
    pub bid_price: Option<Decimal>,
    /// Bid yield.
    pub bid_yield: Option<Decimal>,
    /// Bid size (quantity).
    pub bid_size: Option<u64>,
    /// Ask price.
    pub ask_price: Option<Decimal>,
    /// Ask yield.
    pub ask_yield: Option<Decimal>,
    /// Ask size (quantity).
    pub ask_size: Option<u64>,
    /// Mid price (calculated if not provided).
    pub mid_price: Option<Decimal>,
    /// Last trade price.
    pub last_price: Option<Decimal>,
    /// Last trade size.
    pub last_size: Option<u64>,
    /// Quote condition.
    pub condition: QuoteCondition,
    /// Quote source.
    pub source: QuoteSource,
    /// Quote timestamp.
    pub timestamp: DateTime<Utc>,
    /// Sequence number for ordering.
    pub sequence: u64,
    /// Additional fields.
    pub metadata: HashMap<String, String>,
}

impl BondQuote {
    /// Creates a new bond quote.
    pub fn new(instrument_id: impl Into<String>) -> Self {
        Self {
            instrument_id: instrument_id.into(),
            bid_price: None,
            bid_yield: None,
            bid_size: None,
            ask_price: None,
            ask_yield: None,
            ask_size: None,
            mid_price: None,
            last_price: None,
            last_size: None,
            condition: QuoteCondition::Normal,
            source: QuoteSource::Unknown,
            timestamp: Utc::now(),
            sequence: 0,
            metadata: HashMap::new(),
        }
    }

    /// Sets the bid price and size.
    pub fn with_bid(mut self, price: impl Into<Decimal>, size: u64) -> Self {
        self.bid_price = Some(price.into());
        self.bid_size = Some(size);
        self.update_mid();
        self
    }

    /// Sets the ask price and size.
    pub fn with_ask(mut self, price: impl Into<Decimal>, size: u64) -> Self {
        self.ask_price = Some(price.into());
        self.ask_size = Some(size);
        self.update_mid();
        self
    }

    /// Sets the bid yield.
    pub fn with_bid_yield(mut self, yield_value: impl Into<Decimal>) -> Self {
        self.bid_yield = Some(yield_value.into());
        self
    }

    /// Sets the ask yield.
    pub fn with_ask_yield(mut self, yield_value: impl Into<Decimal>) -> Self {
        self.ask_yield = Some(yield_value.into());
        self
    }

    /// Sets the last trade.
    pub fn with_last(mut self, price: impl Into<Decimal>, size: u64) -> Self {
        self.last_price = Some(price.into());
        self.last_size = Some(size);
        self
    }

    /// Sets the condition.
    pub fn with_condition(mut self, condition: QuoteCondition) -> Self {
        self.condition = condition;
        self
    }

    /// Sets the source.
    pub fn with_source(mut self, source: QuoteSource) -> Self {
        self.source = source;
        self
    }

    /// Sets the sequence number.
    pub fn with_sequence(mut self, sequence: u64) -> Self {
        self.sequence = sequence;
        self
    }

    /// Adds metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Updates the mid price from bid/ask.
    fn update_mid(&mut self) {
        self.mid_price = match (self.bid_price, self.ask_price) {
            (Some(bid), Some(ask)) => Some((bid + ask) / Decimal::from(2)),
            (Some(bid), None) => Some(bid),
            (None, Some(ask)) => Some(ask),
            (None, None) => None,
        };
    }

    /// Returns the bid-ask spread.
    pub fn spread(&self) -> Option<Decimal> {
        match (self.bid_price, self.ask_price) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    /// Returns the bid-ask spread in basis points.
    pub fn spread_bps(&self) -> Option<Decimal> {
        match (self.bid_price, self.ask_price) {
            (Some(bid), Some(ask)) if bid > Decimal::ZERO => {
                let spread = ask - bid;
                let mid = (bid + ask) / Decimal::from(2);
                Some((spread / mid) * Decimal::from(10000))
            }
            _ => None,
        }
    }

    /// Returns true if this is a two-sided quote.
    pub fn is_two_sided(&self) -> bool {
        self.bid_price.is_some() && self.ask_price.is_some()
    }

    /// Returns true if the quote is stale.
    pub fn is_stale(&self) -> bool {
        self.condition == QuoteCondition::Stale
    }
}

// =============================================================================
// QUOTE UPDATE
// =============================================================================

/// Type of quote update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateType {
    /// New quote.
    New,
    /// Quote changed.
    Change,
    /// Quote cancelled/removed.
    Cancel,
    /// Trade occurred.
    Trade,
}

/// A quote update event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteUpdate {
    /// Update type.
    pub update_type: UpdateType,
    /// The quote.
    pub quote: BondQuote,
}

impl QuoteUpdate {
    /// Creates a new quote update.
    pub fn new(update_type: UpdateType, quote: BondQuote) -> Self {
        Self { update_type, quote }
    }

    /// Creates a new quote event.
    pub fn new_quote(quote: BondQuote) -> Self {
        Self::new(UpdateType::New, quote)
    }

    /// Creates a change event.
    pub fn change(quote: BondQuote) -> Self {
        Self::new(UpdateType::Change, quote)
    }

    /// Creates a cancel event.
    pub fn cancel(instrument_id: impl Into<String>) -> Self {
        Self::new(UpdateType::Cancel, BondQuote::new(instrument_id))
    }

    /// Creates a trade event.
    pub fn trade(quote: BondQuote) -> Self {
        Self::new(UpdateType::Trade, quote)
    }
}

// =============================================================================
// STREAM PUBLISHER
// =============================================================================

/// Fan-out publisher for real-time quote updates.
pub struct StreamPublisher<T: Clone + Send + 'static> {
    /// Broadcast sender for fan-out.
    sender: broadcast::Sender<T>,
    /// Message count.
    message_count: AtomicU64,
    /// Active subscriber count (shared with subscribers for decrement on drop).
    subscriber_count: Arc<AtomicU64>,
}

impl<T: Clone + Send + 'static> StreamPublisher<T> {
    /// Creates a new publisher with the given buffer capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            message_count: AtomicU64::new(0),
            subscriber_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Publishes a message to all subscribers.
    pub fn publish(&self, message: T) -> bool {
        self.message_count.fetch_add(1, Ordering::Relaxed);
        self.sender.send(message).is_ok()
    }

    /// Creates a new subscriber.
    pub fn subscribe(&self) -> StreamSubscriber<T> {
        let receiver = self.sender.subscribe();
        self.subscriber_count.fetch_add(1, Ordering::Relaxed);
        StreamSubscriber {
            receiver,
            _subscriber_count: Arc::clone(&self.subscriber_count),
        }
    }

    /// Returns the number of messages published.
    pub fn message_count(&self) -> u64 {
        self.message_count.load(Ordering::Relaxed)
    }

    /// Returns the number of active subscribers.
    pub fn subscriber_count(&self) -> u64 {
        self.subscriber_count.load(Ordering::Relaxed)
    }
}

impl<T: Clone + Send + 'static> Default for StreamPublisher<T> {
    fn default() -> Self {
        Self::new(1024)
    }
}

/// Subscriber for receiving real-time updates.
pub struct StreamSubscriber<T: Clone + Send + 'static> {
    receiver: broadcast::Receiver<T>,
    _subscriber_count: Arc<AtomicU64>,
}

impl<T: Clone + Send + 'static> StreamSubscriber<T> {
    /// Receives the next message.
    pub async fn recv(&mut self) -> Result<T, broadcast::error::RecvError> {
        self.receiver.recv().await
    }

    /// Tries to receive a message without blocking.
    pub fn try_recv(&mut self) -> Result<T, broadcast::error::TryRecvError> {
        self.receiver.try_recv()
    }
}

impl<T: Clone + Send + 'static> Drop for StreamSubscriber<T> {
    fn drop(&mut self) {
        self._subscriber_count.fetch_sub(1, Ordering::Relaxed);
    }
}

// =============================================================================
// QUOTE BOOK
// =============================================================================

/// Maintains the latest quotes for all instruments.
pub struct QuoteBook {
    /// Latest quotes by instrument ID.
    quotes: DashMap<String, BondQuote>,
    /// Publisher for updates.
    publisher: StreamPublisher<QuoteUpdate>,
    /// Sequence counter.
    sequence: AtomicU64,
}

impl QuoteBook {
    /// Creates a new quote book.
    pub fn new() -> Self {
        Self {
            quotes: DashMap::new(),
            publisher: StreamPublisher::new(10000),
            sequence: AtomicU64::new(0),
        }
    }

    /// Updates a quote.
    pub fn update(&self, mut quote: BondQuote) {
        let instrument_id = quote.instrument_id.clone();
        quote.sequence = self.sequence.fetch_add(1, Ordering::SeqCst);

        let update_type = if self.quotes.contains_key(&instrument_id) {
            UpdateType::Change
        } else {
            UpdateType::New
        };

        self.quotes.insert(instrument_id, quote.clone());
        self.publisher.publish(QuoteUpdate::new(update_type, quote));
    }

    /// Gets the latest quote for an instrument.
    pub fn get(&self, instrument_id: &str) -> Option<BondQuote> {
        self.quotes.get(instrument_id).map(|r| r.clone())
    }

    /// Removes a quote.
    pub fn remove(&self, instrument_id: &str) -> Option<BondQuote> {
        let removed = self.quotes.remove(instrument_id);
        if removed.is_some() {
            self.publisher.publish(QuoteUpdate::cancel(instrument_id));
        }
        removed.map(|(_, q)| q)
    }

    /// Returns all instrument IDs.
    pub fn instruments(&self) -> Vec<String> {
        self.quotes.iter().map(|r| r.key().clone()).collect()
    }

    /// Returns the number of quotes.
    pub fn len(&self) -> usize {
        self.quotes.len()
    }

    /// Returns true if empty.
    pub fn is_empty(&self) -> bool {
        self.quotes.is_empty()
    }

    /// Subscribes to quote updates.
    pub fn subscribe(&self) -> StreamSubscriber<QuoteUpdate> {
        self.publisher.subscribe()
    }

    /// Returns publisher statistics.
    pub fn stats(&self) -> QuoteBookStats {
        QuoteBookStats {
            quote_count: self.quotes.len(),
            message_count: self.publisher.message_count(),
            subscriber_count: self.publisher.subscriber_count(),
            current_sequence: self.sequence.load(Ordering::SeqCst),
        }
    }
}

impl Default for QuoteBook {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics for the quote book.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteBookStats {
    /// Number of quotes in the book.
    pub quote_count: usize,
    /// Total messages published.
    pub message_count: u64,
    /// Active subscribers.
    pub subscriber_count: u64,
    /// Current sequence number.
    pub current_sequence: u64,
}

// =============================================================================
// FILTERED SUBSCRIPTION
// =============================================================================

/// Filter for quote subscriptions.
#[derive(Debug, Clone, Default)]
pub struct QuoteFilter {
    /// Filter by instrument IDs (empty = all).
    pub instruments: Vec<String>,
    /// Minimum spread in bps.
    pub min_spread_bps: Option<Decimal>,
    /// Maximum spread in bps.
    pub max_spread_bps: Option<Decimal>,
    /// Only two-sided quotes.
    pub two_sided_only: bool,
    /// Exclude stale quotes.
    pub exclude_stale: bool,
}

impl QuoteFilter {
    /// Creates a new filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filters by instruments.
    pub fn instruments(mut self, ids: Vec<String>) -> Self {
        self.instruments = ids;
        self
    }

    /// Requires two-sided quotes.
    pub fn two_sided_only(mut self) -> Self {
        self.two_sided_only = true;
        self
    }

    /// Excludes stale quotes.
    pub fn exclude_stale(mut self) -> Self {
        self.exclude_stale = true;
        self
    }

    /// Checks if a quote matches the filter.
    pub fn matches(&self, quote: &BondQuote) -> bool {
        // Check instrument filter
        if !self.instruments.is_empty() && !self.instruments.contains(&quote.instrument_id) {
            return false;
        }

        // Check two-sided requirement
        if self.two_sided_only && !quote.is_two_sided() {
            return false;
        }

        // Check stale exclusion
        if self.exclude_stale && quote.is_stale() {
            return false;
        }

        // Check spread filters
        if let Some(spread_bps) = quote.spread_bps() {
            if let Some(min) = self.min_spread_bps {
                if spread_bps < min {
                    return false;
                }
            }
            if let Some(max) = self.max_spread_bps {
                if spread_bps > max {
                    return false;
                }
            }
        }

        true
    }
}

// =============================================================================
// SYNCHRONOUS CHANNEL
// =============================================================================

/// Synchronous multi-producer, single-consumer channel for quotes.
pub struct QuoteChannel {
    sender: Sender<QuoteUpdate>,
    receiver: Receiver<QuoteUpdate>,
}

impl QuoteChannel {
    /// Creates a new bounded channel.
    pub fn bounded(capacity: usize) -> Self {
        let (sender, receiver) = channel::bounded(capacity);
        Self { sender, receiver }
    }

    /// Creates a new unbounded channel.
    pub fn unbounded() -> Self {
        let (sender, receiver) = channel::unbounded();
        Self { sender, receiver }
    }

    /// Gets the sender.
    pub fn sender(&self) -> Sender<QuoteUpdate> {
        self.sender.clone()
    }

    /// Gets the receiver.
    pub fn receiver(&self) -> &Receiver<QuoteUpdate> {
        &self.receiver
    }

    /// Receives a message, blocking until available.
    pub fn recv(&self) -> Result<QuoteUpdate, channel::RecvError> {
        self.receiver.recv()
    }

    /// Tries to receive without blocking.
    pub fn try_recv(&self) -> Result<QuoteUpdate, channel::TryRecvError> {
        self.receiver.try_recv()
    }

    /// Sends a message.
    #[allow(clippy::result_large_err)]
    pub fn send(&self, update: QuoteUpdate) -> Result<(), channel::SendError<QuoteUpdate>> {
        self.sender.send(update)
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_bond_quote_creation() {
        let quote = BondQuote::new("US912828Z229")
            .with_bid(dec!(99.50), 1_000_000)
            .with_ask(dec!(99.75), 500_000)
            .with_source(QuoteSource::Dealer("GS".into()));

        assert_eq!(quote.instrument_id, "US912828Z229");
        assert_eq!(quote.bid_price, Some(dec!(99.50)));
        assert_eq!(quote.ask_price, Some(dec!(99.75)));
        assert_eq!(quote.mid_price, Some(dec!(99.625)));
        assert!(quote.is_two_sided());
    }

    #[test]
    fn test_bond_quote_spread() {
        let quote = BondQuote::new("TEST")
            .with_bid(dec!(99.00), 100)
            .with_ask(dec!(100.00), 100);

        assert_eq!(quote.spread(), Some(dec!(1.00)));

        // Spread in bps: (100 - 99) / 99.5 * 10000 ≈ 100.50 bps
        let spread_bps = quote.spread_bps().unwrap();
        assert!(spread_bps > dec!(100) && spread_bps < dec!(101));
    }

    #[test]
    fn test_quote_book() {
        let book = QuoteBook::new();

        let quote = BondQuote::new("BOND1")
            .with_bid(dec!(99.50), 1_000_000)
            .with_ask(dec!(99.75), 500_000);

        book.update(quote);
        assert_eq!(book.len(), 1);

        let retrieved = book.get("BOND1").unwrap();
        assert_eq!(retrieved.bid_price, Some(dec!(99.50)));

        book.remove("BOND1");
        assert!(book.is_empty());
    }

    #[test]
    fn test_quote_filter() {
        let filter = QuoteFilter::new()
            .instruments(vec!["BOND1".into(), "BOND2".into()])
            .two_sided_only()
            .exclude_stale();

        // Should match
        let quote1 = BondQuote::new("BOND1")
            .with_bid(dec!(99.50), 100)
            .with_ask(dec!(99.75), 100);
        assert!(filter.matches(&quote1));

        // Should not match (wrong instrument)
        let quote2 = BondQuote::new("BOND3")
            .with_bid(dec!(99.50), 100)
            .with_ask(dec!(99.75), 100);
        assert!(!filter.matches(&quote2));

        // Should not match (not two-sided)
        let quote3 = BondQuote::new("BOND1").with_bid(dec!(99.50), 100);
        assert!(!filter.matches(&quote3));

        // Should not match (stale)
        let quote4 = BondQuote::new("BOND1")
            .with_bid(dec!(99.50), 100)
            .with_ask(dec!(99.75), 100)
            .with_condition(QuoteCondition::Stale);
        assert!(!filter.matches(&quote4));
    }

    #[tokio::test]
    async fn test_stream_publisher() {
        let publisher: StreamPublisher<i32> = StreamPublisher::new(10);

        let mut sub1 = publisher.subscribe();
        let mut sub2 = publisher.subscribe();

        assert_eq!(publisher.subscriber_count(), 2);

        // Publish
        publisher.publish(42);
        publisher.publish(43);

        // Both subscribers should receive
        assert_eq!(sub1.recv().await.unwrap(), 42);
        assert_eq!(sub2.recv().await.unwrap(), 42);
    }

    #[test]
    fn test_quote_channel() {
        let channel = QuoteChannel::bounded(10);

        let update = QuoteUpdate::new_quote(BondQuote::new("TEST"));
        channel.send(update).unwrap();

        let received = channel.recv().unwrap();
        assert_eq!(received.quote.instrument_id, "TEST");
        assert_eq!(received.update_type, UpdateType::New);
    }
}
