//! Runtime patterns for production deployment.
//!
//! This module provides production-grade patterns for:
//!
//! - **Health Checks**: Liveness and readiness probes for container orchestration
//! - **Circuit Breaker**: Fail-fast protection for external dependencies
//! - **Rate Limiting**: Protect against resource exhaustion
//! - **Retry Logic**: Configurable retry with exponential backoff
//! - **Graceful Shutdown**: Clean termination with in-flight request handling
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_engine::runtime::{CircuitBreaker, CircuitBreakerConfig};
//!
//! let breaker = CircuitBreaker::new(CircuitBreakerConfig {
//!     failure_threshold: 5,
//!     success_threshold: 3,
//!     timeout: Duration::from_secs(30),
//!     half_open_max_calls: 1,
//! });
//!
//! // Execute with circuit breaker protection
//! let result = breaker.call(|| async {
//!     external_service.call().await
//! }).await;
//! ```

use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tokio::sync::{Semaphore, broadcast};
use tokio::time::sleep;

use crate::error::EngineError;

// =============================================================================
// HEALTH CHECK
// =============================================================================

/// Health status of a service component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Component is healthy and ready to serve requests.
    Healthy,
    /// Component is degraded but still functional.
    Degraded,
    /// Component is unhealthy and cannot serve requests.
    Unhealthy,
    /// Component status is unknown.
    Unknown,
}

impl HealthStatus {
    /// Returns true if the status indicates the service is operational.
    pub fn is_operational(&self) -> bool {
        matches!(self, Self::Healthy | Self::Degraded)
    }
}

/// Status of an individual service component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    /// Service name.
    pub name: String,
    /// Current health status.
    pub status: HealthStatus,
    /// Optional message.
    pub message: Option<String>,
    /// Last check timestamp.
    pub last_checked: DateTime<Utc>,
    /// Response time in milliseconds.
    pub response_time_ms: Option<u64>,
}

impl ServiceStatus {
    /// Creates a healthy status.
    pub fn healthy(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: HealthStatus::Healthy,
            message: None,
            last_checked: Utc::now(),
            response_time_ms: None,
        }
    }

    /// Creates an unhealthy status.
    pub fn unhealthy(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: HealthStatus::Unhealthy,
            message: Some(message.into()),
            last_checked: Utc::now(),
            response_time_ms: None,
        }
    }

    /// Sets the response time.
    pub fn with_response_time(mut self, ms: u64) -> Self {
        self.response_time_ms = Some(ms);
        self
    }
}

/// Health check result aggregating all component statuses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Overall status.
    pub status: HealthStatus,
    /// Individual component statuses.
    pub components: Vec<ServiceStatus>,
    /// Check timestamp.
    pub timestamp: DateTime<Utc>,
    /// Version information.
    pub version: Option<String>,
}

impl HealthCheck {
    /// Creates a new health check from component statuses.
    pub fn from_components(components: Vec<ServiceStatus>) -> Self {
        let status = if components.is_empty() {
            HealthStatus::Unknown
        } else if components.iter().all(|c| c.status == HealthStatus::Healthy) {
            HealthStatus::Healthy
        } else if components.iter().any(|c| c.status == HealthStatus::Unhealthy) {
            HealthStatus::Unhealthy
        } else {
            HealthStatus::Degraded
        };

        Self {
            status,
            components,
            timestamp: Utc::now(),
            version: None,
        }
    }

    /// Adds version information.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Returns true if the health check indicates the service is ready.
    pub fn is_ready(&self) -> bool {
        self.status.is_operational()
    }

    /// Returns true if the service is live (not completely failed).
    pub fn is_live(&self) -> bool {
        !matches!(self.status, HealthStatus::Unhealthy)
    }
}

// =============================================================================
// CIRCUIT BREAKER
// =============================================================================

/// State of the circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    /// Circuit is closed - requests are allowed.
    Closed,
    /// Circuit is open - requests are blocked.
    Open,
    /// Circuit is half-open - limited requests allowed for testing.
    HalfOpen,
}

/// Configuration for the circuit breaker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit.
    pub failure_threshold: u32,
    /// Number of successes required to close the circuit.
    pub success_threshold: u32,
    /// How long to wait before transitioning from Open to HalfOpen.
    pub timeout: Duration,
    /// Maximum number of calls allowed in HalfOpen state.
    pub half_open_max_calls: u32,
    /// Name for logging/metrics.
    pub name: Option<String>,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            timeout: Duration::from_secs(30),
            half_open_max_calls: 1,
            name: None,
        }
    }
}

/// Circuit breaker for protecting against cascading failures.
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: RwLock<CircuitState>,
    failure_count: AtomicU64,
    success_count: AtomicU64,
    last_failure_time: RwLock<Option<Instant>>,
    half_open_calls: AtomicU64,
}

impl CircuitBreaker {
    /// Creates a new circuit breaker with the given configuration.
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: RwLock::new(CircuitState::Closed),
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            last_failure_time: RwLock::new(None),
            half_open_calls: AtomicU64::new(0),
        }
    }

    /// Creates a circuit breaker with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Returns the current state of the circuit.
    pub fn state(&self) -> CircuitState {
        self.maybe_transition_to_half_open();
        *self.state.read()
    }

    /// Returns true if the circuit allows requests.
    pub fn is_closed(&self) -> bool {
        self.state() == CircuitState::Closed
    }

    /// Executes an async operation with circuit breaker protection.
    pub async fn call<F, Fut, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: From<EngineError>,
    {
        // Check if we can proceed
        if !self.can_proceed() {
            return Err(EngineError::ServiceUnavailable(
                self.config
                    .name
                    .clone()
                    .unwrap_or_else(|| "Circuit open".into()),
            )
            .into());
        }

        // Execute the function
        match f().await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(e) => {
                self.record_failure();
                Err(e)
            }
        }
    }

    /// Checks if the circuit allows a request.
    fn can_proceed(&self) -> bool {
        self.maybe_transition_to_half_open();

        match *self.state.read() {
            CircuitState::Closed => true,
            CircuitState::Open => false,
            CircuitState::HalfOpen => {
                let calls = self.half_open_calls.fetch_add(1, Ordering::SeqCst);
                calls < self.config.half_open_max_calls as u64
            }
        }
    }

    /// Records a successful call.
    fn record_success(&self) {
        let mut state = self.state.write();

        match *state {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::SeqCst);
            }
            CircuitState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;
                if successes >= self.config.success_threshold as u64 {
                    *state = CircuitState::Closed;
                    self.reset_counts();
                    tracing::info!(
                        name = ?self.config.name,
                        "Circuit breaker closed"
                    );
                }
            }
            CircuitState::Open => {
                // Shouldn't happen, but handle gracefully
            }
        }
    }

    /// Records a failed call.
    fn record_failure(&self) {
        let mut state = self.state.write();
        *self.last_failure_time.write() = Some(Instant::now());

        match *state {
            CircuitState::Closed => {
                let failures = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
                if failures >= self.config.failure_threshold as u64 {
                    *state = CircuitState::Open;
                    tracing::warn!(
                        name = ?self.config.name,
                        failures,
                        "Circuit breaker opened"
                    );
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open reopens the circuit
                *state = CircuitState::Open;
                self.reset_counts();
                tracing::warn!(
                    name = ?self.config.name,
                    "Circuit breaker reopened"
                );
            }
            CircuitState::Open => {
                // Already open
            }
        }
    }

    /// Checks if we should transition from Open to HalfOpen.
    fn maybe_transition_to_half_open(&self) {
        let state = *self.state.read();
        if state != CircuitState::Open {
            return;
        }

        let last_failure = *self.last_failure_time.read();
        if let Some(last) = last_failure {
            if last.elapsed() >= self.config.timeout {
                let mut state = self.state.write();
                if *state == CircuitState::Open {
                    *state = CircuitState::HalfOpen;
                    self.half_open_calls.store(0, Ordering::SeqCst);
                    self.success_count.store(0, Ordering::SeqCst);
                    tracing::info!(
                        name = ?self.config.name,
                        "Circuit breaker half-open"
                    );
                }
            }
        }
    }

    /// Resets all counts.
    fn reset_counts(&self) {
        self.failure_count.store(0, Ordering::SeqCst);
        self.success_count.store(0, Ordering::SeqCst);
        self.half_open_calls.store(0, Ordering::SeqCst);
    }

    /// Manually opens the circuit.
    pub fn trip(&self) {
        let mut state = self.state.write();
        *state = CircuitState::Open;
        *self.last_failure_time.write() = Some(Instant::now());
        tracing::warn!(
            name = ?self.config.name,
            "Circuit breaker manually tripped"
        );
    }

    /// Manually closes the circuit.
    pub fn reset(&self) {
        let mut state = self.state.write();
        *state = CircuitState::Closed;
        self.reset_counts();
        tracing::info!(
            name = ?self.config.name,
            "Circuit breaker manually reset"
        );
    }

    /// Returns statistics about the circuit breaker.
    pub fn stats(&self) -> CircuitBreakerStats {
        CircuitBreakerStats {
            state: self.state(),
            failure_count: self.failure_count.load(Ordering::SeqCst),
            success_count: self.success_count.load(Ordering::SeqCst),
        }
    }
}

/// Statistics for a circuit breaker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerStats {
    /// Current state.
    pub state: CircuitState,
    /// Current failure count.
    pub failure_count: u64,
    /// Current success count (in half-open state).
    pub success_count: u64,
}

// =============================================================================
// RETRY CONFIGURATION
// =============================================================================

/// Configuration for retry logic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
    /// Initial delay between retries.
    pub initial_delay: Duration,
    /// Maximum delay between retries.
    pub max_delay: Duration,
    /// Multiplier for exponential backoff.
    pub backoff_multiplier: f64,
    /// Add random jitter to delays.
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

impl RetryConfig {
    /// Calculates the delay for a given attempt number.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base_delay = self.initial_delay.as_millis() as f64
            * self.backoff_multiplier.powi(attempt as i32);
        let delay_ms = base_delay.min(self.max_delay.as_millis() as f64);

        let final_delay_ms = if self.jitter {
            // Add up to 25% jitter
            let jitter_factor = 1.0 + (rand_jitter() * 0.25);
            delay_ms * jitter_factor
        } else {
            delay_ms
        };

        Duration::from_millis(final_delay_ms as u64)
    }

    /// Executes an async operation with retries.
    pub async fn execute<F, Fut, T, E>(&self, mut f: F) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::fmt::Debug,
    {
        let mut attempt = 0;

        loop {
            match f().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    attempt += 1;
                    if attempt >= self.max_attempts {
                        tracing::warn!(
                            attempt,
                            max_attempts = self.max_attempts,
                            error = ?e,
                            "All retry attempts exhausted"
                        );
                        return Err(e);
                    }

                    let delay = self.delay_for_attempt(attempt);
                    tracing::debug!(
                        attempt,
                        delay_ms = delay.as_millis(),
                        error = ?e,
                        "Retrying after delay"
                    );
                    sleep(delay).await;
                }
            }
        }
    }
}

/// Simple pseudo-random jitter (no external dependency).
fn rand_jitter() -> f64 {
    // Use current time nanoseconds for simple randomness
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (nanos % 1000) as f64 / 1000.0
}

// =============================================================================
// RATE LIMITER
// =============================================================================

/// Token bucket rate limiter.
pub struct RateLimiter {
    /// Maximum tokens (burst size).
    capacity: u64,
    /// Tokens added per second.
    refill_rate: f64,
    /// Current token count.
    tokens: Mutex<f64>,
    /// Last refill time.
    last_refill: Mutex<Instant>,
    /// Semaphore for async waiting (reserved for future use).
    #[allow(dead_code)]
    semaphore: Semaphore,
}

impl RateLimiter {
    /// Creates a new rate limiter.
    ///
    /// # Arguments
    ///
    /// * `requests_per_second` - Maximum sustained request rate
    /// * `burst_size` - Maximum burst size
    pub fn new(requests_per_second: f64, burst_size: u64) -> Self {
        Self {
            capacity: burst_size,
            refill_rate: requests_per_second,
            tokens: Mutex::new(burst_size as f64),
            last_refill: Mutex::new(Instant::now()),
            semaphore: Semaphore::new(burst_size as usize),
        }
    }

    /// Attempts to acquire a token without blocking.
    ///
    /// Returns true if a token was acquired, false if rate limited.
    pub fn try_acquire(&self) -> bool {
        self.refill();

        let mut tokens = self.tokens.lock();
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Acquires a token, blocking if necessary.
    pub async fn acquire(&self) {
        loop {
            if self.try_acquire() {
                return;
            }
            // Wait a bit and try again
            sleep(Duration::from_millis(10)).await;
        }
    }

    /// Refills tokens based on elapsed time.
    fn refill(&self) {
        let mut tokens = self.tokens.lock();
        let mut last_refill = self.last_refill.lock();

        let elapsed = last_refill.elapsed();
        let new_tokens = elapsed.as_secs_f64() * self.refill_rate;

        *tokens = (*tokens + new_tokens).min(self.capacity as f64);
        *last_refill = Instant::now();
    }

    /// Returns the current token count.
    pub fn available_tokens(&self) -> f64 {
        self.refill();
        *self.tokens.lock()
    }

    /// Returns statistics about the rate limiter.
    pub fn stats(&self) -> RateLimiterStats {
        self.refill();
        RateLimiterStats {
            capacity: self.capacity,
            refill_rate: self.refill_rate,
            available_tokens: *self.tokens.lock(),
        }
    }
}

/// Statistics for a rate limiter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimiterStats {
    /// Maximum capacity.
    pub capacity: u64,
    /// Refill rate (tokens per second).
    pub refill_rate: f64,
    /// Current available tokens.
    pub available_tokens: f64,
}

// =============================================================================
// GRACEFUL SHUTDOWN
// =============================================================================

/// Coordinates graceful shutdown across components.
pub struct GracefulShutdown {
    /// Whether shutdown has been initiated.
    shutdown_initiated: AtomicBool,
    /// Broadcast channel for shutdown signal.
    shutdown_tx: broadcast::Sender<()>,
    /// Number of active operations.
    active_operations: AtomicU64,
    /// Timeout for graceful shutdown.
    timeout: Duration,
}

impl GracefulShutdown {
    /// Creates a new graceful shutdown coordinator.
    pub fn new(timeout: Duration) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            shutdown_initiated: AtomicBool::new(false),
            shutdown_tx,
            active_operations: AtomicU64::new(0),
            timeout,
        }
    }

    /// Creates with default 30-second timeout.
    pub fn with_defaults() -> Self {
        Self::new(Duration::from_secs(30))
    }

    /// Returns true if shutdown has been initiated.
    pub fn is_shutting_down(&self) -> bool {
        self.shutdown_initiated.load(Ordering::SeqCst)
    }

    /// Initiates shutdown.
    pub fn shutdown(&self) {
        if self
            .shutdown_initiated
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let _ = self.shutdown_tx.send(());
            tracing::info!("Graceful shutdown initiated");
        }
    }

    /// Returns a receiver for shutdown notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    /// Registers an active operation.
    pub fn register_operation(&self) -> Option<OperationGuard<'_>> {
        if self.is_shutting_down() {
            None
        } else {
            self.active_operations.fetch_add(1, Ordering::SeqCst);
            Some(OperationGuard { shutdown: self })
        }
    }

    /// Returns the number of active operations.
    pub fn active_operations(&self) -> u64 {
        self.active_operations.load(Ordering::SeqCst)
    }

    /// Waits for all operations to complete or timeout.
    pub async fn wait_for_completion(&self) {
        let start = Instant::now();

        while self.active_operations() > 0 {
            if start.elapsed() >= self.timeout {
                tracing::warn!(
                    active = self.active_operations(),
                    "Shutdown timeout reached, forcing termination"
                );
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }

        tracing::info!("All operations completed, shutdown complete");
    }
}

/// Guard that tracks an active operation.
pub struct OperationGuard<'a> {
    shutdown: &'a GracefulShutdown,
}

impl<'a> Drop for OperationGuard<'a> {
    fn drop(&mut self) {
        self.shutdown
            .active_operations
            .fetch_sub(1, Ordering::SeqCst);
    }
}

// =============================================================================
// METRICS COLLECTOR
// =============================================================================

/// Simple metrics collector for engine statistics.
pub struct MetricsCollector {
    /// Request count.
    request_count: AtomicU64,
    /// Error count.
    error_count: AtomicU64,
    /// Response times (circular buffer).
    response_times: Mutex<VecDeque<u64>>,
    /// Maximum response times to track.
    max_samples: usize,
}

impl MetricsCollector {
    /// Creates a new metrics collector.
    pub fn new(max_samples: usize) -> Self {
        Self {
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            response_times: Mutex::new(VecDeque::with_capacity(max_samples)),
            max_samples,
        }
    }

    /// Records a successful request.
    pub fn record_success(&self, response_time_us: u64) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.record_response_time(response_time_us);
    }

    /// Records a failed request.
    pub fn record_error(&self) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a response time.
    fn record_response_time(&self, us: u64) {
        let mut times = self.response_times.lock();
        if times.len() >= self.max_samples {
            times.pop_front();
        }
        times.push_back(us);
    }

    /// Returns the current metrics snapshot.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let times = self.response_times.lock();
        let request_count = self.request_count.load(Ordering::Relaxed);
        let error_count = self.error_count.load(Ordering::Relaxed);

        let (avg_response_time_us, p99_response_time_us) = if times.is_empty() {
            (0, 0)
        } else {
            let mut sorted: Vec<_> = times.iter().copied().collect();
            sorted.sort_unstable();
            let avg = sorted.iter().sum::<u64>() / sorted.len() as u64;
            let p99_idx = (sorted.len() as f64 * 0.99) as usize;
            let p99 = sorted.get(p99_idx).copied().unwrap_or(0);
            (avg, p99)
        };

        MetricsSnapshot {
            request_count,
            error_count,
            error_rate: if request_count > 0 {
                error_count as f64 / request_count as f64
            } else {
                0.0
            },
            avg_response_time_us,
            p99_response_time_us,
        }
    }
}

/// Snapshot of collected metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Total request count.
    pub request_count: u64,
    /// Total error count.
    pub error_count: u64,
    /// Error rate (0.0 to 1.0).
    pub error_rate: f64,
    /// Average response time in microseconds.
    pub avg_response_time_us: u64,
    /// 99th percentile response time in microseconds.
    pub p99_response_time_us: u64,
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status() {
        assert!(HealthStatus::Healthy.is_operational());
        assert!(HealthStatus::Degraded.is_operational());
        assert!(!HealthStatus::Unhealthy.is_operational());
    }

    #[test]
    fn test_health_check() {
        let components = vec![
            ServiceStatus::healthy("database"),
            ServiceStatus::healthy("cache"),
        ];
        let check = HealthCheck::from_components(components);

        assert_eq!(check.status, HealthStatus::Healthy);
        assert!(check.is_ready());
        assert!(check.is_live());
    }

    #[test]
    fn test_health_check_degraded() {
        let components = vec![
            ServiceStatus::healthy("database"),
            ServiceStatus {
                name: "cache".into(),
                status: HealthStatus::Degraded,
                message: Some("High latency".into()),
                last_checked: Utc::now(),
                response_time_ms: Some(500),
            },
        ];
        let check = HealthCheck::from_components(components);

        assert_eq!(check.status, HealthStatus::Degraded);
        assert!(check.is_ready());
    }

    #[test]
    fn test_circuit_breaker_closed() {
        let breaker = CircuitBreaker::with_defaults();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.is_closed());
    }

    #[test]
    fn test_circuit_breaker_opens_on_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // Record failures
        for _ in 0..3 {
            breaker.record_failure();
        }

        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[test]
    fn test_retry_delay() {
        let config = RetryConfig {
            initial_delay: Duration::from_millis(100),
            backoff_multiplier: 2.0,
            jitter: false,
            ..Default::default()
        };

        assert_eq!(config.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(config.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(config.delay_for_attempt(2), Duration::from_millis(400));
    }

    #[test]
    fn test_rate_limiter() {
        let limiter = RateLimiter::new(10.0, 5);

        // Should be able to acquire burst_size tokens immediately
        for _ in 0..5 {
            assert!(limiter.try_acquire());
        }

        // Should be rate limited now
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn test_graceful_shutdown() {
        let shutdown = GracefulShutdown::with_defaults();

        assert!(!shutdown.is_shutting_down());

        // Register an operation
        let _guard = shutdown.register_operation();
        assert_eq!(shutdown.active_operations(), 1);

        // Initiate shutdown
        shutdown.shutdown();
        assert!(shutdown.is_shutting_down());

        // Cannot register new operations during shutdown
        assert!(shutdown.register_operation().is_none());
    }

    #[test]
    fn test_metrics_collector() {
        let collector = MetricsCollector::new(100);

        collector.record_success(100);
        collector.record_success(200);
        collector.record_error();

        let snapshot = collector.snapshot();
        assert_eq!(snapshot.request_count, 3);
        assert_eq!(snapshot.error_count, 1);
        assert!((snapshot.error_rate - 0.333).abs() < 0.01);
    }
}
