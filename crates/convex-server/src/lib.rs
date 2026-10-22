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
//! // The executable and the acceptance tests share this one composition root.
//! let composition = convex_server::compose(config).await?;
//! composition.engine.start().await?;
//! composition.server.start().await?;
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod access;
mod analytics;
mod books;
mod composition;
pub mod config;
pub mod configuration_admin;
mod core_workflow;
mod curves;
mod error;
pub mod handlers;
pub mod health;
pub mod market;
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
use tower_http::cors::CorsLayer;
use tracing::info;

use convex_engine::PricingEngine;

pub use composition::{compose, Composition};
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
    /// Missing or inconsistent access composition.
    #[error(transparent)]
    Access(#[from] access::AccessError),
    /// A storage, adapter or identity dependency could not be composed.
    #[error("COMPOSITION_FAILED: {stage}: {source}")]
    Composition {
        /// Composition step that failed.
        stage: &'static str,
        /// Underlying adapter error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
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
    access: Arc<access::AccessControl>,
    core: Option<Arc<convex_engine::core_workflow::CoreWorkflow>>,
    quotes: Option<Arc<market::QuoteIngress>>,
}

impl Server {
    /// Create a new server.
    pub fn new(config: ServerConfig, engine: Arc<PricingEngine>) -> Self {
        let access = if config.access.mode == access::identity::AccessMode::LocalDevelopment {
            access::AccessControl::local_development()
        } else {
            access::AccessControl::deny_all()
        };
        Self {
            config,
            engine,
            access,
            core: None,
            quotes: None,
        }
    }

    /// Explicit verified composition; start/serve both enforce listener and access settings.
    pub fn with_access_control(
        config: ServerConfig,
        engine: Arc<PricingEngine>,
        access: Arc<access::AccessControl>,
    ) -> Self {
        Self {
            config,
            engine,
            access,
            core: None,
            quotes: None,
        }
    }

    /// Add core input assembly and durable results through existing backend ports.
    pub fn with_core_workflow(
        mut self,
        core: Arc<convex_engine::core_workflow::CoreWorkflow>,
    ) -> Self {
        self.core = Some(core);
        self
    }

    /// Accept live quotes for the Live runtime over `POST /api/v1/market/quotes`.
    pub fn with_quote_ingress(mut self, quotes: Arc<market::QuoteIngress>) -> Self {
        self.quotes = Some(quotes);
        self
    }

    /// Build the router.
    pub fn router(&self) -> Router {
        let origins: Vec<HeaderValue> = self
            .config
            .access
            .cors_origins
            .iter()
            .filter_map(|origin| HeaderValue::from_str(origin).ok())
            .collect();
        let cors = CorsLayer::new()
            .allow_origin(origins.clone())
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::PUT,
                axum::http::Method::DELETE,
                axum::http::Method::HEAD,
            ])
            .allow_headers([
                axum::http::header::AUTHORIZATION,
                axum::http::header::CONTENT_TYPE,
                HeaderName::from_static("idempotency-key"),
                axum::http::header::IF_MATCH,
            ])
            .expose_headers([
                HeaderName::from_static("x-convex-profile"),
                HeaderName::from_static("x-convex-data-quality"),
                HeaderName::from_static("x-convex-reference-version"),
                HeaderName::from_static("x-convex-curve-version"),
                HeaderName::from_static("x-convex-curve-versions"),
                HeaderName::from_static("x-convex-pricing-config"),
                HeaderName::from_static("x-convex-known-at"),
                HeaderName::from_static("etag"),
            ]);

        let profile = self.config.profile;
        let mut router = routes::create_router_with_access(
            self.engine.clone(),
            self.config.websocket_enabled,
            self.access.clone(),
        );
        if let Some(core) = &self.core {
            router = router.layer(Extension(core.clone()));
        }
        if let Some(quotes) = &self.quotes {
            router = router.layer(Extension(quotes.clone()));
        }
        router
            .layer(Extension(profile))
            .layer(Extension(access::http::AllowedOrigins(origins.clone())))
            .layer(cors)
            .layer(middleware::from_fn_with_state(
                (self.access.clone(), access::http::AllowedOrigins(origins)),
                access::http::guard_origin,
            ))
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
        self.validate_access()?;
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
        self.config.validate_startup()?;
        self.validate_access()?;
        if !listener.local_addr()?.ip().is_loopback() {
            return Err(access::AccessError::InvalidPolicy.into());
        }
        let (policy_stop, policy_rx) = tokio::sync::watch::channel(false);
        let policy_task =
            if self.access.is_configured() {
                self.config.access.policy_file.as_ref().map(|path| {
                    tokio::spawn(self.access.clone().watch_policy(path.clone(), policy_rx))
                })
            } else {
                None
            };
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
        let _ = policy_stop.send(true);
        if let Some(task) = policy_task {
            task.await.map_err(|_| access::AccessError::Unavailable)?;
        }
        self.engine.shutdown().await?;
        if let Some(error) = shutdown_error.lock().await.take() {
            return Err(error.into());
        }
        result?;
        Ok(())
    }
    fn validate_access(&self) -> Result<(), access::AccessError> {
        match self.config.access.mode {
            access::identity::AccessMode::LocalDevelopment
                if self.access.is_local_development() =>
            {
                Ok(())
            }
            access::identity::AccessMode::Required
                if self.access.is_configured() && !self.access.is_local_development() =>
            {
                Ok(())
            }
            _ => Err(access::AccessError::InvalidPolicy),
        }
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
