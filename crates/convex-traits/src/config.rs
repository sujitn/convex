//! Configuration source traits.
//!
//! Configuration contains:
//! - Model parameters (tree steps, mean reversion)
//! - Curve ID references (not curve data itself)
//! - Update frequencies
//! - Pricing model selection
//!
//! Configuration does NOT contain:
//! - Actual market data values
//! - Bond terms (that's reference data)

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::TraitError;

// =============================================================================
// UPDATE FREQUENCY
// =============================================================================

/// Update frequency for calculation nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UpdateFrequency {
    /// Recalculate immediately on any input change.
    Immediate,

    /// Recalculate at most once per interval (debounce).
    Throttled {
        /// Minimum interval between calculations
        interval: Duration,
    },

    /// Recalculate on fixed schedule regardless of input changes.
    Interval {
        /// Fixed interval
        interval: Duration,
    },

    /// Only recalculate when explicitly requested.
    OnDemand,

    /// Recalculate once per day at specified time.
    EndOfDay {
        /// Time in HH:MM:SS format
        time: String,
    },

    /// Recalculate on cron schedule.
    Scheduled {
        /// Cron expression
        cron: String,
    },
}

impl Default for UpdateFrequency {
    fn default() -> Self {
        UpdateFrequency::Immediate
    }
}

// =============================================================================
// NODE CONFIG
// =============================================================================

/// Configuration for a calculation node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Update frequency
    pub frequency: UpdateFrequency,

    /// Priority (higher = calculated first within same batch)
    pub priority: i32,

    /// Whether to publish output
    pub publish: bool,

    /// Staleness threshold (mark stale if no update within this duration)
    pub stale_threshold: Option<Duration>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            frequency: UpdateFrequency::Immediate,
            priority: 0,
            publish: true,
            stale_threshold: None,
        }
    }
}

impl NodeConfig {
    /// Config for liquid bond prices.
    pub fn bond_price_liquid() -> Self {
        Self {
            frequency: UpdateFrequency::Immediate,
            priority: 100,
            publish: true,
            stale_threshold: Some(Duration::from_secs(60)),
        }
    }

    /// Config for illiquid bond prices.
    pub fn bond_price_illiquid() -> Self {
        Self {
            frequency: UpdateFrequency::Throttled {
                interval: Duration::from_secs(1),
            },
            priority: 50,
            publish: true,
            stale_threshold: Some(Duration::from_secs(300)),
        }
    }

    /// Config for ETF iNAV.
    pub fn etf_inav() -> Self {
        Self {
            frequency: UpdateFrequency::Interval {
                interval: Duration::from_secs(15),
            },
            priority: 200,
            publish: true,
            stale_threshold: Some(Duration::from_secs(30)),
        }
    }

    /// Config for ETF NAV.
    pub fn etf_nav() -> Self {
        Self {
            frequency: UpdateFrequency::EndOfDay {
                time: "16:00:00".to_string(),
            },
            priority: 200,
            publish: true,
            stale_threshold: None,
        }
    }

    /// Config for portfolio analytics.
    pub fn portfolio() -> Self {
        Self {
            frequency: UpdateFrequency::Throttled {
                interval: Duration::from_secs(5),
            },
            priority: 150,
            publish: true,
            stale_threshold: Some(Duration::from_secs(60)),
        }
    }

    /// Config for risk metrics.
    pub fn risk_metrics() -> Self {
        Self {
            frequency: UpdateFrequency::Interval {
                interval: Duration::from_secs(60),
            },
            priority: 100,
            publish: true,
            stale_threshold: Some(Duration::from_secs(120)),
        }
    }
}

// =============================================================================
// SHARDING CONFIG
// =============================================================================

/// Sharding strategy.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ShardingStrategy {
    /// Hash-based sharding
    #[default]
    HashBased,
    /// Shard by currency
    ByCurrency,
    /// Shard by pricing model
    ByPricingModel,
    /// Custom mapping from file
    Custom {
        /// Path to mapping file
        mapping_file: String,
    },
}

/// Sharding configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardingConfig {
    /// Enable sharding
    pub enabled: bool,
    /// Number of shards
    pub num_shards: u32,
    /// Sharding strategy
    pub strategy: ShardingStrategy,
}

impl Default for ShardingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            num_shards: 1,
            strategy: ShardingStrategy::default(),
        }
    }
}

// =============================================================================
// ENGINE CONFIG
// =============================================================================

/// Pricing engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Engine name/identifier
    pub name: String,

    /// Sharding configuration
    pub sharding: ShardingConfig,

    /// Default node config for bonds
    pub default_bond_config: NodeConfig,

    /// Default node config for curves
    pub default_curve_config: NodeConfig,

    /// Default node config for ETFs
    pub default_etf_config: NodeConfig,

    /// Enable metrics collection
    pub metrics_enabled: bool,

    /// Metrics prefix
    pub metrics_prefix: String,

    /// Max concurrent calculations
    pub max_concurrent_calcs: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            name: "convex-engine".to_string(),
            sharding: ShardingConfig::default(),
            default_bond_config: NodeConfig::default(),
            default_curve_config: NodeConfig::default(),
            default_etf_config: NodeConfig::etf_inav(),
            metrics_enabled: true,
            metrics_prefix: "convex".to_string(),
            max_concurrent_calcs: 1000,
        }
    }
}

// =============================================================================
// CONFIG SOURCE TRAIT
// =============================================================================

/// Source for configuration values.
#[async_trait]
pub trait ConfigSource: Send + Sync {
    /// Get engine configuration.
    async fn get_engine_config(&self) -> Result<EngineConfig, TraitError>;

    /// Get node configuration for a specific node.
    async fn get_node_config(&self, node_id: &str) -> Result<Option<NodeConfig>, TraitError>;

    /// Subscribe to config changes.
    async fn subscribe(&self) -> Result<ConfigChangeReceiver, TraitError>;

    /// Reload configuration.
    async fn reload(&self) -> Result<(), TraitError>;
}

/// Configuration change notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChange {
    /// Changed key
    pub key: String,
    /// Previous value (JSON)
    pub previous_value: Option<String>,
    /// New value (JSON)
    pub new_value: Option<String>,
    /// Timestamp
    pub timestamp: i64,
}

/// Receiver for config changes.
pub struct ConfigChangeReceiver {
    rx: tokio::sync::broadcast::Receiver<ConfigChange>,
}

impl ConfigChangeReceiver {
    /// Create a new config change receiver.
    pub fn new(rx: tokio::sync::broadcast::Receiver<ConfigChange>) -> Self {
        Self { rx }
    }

    /// Receive the next config change.
    pub async fn recv(&mut self) -> Option<ConfigChange> {
        self.rx.recv().await.ok()
    }
}

// =============================================================================
// CONFIG WATCHER (HOT RELOAD)
// =============================================================================

/// Error that can occur during config watching.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigWatchError {
    /// Failed to watch path.
    #[error("Failed to watch path: {0}")]
    WatchFailed(String),

    /// Failed to read config file.
    #[error("Failed to read config: {0}")]
    ReadFailed(String),

    /// Failed to parse config.
    #[error("Failed to parse config: {0}")]
    ParseFailed(String),

    /// Invalid config.
    #[error("Invalid config: {0}")]
    InvalidConfig(String),

    /// Config version mismatch.
    #[error("Config version mismatch: expected {expected}, got {actual}")]
    VersionMismatch {
        /// Expected version.
        expected: u64,
        /// Actual version.
        actual: u64,
    },
}

/// Watched configuration with version tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchedConfig<T> {
    /// The configuration value.
    pub config: T,
    /// Version number (incremented on each change).
    pub version: u64,
    /// Path being watched.
    pub path: String,
    /// Last modified timestamp (Unix epoch seconds).
    pub last_modified: i64,
    /// Hash of the configuration content.
    pub content_hash: u64,
}

impl<T> WatchedConfig<T> {
    /// Create a new watched config.
    pub fn new(config: T, path: &str) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        Self {
            config,
            version: 1,
            path: path.to_string(),
            last_modified: timestamp,
            content_hash: 0,
        }
    }

    /// Increment version on change.
    pub fn increment_version(&mut self) {
        self.version += 1;
    }
}

/// Event emitted when configuration changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigWatchEvent<T> {
    /// Configuration was loaded or reloaded.
    Loaded {
        /// The new configuration.
        config: T,
        /// Version number.
        version: u64,
    },
    /// Configuration was modified.
    Modified {
        /// Previous version.
        previous_version: u64,
        /// New version.
        new_version: u64,
        /// Changed keys (if trackable).
        changed_keys: Vec<String>,
    },
    /// Configuration file was deleted.
    Deleted {
        /// Last version before deletion.
        last_version: u64,
    },
    /// Error occurred while watching.
    Error {
        /// Error description.
        error: String,
    },
}

/// Configuration for the config watcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigWatcherOptions {
    /// Path(s) to watch.
    pub paths: Vec<String>,
    /// Poll interval for file-based watching (if inotify unavailable).
    pub poll_interval: Duration,
    /// Whether to reload on change automatically.
    pub auto_reload: bool,
    /// Debounce duration (ignore rapid changes within this window).
    pub debounce: Duration,
    /// Maximum config size in bytes.
    pub max_size: usize,
}

impl Default for ConfigWatcherOptions {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            poll_interval: Duration::from_secs(5),
            auto_reload: true,
            debounce: Duration::from_millis(500),
            max_size: 10 * 1024 * 1024, // 10MB
        }
    }
}

impl ConfigWatcherOptions {
    /// Create options for watching a single path.
    pub fn single_path(path: &str) -> Self {
        Self {
            paths: vec![path.to_string()],
            ..Default::default()
        }
    }

    /// Add a path to watch.
    pub fn add_path(mut self, path: &str) -> Self {
        self.paths.push(path.to_string());
        self
    }

    /// Set poll interval.
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Set debounce duration.
    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }
}

/// Trait for configuration watchers that support hot reload.
///
/// Implementations should monitor configuration files/sources and emit
/// events when changes are detected.
///
/// # Example
///
/// ```ignore
/// let watcher = FileConfigWatcher::new(options)?;
///
/// // Start watching
/// watcher.start().await?;
///
/// // Subscribe to changes
/// let mut events = watcher.subscribe();
/// while let Some(event) = events.recv().await {
///     match event {
///         ConfigWatchEvent::Modified { new_version, .. } => {
///             println!("Config updated to version {}", new_version);
///             // Apply new configuration
///         }
///         ConfigWatchEvent::Error { error } => {
///             println!("Config watch error: {}", error);
///         }
///         _ => {}
///     }
/// }
/// ```
#[async_trait]
pub trait ConfigWatcher: Send + Sync {
    /// The configuration type being watched.
    type Config: Send + Sync + Clone + 'static;

    /// Start watching for configuration changes.
    async fn start(&self) -> Result<(), ConfigWatchError>;

    /// Stop watching.
    async fn stop(&self) -> Result<(), ConfigWatchError>;

    /// Get the current configuration.
    fn current(&self) -> WatchedConfig<Self::Config>;

    /// Get the current version.
    fn version(&self) -> u64 {
        self.current().version
    }

    /// Force reload configuration from source.
    async fn reload(&self) -> Result<WatchedConfig<Self::Config>, ConfigWatchError>;

    /// Subscribe to configuration change events.
    fn subscribe(&self) -> ConfigEventReceiver<Self::Config>;

    /// Check if the watcher is currently running.
    fn is_running(&self) -> bool;
}

/// Receiver for config watch events.
pub struct ConfigEventReceiver<T> {
    rx: tokio::sync::broadcast::Receiver<ConfigWatchEvent<T>>,
}

impl<T: Clone> ConfigEventReceiver<T> {
    /// Create a new config event receiver.
    pub fn new(rx: tokio::sync::broadcast::Receiver<ConfigWatchEvent<T>>) -> Self {
        Self { rx }
    }

    /// Receive the next config event.
    pub async fn recv(&mut self) -> Option<ConfigWatchEvent<T>> {
        self.rx.recv().await.ok()
    }
}

/// Empty config watcher for testing.
///
/// This implementation does nothing and always returns the default config.
#[derive(Debug)]
pub struct EmptyConfigWatcher<T> {
    config: std::sync::RwLock<WatchedConfig<T>>,
    tx: tokio::sync::broadcast::Sender<ConfigWatchEvent<T>>,
}

impl<T: Default + Clone + Send + Sync + 'static> EmptyConfigWatcher<T> {
    /// Create a new empty config watcher.
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(16);
        Self {
            config: std::sync::RwLock::new(WatchedConfig::new(T::default(), "")),
            tx,
        }
    }

    /// Create with a specific config.
    pub fn with_config(config: T) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(16);
        Self {
            config: std::sync::RwLock::new(WatchedConfig::new(config, "")),
            tx,
        }
    }
}

impl<T: Default + Clone + Send + Sync + 'static> Default for EmptyConfigWatcher<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<T: Default + Clone + Send + Sync + 'static> ConfigWatcher for EmptyConfigWatcher<T> {
    type Config = T;

    async fn start(&self) -> Result<(), ConfigWatchError> {
        Ok(())
    }

    async fn stop(&self) -> Result<(), ConfigWatchError> {
        Ok(())
    }

    fn current(&self) -> WatchedConfig<Self::Config> {
        self.config.read().unwrap().clone()
    }

    async fn reload(&self) -> Result<WatchedConfig<Self::Config>, ConfigWatchError> {
        Ok(self.current())
    }

    fn subscribe(&self) -> ConfigEventReceiver<Self::Config> {
        ConfigEventReceiver::new(self.tx.subscribe())
    }

    fn is_running(&self) -> bool {
        false
    }
}

// =============================================================================
// CONFIG WATCHER TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watched_config_versioning() {
        let mut watched = WatchedConfig::new(EngineConfig::default(), "/path/to/config.yaml");
        assert_eq!(watched.version, 1);

        watched.increment_version();
        assert_eq!(watched.version, 2);
    }

    #[test]
    fn test_config_watcher_options() {
        let options = ConfigWatcherOptions::single_path("/etc/convex/config.yaml")
            .add_path("/etc/convex/overrides.yaml")
            .with_poll_interval(Duration::from_secs(10))
            .with_debounce(Duration::from_millis(200));

        assert_eq!(options.paths.len(), 2);
        assert_eq!(options.poll_interval, Duration::from_secs(10));
        assert_eq!(options.debounce, Duration::from_millis(200));
    }

    #[tokio::test]
    async fn test_empty_config_watcher() {
        let watcher: EmptyConfigWatcher<EngineConfig> = EmptyConfigWatcher::new();

        watcher.start().await.unwrap();
        assert!(!watcher.is_running());

        let current = watcher.current();
        assert_eq!(current.version, 1);

        watcher.stop().await.unwrap();
    }

    #[test]
    fn test_config_watch_event() {
        let event: ConfigWatchEvent<EngineConfig> = ConfigWatchEvent::Modified {
            previous_version: 1,
            new_version: 2,
            changed_keys: vec!["name".to_string()],
        };

        match event {
            ConfigWatchEvent::Modified { new_version, .. } => {
                assert_eq!(new_version, 2);
            }
            _ => panic!("Expected Modified event"),
        }
    }
}
