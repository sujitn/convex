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
