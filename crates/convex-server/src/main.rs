//! Convex pricing server entry point.

use std::sync::Arc;

use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use convex_engine::PricingEngineBuilder;
use convex_ext_file::{
    create_empty_output, EmptyBondReferenceSource, EmptyCurveInputSource, EmptyEtfHoldingsSource,
    EmptyEtfQuoteSource, EmptyFxRateSource, EmptyIndexFixingSource, EmptyInflationFixingSource,
    EmptyIssuerReferenceSource, EmptyQuoteSource, EmptyRatingSource, EmptyVolatilitySource,
};
use convex_server::{Server, ServerConfig};
use convex_traits::config::EngineConfig;
use convex_traits::market_data::MarketDataProvider;
use convex_traits::reference_data::ReferenceDataProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,convex=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Convex Pricing Server v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/convex.toml".to_string());

    let server_config = if std::path::Path::new(&config_path).exists() {
        info!("Loading configuration from {}", config_path);
        ServerConfig::from_file(&config_path)?
    } else {
        info!("Using default configuration");
        ServerConfig::default()
    };

    // Create storage
    let storage = convex_ext_redb::create_redb_storage(&server_config.storage_path)?;

    // Create empty providers (for demo - in production use file sources or real data)
    let market_data = MarketDataProvider {
        quotes: Arc::new(EmptyQuoteSource),
        curve_inputs: Arc::new(EmptyCurveInputSource),
        index_fixings: Arc::new(EmptyIndexFixingSource),
        volatility: Arc::new(EmptyVolatilitySource),
        fx_rates: Arc::new(EmptyFxRateSource),
        inflation_fixings: Arc::new(EmptyInflationFixingSource),
        etf_quotes: Arc::new(EmptyEtfQuoteSource),
    };

    let reference_data = ReferenceDataProvider {
        bonds: Arc::new(EmptyBondReferenceSource),
        issuers: Arc::new(EmptyIssuerReferenceSource),
        ratings: Arc::new(EmptyRatingSource),
        etf_holdings: Arc::new(EmptyEtfHoldingsSource),
    };

    let output = create_empty_output();

    // Build engine
    let engine = PricingEngineBuilder::new()
        .with_config(EngineConfig::default())
        .with_market_data(Arc::new(market_data))
        .with_reference_data(Arc::new(reference_data))
        .with_storage(Arc::new(storage))
        .with_output(Arc::new(output))
        .build()?;

    let engine = Arc::new(engine);

    // Start engine
    engine.start().await?;

    // Start server
    let server = Server::new(server_config, engine);
    server.start().await?;

    Ok(())
}
