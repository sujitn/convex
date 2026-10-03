//! Immutable pricing configuration history with local version compare-and-set.

use crate::database::*;
use async_trait::async_trait;
use convex_ports::error::TraitError;
use convex_ports::storage::{
    BenchmarkReference, BondPricingConfig, ConfigStore, ConfigVersion, PricingSpec,
};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::sync::Arc;

pub(crate) const CONFIGS: TableDefinition<&str, &[u8]> = TableDefinition::new("pricing_configs");
pub(crate) const CONFIG_HISTORY: TableDefinition<(&str, u64), &[u8]> =
    TableDefinition::new("pricing_config_history");

/// Pricing configuration storage with atomic current content and immutable history.
pub struct RedbConfigStore {
    db: Arc<Database>,
}

impl RedbConfigStore {
    /// Validate/migrate the schema before accepting configuration operations.
    pub fn new(db: Arc<Database>) -> Result<Self, TraitError> {
        crate::migrate_database(&db)?;
        Ok(Self { db })
    }
}

pub(crate) fn validate_config(config: &BondPricingConfig) -> Result<(), TraitError> {
    validate_id(&config.config_id)?;
    if config.version == 0
        || config.updated_at < config.created_at
        || config.description.len() > 4096
    {
        return Err(TraitError::InvalidInput(
            "invalid configuration version, time or description",
        ));
    }
    config
        .applies_to
        .validate()
        .map_err(TraitError::InvalidInput)?;
    validate_id(config.analytics_curves.discount_curve.as_str())?;
    for id in [
        &config.analytics_curves.government_curve,
        &config.analytics_curves.swap_curve,
        &config.analytics_curves.frn_reference_curve,
    ]
    .into_iter()
    .flatten()
    {
        validate_id(id.as_str())?;
    }
    let valid = match &config.pricing_spec {
        PricingSpec::Auto | PricingSpec::MarketQuote { .. } => true,
        PricingSpec::BenchmarkSpread {
            benchmark,
            spread_bps,
        } => {
            let valid_benchmark = match benchmark {
                BenchmarkReference::ExplicitYield { yield_pct } => yield_pct.is_finite(),
                BenchmarkReference::GovernmentCurve { curve }
                | BenchmarkReference::SwapCurve { curve }
                | BenchmarkReference::OnTheRun { curve, .. } => validate_id(curve.as_str()).is_ok(),
                BenchmarkReference::SpecificBond { security_id, .. } => {
                    validate_id(security_id.as_str()).is_ok()
                }
            };
            valid_benchmark && spread_bps.is_finite()
        }
        PricingSpec::DiscountMargin {
            reference_curve,
            quoted_margin_bps,
            discount_margin_bps,
        } => {
            validate_id(reference_curve.as_str()).is_ok()
                && quoted_margin_bps.is_finite()
                && discount_margin_bps.is_finite()
        }
        PricingSpec::ZSpread {
            discount_curve,
            z_spread_bps,
        } => validate_id(discount_curve.as_str()).is_ok() && z_spread_bps.is_finite(),
        PricingSpec::OptionAdjusted {
            discount_curve,
            vol_surface,
            oas_bps,
            mean_reversion,
            tree_steps,
        } => {
            validate_id(discount_curve.as_str()).is_ok()
                && validate_id(vol_surface.as_str()).is_ok()
                && oas_bps.is_finite()
                && mean_reversion.is_none_or(|v| v.is_finite() && v >= 0.0)
                && tree_steps.is_none_or(|v| (1..=10000).contains(&v))
        }
        PricingSpec::RealYield {
            real_yield_pct,
            inflation_curve,
            index_ratio,
        } => {
            real_yield_pct.is_finite()
                && validate_id(inflation_curve.as_str()).is_ok()
                && index_ratio.is_sign_positive()
                && !index_ratio.is_zero()
        }
    };
    let valid_spread = config.bid_ask_spread.as_ref().is_none_or(|s| {
        [Some(s.half_spread_bps), s.bid_spread_bps, s.ask_spread_bps]
            .into_iter()
            .flatten()
            .all(|v| v.is_finite() && v >= 0.0)
    });
    if !valid || !valid_spread {
        return Err(TraitError::InvalidInput(
            "invalid configuration model inputs",
        ));
    }
    Ok(())
}

pub(crate) fn encode_config(config: &BondPricingConfig) -> Result<Vec<u8>, TraitError> {
    // The workspace's serde-float feature must not truncate the stored Decimal.
    // This is a storage representation; the existing native/wire config remains compatible.
    let mut value =
        serde_json::to_value(config).map_err(|e| TraitError::SerializationError(e.to_string()))?;
    if let PricingSpec::RealYield { index_ratio, .. } = &config.pricing_spec {
        let stored_ratio = value
            .get_mut("pricing_spec")
            .and_then(|spec| spec.get_mut("RealYield"))
            .and_then(|spec| spec.get_mut("index_ratio"))
            .ok_or(TraitError::Unsupported("configuration serialization shape"))?;
        *stored_ratio = serde_json::Value::String(index_ratio.to_string());
    }
    encode(&value)
}

pub(crate) fn decode_config(bytes: &[u8]) -> Result<BondPricingConfig, TraitError> {
    let config = decode(bytes)?;
    validate_config(&config)?;
    Ok(config)
}

#[async_trait]
impl ConfigStore for RedbConfigStore {
    async fn get(&self, id: &str) -> Result<Option<BondPricingConfig>, TraitError> {
        validate_id(id)?;
        let read = self.db.begin_read().map_err(database_error)?;
        read.open_table(CONFIGS)
            .map_err(database_error)?
            .get(id)
            .map_err(database_error)?
            .map(|v| decode_config(v.value()))
            .transpose()
    }

    async fn save(&self, config: &BondPricingConfig) -> Result<(), TraitError> {
        validate_config(config)?;
        let bytes = encode_config(config)?;
        let write = begin_write(&self.db)?;
        {
            let mut current = write.open_table(CONFIGS).map_err(database_error)?;
            let mut history = write.open_table(CONFIG_HISTORY).map_err(database_error)?;
            if let Some(previous) = history
                .get((config.config_id.as_str(), config.version))
                .map_err(database_error)?
            {
                return if encode_config(&decode_config(previous.value())?)? == bytes {
                    Ok(())
                } else {
                    Err(TraitError::Conflict(
                        "configuration version has different content",
                    ))
                };
            }
            let prior = current
                .get(config.config_id.as_str())
                .map_err(database_error)?
                .map(|v| decode_config(v.value()))
                .transpose()?;
            let expected = prior.as_ref().map_or(Some(1), |v| v.version.checked_add(1));
            if expected != Some(config.version) {
                return Err(TraitError::Conflict(
                    "configuration must use the next version",
                ));
            }
            if prior.as_ref().is_some_and(|v| {
                v.created_at != config.created_at || v.updated_at > config.updated_at
            }) {
                return Err(TraitError::InvalidInput(
                    "configuration creation time changed or update time regressed",
                ));
            }
            if (prior.is_none() && current.len().map_err(database_error)? >= MAX_RECORDS)
                || history.len().map_err(database_error)? >= MAX_HISTORY
            {
                return Err(TraitError::CapacityExceeded);
            }
            current
                .insert(config.config_id.as_str(), bytes.as_slice())
                .map_err(database_error)?;
            history
                .insert(
                    (config.config_id.as_str(), config.version),
                    bytes.as_slice(),
                )
                .map_err(database_error)?;
        }
        commit(write)
    }

    async fn list(&self) -> Result<Vec<BondPricingConfig>, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        let table = read.open_table(CONFIGS).map_err(database_error)?;
        if table.len().map_err(database_error)? > MAX_RECORDS {
            return Err(TraitError::CapacityExceeded);
        }
        table
            .iter()
            .map_err(database_error)?
            .map(|r| {
                let (_, v) = r.map_err(database_error)?;
                decode_config(v.value())
            })
            .collect()
    }

    async fn delete(&self, _id: &str) -> Result<bool, TraitError> {
        Err(TraitError::Unsupported(
            "versioned config deletion; save a new inactive version",
        ))
    }

    async fn get_version(
        &self,
        id: &str,
        version: u64,
    ) -> Result<Option<BondPricingConfig>, TraitError> {
        validate_id(id)?;
        if version == 0 {
            return Err(TraitError::InvalidInput("zero configuration version"));
        }
        let read = self.db.begin_read().map_err(database_error)?;
        read.open_table(CONFIG_HISTORY)
            .map_err(database_error)?
            .get((id, version))
            .map_err(database_error)?
            .map(|v| decode_config(v.value()))
            .transpose()
    }

    async fn list_versions(&self, id: &str) -> Result<Vec<ConfigVersion>, TraitError> {
        validate_id(id)?;
        let read = self.db.begin_read().map_err(database_error)?;
        let table = read.open_table(CONFIG_HISTORY).map_err(database_error)?;
        if table.len().map_err(database_error)? > MAX_HISTORY {
            return Err(TraitError::CapacityExceeded);
        }
        table
            .range((id, 1)..=(id, u64::MAX))
            .map_err(database_error)?
            .map(|r| {
                let (_, v) = r.map_err(database_error)?;
                let config = decode_config(v.value())?;
                Ok(ConfigVersion {
                    version: config.version,
                    timestamp: config.updated_at,
                    changed_by: None,
                    change_description: None,
                })
            })
            .collect()
    }
}
