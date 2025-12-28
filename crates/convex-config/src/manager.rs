//! Configuration manager.
//!
//! Provides centralized configuration management with storage integration,
//! override support, and caching.

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::Utc;

use convex_storage::{ConfigRecord, InMemoryStorage, StorageAdapter};

use crate::curve::CurveConfig;
use crate::error::{ConfigError, ConfigResult, Validate};
use crate::override_system::{ApplyOverrides, ConfigOverride, OverrideContext, OverrideSet};
use crate::pricing::{PricingConfig, RiskConfig, SpreadConfig};

// =============================================================================
// CONFIG TYPE ENUM
// =============================================================================

/// Enum representing different configuration types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigType {
    /// Pricing configuration.
    Pricing,
    /// Curve configuration.
    Curve,
    /// Spread configuration.
    Spread,
    /// Risk configuration.
    Risk,
}

impl ConfigType {
    /// Returns the string identifier for this config type.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pricing => "pricing",
            Self::Curve => "curve",
            Self::Spread => "spread",
            Self::Risk => "risk",
        }
    }

    /// Parses a config type from string identifier.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pricing" => Some(Self::Pricing),
            "curve" => Some(Self::Curve),
            "spread" => Some(Self::Spread),
            "risk" => Some(Self::Risk),
            _ => None,
        }
    }
}

// =============================================================================
// CONFIGURATION MANAGER
// =============================================================================

/// Central configuration manager.
///
/// Manages all configuration types with:
/// - In-memory caching for fast access
/// - Persistent storage integration
/// - Override support with priority resolution
/// - Built-in standard configurations
///
/// # Example
///
/// ```rust
/// use convex_config::{ConfigManager, PricingConfig};
///
/// let manager = ConfigManager::new();
///
/// // Get default US corporate pricing config
/// let config = manager.get_pricing("US.CORPORATE").unwrap();
///
/// // Or use with context for overrides
/// let context = manager.context().with_currency("USD");
/// let effective_config = manager.get_pricing_with_context("US.CORPORATE", &context).unwrap();
/// ```
pub struct ConfigManager {
    /// Storage adapter for persistence (using InMemoryStorage).
    storage: InMemoryStorage,

    /// In-memory cache for pricing configs.
    pricing_cache: RwLock<HashMap<String, PricingConfig>>,

    /// In-memory cache for curve configs.
    curve_cache: RwLock<HashMap<String, CurveConfig>>,

    /// In-memory cache for spread configs.
    spread_cache: RwLock<HashMap<String, SpreadConfig>>,

    /// In-memory cache for risk configs.
    risk_cache: RwLock<HashMap<String, RiskConfig>>,

    /// Active overrides.
    overrides: RwLock<OverrideSet>,
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigManager {
    /// Creates a new configuration manager with in-memory storage.
    pub fn new() -> Self {
        let manager = Self {
            storage: InMemoryStorage::new(),
            pricing_cache: RwLock::new(HashMap::new()),
            curve_cache: RwLock::new(HashMap::new()),
            spread_cache: RwLock::new(HashMap::new()),
            risk_cache: RwLock::new(HashMap::new()),
            overrides: RwLock::new(OverrideSet::new()),
        };
        manager.load_standard_configs();
        manager
    }

    /// Creates a configuration manager with provided storage.
    pub fn with_storage(storage: InMemoryStorage) -> Self {
        let manager = Self {
            storage,
            pricing_cache: RwLock::new(HashMap::new()),
            curve_cache: RwLock::new(HashMap::new()),
            spread_cache: RwLock::new(HashMap::new()),
            risk_cache: RwLock::new(HashMap::new()),
            overrides: RwLock::new(OverrideSet::new()),
        };
        manager.load_standard_configs();
        manager
    }

    /// Loads standard/built-in configurations.
    fn load_standard_configs(&self) {
        // Standard pricing configs
        let _ = self.register_pricing(PricingConfig::us_corporate());
        let _ = self.register_pricing(PricingConfig::us_treasury());
        let _ = self.register_pricing(PricingConfig::uk_gilt());
        let _ = self.register_pricing(PricingConfig::euro_govt());

        // Standard curve configs
        let _ = self.register_curve(CurveConfig::sofr_ois());
        let _ = self.register_curve(CurveConfig::estr_ois());
        let _ = self.register_curve(CurveConfig::sonia_ois());
        let _ = self.register_curve(CurveConfig::govt_bond("USD"));
        let _ = self.register_curve(CurveConfig::govt_bond("EUR"));
        let _ = self.register_curve(CurveConfig::govt_bond("GBP"));

        // Standard spread configs
        let _ = self.register_spread(SpreadConfig::usd());
        let _ = self.register_spread(SpreadConfig::eur());

        // Standard risk configs
        let _ = self.register_risk(RiskConfig::standard());
        let _ = self.register_risk(RiskConfig::high_precision());
    }

    /// Creates an override context builder.
    pub fn context(&self) -> OverrideContext {
        OverrideContext::new()
    }

    // =========================================================================
    // PRICING CONFIGURATION
    // =========================================================================

    /// Registers a pricing configuration.
    pub fn register_pricing(&self, config: PricingConfig) -> ConfigResult<()> {
        config.validate_or_error()?;

        let mut cache = self
            .pricing_cache
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        // Check if read-only config already exists
        if let Some(existing) = cache.get(&config.name) {
            if existing.read_only {
                return Err(ConfigError::ReadOnly {
                    key: config.name.clone(),
                });
            }
        }

        // Persist to storage
        self.persist_config(ConfigType::Pricing, &config.name, &config)?;

        cache.insert(config.name.clone(), config);
        Ok(())
    }

    /// Gets a pricing configuration by name.
    pub fn get_pricing(&self, name: &str) -> ConfigResult<PricingConfig> {
        let cache = self
            .pricing_cache
            .read()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        cache.get(name).cloned().ok_or_else(|| ConfigError::NotFound {
            key: name.to_string(),
        })
    }

    /// Gets a pricing configuration with overrides applied.
    pub fn get_pricing_with_context(
        &self,
        name: &str,
        context: &OverrideContext,
    ) -> ConfigResult<PricingConfig> {
        let base = self.get_pricing(name)?;
        let overrides = self
            .overrides
            .read()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        base.apply_overrides(&overrides, context)
    }

    /// Lists all pricing configuration names.
    pub fn list_pricing_configs(&self) -> ConfigResult<Vec<String>> {
        let cache = self
            .pricing_cache
            .read()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        Ok(cache.keys().cloned().collect())
    }

    /// Deletes a pricing configuration.
    pub fn delete_pricing(&self, name: &str) -> ConfigResult<bool> {
        let mut cache = self
            .pricing_cache
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        if let Some(config) = cache.get(name) {
            if config.read_only {
                return Err(ConfigError::ReadOnly {
                    key: name.to_string(),
                });
            }
        }

        Ok(cache.remove(name).is_some())
    }

    // =========================================================================
    // CURVE CONFIGURATION
    // =========================================================================

    /// Registers a curve configuration.
    pub fn register_curve(&self, config: CurveConfig) -> ConfigResult<()> {
        config.validate_or_error()?;

        let mut cache = self
            .curve_cache
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        if let Some(existing) = cache.get(&config.name) {
            if existing.read_only {
                return Err(ConfigError::ReadOnly {
                    key: config.name.clone(),
                });
            }
        }

        self.persist_config(ConfigType::Curve, &config.name, &config)?;

        cache.insert(config.name.clone(), config);
        Ok(())
    }

    /// Gets a curve configuration by name.
    pub fn get_curve(&self, name: &str) -> ConfigResult<CurveConfig> {
        let cache = self
            .curve_cache
            .read()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        cache.get(name).cloned().ok_or_else(|| ConfigError::NotFound {
            key: name.to_string(),
        })
    }

    /// Gets a curve configuration with overrides applied.
    pub fn get_curve_with_context(
        &self,
        name: &str,
        context: &OverrideContext,
    ) -> ConfigResult<CurveConfig> {
        let base = self.get_curve(name)?;
        let overrides = self
            .overrides
            .read()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        base.apply_overrides(&overrides, context)
    }

    /// Lists all curve configuration names.
    pub fn list_curve_configs(&self) -> ConfigResult<Vec<String>> {
        let cache = self
            .curve_cache
            .read()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        Ok(cache.keys().cloned().collect())
    }

    // =========================================================================
    // SPREAD CONFIGURATION
    // =========================================================================

    /// Registers a spread configuration.
    pub fn register_spread(&self, config: SpreadConfig) -> ConfigResult<()> {
        config.validate_or_error()?;

        let mut cache = self
            .spread_cache
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        if let Some(existing) = cache.get(&config.name) {
            if existing.read_only {
                return Err(ConfigError::ReadOnly {
                    key: config.name.clone(),
                });
            }
        }

        self.persist_config(ConfigType::Spread, &config.name, &config)?;

        cache.insert(config.name.clone(), config);
        Ok(())
    }

    /// Gets a spread configuration by name.
    pub fn get_spread(&self, name: &str) -> ConfigResult<SpreadConfig> {
        let cache = self
            .spread_cache
            .read()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        cache.get(name).cloned().ok_or_else(|| ConfigError::NotFound {
            key: name.to_string(),
        })
    }

    /// Lists all spread configuration names.
    pub fn list_spread_configs(&self) -> ConfigResult<Vec<String>> {
        let cache = self
            .spread_cache
            .read()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        Ok(cache.keys().cloned().collect())
    }

    // =========================================================================
    // RISK CONFIGURATION
    // =========================================================================

    /// Registers a risk configuration.
    pub fn register_risk(&self, config: RiskConfig) -> ConfigResult<()> {
        config.validate_or_error()?;

        let mut cache = self
            .risk_cache
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        if let Some(existing) = cache.get(&config.name) {
            if existing.read_only {
                return Err(ConfigError::ReadOnly {
                    key: config.name.clone(),
                });
            }
        }

        self.persist_config(ConfigType::Risk, &config.name, &config)?;

        cache.insert(config.name.clone(), config);
        Ok(())
    }

    /// Gets a risk configuration by name.
    pub fn get_risk(&self, name: &str) -> ConfigResult<RiskConfig> {
        let cache = self
            .risk_cache
            .read()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        cache.get(name).cloned().ok_or_else(|| ConfigError::NotFound {
            key: name.to_string(),
        })
    }

    /// Lists all risk configuration names.
    pub fn list_risk_configs(&self) -> ConfigResult<Vec<String>> {
        let cache = self
            .risk_cache
            .read()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        Ok(cache.keys().cloned().collect())
    }

    // =========================================================================
    // OVERRIDE MANAGEMENT
    // =========================================================================

    /// Adds a configuration override.
    pub fn add_override(&self, override_item: ConfigOverride) -> ConfigResult<String> {
        override_item.validate_or_error()?;

        let id = override_item.id.clone();
        let mut overrides = self
            .overrides
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        overrides.add(override_item);
        Ok(id)
    }

    /// Removes an override by ID.
    pub fn remove_override(&self, id: &str) -> ConfigResult<bool> {
        let mut overrides = self
            .overrides
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        Ok(overrides.remove(id))
    }

    /// Deactivates an override by ID.
    pub fn deactivate_override(&self, id: &str) -> ConfigResult<bool> {
        let mut overrides = self
            .overrides
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        Ok(overrides.deactivate(id))
    }

    /// Lists all active overrides.
    pub fn list_overrides(&self) -> ConfigResult<Vec<ConfigOverride>> {
        let overrides = self
            .overrides
            .read()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        Ok(overrides.active().cloned().collect())
    }

    /// Cleans up expired overrides.
    pub fn cleanup_overrides(&self) -> ConfigResult<usize> {
        let mut overrides = self
            .overrides
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?;

        Ok(overrides.cleanup_expired())
    }

    // =========================================================================
    // STORAGE HELPERS
    // =========================================================================

    /// Persists a configuration to storage.
    fn persist_config<T: serde::Serialize>(
        &self,
        config_type: ConfigType,
        name: &str,
        config: &T,
    ) -> ConfigResult<()> {
        let data = serde_json::to_string(config)?;
        let now = Utc::now();

        let record = ConfigRecord {
            key: format!("{}.{}", config_type.as_str(), name),
            config_type: config_type.as_str().to_string(),
            data,
            version: 1, // TODO: Implement versioning
            updated_at: now,
            updated_by: None,
            is_active: true,
        };

        self.storage.store_config(&record)?;
        Ok(())
    }

    /// Loads configurations from storage.
    pub fn load_from_storage(&self) -> ConfigResult<usize> {
        let mut count = 0;

        // Load pricing configs
        for record in self.storage.list_configs_by_type("pricing")? {
            if let Ok(config) = serde_json::from_str::<PricingConfig>(&record.data) {
                if let Ok(mut cache) = self.pricing_cache.write() {
                    cache.insert(config.name.clone(), config);
                    count += 1;
                }
            }
        }

        // Load curve configs
        for record in self.storage.list_configs_by_type("curve")? {
            if let Ok(config) = serde_json::from_str::<CurveConfig>(&record.data) {
                if let Ok(mut cache) = self.curve_cache.write() {
                    cache.insert(config.name.clone(), config);
                    count += 1;
                }
            }
        }

        // Load spread configs
        for record in self.storage.list_configs_by_type("spread")? {
            if let Ok(config) = serde_json::from_str::<SpreadConfig>(&record.data) {
                if let Ok(mut cache) = self.spread_cache.write() {
                    cache.insert(config.name.clone(), config);
                    count += 1;
                }
            }
        }

        // Load risk configs
        for record in self.storage.list_configs_by_type("risk")? {
            if let Ok(config) = serde_json::from_str::<RiskConfig>(&record.data) {
                if let Ok(mut cache) = self.risk_cache.write() {
                    cache.insert(config.name.clone(), config);
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// Clears all cached configurations (does not affect storage).
    pub fn clear_cache(&self) -> ConfigResult<()> {
        self.pricing_cache
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?
            .clear();

        self.curve_cache
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?
            .clear();

        self.spread_cache
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?
            .clear();

        self.risk_cache
            .write()
            .map_err(|e| ConfigError::Conflict(format!("Lock error: {}", e)))?
            .clear();

        Ok(())
    }
}

// =============================================================================
// APPLY OVERRIDES IMPLEMENTATIONS
// =============================================================================

impl ApplyOverrides for PricingConfig {
    fn config_key(&self) -> &str {
        "pricing"
    }
}

impl ApplyOverrides for CurveConfig {
    fn config_key(&self) -> &str {
        "curve"
    }
}

impl ApplyOverrides for SpreadConfig {
    fn config_key(&self) -> &str {
        "spread"
    }
}

impl ApplyOverrides for RiskConfig {
    fn config_key(&self) -> &str {
        "risk"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_manager_new() {
        let manager = ConfigManager::new();

        // Should have standard configs loaded
        assert!(manager.get_pricing("US.CORPORATE").is_ok());
        assert!(manager.get_pricing("US.TREASURY").is_ok());
        assert!(manager.get_curve("USD.SOFR.OIS").is_ok());
        assert!(manager.get_spread("USD.SPREAD").is_ok());
        assert!(manager.get_risk("STANDARD").is_ok());
    }

    #[test]
    fn test_register_custom_pricing() {
        let manager = ConfigManager::new();

        let custom = PricingConfig::new("CUSTOM.PRICING")
            .with_description("Custom pricing config")
            .with_settlement_days(3);

        manager.register_pricing(custom).unwrap();

        let retrieved = manager.get_pricing("CUSTOM.PRICING").unwrap();
        assert_eq!(retrieved.settlement_days, 3);
    }

    #[test]
    fn test_read_only_config() {
        let manager = ConfigManager::new();

        // Try to overwrite read-only config
        let result = manager.register_pricing(PricingConfig::us_corporate());
        assert!(matches!(result, Err(ConfigError::ReadOnly { .. })));
    }

    #[test]
    fn test_config_not_found() {
        let manager = ConfigManager::new();

        let result = manager.get_pricing("NONEXISTENT");
        assert!(matches!(result, Err(ConfigError::NotFound { .. })));
    }

    #[test]
    fn test_list_configs() {
        let manager = ConfigManager::new();

        let pricing_configs = manager.list_pricing_configs().unwrap();
        assert!(pricing_configs.contains(&"US.CORPORATE".to_string()));
        assert!(pricing_configs.contains(&"US.TREASURY".to_string()));

        let curve_configs = manager.list_curve_configs().unwrap();
        assert!(curve_configs.contains(&"USD.SOFR.OIS".to_string()));
    }

    #[test]
    fn test_override_management() {
        let manager = ConfigManager::new();

        let override_item = ConfigOverride::new(
            "pricing",
            "settlement_days",
            serde_json::json!(5),
        )
        .with_reason("Holiday adjustment");

        let id = manager.add_override(override_item).unwrap();

        let overrides = manager.list_overrides().unwrap();
        assert_eq!(overrides.len(), 1);

        manager.remove_override(&id).unwrap();

        let overrides = manager.list_overrides().unwrap();
        assert!(overrides.is_empty());
    }

    #[test]
    fn test_delete_custom_config() {
        let manager = ConfigManager::new();

        let custom = PricingConfig::new("TO.DELETE");
        manager.register_pricing(custom).unwrap();

        assert!(manager.get_pricing("TO.DELETE").is_ok());

        let deleted = manager.delete_pricing("TO.DELETE").unwrap();
        assert!(deleted);

        assert!(manager.get_pricing("TO.DELETE").is_err());
    }

    #[test]
    fn test_cannot_delete_readonly() {
        let manager = ConfigManager::new();

        let result = manager.delete_pricing("US.CORPORATE");
        assert!(matches!(result, Err(ConfigError::ReadOnly { .. })));
    }
}
