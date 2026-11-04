//! Convex pricing server entry point.

use std::path::PathBuf;
use std::process::ExitCode;

use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use convex_server::{ConfigError, ServerConfig};

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

    let composition = convex_server::compose(server_config).await?;
    composition.engine.start().await?;
    composition.server.start().await?;
    Ok(())
}
