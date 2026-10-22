//! Resolve stored curve recipes for one request at an explicit information
//! cutoff. The curve store is the only curve owner; nothing here is cached.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use convex_core::contracts::{DataQuality, UtcTimestamp, Version};
use convex_core::ids::CurveId;
use convex_core::Currency;
use convex_curves::contracts::{CurveBuildOutput, CurveRecipe, CurveRole};
use convex_ports::snapshot::{SnapshotCurveRecipe, MAX_SNAPSHOT_CURVES};
use convex_ports::storage::CurveStore;
use convex_ports::TraitError;

use crate::curve_builder::BuiltCurve;

/// Why a request could not obtain a curve it needs. Nothing is substituted.
#[derive(Debug, thiserror::Error)]
pub enum CurveResolutionError {
    /// No pricing configuration applies to the instrument.
    #[error("no applicable pricing configuration")]
    NoConfiguration,
    /// No revision recorded by the cutoff, or a named dependency version is absent.
    #[error("missing curve {0}")]
    Missing(CurveId),
    /// The revision at the cutoff is marked stale/invalid or outside its freshness budget.
    #[error("stale curve {0}")]
    Stale(CurveId),
    /// The curve exists but its role or currency does not fit the configured use.
    #[error("curve {0} does not fit its configured role or currency")]
    Role(CurveId),
    /// Storage, decoding, capacity or native build failure.
    #[error(transparent)]
    Port(#[from] TraitError),
}

/// Stored recipes consumed by one request, with their built native graph.
pub struct ResolvedCurves {
    records: Vec<SnapshotCurveRecipe>,
    built: CurveBuildOutput,
}

impl ResolvedCurves {
    /// Every consumed (curve, version) pin, roots and dependencies.
    pub fn pins(&self) -> impl Iterator<Item = (&str, Version)> {
        self.records
            .iter()
            .map(|r| (r.curve_id.as_str(), r.version))
    }

    /// Every consumed stored record, roots first.
    pub fn records(&self) -> &[SnapshotCurveRecipe] {
        &self.records
    }

    /// The stored record consumed for `id`.
    pub fn record(&self, id: &CurveId) -> Result<&SnapshotCurveRecipe, CurveResolutionError> {
        self.records
            .iter()
            .find(|r| r.curve_id.as_str() == id.as_str())
            .ok_or_else(|| CurveResolutionError::Missing(id.clone()))
    }

    /// The native built curve for `id`, with its original recipe and diagnostics.
    pub fn native(
        &self,
        id: &CurveId,
    ) -> Result<&Arc<convex_curves::contracts::BuiltCurve>, CurveResolutionError> {
        let key = self
            .built
            .curves
            .keys()
            .find(|key| key.id == id.as_str())
            .ok_or_else(|| CurveResolutionError::Missing(id.clone()))?;
        self.built
            .curves
            .get(key)
            .map_err(|_| CurveResolutionError::Missing(id.clone()))
    }

    /// The priceable representation of `id`.
    pub fn curve(&self, id: &CurveId) -> Result<BuiltCurve, CurveResolutionError> {
        Ok(BuiltCurve::from_recipe(
            self.record(id)?,
            self.native(id)?.clone(),
        )?)
    }
}

/// Resolve `roots` at `known_at`. Each root is the highest revision recorded by
/// the cutoff; each dependency is a revision whose native key is exactly the
/// one its recipe names, also recorded by the cutoff. Every consumed record must
/// be fresh at it.
pub async fn resolve_curves(
    store: &dyn CurveStore,
    roots: &[CurveId],
    known_at: UtcTimestamp,
) -> Result<ResolvedCurves, CurveResolutionError> {
    if roots.len() > MAX_SNAPSHOT_CURVES {
        return Err(TraitError::CapacityExceeded.into());
    }
    let mut records: Vec<SnapshotCurveRecipe> = Vec::new();
    for id in roots {
        if records.iter().any(|r| r.curve_id.as_str() == id.as_str()) {
            continue;
        }
        let record = store
            .recipe_at(id, known_at)
            .await?
            .ok_or_else(|| CurveResolutionError::Missing(id.clone()))?;
        records.push(record);
    }
    let mut next = 0;
    while next < records.len() {
        for (id, label) in dependencies(&records[next]) {
            if records.iter().any(|r| r.curve_id.as_str() == id.as_str()) {
                continue; // a different pinned version fails the native build below
            }
            if records.len() >= MAX_SNAPSHOT_CURVES {
                return Err(TraitError::CapacityExceeded.into());
            }
            let missing = || CurveResolutionError::Missing(id.clone());
            let label = label.ok_or_else(missing)?;
            let record = dependency_record(store, &id, &label, known_at)
                .await?
                .ok_or_else(missing)?;
            records.push(record);
        }
        next += 1;
    }
    for record in &records {
        check_fresh(record, known_at)?;
    }
    let built = crate::curve_replay::rebuild_curve_recipes(&records).map_err(TraitError::from)?;
    Ok(ResolvedCurves { records, built })
}

/// Pricing role admission shared with snapshot valuation: the configured
/// discount curve must discount in the instrument currency; other roles project.
pub(crate) fn pricing_role_matches(
    recipe: &CurveRecipe,
    discount: bool,
    currency: Currency,
) -> bool {
    let role = if discount {
        matches!(recipe.role, CurveRole::Discount { collateral_currency } if collateral_currency == currency)
    } else {
        matches!(recipe.role, CurveRole::Projection { .. })
    };
    role && recipe.currency == currency
}

/// The same freshness rule as current core snapshot assembly, at one cutoff.
fn check_fresh(
    record: &SnapshotCurveRecipe,
    known_at: UtcTimestamp,
) -> Result<(), CurveResolutionError> {
    let age = |at: UtcTimestamp| (known_at.datetime() - at.datetime()).num_milliseconds();
    if matches!(record.quality, DataQuality::Invalid | DataQuality::Stale)
        || record.source_time > known_at
        || record.recorded_at > known_at
        || age(record.source_time) > record.max_age_ms
        || age(record.recorded_at) > record.max_age_ms
    {
        return Err(CurveResolutionError::Stale(record.curve_id.clone().into()));
    }
    Ok(())
}

/// The stored revision of `id` whose native key version is exactly `label`,
/// recorded by the cutoff. Two candidates are tried, nothing else is searched:
/// the revision numbered `label` when it is decimal, then the revision current
/// at the cutoff.
async fn dependency_record(
    store: &dyn CurveStore,
    id: &CurveId,
    label: &str,
    known_at: UtcTimestamp,
) -> Result<Option<SnapshotCurveRecipe>, TraitError> {
    let exact = |r: &SnapshotCurveRecipe| {
        r.recorded_at <= known_at
            && r.recipe.pointer("/key/version").and_then(|v| v.as_str()) == Some(label)
    };
    if let Some(version) = label.parse::<u64>().ok().and_then(|v| Version::new(v).ok()) {
        if let Some(record) = store.get_recipe(id, version).await?.filter(exact) {
            return Ok(Some(record));
        }
    }
    Ok(store.recipe_at(id, known_at).await?.filter(exact))
}

/// Dependencies named by the native recipe, with their native key version
/// labels; `None` when a dependency names no version.
fn dependencies(record: &SnapshotCurveRecipe) -> Vec<(CurveId, Option<String>)> {
    let Some(items) = record.recipe.get("dependencies").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?;
            let label = item
                .get("version")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            Some((CurveId::new(id), label))
        })
        .collect()
}
