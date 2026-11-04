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

    /// Exact serialized browser Origin allowed by HTTP. Repeat for more origins.
    /// Without this flag, only this listener's own HTTP origin is allowed.
    #[arg(long = "allow-origin")]
    allowed_origins: Vec<String>,

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
    if args.http {
        run_http_server(&args.host, args.port, &args.allowed_origins).await
    } else {
        run_stdio_server(ConvexMcpServer::new()).await
    }
}

/// Run the server with stdio transport (for Claude Desktop)
async fn run_stdio_server(server: ConvexMcpServer) -> anyhow::Result<()> {
    use rmcp::{transport::stdio, ServiceExt};

    tracing::info!("Using stdio transport");

    let service = server.serve(stdio()).await?;

    tracing::info!("Convex MCP Server ready");

    service.waiting().await?;

    Ok(())
}

/// Run the server with HTTP transport (for remote hosting)
#[cfg(feature = "http")]
async fn run_http_server(host: &str, port: u16, allowed_origins: &[String]) -> anyhow::Result<()> {
    use axum::{http::HeaderValue, Router};
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };
    use tower_http::cors::{Any, CorsLayer};

    tracing::info!("Using HTTP transport on {}:{}", host, port);

    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    let addr = listener.local_addr()?;
    let origins = checked_http_origins(&listener_origin(addr), allowed_origins)?;
    let cors_origins = origins
        .iter()
        .map(|origin| origin.parse::<HeaderValue>())
        .collect::<Result<Vec<_>, _>>()?;
    let transport = StreamableHttpServerConfig::default().with_allowed_origins(origins);

    let mcp_service = StreamableHttpService::new(
        || Ok(ConvexMcpServer::new()),
        LocalSessionManager::default().into(),
        transport,
    );

    // Configure CORS for browser clients
    let cors = CorsLayer::new()
        .allow_origin(cors_origins)
        .allow_methods(Any)
        .allow_headers(Any);

    let router = Router::new()
        .nest_service("/mcp", mcp_service)
        .route("/health", axum::routing::get(health_check))
        .layer(cors);

    tracing::info!("Convex MCP Server listening on http://{}/mcp", addr);
    tracing::info!("Health check: http://{}/health", addr);

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::error!(%error, "CTRL+C listener failed; shutting down");
            }
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
async fn run_http_server(
    _host: &str,
    _port: u16,
    _allowed_origins: &[String],
) -> anyhow::Result<()> {
    anyhow::bail!("HTTP transport not available. Rebuild with: cargo build --features http")
}

/// Serialize the actual HTTP listener, retaining IPv6 brackets and omitting port80.
#[cfg(feature = "http")]
fn listener_origin(address: std::net::SocketAddr) -> String {
    let authority = address.to_string();
    if address.port() == 80 {
        format!("http://{}", authority.trim_end_matches(":80"))
    } else {
        format!("http://{authority}")
    }
}

/// Validate bounded exact RFC6454 serialized origins once at startup. Explicit
/// configuration replaces the listener default; never admit a wildcard or null.
#[cfg(feature = "http")]
fn checked_http_origins(
    default_origin: &str,
    configured: &[String],
) -> anyhow::Result<Vec<String>> {
    let values = if configured.is_empty() {
        vec![default_origin.to_owned()]
    } else {
        configured.to_vec()
    };
    anyhow::ensure!(values.len() <= 64, "at most64HTTP origins are allowed");
    let mut seen = std::collections::HashSet::new();
    for origin in &values {
        anyhow::ensure!(
            origin.len() <= 8192 && seen.insert(origin.as_str()),
            "origins must be bounded and unique"
        );
        let uri: axum::http::Uri = origin.parse()?;
        let scheme = uri
            .scheme_str()
            .ok_or_else(|| anyhow::anyhow!("origin requires http or https"))?;
        let authority = uri
            .authority()
            .ok_or_else(|| anyhow::anyhow!("origin requires a host"))?;
        let host = uri
            .host()
            .ok_or_else(|| anyhow::anyhow!("origin requires a host"))?;
        anyhow::ensure!(
            (scheme == "http" || scheme == "https")
                && !authority.as_str().contains('@')
                && !host.contains('*')
                && !host.is_empty(),
            "only explicit HTTP(S) origins are allowed"
        );
        anyhow::ensure!(
            host == host.to_ascii_lowercase() && origin == &format!("{scheme}://{authority}"),
            "origin must be serialized without path, query, credentials or fragment"
        );
        let port_suffix = authority
            .as_str()
            .strip_prefix(host)
            .ok_or_else(|| anyhow::anyhow!("invalid origin authority"))?;
        if !port_suffix.is_empty() {
            let port: u16 = port_suffix
                .strip_prefix(':')
                .ok_or_else(|| anyhow::anyhow!("invalid origin port"))?
                .parse()?;
            anyhow::ensure!(
                !((scheme == "http" && port == 80) || (scheme == "https" && port == 443)),
                "omit the default origin port"
            );
        }
    }
    Ok(values)
}

#[cfg(all(test, feature = "http"))]
mod http_origin_tests {
    use super::{checked_http_origins, listener_origin};
    #[test]
    fn default_and_explicit_origin_lists_remain_exact_and_bounded() {
        assert_eq!(
            listener_origin("127.0.0.1:80".parse().unwrap()),
            "http://127.0.0.1"
        );
        assert_eq!(listener_origin("[::1]:80".parse().unwrap()), "http://[::1]");
        assert_eq!(
            listener_origin("[::1]:34567".parse().unwrap()),
            "http://[::1]:34567"
        );
        assert_eq!(
            checked_http_origins("http://127.0.0.1:34567", &[]).unwrap(),
            ["http://127.0.0.1:34567"]
        );
        let supplied = vec![
            "https://pricing.example".to_owned(),
            "http://localhost:34567".to_owned(),
        ];
        assert_eq!(
            checked_http_origins("http://127.0.0.1:34567", &supplied).unwrap(),
            supplied
        );
        let maximum: Vec<_> = (0..64)
            .map(|n| format!("https://pricing{n}.example"))
            .collect();
        assert_eq!(
            checked_http_origins("http://127.0.0.1:34567", &maximum).unwrap(),
            maximum
        );
    }
    #[test]
    fn malformed_ambiguous_or_unbounded_origin_configuration_fails() {
        for value in [
            "*",
            "null",
            "https://*.example",
            "https://",
            "ftp://pricing.example",
            "https://pricing.example/",
            "https://pricing.example/path",
            "https://pricing.example?x=1",
            "https://pricing.example#x",
            "https://user@pricing.example",
            "https://Pricing.example",
            "https://pricing.example:443",
            "http://pricing.example:80",
            "http://pricing.example:99999",
        ] {
            assert!(
                checked_http_origins("http://127.0.0.1:34567", &[value.to_owned()]).is_err(),
                "{value}"
            );
        }
        assert!(checked_http_origins(
            "http://127.0.0.1:34567",
            &vec!["https://pricing.example".to_owned(); 2]
        )
        .is_err());
        assert!(checked_http_origins(
            "http://127.0.0.1:34567",
            &vec!["https://pricing.example".to_owned(); 65]
        )
        .is_err());
        assert!(checked_http_origins("http://127.0.0.1:34567", &["x".repeat(8193)]).is_err());
    }
}
