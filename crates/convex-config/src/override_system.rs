//! Configuration override system.
//!
//! Provides runtime overrides for configuration values with priority levels.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::error::{ConfigError, ConfigResult, Validate, ValidationError};

// =============================================================================
// OVERRIDE PRIORITY
// =============================================================================

/// Priority level for configuration overrides.
///
/// Higher priority overrides take precedence over lower ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub enum OverridePriority {
    /// Base configuration (lowest priority).
    Base = 0,
    /// Environment-level override.
    Environment = 10,
    /// User-level override.
    #[default]
    User = 20,
    /// Session-level override.
    Session = 30,
    /// Request-level override (highest priority).
    Request = 40,
}

// =============================================================================
// OVERRIDE SCOPE
// =============================================================================

/// Scope for configuration overrides.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum OverrideScope {
    /// Global scope - applies to all calculations.
    #[default]
    Global,
    /// Security-specific scope.
    Security(String),
    /// Curve-specific scope.
    Curve(String),
    /// Currency-specific scope.
    Currency(String),
    /// Issuer-specific scope.
    Issuer(String),
    /// Sector-specific scope.
    Sector(String),
    /// Custom scope with key-value pair.
    Custom {
        /// The scope key.
        key: String,
        /// The scope value to match.
        value: String,
    },
}

impl OverrideScope {
    /// Returns true if this scope matches the given context.
    pub fn matches(&self, context: &OverrideContext) -> bool {
        match self {
            Self::Global => true,
            Self::Security(id) => context.security_id.as_ref() == Some(id),
            Self::Curve(name) => context.curve_name.as_ref() == Some(name),
            Self::Currency(ccy) => context.currency.as_ref() == Some(ccy),
            Self::Issuer(issuer) => context.issuer.as_ref() == Some(issuer),
            Self::Sector(sector) => context.sector.as_ref() == Some(sector),
            Self::Custom { key, value } => context.custom.get(key) == Some(value),
        }
    }
}

// =============================================================================
// OVERRIDE CONTEXT
// =============================================================================

/// Context for evaluating override applicability.
#[derive(Debug, Clone, Default)]
pub struct OverrideContext {
    /// Security identifier.
    pub security_id: Option<String>,
    /// Curve name.
    pub curve_name: Option<String>,
    /// Currency.
    pub currency: Option<String>,
    /// Issuer.
    pub issuer: Option<String>,
    /// Sector.
    pub sector: Option<String>,
    /// Custom key-value pairs for matching.
    pub custom: HashMap<String, String>,
}

impl OverrideContext {
    /// Creates a new empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder method to set security ID.
    pub fn with_security(mut self, id: impl Into<String>) -> Self {
        self.security_id = Some(id.into());
        self
    }

    /// Builder method to set curve name.
    pub fn with_curve(mut self, name: impl Into<String>) -> Self {
        self.curve_name = Some(name.into());
        self
    }

    /// Builder method to set currency.
    pub fn with_currency(mut self, currency: impl Into<String>) -> Self {
        self.currency = Some(currency.into());
        self
    }

    /// Builder method to set issuer.
    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = Some(issuer.into());
        self
    }

    /// Builder method to set sector.
    pub fn with_sector(mut self, sector: impl Into<String>) -> Self {
        self.sector = Some(sector.into());
        self
    }

    /// Builder method to add custom key-value.
    pub fn with_custom(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom.insert(key.into(), value.into());
        self
    }
}

// =============================================================================
// CONFIGURATION OVERRIDE
// =============================================================================

/// A single configuration override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigOverride {
    /// Unique identifier for this override.
    pub id: String,

    /// Configuration key being overridden (e.g., "pricing.settlement_days").
    pub config_key: String,

    /// Field path within the configuration.
    pub field_path: String,

    /// Override value as JSON.
    pub value: Value,

    /// Override priority.
    #[serde(default)]
    pub priority: OverridePriority,

    /// Override scope.
    #[serde(default)]
    pub scope: OverrideScope,

    /// Optional reason/description for the override.
    pub reason: Option<String>,

    /// Who created this override.
    pub created_by: Option<String>,

    /// When the override was created.
    pub created_at: DateTime<Utc>,

    /// When the override expires (None = never).
    pub expires_at: Option<DateTime<Utc>>,

    /// Whether the override is active.
    #[serde(default = "default_true")]
    pub active: bool,
}

fn default_true() -> bool {
    true
}

impl ConfigOverride {
    /// Creates a new configuration override.
    pub fn new(
        config_key: impl Into<String>,
        field_path: impl Into<String>,
        value: Value,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            config_key: config_key.into(),
            field_path: field_path.into(),
            value,
            priority: OverridePriority::default(),
            scope: OverrideScope::default(),
            reason: None,
            created_by: None,
            created_at: Utc::now(),
            expires_at: None,
            active: true,
        }
    }

    /// Builder method to set priority.
    pub fn with_priority(mut self, priority: OverridePriority) -> Self {
        self.priority = priority;
        self
    }

    /// Builder method to set scope.
    pub fn with_scope(mut self, scope: OverrideScope) -> Self {
        self.scope = scope;
        self
    }

    /// Builder method to set reason.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Builder method to set creator.
    pub fn with_created_by(mut self, created_by: impl Into<String>) -> Self {
        self.created_by = Some(created_by.into());
        self
    }

    /// Builder method to set expiration.
    pub fn with_expiration(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Returns true if the override is currently active and not expired.
    pub fn is_effective(&self) -> bool {
        if !self.active {
            return false;
        }
        if let Some(expires_at) = self.expires_at {
            if Utc::now() > expires_at {
                return false;
            }
        }
        true
    }

    /// Returns true if this override applies to the given context.
    pub fn applies_to(&self, context: &OverrideContext) -> bool {
        self.is_effective() && self.scope.matches(context)
    }
}

impl Validate for ConfigOverride {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if self.config_key.is_empty() {
            errors.push(ValidationError::new("config_key", "Config key cannot be empty"));
        }

        if self.field_path.is_empty() {
            errors.push(ValidationError::new("field_path", "Field path cannot be empty"));
        }

        if self.value.is_null() {
            errors.push(ValidationError::new("value", "Override value cannot be null"));
        }

        errors
    }
}

// =============================================================================
// OVERRIDE SET
// =============================================================================

/// A collection of configuration overrides with resolution logic.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OverrideSet {
    /// List of overrides.
    overrides: Vec<ConfigOverride>,
}

impl OverrideSet {
    /// Creates a new empty override set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an override to the set.
    pub fn add(&mut self, override_item: ConfigOverride) {
        self.overrides.push(override_item);
    }

    /// Removes an override by ID.
    pub fn remove(&mut self, id: &str) -> bool {
        let initial_len = self.overrides.len();
        self.overrides.retain(|o| o.id != id);
        self.overrides.len() < initial_len
    }

    /// Deactivates an override by ID.
    pub fn deactivate(&mut self, id: &str) -> bool {
        if let Some(override_item) = self.overrides.iter_mut().find(|o| o.id == id) {
            override_item.active = false;
            true
        } else {
            false
        }
    }

    /// Returns all overrides.
    pub fn all(&self) -> &[ConfigOverride] {
        &self.overrides
    }

    /// Returns active overrides.
    pub fn active(&self) -> impl Iterator<Item = &ConfigOverride> {
        self.overrides.iter().filter(|o| o.is_effective())
    }

    /// Finds overrides for a specific config key.
    pub fn for_config<'a>(&'a self, config_key: &'a str) -> impl Iterator<Item = &'a ConfigOverride> {
        self.overrides
            .iter()
            .filter(move |o| o.config_key == config_key && o.is_effective())
    }

    /// Resolves the effective value for a field, considering context.
    ///
    /// Returns the highest-priority override value that matches the context,
    /// or None if no overrides apply.
    pub fn resolve(
        &self,
        config_key: &str,
        field_path: &str,
        context: &OverrideContext,
    ) -> Option<&Value> {
        self.overrides
            .iter()
            .filter(|o| {
                o.config_key == config_key
                    && o.field_path == field_path
                    && o.applies_to(context)
            })
            .max_by_key(|o| o.priority)
            .map(|o| &o.value)
    }

    /// Cleans up expired overrides.
    pub fn cleanup_expired(&mut self) -> usize {
        let initial_len = self.overrides.len();
        let now = Utc::now();
        self.overrides.retain(|o| {
            o.expires_at.map_or(true, |exp| exp > now)
        });
        initial_len - self.overrides.len()
    }

    /// Returns the number of overrides.
    pub fn len(&self) -> usize {
        self.overrides.len()
    }

    /// Returns true if there are no overrides.
    pub fn is_empty(&self) -> bool {
        self.overrides.is_empty()
    }
}

// =============================================================================
// OVERRIDE APPLICATION
// =============================================================================

/// Trait for types that can have overrides applied.
pub trait ApplyOverrides: Sized + Serialize + for<'de> Deserialize<'de> {
    /// Returns the configuration key for this type.
    fn config_key(&self) -> &str;

    /// Applies overrides from the set to this configuration.
    fn apply_overrides(
        &self,
        overrides: &OverrideSet,
        context: &OverrideContext,
    ) -> ConfigResult<Self> {
        // Convert to JSON
        let mut json = serde_json::to_value(self)?;

        // Apply each matching override
        for override_item in overrides.for_config(self.config_key()) {
            if override_item.applies_to(context) {
                apply_json_override(&mut json, &override_item.field_path, &override_item.value)?;
            }
        }

        // Convert back
        let result = serde_json::from_value(json)?;
        Ok(result)
    }
}

/// Applies a single override to a JSON value at the given path.
fn apply_json_override(target: &mut Value, path: &str, value: &Value) -> ConfigResult<()> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = target;

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            // Last part - apply the value
            if let Some(obj) = current.as_object_mut() {
                if !obj.contains_key(*part) {
                    return Err(ConfigError::InvalidOverride {
                        config: "".to_string(),
                        field: path.to_string(),
                    });
                }
                obj.insert(part.to_string(), value.clone());
            } else {
                return Err(ConfigError::InvalidOverride {
                    config: "".to_string(),
                    field: path.to_string(),
                });
            }
        } else {
            // Navigate to nested object
            current = current.get_mut(*part).ok_or_else(|| ConfigError::InvalidOverride {
                config: "".to_string(),
                field: path.to_string(),
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_override_priority_ordering() {
        assert!(OverridePriority::Request > OverridePriority::Session);
        assert!(OverridePriority::Session > OverridePriority::User);
        assert!(OverridePriority::User > OverridePriority::Environment);
        assert!(OverridePriority::Environment > OverridePriority::Base);
    }

    #[test]
    fn test_override_scope_matching() {
        let context = OverrideContext::new()
            .with_security("BOND001")
            .with_currency("USD");

        assert!(OverrideScope::Global.matches(&context));
        assert!(OverrideScope::Security("BOND001".to_string()).matches(&context));
        assert!(OverrideScope::Currency("USD".to_string()).matches(&context));
        assert!(!OverrideScope::Security("BOND002".to_string()).matches(&context));
        assert!(!OverrideScope::Currency("EUR".to_string()).matches(&context));
    }

    #[test]
    fn test_config_override_creation() {
        let override_item = ConfigOverride::new(
            "pricing",
            "settlement_days",
            serde_json::json!(3),
        )
        .with_priority(OverridePriority::User)
        .with_reason("Holiday adjustment")
        .with_scope(OverrideScope::Currency("USD".to_string()));

        assert_eq!(override_item.config_key, "pricing");
        assert_eq!(override_item.field_path, "settlement_days");
        assert_eq!(override_item.priority, OverridePriority::User);
        assert!(override_item.is_effective());
    }

    #[test]
    fn test_override_expiration() {
        let expired = ConfigOverride::new("test", "field", serde_json::json!(1))
            .with_expiration(Utc::now() - chrono::Duration::hours(1));

        assert!(!expired.is_effective());

        let active = ConfigOverride::new("test", "field", serde_json::json!(1))
            .with_expiration(Utc::now() + chrono::Duration::hours(1));

        assert!(active.is_effective());
    }

    #[test]
    fn test_override_set_resolution() {
        let mut set = OverrideSet::new();

        // Add global override
        set.add(
            ConfigOverride::new("pricing", "settlement_days", serde_json::json!(2))
                .with_priority(OverridePriority::Base),
        );

        // Add USD-specific override
        set.add(
            ConfigOverride::new("pricing", "settlement_days", serde_json::json!(3))
                .with_priority(OverridePriority::User)
                .with_scope(OverrideScope::Currency("USD".to_string())),
        );

        let context = OverrideContext::new().with_currency("USD");
        let value = set.resolve("pricing", "settlement_days", &context);

        assert!(value.is_some());
        assert_eq!(value.unwrap(), &serde_json::json!(3));
    }

    #[test]
    fn test_override_set_cleanup() {
        let mut set = OverrideSet::new();

        set.add(
            ConfigOverride::new("test", "field", serde_json::json!(1))
                .with_expiration(Utc::now() - chrono::Duration::hours(1)),
        );
        set.add(ConfigOverride::new("test", "field2", serde_json::json!(2)));

        assert_eq!(set.len(), 2);

        let cleaned = set.cleanup_expired();
        assert_eq!(cleaned, 1);
        assert_eq!(set.len(), 1);
    }
}
