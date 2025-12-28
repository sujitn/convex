//! Convex API Server binary.

use clap::Parser;
use convex_api::{server::run_server, state::AppState};

/// Convex Fixed Income Analytics REST API Server
#[derive(Parser, Debug)]
#[command(name = "convex-api-server")]
#[command(version, about, long_about = None)]
struct Args {
    /// Host address to bind to
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    host: String,

    /// Port to listen on
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Enable demo mode with sample bonds and curves
    #[arg(short, long)]
    demo: bool,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging
    let filter = if args.verbose {
        "debug,tower_http=debug"
    } else {
        "info,tower_http=info"
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    // Create state
    let state = if args.demo {
        tracing::info!("Starting in DEMO mode with December 2025 market data");
        AppState::with_demo_mode()
    } else {
        tracing::info!("Starting with empty state");
        AppState::new()
    };

    run_server(state, &args.host, args.port).await
}
