//! Configuration resolver for valuation contexts.
//!
//! Matches bonds to the appropriate pricing configuration based on
//! specificity rules (ISIN > Issuer > Sector > Default).

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

use convex_traits::reference_data::BondReferenceData;
use convex_traits::storage::{BondPricingConfig, ConfigStore};

/// Resolves pricing configuration for bonds.
///
/// caches configurations in memory for performance and updates them
/// when notified of changes (TODO: hook up to config watcher).
pub struct ConfigResolver {
    /// Cached configurations, sorted by priority (descending).
    configs: RwLock<Vec<BondPricingConfig>>,
    /// Underlying store.
    store: Arc<dyn ConfigStore>,
}

impl ConfigResolver {
    /// Create a new config resolver.
    pub fn new(store: Arc<dyn ConfigStore>) -> Self {
        Self {
            configs: RwLock::new(Vec::new()),
            store,
        }
    }

    /// Initialize the resolver by loading all configs.
    pub async fn init(&self) -> Result<(), convex_traits::error::TraitError> {
        let mut configs = self.store.list().await?;
        // Sort by priority descending (higher priority first)
        configs.sort_by(|a, b| b.effective_priority().cmp(&a.effective_priority()));

        let mut cache = self.configs.write().await;
        *cache = configs;

        debug!("ConfigResolver initialized with {} configs", cache.len());
        Ok(())
    }

    /// Reload configurations from store.
    pub async fn reload(&self) -> Result<(), convex_traits::error::TraitError> {
        self.init().await
    }

    /// Resolve the best configuration for a bond.
    pub async fn resolve(&self, bond: &BondReferenceData) -> Option<BondPricingConfig> {
        let configs = self.configs.read().await;

        for config in configs.iter() {
            if !config.active {
                continue;
            }

            if config.applies_to.matches(bond) {
                // Return the first match (highest priority because of sort)
                return Some(config.clone());
            }
        }

        None
    }
}
