//! Immutable, versioned native curve recipes: the single durable curve owner.
use crate::database::*;
use async_trait::async_trait;
use convex_core::contracts::{UtcTimestamp, Version};
use convex_core::ids::CurveId;
use convex_ports::error::TraitError;
use convex_ports::snapshot::SnapshotCurveRecipe;
use convex_ports::storage::{CurveRecipeHead, CurveStore, MAX_CURVE_LISTING};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::sync::Arc;
const RECIPES: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("curve_recipes_v1");

/// Recipe records only; no model calibration is performed here.
pub struct RedbCurveStore {
    db: Arc<Database>,
}
impl RedbCurveStore {
    /// Construct a curve store on the shared database handle.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CurveStore for RedbCurveStore {
    async fn save_recipe(&self, recipe: &SnapshotCurveRecipe) -> Result<(), TraitError> {
        recipe.validate()?;
        let bytes = serde_json::to_vec(recipe)
            .map_err(|_| TraitError::InvalidInput("curve recipe JSON"))?;
        if bytes.len() > convex_ports::snapshot::MAX_SNAPSHOT_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        let write = begin_write(&self.db)?;
        {
            let mut table = write.open_table(RECIPES).map_err(database_error)?;
            let key = (recipe.curve_id.as_str(), recipe.version.get());
            if let Some(prior) = table.get(key).map_err(database_error)? {
                return if prior.value() == bytes.as_slice() {
                    Ok(())
                } else {
                    Err(TraitError::Conflict("immutable curve recipe changed"))
                };
            }
            if !in_recording_order(&table, recipe)? {
                return Err(TraitError::Conflict("temporally reversed curve revision"));
            }
            if table.len().map_err(database_error)? >= MAX_HISTORY {
                return Err(TraitError::CapacityExceeded);
            }
            table
                .insert(key, bytes.as_slice())
                .map_err(database_error)?;
        }
        commit(write)
    }
    async fn get_recipe(
        &self,
        id: &CurveId,
        version: Version,
    ) -> Result<Option<SnapshotCurveRecipe>, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(RECIPES) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(database_error(e)),
        };
        table
            .get((id.as_str(), version.get()))
            .map_err(database_error)?
            .map(|v| decode_recipe(v.value(), id, version.get()))
            .transpose()
    }
    async fn recipe_at(
        &self,
        id: &CurveId,
        known_at: UtcTimestamp,
    ) -> Result<Option<SnapshotCurveRecipe>, TraitError> {
        validate_id(id.as_str())?;
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(RECIPES) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(database_error(e)),
        };
        let versions = table
            .range((id.as_str(), 1)..=(id.as_str(), u64::MAX))
            .map_err(database_error)?
            .rev();
        for (scanned, entry) in versions.enumerate() {
            if scanned >= MAX_RECIPE_SCAN {
                return Err(TraitError::CapacityExceeded);
            }
            let (key, value) = entry.map_err(database_error)?;
            let record = decode_recipe(value.value(), id, key.value().1)?;
            if record.recorded_at <= known_at {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }
    async fn recipe_heads(
        &self,
        after: Option<&CurveId>,
        limit: usize,
    ) -> Result<Vec<CurveRecipeHead>, TraitError> {
        if limit > MAX_CURVE_LISTING {
            return Err(TraitError::CapacityExceeded);
        }
        let read = self.db.begin_read().map_err(database_error)?;
        let table = match read.open_table(RECIPES) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(database_error(e)),
        };
        // One range seek per distinct identity: the next identity sorts after
        // every version of the current one, so (id + NUL, 0) skips them.
        let mut cursor = after.map(|id| format!("{}\0", id.as_str()));
        let mut heads = Vec::with_capacity(limit);
        while heads.len() < limit {
            let start = cursor.as_deref().unwrap_or("");
            let Some(first) = table.range((start, 0)..).map_err(database_error)?.next() else {
                break;
            };
            let id = CurveId::new(first.map_err(database_error)?.0.value().0);
            let Some(last) = table
                .range((id.as_str(), 0)..=(id.as_str(), u64::MAX))
                .map_err(database_error)?
                .next_back()
            else {
                break;
            };
            let (key, value) = last.map_err(database_error)?;
            let record = decode_recipe(value.value(), &id, key.value().1)?;
            heads.push(CurveRecipeHead {
                curve_id: id.clone(),
                version: record.version,
                recorded_at: record.recorded_at,
            });
            cursor = Some(format!("{}\0", id.as_str()));
        }
        Ok(heads)
    }
}

/// Versions inspected by one `recipe_at` before it refuses, bounding a read
/// against a curve with a very long revision history.
const MAX_RECIPE_SCAN: usize = 10_000;

/// D73: a curve's recording times never decrease as its version rises, the fixing
/// repository's rule; equal times are allowed. A new revision is checked against its
/// stored neighbours, which keeps a history written under this rule in order. Revisions
/// stored before the rule are not re-checked.
fn in_recording_order(
    table: &impl ReadableTable<(&'static str, u64), &'static [u8]>,
    recipe: &SnapshotCurveRecipe,
) -> Result<bool, TraitError> {
    let id = CurveId::new(recipe.curve_id.as_str());
    let version = recipe.version.get();
    let below = table
        .range((id.as_str(), 0)..(id.as_str(), version))
        .map_err(database_error)?
        .next_back();
    if let Some(entry) = below {
        let (key, value) = entry.map_err(database_error)?;
        if decode_recipe(value.value(), &id, key.value().1)?.recorded_at > recipe.recorded_at {
            return Ok(false);
        }
    }
    let above = table
        .range((id.as_str(), version)..=(id.as_str(), u64::MAX))
        .map_err(database_error)?
        .next();
    if let Some(entry) = above {
        let (key, value) = entry.map_err(database_error)?;
        if decode_recipe(value.value(), &id, key.value().1)?.recorded_at < recipe.recorded_at {
            return Ok(false);
        }
    }
    Ok(true)
}

fn decode_recipe(
    bytes: &[u8],
    id: &CurveId,
    version: u64,
) -> Result<SnapshotCurveRecipe, TraitError> {
    if bytes.len() > convex_ports::snapshot::MAX_SNAPSHOT_BYTES {
        return Err(TraitError::CapacityExceeded);
    }
    let record: SnapshotCurveRecipe = serde_json::from_slice(bytes)
        .map_err(|_| TraitError::InvalidInput("corrupt stored curve recipe"))?;
    record.validate()?;
    if record.curve_id.as_str() != id.as_str() || record.version.get() != version {
        return Err(TraitError::Conflict("curve recipe key mismatch"));
    }
    Ok(record)
}
