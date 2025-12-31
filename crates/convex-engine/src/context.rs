//! Calculation context - provides access to caches and data during calculation.

#![allow(dead_code)]

use std::sync::Arc;

use convex_traits::ids::{CurveId, InstrumentId};
use convex_traits::market_data::RawQuote;

use crate::cache::QuoteCache;
use crate::curve_builder::{BuiltCurve, CurveBuilder};

/// Context for a calculation run.
pub struct CalculationContext {
    /// Quote cache
    pub quotes: Arc<QuoteCache>,

    /// Curve builder
    pub curves: Arc<CurveBuilder>,

    /// Calculation timestamp
    pub timestamp: i64,
}

impl CalculationContext {
    /// Create a new calculation context.
    pub fn new(quotes: Arc<QuoteCache>, curves: Arc<CurveBuilder>) -> Self {
        Self {
            quotes,
            curves,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
        }
    }

    /// Get a quote from cache.
    pub fn get_quote(&self, instrument_id: &InstrumentId) -> Option<RawQuote> {
        self.quotes.get(instrument_id)
    }

    /// Get a built curve.
    pub fn get_curve(&self, curve_id: &CurveId) -> Option<BuiltCurve> {
        self.curves.get(curve_id)
    }
}
