//! Server configuration and startup.

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::routes;
use crate::state::AppState;

/// Create the API router.
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Health check
        .route("/health", get(routes::health::health_check))
        // API v1
        .nest("/api/v1", api_v1_routes())
        // Middleware
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// API v1 routes.
fn api_v1_routes() -> Router<AppState> {
    Router::new()
        // Bonds
        .route("/bonds", get(routes::bonds::list).post(routes::bonds::create))
        .route(
            "/bonds/{id}",
            get(routes::bonds::get).delete(routes::bonds::delete),
        )
        .route("/bonds/{id}/yield", post(routes::bonds::calculate_yield))
        .route("/bonds/{id}/price", post(routes::bonds::calculate_price))
        .route("/bonds/{id}/analytics", post(routes::bonds::analytics))
        .route("/bonds/{id}/cashflows", get(routes::bonds::cashflows))
        .route("/bonds/{id}/spreads", post(routes::bonds::spreads))
        // Curves
        .route(
            "/curves",
            get(routes::curves::list).post(routes::curves::create),
        )
        .route(
            "/curves/{id}",
            get(routes::curves::get).delete(routes::curves::delete),
        )
        .route("/curves/{id}/zero-rate", get(routes::curves::zero_rate))
        .route("/curves/{id}/forward-rate", get(routes::curves::forward_rate))
        .route(
            "/curves/{id}/discount-factor",
            get(routes::curves::discount_factor),
        )
        .route("/curves/bootstrap", post(routes::curves::bootstrap))
        // Analytics
        .route("/analytics/batch-yield", post(routes::analytics::batch_yield))
        .route(
            "/analytics/batch-analytics",
            post(routes::analytics::batch_analytics),
        )
}

/// Run the server.
pub async fn run_server(state: AppState, host: &str, port: u16) -> anyhow::Result<()> {
    let app = create_router(state);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("Convex API Server listening on http://{}", addr);
    tracing::info!("API endpoints:");
    tracing::info!("  GET  /health");
    tracing::info!("  GET  /api/v1/bonds");
    tracing::info!("  POST /api/v1/bonds");
    tracing::info!("  GET  /api/v1/bonds/{{id}}");
    tracing::info!("  POST /api/v1/bonds/{{id}}/yield");
    tracing::info!("  POST /api/v1/bonds/{{id}}/price");
    tracing::info!("  POST /api/v1/bonds/{{id}}/analytics");
    tracing::info!("  GET  /api/v1/curves");
    tracing::info!("  POST /api/v1/curves");
    tracing::info!("  POST /api/v1/curves/bootstrap");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Shutdown signal handler.
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C handler");
    tracing::info!("Shutting down...");
}
