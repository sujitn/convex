//! # Convex Server
//!
//! REST and WebSocket server for the Convex pricing engine.
//!
//! ## Features
//!
//! - REST API for bond quotes, analytics, and curves
//! - WebSocket streaming for real-time updates
//! - Health and metrics endpoints
//! - Configuration via TOML file
//!
//! ## Usage
//!
//! ```ignore
//! use convex_server::Server;
//!
//! let server = Server::new(config)?;
//! server.start().await?;
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod config;
pub mod handlers;
pub mod routes;
pub mod websocket;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

use convex_engine::PricingEngine;

pub use config::ServerConfig;

/// The Convex server.
pub struct Server {
    config: ServerConfig,
    engine: Arc<PricingEngine>,
}

impl Server {
    /// Create a new server.
    pub fn new(config: ServerConfig, engine: Arc<PricingEngine>) -> Self {
        Self { config, engine }
    }

    /// Build the router.
    pub fn router(&self) -> Router {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        routes::create_router(self.engine.clone())
            .layer(TraceLayer::new_for_http())
            .layer(cors)
    }

    /// Start the server.
    pub async fn start(&self) -> Result<(), std::io::Error> {
        let addr = SocketAddr::new(
            self.config.host.parse().unwrap_or([0, 0, 0, 0].into()),
            self.config.port,
        );

        info!("Starting Convex server on {}", addr);

        let listener = TcpListener::bind(addr).await?;
        axum::serve(listener, self.router()).await
    }
}
