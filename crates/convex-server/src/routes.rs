//! Route definitions.

use std::sync::Arc;

use axum::routing::{get, post, put, delete};
use axum::Router;

use convex_engine::PricingEngine;

use crate::handlers::{self, AppState};
use crate::websocket::{self, WebSocketState};

/// Create the API router.
///
/// # Arguments
/// * `engine` - The pricing engine
pub fn create_router(engine: Arc<PricingEngine>) -> Router {
    let state = Arc::new(AppState {
        engine,
        ws_state: WebSocketState::new(),
    });

    Router::new()
        // Health
        .route("/health", get(handlers::health))
        .route("/api/v1/health", get(handlers::health))

        // Market Data (Real-time updates)
        .route("/api/v1/market-data/quote", post(handlers::market_data::update_quote))
        .route("/api/v1/market-data/curve", post(handlers::market_data::update_curve))

        // Admin / Configuration
        // Bond Configs
        .route("/api/v1/config/bond", get(handlers::config::list_bond_configs).post(handlers::config::create_bond_config))
        .route("/api/v1/config/bond/:config_id", get(handlers::config::get_bond_config).delete(handlers::config::delete_bond_config))

        // Curve Configs
        .route("/api/v1/config/curve", get(handlers::config::list_curve_configs).post(handlers::config::create_curve_config))
        .route("/api/v1/config/curve/:curve_id", get(handlers::config::get_curve_config).delete(handlers::config::delete_curve_config))

        // Overrides
        .route("/api/v1/config/override", get(handlers::config::list_overrides).post(handlers::config::create_override))
        .route("/api/v1/config/override/:instrument_id", get(handlers::config::get_override).delete(handlers::config::delete_override))

        // Bond Reference Data CRUD
        .route(
            "/api/v1/bonds",
            get(handlers::list_bonds).post(handlers::create_bond),
        )
        .route("/api/v1/bonds/batch", post(handlers::batch_create_bonds))
        .route("/api/v1/bonds/isin/:isin", get(handlers::get_bond_by_isin))
        .route(
            "/api/v1/bonds/cusip/:cusip",
            get(handlers::get_bond_by_cusip),
        )
        .route(
            "/api/v1/bonds/:instrument_id",
            get(handlers::get_bond)
                .put(handlers::update_bond)
                .delete(handlers::delete_bond),
        )
        // Quotes (Pricing)
        .route(
            "/api/v1/quotes/:instrument_id",
            get(handlers::get_bond_quote),
        )
        // .route("/api/v1/quote", post(handlers::price_single_bond))
        /*
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
        */
        /*
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
        // Stress Testing
        .route("/api/v1/stress/test", post(handlers::run_stress_test))
        .route(
            "/api/v1/stress/standard",
            post(handlers::run_standard_stress_test),
        )
        .route(
            "/api/v1/stress/single",
            post(handlers::run_single_stress_test),
        )
        .route(
            "/api/v1/stress/scenarios",
            get(handlers::list_standard_scenarios),
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
            "/api/v1/liquidity/days-to-liquidate",
            post(handlers::calculate_days_to_liquidate),
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
        */
        // Portfolio Reference Data CRUD
        .route(
            "/api/v1/portfolios",
            get(handlers::list_portfolios).post(handlers::create_portfolio),
        )
        .route(
            "/api/v1/portfolios/batch",
            post(handlers::batch_create_portfolios),
        )
        .route(
            "/api/v1/portfolios/:portfolio_id",
            get(handlers::get_portfolio)
                .put(handlers::update_portfolio)
                .delete(handlers::delete_portfolio),
        )
        .route(
            "/api/v1/portfolios/:portfolio_id/positions",
            post(handlers::add_portfolio_position),
        )
        .route(
            "/api/v1/portfolios/:portfolio_id/positions/:instrument_id",
            axum::routing::put(handlers::update_portfolio_position)
                .delete(handlers::remove_portfolio_position),
        )
        // WebSocket
        .route("/ws", get(websocket::ws_handler))
        .route("/api/v1/ws", get(websocket::ws_handler))
        .route("/api/v1/ws/status", get(websocket::ws_status))
        // State
        .with_state(state)
}
