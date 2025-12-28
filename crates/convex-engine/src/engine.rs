//! High-level pricing engine orchestration.
//!
//! The `PricingEngine` is the main entry point for the convex-engine crate.
//! It orchestrates all components:
//!
//! - Calculation graph for dependency-driven recalculation
//! - Curve cache for atomic curve updates
//! - Services for bond, curve, and pricing operations
//! - Enterprise patterns (circuit breakers, health checks)
//! - Streaming infrastructure for real-time updates
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_engine::{PricingEngine, PricingEngineConfig};
//!
//! let config = PricingEngineConfig::default();
//! let engine = PricingEngine::new(config).await?;
//!
//! // Start the engine
//! engine.start().await?;
//!
//! // Price a bond
//! let result = engine.price_bond("US912828Z229", None).await?;
//!
//! // Shutdown gracefully
//! engine.shutdown().await;
//! ```

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::cache::{CurveCache, CurveSnapshot};
use crate::runtime::{
    CircuitBreaker, CircuitBreakerConfig, GracefulShutdown, HealthCheck, HealthStatus,
    MetricsCollector, RateLimiter, RetryConfig, ServiceStatus,
};
use crate::error::{EngineError, EngineResult};
use crate::graph::{CalculationGraph, NodeId, NodeValue};
use crate::nodes::{BondPricingConfig, BondPricingNode, CurveConfig, CurveNode};
use crate::services::PricingResult;
use crate::streaming::{BondQuote, QuoteBook, QuoteUpdate, StreamPublisher};

// =============================================================================
// ENGINE CONFIGURATION
// =============================================================================

/// Configuration for the pricing engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingEngineConfig {
    /// Maximum curves to cache.
    pub max_cached_curves: usize,
    /// Curve TTL (0 = no expiration).
    pub curve_ttl_seconds: u64,
    /// Whether to auto-recalculate on quote updates.
    pub auto_recalculate: bool,
    /// Recalculation debounce interval.
    pub recalculate_debounce_ms: u64,
    /// Circuit breaker configuration.
    pub circuit_breaker: CircuitBreakerConfig,
    /// Retry configuration.
    pub retry: RetryConfig,
    /// Rate limit (requests per second, 0 = unlimited).
    pub rate_limit_rps: f64,
    /// Rate limit burst size.
    pub rate_limit_burst: u64,
    /// Graceful shutdown timeout.
    pub shutdown_timeout_seconds: u64,
    /// Metrics sample size.
    pub metrics_sample_size: usize,
}

impl Default for PricingEngineConfig {
    fn default() -> Self {
        Self {
            max_cached_curves: 100,
            curve_ttl_seconds: 3600, // 1 hour
            auto_recalculate: true,
            recalculate_debounce_ms: 100,
            circuit_breaker: CircuitBreakerConfig::default(),
            retry: RetryConfig::default(),
            rate_limit_rps: 1000.0,
            rate_limit_burst: 100,
            shutdown_timeout_seconds: 30,
            metrics_sample_size: 10000,
        }
    }
}

impl PricingEngineConfig {
    /// Creates a minimal configuration for testing.
    pub fn minimal() -> Self {
        Self {
            max_cached_curves: 10,
            curve_ttl_seconds: 0,
            auto_recalculate: false,
            recalculate_debounce_ms: 0,
            circuit_breaker: CircuitBreakerConfig::default(),
            retry: RetryConfig {
                max_attempts: 1,
                ..Default::default()
            },
            rate_limit_rps: 0.0,
            rate_limit_burst: 0,
            shutdown_timeout_seconds: 5,
            metrics_sample_size: 100,
        }
    }
}

// =============================================================================
// ENGINE STATE
// =============================================================================

/// Current state of the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineState {
    /// Engine is initializing.
    Initializing,
    /// Engine is running and ready.
    Running,
    /// Engine is shutting down.
    ShuttingDown,
    /// Engine has stopped.
    Stopped,
}

// =============================================================================
// PRICING ENGINE
// =============================================================================

/// High-level pricing engine that orchestrates all components.
pub struct PricingEngine {
    /// Configuration.
    config: PricingEngineConfig,
    /// Current state.
    state: RwLock<EngineState>,
    /// Calculation graph.
    graph: Arc<CalculationGraph>,
    /// Curve cache.
    curve_cache: Arc<CurveCache>,
    /// Quote book.
    quote_book: Arc<QuoteBook>,
    /// Analytics publisher.
    analytics_publisher: Arc<StreamPublisher<NodeValue>>,
    /// Circuit breaker for external calls.
    circuit_breaker: Arc<CircuitBreaker>,
    /// Rate limiter.
    rate_limiter: Option<Arc<RateLimiter>>,
    /// Metrics collector.
    metrics: Arc<MetricsCollector>,
    /// Graceful shutdown coordinator.
    shutdown: Arc<GracefulShutdown>,
    /// Start time.
    start_time: DateTime<Utc>,
}

impl PricingEngine {
    /// Creates a new pricing engine.
    pub fn new(config: PricingEngineConfig) -> Self {
        let mut curve_cache = CurveCache::with_max_size(config.max_cached_curves);
        if config.curve_ttl_seconds > 0 {
            curve_cache.set_ttl(Some(Duration::from_secs(config.curve_ttl_seconds)));
        }

        let rate_limiter = if config.rate_limit_rps > 0.0 {
            Some(Arc::new(RateLimiter::new(
                config.rate_limit_rps,
                config.rate_limit_burst,
            )))
        } else {
            None
        };

        Self {
            config: config.clone(),
            state: RwLock::new(EngineState::Initializing),
            graph: Arc::new(CalculationGraph::new()),
            curve_cache: Arc::new(curve_cache),
            quote_book: Arc::new(QuoteBook::new()),
            analytics_publisher: Arc::new(StreamPublisher::new(10000)),
            circuit_breaker: Arc::new(CircuitBreaker::new(config.circuit_breaker)),
            rate_limiter,
            metrics: Arc::new(MetricsCollector::new(config.metrics_sample_size)),
            shutdown: Arc::new(GracefulShutdown::new(Duration::from_secs(
                config.shutdown_timeout_seconds,
            ))),
            start_time: Utc::now(),
        }
    }

    /// Creates an engine with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(PricingEngineConfig::default())
    }

    /// Returns the engine configuration.
    pub fn config(&self) -> &PricingEngineConfig {
        &self.config
    }

    /// Returns the current state.
    pub async fn state(&self) -> EngineState {
        *self.state.read().await
    }

    /// Starts the engine.
    pub async fn start(&self) -> EngineResult<()> {
        let mut state = self.state.write().await;
        if *state != EngineState::Initializing {
            return Err(EngineError::InvalidConfiguration(
                "Engine already started".into(),
            ));
        }

        *state = EngineState::Running;
        tracing::info!("Pricing engine started");
        Ok(())
    }

    /// Shuts down the engine gracefully.
    pub async fn shutdown(&self) {
        {
            let mut state = self.state.write().await;
            *state = EngineState::ShuttingDown;
        }

        self.shutdown.shutdown();
        self.shutdown.wait_for_completion().await;

        {
            let mut state = self.state.write().await;
            *state = EngineState::Stopped;
        }

        tracing::info!("Pricing engine stopped");
    }

    /// Returns the calculation graph.
    pub fn graph(&self) -> &CalculationGraph {
        &self.graph
    }

    /// Returns the curve cache.
    pub fn curve_cache(&self) -> &CurveCache {
        &self.curve_cache
    }

    /// Returns the quote book.
    pub fn quote_book(&self) -> &QuoteBook {
        &self.quote_book
    }

    // =========================================================================
    // QUOTE MANAGEMENT
    // =========================================================================

    /// Updates a bond quote.
    pub async fn update_quote(&self, quote: BondQuote) -> EngineResult<()> {
        self.check_running().await?;

        let instrument_id = quote.instrument_id.clone();
        self.quote_book.update(quote);

        // Invalidate dependent nodes
        self.graph.invalidate(&NodeId::quote(&instrument_id));

        // Auto-recalculate if enabled
        if self.config.auto_recalculate {
            let _ = self.graph.recalculate();
        }

        Ok(())
    }

    /// Gets the latest quote for an instrument.
    pub fn get_quote(&self, instrument_id: &str) -> Option<BondQuote> {
        self.quote_book.get(instrument_id)
    }

    // =========================================================================
    // CURVE MANAGEMENT
    // =========================================================================

    /// Registers a curve in the engine.
    pub async fn register_curve(&self, config: CurveConfig) -> EngineResult<()> {
        self.check_running().await?;

        let node = Arc::new(CurveNode::new(config));
        self.graph.register(node);

        Ok(())
    }

    /// Updates a curve in the cache.
    pub fn update_curve(
        &self,
        curve_id: &str,
        curve: convex_curves::CurveRef,
    ) -> Option<convex_curves::CurveRef> {
        let old = self.curve_cache.swap(curve_id, curve);
        self.graph.invalidate(&NodeId::curve(curve_id));

        if self.config.auto_recalculate {
            let _ = self.graph.recalculate();
        }

        old
    }

    /// Gets a curve from the cache.
    pub fn get_curve(&self, curve_id: &str) -> Option<convex_curves::CurveRef> {
        self.curve_cache.get(curve_id)
    }

    /// Creates a curve snapshot for consistent pricing.
    pub fn curve_snapshot(&self) -> CurveSnapshot {
        CurveSnapshot::from_cache(&self.curve_cache)
    }

    // =========================================================================
    // BOND MANAGEMENT
    // =========================================================================

    /// Registers a bond for pricing.
    pub async fn register_bond(&self, config: BondPricingConfig) -> EngineResult<()> {
        self.check_running().await?;

        let node = Arc::new(BondPricingNode::new(config));
        self.graph.register(node);

        Ok(())
    }

    /// Unregisters a bond.
    pub fn unregister_bond(&self, instrument_id: &str) -> bool {
        self.graph.unregister(&NodeId::bond(instrument_id))
    }

    // =========================================================================
    // PRICING
    // =========================================================================

    /// Prices a bond.
    pub async fn price_bond(
        &self,
        instrument_id: &str,
        settlement_date: Option<NaiveDate>,
    ) -> EngineResult<PricingResult> {
        self.check_running().await?;

        let start = std::time::Instant::now();

        // Rate limiting
        if let Some(limiter) = &self.rate_limiter {
            limiter.acquire().await;
        }

        // Register operation for graceful shutdown
        let _guard = self
            .shutdown
            .register_operation()
            .ok_or(EngineError::ShuttingDown)?;

        // Ensure bond is registered
        if self.graph.get_cached(&NodeId::bond(instrument_id)).is_none() {
            // Register with default config
            let config = BondPricingConfig::new(instrument_id);
            self.register_bond(config).await?;
        }

        // Recalculate
        self.graph.recalculate()?;

        // Get result
        let result = match self.graph.get_cached(&NodeId::bond(instrument_id)) {
            Some(NodeValue::BondAnalytics {
                clean_price,
                dirty_price,
                ytm,
                z_spread,
                modified_duration,
                dv01,
                timestamp,
                ..
            }) => PricingResult {
                instrument_id: instrument_id.to_string(),
                settlement_date: settlement_date
                    .unwrap_or_else(|| Utc::now().naive_utc().date()),
                clean_price,
                dirty_price,
                accrued_interest: None,
                ytm,
                ytw: None,
                modified_duration,
                macaulay_duration: None,
                convexity: None,
                dv01,
                z_spread,
                oas: None,
                timestamp,
                warnings: Vec::new(),
            },
            _ => {
                return Err(EngineError::calculation_failed(
                    instrument_id,
                    "No analytics available",
                ));
            }
        };

        let elapsed_us = start.elapsed().as_micros() as u64;
        self.metrics.record_success(elapsed_us);

        // Publish analytics
        if let Some(value) = self.graph.get_cached(&NodeId::bond(instrument_id)) {
            self.analytics_publisher.publish(value);
        }

        Ok(result)
    }

    /// Prices multiple bonds.
    pub async fn price_bonds(
        &self,
        instrument_ids: &[&str],
        settlement_date: Option<NaiveDate>,
    ) -> EngineResult<Vec<PricingResult>> {
        let mut results = Vec::with_capacity(instrument_ids.len());

        for &id in instrument_ids {
            match self.price_bond(id, settlement_date).await {
                Ok(result) => results.push(result),
                Err(e) => {
                    self.metrics.record_error();
                    tracing::warn!(instrument_id = id, error = %e, "Bond pricing failed");
                    // Continue with other bonds
                }
            }
        }

        Ok(results)
    }

    // =========================================================================
    // ANALYTICS
    // =========================================================================

    /// Calculates yield from price.
    pub async fn calculate_yield(
        &self,
        instrument_id: &str,
        price: Decimal,
        _settlement_date: NaiveDate,
    ) -> EngineResult<Decimal> {
        self.check_running().await?;

        // Update quote with the price
        let quote = BondQuote::new(instrument_id).with_bid(price, 0).with_ask(price, 0);
        self.update_quote(quote).await?;

        // Price the bond (which calculates yield)
        let result = self.price_bond(instrument_id, None).await?;

        result
            .ytm
            .ok_or_else(|| EngineError::calculation_failed(instrument_id, "Yield calculation failed"))
    }

    /// Calculates price from yield.
    pub async fn calculate_price(
        &self,
        instrument_id: &str,
        _yield_value: Decimal,
        _settlement_date: NaiveDate,
    ) -> EngineResult<Decimal> {
        self.check_running().await?;

        // In a full implementation, this would use convex-analytics
        // For now, return a placeholder
        Err(EngineError::calculation_failed(
            instrument_id,
            "Price calculation not yet implemented",
        ))
    }

    // =========================================================================
    // RECALCULATION
    // =========================================================================

    /// Triggers a full recalculation.
    pub async fn recalculate(&self) -> EngineResult<Vec<NodeId>> {
        self.check_running().await?;
        self.graph.recalculate()
    }

    /// Forces recalculation of specific nodes.
    pub async fn recalculate_nodes(&self, node_ids: &[NodeId]) -> EngineResult<Vec<NodeId>> {
        self.check_running().await?;

        for node_id in node_ids {
            self.graph.invalidate(node_id);
        }

        self.graph.recalculate()
    }

    /// Clears all caches and marks everything dirty.
    pub async fn clear_caches(&self) -> EngineResult<()> {
        self.check_running().await?;

        self.curve_cache.clear();
        self.graph.clear_cache();

        Ok(())
    }

    // =========================================================================
    // HEALTH & METRICS
    // =========================================================================

    /// Returns a health check.
    pub async fn health_check(&self) -> HealthCheck {
        let state = self.state().await;

        let engine_status = match state {
            EngineState::Running => ServiceStatus::healthy("engine"),
            EngineState::Initializing => ServiceStatus {
                name: "engine".into(),
                status: HealthStatus::Degraded,
                message: Some("Initializing".into()),
                last_checked: Utc::now(),
                response_time_ms: None,
            },
            EngineState::ShuttingDown | EngineState::Stopped => ServiceStatus::unhealthy("engine", "Not running"),
        };

        let graph_status = {
            let stats = self.graph.stats();
            if stats.dirty_count > 100 {
                ServiceStatus {
                    name: "calculation_graph".into(),
                    status: HealthStatus::Degraded,
                    message: Some(format!("{} dirty nodes", stats.dirty_count)),
                    last_checked: Utc::now(),
                    response_time_ms: None,
                }
            } else {
                ServiceStatus::healthy("calculation_graph")
            }
        };

        let cache_status = {
            let stats = self.curve_cache.stats();
            if stats.hit_rate() < 0.5 && stats.hits + stats.misses > 100 {
                ServiceStatus {
                    name: "curve_cache".into(),
                    status: HealthStatus::Degraded,
                    message: Some(format!("Low hit rate: {:.1}%", stats.hit_rate() * 100.0)),
                    last_checked: Utc::now(),
                    response_time_ms: None,
                }
            } else {
                ServiceStatus::healthy("curve_cache")
            }
        };

        let circuit_status = match self.circuit_breaker.state() {
            crate::runtime::CircuitState::Open => {
                ServiceStatus::unhealthy("circuit_breaker", "Circuit open")
            }
            crate::runtime::CircuitState::HalfOpen => ServiceStatus {
                name: "circuit_breaker".into(),
                status: HealthStatus::Degraded,
                message: Some("Half-open".into()),
                last_checked: Utc::now(),
                response_time_ms: None,
            },
            crate::runtime::CircuitState::Closed => ServiceStatus::healthy("circuit_breaker"),
        };

        HealthCheck::from_components(vec![
            engine_status,
            graph_status,
            cache_status,
            circuit_status,
        ])
        .with_version(env!("CARGO_PKG_VERSION"))
    }

    /// Returns engine statistics.
    pub fn stats(&self) -> EngineStats {
        EngineStats {
            uptime_seconds: (Utc::now() - self.start_time).num_seconds() as u64,
            graph_stats: self.graph.stats(),
            cache_stats: self.curve_cache.stats(),
            quote_book_stats: self.quote_book.stats(),
            metrics: self.metrics.snapshot(),
            circuit_breaker: self.circuit_breaker.stats(),
        }
    }

    /// Subscribes to analytics updates.
    pub fn subscribe_analytics(
        &self,
    ) -> crate::streaming::StreamSubscriber<NodeValue> {
        self.analytics_publisher.subscribe()
    }

    /// Subscribes to quote updates.
    pub fn subscribe_quotes(&self) -> crate::streaming::StreamSubscriber<QuoteUpdate> {
        self.quote_book.subscribe()
    }

    // =========================================================================
    // INTERNAL
    // =========================================================================

    /// Checks that the engine is running.
    async fn check_running(&self) -> EngineResult<()> {
        let state = self.state().await;
        match state {
            EngineState::Running => Ok(()),
            EngineState::Initializing => Err(EngineError::InvalidConfiguration(
                "Engine not started".into(),
            )),
            EngineState::ShuttingDown | EngineState::Stopped => Err(EngineError::ShuttingDown),
        }
    }
}

// =============================================================================
// ENGINE STATISTICS
// =============================================================================

/// Comprehensive engine statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStats {
    /// Uptime in seconds.
    pub uptime_seconds: u64,
    /// Graph statistics.
    pub graph_stats: crate::graph::GraphStats,
    /// Cache statistics.
    pub cache_stats: crate::cache::CacheStats,
    /// Quote book statistics.
    pub quote_book_stats: crate::streaming::QuoteBookStats,
    /// Request metrics.
    pub metrics: crate::runtime::MetricsSnapshot,
    /// Circuit breaker stats.
    pub circuit_breaker: crate::runtime::CircuitBreakerStats,
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_engine_lifecycle() {
        let engine = PricingEngine::new(PricingEngineConfig::minimal());

        assert_eq!(engine.state().await, EngineState::Initializing);

        engine.start().await.unwrap();
        assert_eq!(engine.state().await, EngineState::Running);

        engine.shutdown().await;
        assert_eq!(engine.state().await, EngineState::Stopped);
    }

    #[tokio::test]
    async fn test_engine_quote_update() {
        let engine = PricingEngine::new(PricingEngineConfig::minimal());
        engine.start().await.unwrap();

        let quote = BondQuote::new("TEST001")
            .with_bid(dec!(99.50), 1_000_000)
            .with_ask(dec!(99.75), 500_000);

        engine.update_quote(quote).await.unwrap();

        let retrieved = engine.get_quote("TEST001");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().bid_price, Some(dec!(99.50)));
    }

    #[tokio::test]
    async fn test_engine_health_check() {
        let engine = PricingEngine::new(PricingEngineConfig::minimal());
        engine.start().await.unwrap();

        let health = engine.health_check().await;
        assert!(health.is_ready());
        assert!(health.is_live());
    }

    #[tokio::test]
    async fn test_engine_stats() {
        let engine = PricingEngine::new(PricingEngineConfig::minimal());
        engine.start().await.unwrap();

        let stats = engine.stats();
        let _ = stats.uptime_seconds; // Verify field exists
        assert_eq!(stats.graph_stats.node_count, 0);
    }

    #[tokio::test]
    async fn test_engine_not_running() {
        let engine = PricingEngine::new(PricingEngineConfig::minimal());

        // Should fail when not started
        let result = engine.update_quote(BondQuote::new("TEST")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_engine_shutdown_blocks_new_ops() {
        let engine = PricingEngine::new(PricingEngineConfig::minimal());
        engine.start().await.unwrap();
        engine.shutdown().await;

        // Should fail after shutdown
        let result = engine.update_quote(BondQuote::new("TEST")).await;
        assert!(result.is_err());
    }
}
