//! Convex MCP server binary. stdio by default; pass `--http` for remote hosting.

use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use convex_mcp::server::ConvexMcpServer;

#[derive(Parser, Debug)]
#[command(name = "convex-mcp-server")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Use HTTP transport instead of stdio.
    #[arg(long)]
    http: bool,

    /// HTTP port (only used with --http).
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// HTTP host to bind to (only used with --http).
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Enable verbose logging.
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(if args.verbose {
            "convex_mcp=debug,rmcp=debug"
        } else {
            "convex_mcp=info,rmcp=warn"
        })
    });

    // stdio transport must keep stdout clean for the protocol — log to stderr only.
    let registry = tracing_subscriber::registry().with(filter);
    if args.http {
        registry.with(tracing_subscriber::fmt::layer()).init();
    } else {
        registry
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .init();
    }

    tracing::info!("Starting Convex MCP Server");
    let server = ConvexMcpServer::new();

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
async fn run_http_server(_server: ConvexMcpServer, host: &str, port: u16) -> anyhow::Result<()> {
    use axum::Router;
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpService,
    };
    use tower_http::cors::{Any, CorsLayer};

    tracing::info!("Using HTTP transport on {}:{}", host, port);

    let mcp_service = StreamableHttpService::new(
        || Ok(ConvexMcpServer::new()),
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
