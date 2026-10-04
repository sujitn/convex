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
pub mod configuration_admin;
pub mod handlers;
mod reference;
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
    /// Runtime lifecycle failed; accepted work remains checkpointed.
    #[error(transparent)]
    Runtime(#[from] convex_engine::EngineError),
}

/// Composition-root executable identity; release builds supply CONVEX_BUILD_ID at compile time.
pub const SERVER_BUILD_ID: &str = match option_env!("CONVEX_BUILD_ID") {
    Some(id) => id,
    None => concat!(
        "convex-server-",
        env!("CARGO_PKG_VERSION"),
        "-local-unverified"
    ),
};

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
                HeaderName::from_static("x-convex-reference-version"),
                HeaderName::from_static("etag"),
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

    /// Bind only after validated composition; SIGINT/SIGTERM drains the engine and transport sessions.
    pub async fn start(&self) -> Result<(), StartupError> {
        self.config.validate_startup()?;
        let listener = match TcpListener::bind(self.config.bind_address()?).await {
            Ok(listener) => listener,
            Err(error) => {
                self.engine.shutdown().await?;
                return Err(error.into());
            }
        };
        self.serve_until(listener, termination_signal()).await
    }

    /// Serve a supplied listener with an explicit shutdown trigger, for embedders and lifecycle tests.
    pub async fn serve_until(
        &self,
        listener: TcpListener,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Result<(), StartupError> {
        info!("Starting Convex server on {}", listener.local_addr()?);
        let engine = self.engine.clone();
        let shutdown_error = Arc::new(tokio::sync::Mutex::new(None));
        let captured = shutdown_error.clone();
        let result = axum::serve(listener, self.router())
            .with_graceful_shutdown(async move {
                shutdown.await;
                if let Err(error) = engine.shutdown().await {
                    *captured.lock().await = Some(error);
                }
            })
            .await;
        self.engine.shutdown().await?;
        if let Some(error) = shutdown_error.lock().await.take() {
            return Err(error.into());
        }
        result?;
        Ok(())
    }
}
async fn termination_signal() {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            }
            Err(error) => {
                tracing::error!("Cannot register termination signal: {error}");
            }
        }
    }
    #[cfg(not(unix))]
    {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!("Cannot register interrupt signal: {error}");
        }
    }
}
