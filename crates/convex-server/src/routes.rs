//! Route definitions.

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;

use convex_engine::PricingEngine;

use crate::books;
use crate::handlers::{self, AppState};
use crate::reference;
use crate::websocket::{self, WebSocketState};

/// Create the API router.
///
/// # Arguments
/// * `engine` - The pricing engine
pub fn create_router(engine: Arc<PricingEngine>) -> Router {
    create_router_with_websocket(engine, true)
}

/// Create the API router with optional WebSocket routes on the HTTP listener.
pub fn create_router_with_websocket(engine: Arc<PricingEngine>, enabled: bool) -> Router {
    create_router_with_access(engine, enabled, crate::access::AccessControl::deny_all())
}

/// Compose explicit access control; default constructors deny protected requests.
/// Portfolios and books live in the engine's reference repository.
pub fn create_router_with_access(
    engine: Arc<PricingEngine>,
    websocket_enabled: bool,
    access: Arc<crate::access::AccessControl>,
) -> Router {
    let state = Arc::new(AppState {
        analytics_slots: Arc::new(tokio::sync::Semaphore::new(
            engine.config().max_concurrent_calcs.min(64),
        )),
        access: access.clone(),
        engine,
        ws_state: WebSocketState::new(),
    });

    let router = Router::new()
        // Health
        .route("/live", get(handlers::health))
        .route("/ready", get(handlers::readiness))
        .route("/strategy-ready", get(crate::health::strategy_readiness))
        .route("/api/v1/dependencies", get(crate::health::dependencies))
        .route("/api/v1/runtime", get(handlers::runtime_health))
        .route("/api/v1/runtime/results", get(handlers::runtime_results))
        .route("/api/v1/runtime/valuation", post(handlers::runtime_value))
        .route("/api/v1/analytics", post(crate::analytics::calculate))
        .route(
            "/api/v1/analytics/curves",
            post(crate::core_workflow::save_curve),
        )
        .route(
            "/api/v1/analytics/fixings",
            post(crate::core_workflow::save_fixings),
        )
        .route(
            "/api/v1/analytics/snapshots",
            post(crate::core_workflow::assemble),
        )
        .route(
            "/api/v1/analytics/snapshots/:snapshot_id",
            get(crate::core_workflow::snapshot),
        )
        .route(
            "/api/v1/analytics/snapshots/:snapshot_id/run/:calculation",
            post(crate::core_workflow::run),
        )
        .route(
            "/api/v1/analytics/results/:result_id",
            get(crate::core_workflow::result),
        )
        .route(
            "/api/v1/configuration/schema",
            get(crate::configuration_admin::schema),
        )
        .route(
            "/api/v1/configuration/:id",
            get(crate::configuration_admin::effective),
        )
        .route("/health", get(handlers::health))
        .route("/api/v1/health", get(handlers::health))
        // Bond Reference Data CRUD
        .route(
            "/api/v1/bonds",
            get(reference::list_bonds).post(reference::create_bond),
        )
        .route("/api/v1/bonds/batch", post(reference::batch_create_bonds))
        .route("/api/v1/bonds/isin/:isin", get(reference::get_bond_by_isin))
        .route(
            "/api/v1/bonds/cusip/:cusip",
            get(reference::get_bond_by_cusip),
        )
        .route(
            "/api/v1/bonds/:instrument_id",
            get(reference::get_bond)
                .put(reference::update_bond)
                .delete(reference::delete_bond),
        )
        .route(
            "/api/v1/bonds/:instrument_id/versions/:version",
            get(reference::get_bond_version),
        )
        .route(
            "/api/v1/reference/commits/:key",
            get(reference::get_reference_commit),
        )
        .route(
            "/api/v1/reference/transactions",
            post(reference::commit_reference_transaction)
                .layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
        .route(
            "/api/v1/reference/resolve",
            post(reference::resolve_reference_at),
        )
        // Quotes (Pricing)
        .route(
            "/api/v1/quotes/:instrument_id",
            get(handlers::get_bond_quote),
        )
        .route("/api/v1/quote", post(handlers::price_single_bond))
        // Curves
        .route(
            "/api/v1/curves",
            get(handlers::list_curves).post(handlers::create_curve),
        )
        .route("/api/v1/curves/bootstrap", post(handlers::bootstrap_curve))
        .route(
            "/api/v1/curves/:curve_id",
            get(handlers::get_curve).delete(handlers::delete_curve),
        )
        .route(
            "/api/v1/curves/:curve_id/zero/:tenor",
            get(handlers::get_curve_zero_rate),
        )
        .route(
            "/api/v1/curves/:curve_id/discount/:tenor",
            get(handlers::get_curve_discount_factor),
        )
        .route(
            "/api/v1/curves/:curve_id/forward/:t1/:t2",
            get(handlers::get_curve_forward_rate),
        )
        // Batch Pricing
        .route("/api/v1/batch/price", post(handlers::batch_price))
        // ETF iNAV
        .route("/api/v1/etf/inav", post(handlers::calculate_inav))
        .route(
            "/api/v1/etf/inav/batch",
            post(handlers::batch_calculate_inav),
        )
        // ETF SEC Yield & Basket
        .route(
            "/api/v1/etf/sec-yield",
            post(handlers::calculate_sec_yield_handler),
        )
        .route(
            "/api/v1/etf/basket",
            post(handlers::build_creation_basket_handler),
        )
        .route(
            "/api/v1/etf/basket/analyze",
            post(handlers::analyze_basket_handler),
        )
        // Portfolio Analytics
        .route(
            "/api/v1/portfolio/analytics",
            post(handlers::calculate_portfolio_analytics),
        )
        .route(
            "/api/v1/portfolio/analytics/batch",
            post(handlers::batch_calculate_portfolio_analytics),
        )
        .route(
            "/api/v1/portfolio/duration-contribution",
            post(handlers::calculate_duration_contribution),
        )
        .route(
            "/api/v1/portfolio/key-rate-duration",
            post(handlers::calculate_key_rate_duration_handler),
        )
        .route(
            "/api/v1/portfolio/risk-contributions",
            post(handlers::calculate_risk_contributions),
        )
        // Portfolio Bucketing
        .route(
            "/api/v1/portfolio/buckets/sector",
            post(handlers::calculate_sector_bucketing),
        )
        .route(
            "/api/v1/portfolio/buckets/rating",
            post(handlers::calculate_rating_bucketing),
        )
        .route(
            "/api/v1/portfolio/buckets/maturity",
            post(handlers::calculate_maturity_bucketing),
        )
        .route(
            "/api/v1/portfolio/buckets/custom",
            post(handlers::calculate_custom_bucketing),
        )
        .route(
            "/api/v1/portfolio/buckets",
            post(handlers::calculate_all_bucketing),
        )
        // Benchmark Comparison
        .route(
            "/api/v1/benchmark/compare",
            post(handlers::compare_to_benchmark),
        )
        .route(
            "/api/v1/benchmark/active-weights",
            post(handlers::calculate_active_weights),
        )
        .route(
            "/api/v1/benchmark/tracking-error",
            post(handlers::calculate_tracking_error),
        )
        .route(
            "/api/v1/benchmark/attribution",
            post(handlers::calculate_attribution),
        )
        // Liquidity Analytics
        .route(
            "/api/v1/liquidity/metrics",
            post(handlers::calculate_liquidity_metrics_handler),
        )
        .route(
            "/api/v1/liquidity/distribution",
            post(handlers::calculate_liquidity_distribution),
        )
        .route(
            "/api/v1/liquidity/analysis",
            post(handlers::calculate_liquidity_analysis),
        )
        // Credit Quality Analytics
        .route(
            "/api/v1/credit/quality",
            post(handlers::calculate_credit_quality_handler),
        )
        .route(
            "/api/v1/credit/migration-risk",
            post(handlers::calculate_migration_risk_handler),
        )
        .route(
            "/api/v1/credit/analysis",
            post(handlers::calculate_credit_analysis),
        )
        // Books: durable position snapshots from an external book management system
        .route("/api/v1/books", get(books::list_books))
        .route(
            "/api/v1/books/:book_id",
            get(books::get_book)
                .put(books::put_book)
                .layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
        .route(
            "/api/v1/books/:book_id/versions/:version",
            get(books::get_book_version),
        )
        .route(
            "/api/v1/books/:book_id/history",
            get(books::get_book_history),
        )
        // Portfolio alias over books (one release)
        .route(
            "/api/v1/portfolios",
            get(books::list_portfolios).post(books::create_portfolio),
        )
        .route(
            "/api/v1/portfolios/batch",
            post(books::batch_create_portfolios),
        )
        .route(
            "/api/v1/portfolios/:portfolio_id",
            get(books::get_portfolio)
                .put(books::update_portfolio)
                .delete(books::delete_portfolio),
        )
        .route(
            "/api/v1/portfolios/:portfolio_id/positions",
            post(books::add_portfolio_position),
        )
        .route(
            "/api/v1/portfolios/:portfolio_id/positions/:instrument_id",
            axum::routing::put(books::update_portfolio_position)
                .delete(books::remove_portfolio_position),
        );
    let router = if websocket_enabled {
        router
            .route("/ws", get(websocket::ws_handler))
            .route("/api/v1/ws", get(websocket::ws_handler))
            .route("/api/v1/ws/status", get(websocket::ws_status))
    } else {
        router
    };
    router
        .with_state(state)
        .layer(axum::middleware::from_fn_with_state(
            access,
            crate::access::http::enforce,
        ))
}
