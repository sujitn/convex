//! Convex MCP Server - Fixed Income Analytics via Model Context Protocol
//!
//! This binary provides an MCP server that exposes Convex's bond pricing
//! and analytics capabilities to AI assistants.
//!
//! # Usage
//!
//! ## stdio transport (for Claude Desktop, local use)
//! ```bash
//! convex-mcp-server
//! convex-mcp-server --demo  # with sample data
//! ```
//!
//! ## HTTP transport (for remote hosting)
//! ```bash
//! convex-mcp-server --http --port 8080
//! convex-mcp-server --http --port 8080 --demo
//! ```

use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use convex_mcp::server::ConvexMcpServer;

/// Convex MCP Server - Fixed Income Analytics
#[derive(Parser, Debug)]
#[command(name = "convex-mcp-server")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Enable demo mode with December 2025 sample market data
    #[arg(short, long)]
    demo: bool,

    /// Use HTTP transport instead of stdio (for remote hosting)
    #[arg(long)]
    http: bool,

    /// HTTP port (only used with --http)
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// HTTP host to bind to (only used with --http)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging
    let filter = if args.verbose {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("convex_mcp=debug,rmcp=debug"))
    } else {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("convex_mcp=info,rmcp=warn"))
    };

    // Only log to stderr for stdio transport to avoid corrupting the protocol
    if args.http {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .init();
    }

    // Create server
    let server = if args.demo {
        tracing::info!("Starting Convex MCP Server in DEMO mode");
        tracing::info!("Demo data: December 2025 market snapshot");
        ConvexMcpServer::with_demo_mode()
    } else {
        tracing::info!("Starting Convex MCP Server");
        ConvexMcpServer::new()
    };

    if args.http {
        run_http_server(server, &args.host, args.port).await
    } else {
        run_stdio_server(server).await
    }
}

/// Run the server with stdio transport (for Claude Desktop)
async fn run_stdio_server(server: ConvexMcpServer) -> anyhow::Result<()> {
    use rmcp::{transport::stdio, ServiceExt};

    tracing::info!("Using stdio transport");

    let service = server.serve(stdio()).await?;

    tracing::info!("Convex MCP Server ready");
    tracing::info!(
        "Available tools: create_bond, calculate_yield, calculate_price, bond_analytics, \
         create_curve, bootstrap_curve, get_zero_rate, get_forward_rate, \
         calculate_z_spread, calculate_i_spread, calculate_g_spread, and more"
    );

    service.waiting().await?;

    Ok(())
}

/// Run the server with HTTP transport (for remote hosting)
#[cfg(feature = "http")]
async fn run_http_server(server: ConvexMcpServer, host: &str, port: u16) -> anyhow::Result<()> {
    use axum::Router;
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpService,
    };
    use tower_http::cors::{Any, CorsLayer};

    tracing::info!("Using HTTP transport on {}:{}", host, port);

    let demo_mode = server.is_demo_mode();

    let mcp_service = StreamableHttpService::new(
        move || {
            if demo_mode {
                Ok(ConvexMcpServer::with_demo_mode())
            } else {
                Ok(ConvexMcpServer::new())
            }
        },
        LocalSessionManager::default().into(),
        Default::default(),
    );

    // Configure CORS for browser clients
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let router = Router::new()
        .nest_service("/mcp", mcp_service)
        .route("/health", axum::routing::get(health_check))
        .layer(cors);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("Convex MCP Server listening on http://{}/mcp", addr);
    tracing::info!("Health check: http://{}/health", addr);

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to install CTRL+C handler");
            tracing::info!("Shutting down...");
        })
        .await?;

    Ok(())
}

/// Health check endpoint for HTTP transport
#[cfg(feature = "http")]
async fn health_check() -> &'static str {
    "OK"
}

/// Fallback when HTTP feature is not enabled
#[cfg(not(feature = "http"))]
async fn run_http_server(_server: ConvexMcpServer, _host: &str, _port: u16) -> anyhow::Result<()> {
    anyhow::bail!("HTTP transport not available. Rebuild with: cargo build --features http")
}
