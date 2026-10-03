//! Convex pricing server entry point.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use convex_engine::ports::config::EngineConfig;
use convex_engine::ports::market_data::MarketDataProvider;
use convex_engine::ports::reference_data::ReferenceDataProvider;
use convex_engine::PricingEngineBuilder;
use convex_ext_file::{
    create_empty_output, EmptyCurveInputSource, EmptyEtfHoldingsSource, EmptyEtfQuoteSource,
    EmptyFxRateSource, EmptyIndexFixingSource, EmptyInflationFixingSource,
    EmptyIssuerReferenceSource, EmptyQuoteSource, EmptyRatingSource, EmptyVolatilitySource,
};
use convex_server::{ConfigError, Server, ServerConfig};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Startup failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,convex=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .init();

    info!("Convex Pricing Server v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let mut arguments = std::env::args_os().skip(1);
    let config_path = arguments.next().map(PathBuf::from);
    if arguments.next().is_some() {
        return Err(ConfigError::Unsupported {
            field: "config_path",
            reason: "only one optional configuration path is supported",
        }
        .into());
    }
    let server_config = ServerConfig::load(config_path.as_deref())?;
    info!(
        configuration = %serde_json::to_string(&server_config.effective_redacted())?,
        "Effective startup configuration"
    );
    server_config.validate_startup()?;
    warn!("Demo profile enabled: all server output is demonstration data");

    // Create storage
    let storage = convex_ext_redb::create_redb_storage(&server_config.storage_path)?;

    // This composition is reachable only after explicit demo opt-in.
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
