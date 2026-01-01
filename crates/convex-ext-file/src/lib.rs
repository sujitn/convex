//! # Convex Ext File
//!
//! File-based market data and reference data for the Convex pricing engine.
//!
//! This crate provides default implementations for testing, EOD loads, and static data:
//! - CSV-based quote source
//! - JSON-based curve input source
//! - CSV-based index fixing source
//!
//! For production real-time market data, use Bloomberg or Refinitiv extensions.

#![warn(missing_docs)]
#![warn(clippy::all)]

mod market_data;
mod reference_data;

pub use market_data::*;
pub use reference_data::*;

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use convex_traits::error::TraitError;
use convex_traits::market_data::MarketDataProvider;
use convex_traits::output::{
    AlertPublisher, AnalyticsPublisher, BondQuoteOutput, CurveOutput, CurvePublisher, EtfPublisher,
    EtfQuoteOutput, OutputPublisher, PortfolioAnalyticsOutput, PricingAlert, QuotePublisher,
};
use convex_traits::reference_data::ReferenceDataProvider;

/// Create a file-based market data provider.
pub fn create_file_market_data(
    quotes_csv: impl AsRef<Path>,
    curves_json: impl AsRef<Path>,
    fixings_csv: impl AsRef<Path>,
) -> Result<MarketDataProvider, TraitError> {
    Ok(MarketDataProvider {
        quotes: Arc::new(CsvQuoteSource::new(quotes_csv)?),
        curve_inputs: Arc::new(JsonCurveInputSource::new(curves_json)?),
        index_fixings: Arc::new(CsvIndexFixingSource::new(fixings_csv)?),
        volatility: Arc::new(EmptyVolatilitySource),
        fx_rates: Arc::new(EmptyFxRateSource),
        inflation_fixings: Arc::new(EmptyInflationFixingSource),
        etf_quotes: Arc::new(EmptyEtfQuoteSource),
    })
}

/// Create a file-based reference data provider.
pub fn create_file_reference_data(
    bonds_csv: impl AsRef<Path>,
) -> Result<ReferenceDataProvider, TraitError> {
    Ok(ReferenceDataProvider {
        bonds: Arc::new(CsvBondReferenceSource::new(bonds_csv)?),
        issuers: Arc::new(EmptyIssuerReferenceSource),
        ratings: Arc::new(EmptyRatingSource),
        etf_holdings: Arc::new(EmptyEtfHoldingsSource),
    })
}

/// Create an empty output publisher (for testing/development).
pub fn create_empty_output() -> OutputPublisher {
    OutputPublisher {
        quotes: Arc::new(EmptyQuotePublisher),
        curves: Arc::new(EmptyCurvePublisher),
        etfs: Arc::new(EmptyEtfPublisher),
        analytics: Arc::new(EmptyAnalyticsPublisher),
        alerts: Arc::new(EmptyAlertPublisher),
    }
}

// =============================================================================
// EMPTY OUTPUT PUBLISHERS
// =============================================================================

/// Empty quote publisher for testing.
pub struct EmptyQuotePublisher;

#[async_trait]
impl QuotePublisher for EmptyQuotePublisher {
    async fn publish(&self, _quote: &BondQuoteOutput) -> Result<(), TraitError> {
        Ok(())
    }

    async fn publish_batch(&self, _quotes: &[BondQuoteOutput]) -> Result<(), TraitError> {
        Ok(())
    }
}

/// Empty curve publisher for testing.
pub struct EmptyCurvePublisher;

#[async_trait]
impl CurvePublisher for EmptyCurvePublisher {
    async fn publish(&self, _curve: &CurveOutput) -> Result<(), TraitError> {
        Ok(())
    }
}

/// Empty ETF publisher for testing.
pub struct EmptyEtfPublisher;

#[async_trait]
impl EtfPublisher for EmptyEtfPublisher {
    async fn publish(&self, _quote: &EtfQuoteOutput) -> Result<(), TraitError> {
        Ok(())
    }

    async fn publish_batch(&self, _quotes: &[EtfQuoteOutput]) -> Result<(), TraitError> {
        Ok(())
    }
}

/// Empty analytics publisher for testing.
pub struct EmptyAnalyticsPublisher;

#[async_trait]
impl AnalyticsPublisher for EmptyAnalyticsPublisher {
    async fn publish(&self, _analytics: &PortfolioAnalyticsOutput) -> Result<(), TraitError> {
        Ok(())
    }
}

/// Empty alert publisher for testing.
pub struct EmptyAlertPublisher;

#[async_trait]
impl AlertPublisher for EmptyAlertPublisher {
    async fn publish(&self, _alert: &PricingAlert) -> Result<(), TraitError> {
        Ok(())
    }
}
