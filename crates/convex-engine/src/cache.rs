//! In-memory caches for the pricing engine.

use std::time::{Duration, Instant};

use dashmap::DashMap;

use convex_traits::ids::InstrumentId;
use convex_traits::market_data::RawQuote;

/// Quote cache with staleness tracking.
pub struct QuoteCache {
    quotes: DashMap<InstrumentId, CachedQuote>,
    stale_threshold: Duration,
}

struct CachedQuote {
    quote: RawQuote,
    received_at: Instant,
}

impl QuoteCache {
    /// Create a new quote cache.
    pub fn new(stale_threshold: Duration) -> Self {
        Self {
            quotes: DashMap::new(),
            stale_threshold,
        }
    }

    /// Update a quote.
    pub fn update(&self, quote: RawQuote) {
        self.quotes.insert(
            quote.instrument_id.clone(),
            CachedQuote {
                quote,
                received_at: Instant::now(),
            },
        );
    }

    /// Get a quote.
    pub fn get(&self, instrument_id: &InstrumentId) -> Option<RawQuote> {
        self.quotes.get(instrument_id).map(|c| c.quote.clone())
    }

    /// Check if a quote is stale.
    pub fn is_stale(&self, instrument_id: &InstrumentId) -> bool {
        self.quotes
            .get(instrument_id)
            .map(|c| c.received_at.elapsed() > self.stale_threshold)
            .unwrap_or(true)
    }

    /// Remove stale quotes.
    pub fn cleanup_stale(&self) {
        self.quotes
            .retain(|_, v| v.received_at.elapsed() <= self.stale_threshold);
    }

    /// Clear all quotes.
    pub fn clear(&self) {
        self.quotes.clear();
    }
}

impl Default for QuoteCache {
    fn default() -> Self {
        Self::new(Duration::from_secs(300)) // 5 minutes
    }
}
