//! # Convex Ext File
//!
//! File-based market data and reference data for the Convex pricing engine.
//!
//! This crate provides default implementations for testing, EOD loads, and static data:
//! - CSV-based quote source
//! - JSON-based curve input source
//! - CSV-based index fixing source
//!
//! Sequenced quote replay is available through `FileQuoteReplay`. Live provider
//! adapters require separate product and entitlement certification.

#![warn(missing_docs)]
#![warn(clippy::all)]

mod fixing_repository;
mod market_data;
mod reference_data;
mod reference_import;
mod reference_repository;
mod replay;
mod result_publisher;
mod snapshot_repository;

pub use fixing_repository::*;
pub use market_data::*;
pub use reference_data::*;
pub use reference_import::*;
pub use reference_repository::*;
pub use replay::*;
pub use result_publisher::*;
pub use snapshot_repository::*;

use std::path::Path;
use std::sync::Arc;

use convex_ports::error::TraitError;
use convex_ports::market_data::MarketDataProvider;
use convex_ports::reference_data::ReferenceDataProvider;

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

/// Create empty auxiliary reference sources for explicit demo/test composition.
/// Bond reference terms always come from the configured repository.
pub fn create_empty_reference_data() -> ReferenceDataProvider {
    ReferenceDataProvider {
        issuers: Arc::new(EmptyIssuerReferenceSource),
        ratings: Arc::new(EmptyRatingSource),
        etf_holdings: Arc::new(EmptyEtfHoldingsSource),
    }
}
