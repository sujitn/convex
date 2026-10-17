//! Frozen bounded fixing history with explicit publication and recording timestamps.
#![forbid(unsafe_code)]

use async_trait::async_trait;
use convex_core::contracts::UtcTimestamp;
use convex_ports::{
    fixing::{select_fixing_at, FixingKey, FixingRepository, PublishedFixing},
    TraitError,
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryFile {
    schema_version: u32,
    fixings: Vec<PublishedFixing>,
}

/// Read-only frozen JSON history. Reopen a new input file to capture appended revisions.
/// The earlier CSV current snapshot lacks publication metadata and is deliberately not inferred here.
pub struct JsonFixingRepository {
    histories: BTreeMap<FixingKey, Vec<PublishedFixing>>,
}
impl JsonFixingRepository {
    /// Explicit empty fixing dependency for products that require no fixing keys.
    pub fn empty() -> Self {
        Self {
            histories: BTreeMap::new(),
        }
    }

    /// Validate the entire regular, non-symlink <=16 MiB schema-1 file before exposing any records.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, TraitError> {
        let bytes =
            super::snapshot_repository::read_regular_bounded(path.as_ref(), 16 * 1024 * 1024)?;
        let file: HistoryFile = serde_json::from_slice(&bytes)
            .map_err(|_| TraitError::InvalidInput("invalid fixing history JSON"))?;
        if file.schema_version != 1 {
            return Err(TraitError::Unsupported("fixing history schema"));
        }
        if file.fixings.len() > 100_000 {
            return Err(TraitError::CapacityExceeded);
        }
        let mut histories: BTreeMap<FixingKey, Vec<PublishedFixing>> = BTreeMap::new();
        for fixing in file.fixings {
            fixing.validate()?;
            histories
                .entry(fixing.key.clone())
                .or_default()
                .push(fixing);
        }
        for history in histories.values_mut() {
            history.sort_by_key(|fixing| fixing.version);
            if history.windows(2).any(|pair| {
                pair[0].version == pair[1].version || pair[0].recorded_at > pair[1].recorded_at
            }) {
                return Err(TraitError::Conflict(
                    "duplicate or temporally reversed fixing revision",
                ));
            }
        }
        Ok(Self { histories })
    }
}
#[async_trait]
impl FixingRepository for JsonFixingRepository {
    async fn history_at(
        &self,
        keys: &[FixingKey],
        known_at: UtcTimestamp,
    ) -> Result<Vec<PublishedFixing>, TraitError> {
        if keys.len() > 1024 {
            return Err(TraitError::CapacityExceeded);
        }
        let selected: std::collections::BTreeSet<_> = keys.iter().collect();
        if selected.len() != keys.len() {
            return Err(TraitError::InvalidInput("duplicate fixing history key"));
        }
        let mut values = Vec::new();
        for key in keys {
            if let Some(history) = self.histories.get(key) {
                values.extend(
                    history
                        .iter()
                        .filter(|f| f.published_at <= known_at && f.recorded_at <= known_at)
                        .cloned(),
                );
            }
        }
        Ok(values)
    }
    async fn get_at(
        &self,
        key: &FixingKey,
        known_at: UtcTimestamp,
    ) -> Result<Option<PublishedFixing>, TraitError> {
        match self.histories.get(key) {
            None => Ok(None),
            Some(history) => select_fixing_at(history, key, known_at),
        }
    }
}
