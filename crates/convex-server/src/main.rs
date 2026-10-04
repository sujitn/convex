//! Convex pricing server entry point.

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use convex_engine::ports::config::EngineConfig;
use convex_engine::ports::market_data::MarketDataProvider;
use convex_engine::ports::reference_data::ReferenceDataProvider;
use convex_engine::{
    PricingEngineBuilder, RuntimeInput, RuntimeMode, RuntimePlan, RuntimeServices,
};
use convex_ext_file::{
    EmptyCurveInputSource, EmptyEtfHoldingsSource, EmptyEtfQuoteSource, EmptyFxRateSource,
    EmptyIndexFixingSource, EmptyInflationFixingSource, EmptyIssuerReferenceSource,
    EmptyQuoteSource, EmptyRatingSource, EmptyVolatilitySource,
};
use convex_server::{ConfigError, Server, ServerConfig, ServerProfile};

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
    let access = convex_server::access::AccessControl::from_settings(&server_config.access).await?;
    if access.is_local_development() {
        warn!("Explicit local development access: loopback API has no authentication");
    }
    if server_config.profile == ServerProfile::Demo {
        warn!("Demo profile enabled: all server output is demonstration data");
    }

    // Create storage
    let (storage, runtime_store) =
        convex_ext_redb::create_redb_storage_with_runtime(&server_config.storage_path)?;
    let storage = Arc::new(storage);

    // Unused auxiliary product ports are explicit; replay reads the selected canonical event source.
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

    // Build engine
    let mut builder = PricingEngineBuilder::new()
        .with_config(EngineConfig::default())
        .with_market_data(Arc::new(market_data))
        .with_reference_data(Arc::new(reference_data))
        .with_storage(storage.clone());
    if server_config.profile == ServerProfile::Demo {
        builder = builder.with_demo_mode();
    } else {
        let path = server_config
            .runtime_plan
            .as_ref()
            .ok_or(ConfigError::InvalidValue {
                field: "runtime_plan",
                expected: "bounded JSON runtime plan",
            })?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(convex_engine::RuntimeError::CapacityExceeded.into());
        }
        let plan: RuntimePlan =
            serde_json::from_slice(&bytes).map_err(|_| ConfigError::InvalidValue {
                field: "runtime_plan",
                expected: "complete supported schema 1 plan",
            })?;
        if plan.mode != RuntimeMode::Replay
            || plan.build_id.as_str() != convex_server::SERVER_BUILD_ID
        {
            return Err(convex_engine::RuntimeError::Configuration(
                "replay plan must select this executable build identity",
            )
            .into());
        }
        let quotes = server_config
            .quotes_file
            .as_ref()
            .ok_or(ConfigError::InvalidValue {
                field: "quotes_file",
                expected: "canonical JSONL quote events",
            })?;
        let snapshots = Arc::new(convex_ext_file::FileSnapshotRepository::new(format!(
            "{}.snapshots",
            server_config.storage_path
        ))?);
        let fixings = Arc::new(convex_ext_file::JsonFixingRepository::empty());
        let sources = plan
            .assembly
            .quotes
            .iter()
            .map(|q| convex_core::contracts::SourceId::new(&q.key.source))
            .collect::<Result<Vec<_>, _>>()?;
        let cases = Arc::new(convex_engine::configuration::ConfigurationUseCases::new(
            storage.configuration.clone(),
            storage.bonds.clone(),
            storage.configs.clone(),
            fixings.clone(),
            snapshots.clone(),
            convex_server::configuration_admin::local_replay_capabilities(sources)?,
        ));
        if let Some(active) = cases.active(&plan.assembly.configuration.id).await? {
            builder = builder.with_config(EngineConfig {
                max_concurrent_calcs: active.staged.document.process.max_concurrent_calcs,
                ..EngineConfig::default()
            });
        }
        builder = builder
            .with_configuration_use_cases(cases)
            .with_runtime(RuntimeServices {
                plan,
                snapshots,
                fixings,
                store: runtime_store,
                publisher: Arc::new(convex_ext_file::JsonlResultPublisher::new(format!(
                    "{}.results.jsonl",
                    server_config.storage_path
                ))?),
                input: RuntimeInput::Replay(Box::new(convex_ext_file::FileQuoteReplay::open(
                    quotes,
                )?)),
            });
    }
    let engine = builder.build()?;

    let engine = Arc::new(engine);

    // Start engine
    engine.start().await?;

    // Start server
    let server = Server::with_access_control(server_config, engine, access);
    server.start().await?;

    Ok(())
}
