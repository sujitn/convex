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

use std::sync::Arc;

use axum::http::{HeaderName, HeaderValue};
use axum::middleware;
use axum::response::Response;
use axum::{Extension, Router};
use thiserror::Error;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

use convex_engine::PricingEngine;

pub use config::{ConfigError, ServerConfig, ServerProfile};

/// Startup failures preserve configuration and listener error classifications.
#[derive(Debug, Error)]
pub enum StartupError {
    /// Invalid or unsupported process configuration.
    #[error(transparent)]
    Configuration(#[from] ConfigError),
    /// Binding or serving the HTTP listener failed.
    #[error("SERVER_IO: {0}")]
    Io(#[from] std::io::Error),
}

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
            .allow_headers(Any)
            .expose_headers([
                HeaderName::from_static("x-convex-profile"),
                HeaderName::from_static("x-convex-data-quality"),
            ]);

        let profile = self.config.profile;
        routes::create_router_with_websocket(self.engine.clone(), self.config.websocket_enabled)
            .layer(Extension(profile))
            .layer(TraceLayer::new_for_http())
            .layer(cors)
            .layer(middleware::map_response(
                move |mut response: Response| async move {
                    if profile == ServerProfile::Demo {
                        response
                            .headers_mut()
                            .insert("x-convex-profile", HeaderValue::from_static("demo"));
                        response
                            .headers_mut()
                            .insert("x-convex-data-quality", HeaderValue::from_static("demo"));
                    }
                    response
                },
            ))
    }

    /// Start the server.
    pub async fn start(&self) -> Result<(), StartupError> {
        self.config.validate_startup()?;
        let listener = TcpListener::bind(self.config.bind_address()?).await?;
        info!("Starting Convex server on {}", listener.local_addr()?);
        axum::serve(listener, self.router()).await?;
        Ok(())
    }
}
