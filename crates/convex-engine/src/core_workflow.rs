//! Core snapshot assembly and durable calculation over existing ports/native owners.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use convex::library::{LibraryInputs, LibraryOperation, LibraryRequest};
use convex_core::{
    contracts::*,
    fixings::{FixingObservation, FixingStatus},
};
use convex_ports::{
    core_workflow::*, fixing::FixingRepository, reference_master::*,
    reference_repository::ReferenceRepository, runtime::RuntimeStore, snapshot::SnapshotRepository,
    storage::CurveStore, TraitError,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
mod checkpoint;
mod issuer;
mod lifecycle_history;

/// Operational failures do not masquerade as accepted native financial results.
#[derive(Debug, thiserror::Error)]
pub enum CoreWorkflowError {
    /// Missing exact input, snapshot, named calculation or result.
    #[error("core input or record not found")]
    NotFound,
    /// Publication may have committed; inspect this deterministic snapshot identity.
    #[error("core snapshot publication outcome unknown")]
    SnapshotUnknown(SnapshotId),
    /// Result may have committed; inspect this deterministic result identity.
    #[error("core result commit outcome unknown")]
    ResultUnknown(String),
    /// Adapter failure with its original class retained inside the application.
    #[error(transparent)]
    Port(#[from] TraitError),
}
type Result<T> = std::result::Result<T, CoreWorkflowError>;
fn invalid(reason: &'static str) -> CoreWorkflowError {
    TraitError::InvalidInput(reason).into()
}

/// Durable acknowledgement; financial digits remain in the original stored envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreRunReceipt {
    /// Named calculation that produced the stored result.
    pub calculation: String,
    /// Exact consumed immutable inputs.
    pub snapshot_id: SnapshotId,
    /// Complete-context stored result key.
    pub result_id: String,
    /// True only when an existing durable result was returned without calculation.
    pub reused: bool,
    /// Host-supplied build identity used in this durable result key.
    pub build_id: ModelId,
}
/// Application use cases. Adapters own DB/files/provider behavior; no new scheduler.
pub struct CoreWorkflow {
    /// Existing versioned reference boundary, also used by reference administration.
    pub references: Arc<dyn ReferenceRepository>,
    /// Existing curve boundary, now retaining whole native recipes.
    pub curves: Arc<dyn CurveStore>,
    /// Existing selected-source published fixing boundary.
    pub fixings: Arc<dyn FixingRepository>,
    snapshots: Arc<dyn SnapshotRepository>,
    results: Arc<dyn RuntimeStore>,
    build: ModelId,
}
impl CoreWorkflow {
    /// Compose the actual existing backend ports. Build identity belongs to the host.
    pub fn new(
        references: Arc<dyn ReferenceRepository>,
        curves: Arc<dyn CurveStore>,
        fixings: Arc<dyn FixingRepository>,
        snapshots: Arc<dyn SnapshotRepository>,
        results: Arc<dyn RuntimeStore>,
        build: ModelId,
    ) -> Self {
        Self {
            references,
            curves,
            fixings,
            snapshots,
            results,
            build,
        }
    }
    /// Resolve exact immutable versions and all eligible history, then publish the whole snapshot.
    pub async fn assemble(&self, plan: CoreSnapshotPlan) -> Result<CoreSnapshot> {
        plan.validate()?;
        let definition = self
            .references
            .get_master_version(
                &MasterKey::Calculation(plan.definition.id.clone()),
                plan.definition.version,
            )
            .await
            .map_err(|e| match e {
                convex_ports::reference_repository::ReferenceError::Unavailable => {
                    TraitError::SourceNotAvailable("reference repository".into())
                }
                convex_ports::reference_repository::ReferenceError::CapacityExceeded => {
                    TraitError::CapacityExceeded
                }
                _ => TraitError::InvalidInput("core reference read failed"),
            })?
            .ok_or(CoreWorkflowError::NotFound)?;
        let issuer_scope = if let Some(pin) = issuer::selection(&definition)? {
            let record = self
                .references
                .get_master_at(
                    &MasterKey::IssuerScope(pin.id.clone()),
                    ReferenceLookup {
                        as_of: plan.as_of,
                        known_at: plan.known_at,
                    },
                )
                .await
                .map_err(|_| invalid("issuer reference read failed"))?
                .ok_or(CoreWorkflowError::NotFound)?;
            if record.version != pin.version {
                return Err(invalid(
                    "issuer scope pin is obsolete at the captured cutoff",
                ));
            }
            Some(record)
        } else {
            None
        };
        let mut curves = BTreeMap::new();
        for (name, pins) in &plan.curve_sets {
            let mut records = Vec::new();
            for pin in pins {
                records.push(
                    self.curves
                        .get_recipe(
                            &convex_core::ids::CurveId::new(pin.id.as_str()),
                            pin.version,
                        )
                        .await?
                        .ok_or(CoreWorkflowError::NotFound)?,
                );
            }
            curves.insert(name.clone(), records);
        }
        let mut fixings = if plan.fixings.is_empty() {
            vec![]
        } else {
            self.fixings
                .history_at(&plan.fixings, plan.known_at)
                .await?
        };
        fixings.sort_by(|a, b| (&a.key, a.version).cmp(&(&b.key, b.version)));
        let mut content = CoreSnapshotContent {
            schema_version: 1,
            plan,
            definition,
            curves,
            fixings,
            issuer_scope,
            checkpoint: None,
        };
        let provisional = CoreSnapshot::new(content.clone())?;
        content.checkpoint = self.capture_checkpoint(&provisional).await?;
        let snapshot = CoreSnapshot::new(content)?;
        validate_snapshot(&snapshot)?;
        match self.snapshots.publish_core(&snapshot).await {
            Err(TraitError::UnknownOutcome) => {
                return Err(CoreWorkflowError::SnapshotUnknown(snapshot.id.clone()))
            }
            other => other?,
        }
        Ok(snapshot)
    }
    /// Read exact saved inputs. Replay never consults current reference/curve/fixing ports.
    pub async fn snapshot(&self, id: &SnapshotId) -> Result<CoreSnapshot> {
        let snapshot = self
            .snapshots
            .get_core(id)
            .await?
            .ok_or(CoreWorkflowError::NotFound)?;
        if snapshot.id != *id {
            return Err(
                TraitError::Conflict("core snapshot adapter returned another identity").into(),
            );
        }
        validate_snapshot(&snapshot)?;
        Ok(snapshot)
    }
    /// Read a durable native envelope without recalculation.
    pub async fn result(&self, id: &str) -> Result<CoreResult> {
        let result = self
            .results
            .get_core_result(id)
            .await?
            .ok_or(CoreWorkflowError::NotFound)?;
        result.verify()?;
        if result.id != id {
            return Err(
                TraitError::Conflict("core result adapter returned another identity").into(),
            );
        }
        Ok(result)
    }
    /// Calculate from saved inputs and acknowledge only after the exact result commits.
    /// Recompute uses saved inputs even if providers have changed or disappeared.
    pub async fn run(
        &self,
        id: SnapshotId,
        calculation: String,
        recompute: bool,
    ) -> Result<CoreRunReceipt> {
        let snapshot = self.snapshot(&id).await?;
        checkpoint::check_build(&snapshot, &self.build)?;
        let request = materialize(&snapshot, &calculation)?;
        let key = CoreResult::key(&id, &calculation, &self.build);
        if !recompute {
            if let Some(prior) = self.results.get_core_result(&key).await? {
                prior.verify()?;
                if prior.id != key {
                    return Err(TraitError::Conflict("core result adapter key mismatch").into());
                }
                return Ok(CoreRunReceipt {
                    calculation,
                    snapshot_id: id,
                    result_id: key,
                    reused: true,
                    build_id: self.build.clone(),
                });
            }
        }
        let json = serde_json::to_string(&request)
            .map_err(|_| invalid("core native request serialization"))?;
        let output = tokio::task::spawn_blocking(move || convex::library::execute_json(&json))
            .await
            .map_err(|_| TraitError::SourceNotAvailable("native calculation worker".into()))?;
        let output = checkpoint::output(&snapshot, &calculation, output)?;
        let result = CoreResult::new(id.clone(), calculation.clone(), self.build.clone(), output)?;
        match self.results.save_core_result(&result).await {
            Err(TraitError::UnknownOutcome) => {
                return Err(CoreWorkflowError::ResultUnknown(result.id))
            }
            other => other?,
        }
        Ok(CoreRunReceipt {
            calculation,
            snapshot_id: id,
            result_id: result.id,
            reused: false,
            build_id: self.build.clone(),
        })
    }
}

fn definition(snapshot: &CoreSnapshot) -> Result<&CoreDefinition> {
    match &snapshot.content.definition.definition {
        MasterDefinition::Calculation(v) => Ok(v),
        _ => Err(invalid("core calculation reference kind")),
    }
}
fn validate_snapshot(snapshot: &CoreSnapshot) -> Result<()> {
    snapshot.verify()?;
    issuer::validate(snapshot)?;
    checkpoint::validate(snapshot)?;
    lifecycle_history::validate(snapshot)?;
    let content = &snapshot.content;
    let plan = &content.plan;
    let record = &content.definition;
    let definition = definition(snapshot)?;
    definition
        .validate()
        .map_err(|_| invalid("invalid core definition"))?;
    if definition.id != plan.definition.id
        || record.version != plan.definition.version
        || record.schema_version != SchemaVersion::V2
        || record.recorded_at > plan.known_at
        || !definition.effective.contains(plan.as_of)
    {
        return Err(invalid("core reference identity/version/cutoff mismatch"));
    }
    if content.curves.keys().ne(plan.curve_sets.keys()) {
        return Err(invalid("incomplete core curve sets"));
    }
    for (name, pins) in &plan.curve_sets {
        // Historical P&L markets remain immutable. Check them at their own
        // information cutoffs below; current calculations still require fresh
        // inputs at the rolled plan cutoffs.
        let current = definition.calculations.values().any(|calculation| {
            calculation
                .request
                .pointer("/operation/kind")
                .and_then(serde_json::Value::as_str)
                != Some("book_pnl")
                && calculation.curve_sets.contains(name)
        });
        let records = &content.curves[name];
        if records.len() != pins.len() {
            return Err(invalid("incomplete core curve set"));
        }
        for (record, pin) in records.iter().zip(pins) {
            record.validate()?;
            if matches!(record.quality, DataQuality::Invalid | DataQuality::Stale) {
                return Err(invalid("core curve source is marked invalid or stale"));
            }
            if record.curve_id.as_str() != pin.id.as_str()
                || record.version != pin.version
                || record.source_time > plan.as_of
                || record.recorded_at > plan.known_at
                || current
                    && (plan.as_of.datetime() - record.source_time.datetime()).num_milliseconds()
                        > record.max_age_ms
                || current
                    && (plan.known_at.datetime() - record.recorded_at.datetime()).num_milliseconds()
                        > record.max_age_ms
            {
                return Err(invalid(
                    "core curve identity/version/cutoff/freshness mismatch",
                ));
            }
        }
    }
    let selected: BTreeSet<_> = plan.fixings.iter().collect();
    let mut present = BTreeSet::new();
    let mut revisions = BTreeSet::new();
    for fixing in &content.fixings {
        fixing.validate()?;
        if !selected.contains(&fixing.key)
            || !fixing.entitled
            || fixing.published_at > plan.known_at
            || fixing.recorded_at > plan.known_at
            || !revisions.insert((&fixing.key, fixing.version))
        {
            return Err(invalid(
                "core fixing identity/revision/cutoff/entitlement mismatch",
            ));
        }
        present.insert(&fixing.key);
    }
    if present != selected {
        return Err(CoreWorkflowError::NotFound);
    }
    let mut used = BTreeSet::new();
    // Shared reference terms are immutable within this snapshot. Decode once;
    // every calculation still receives its own inputs and all existing guards.
    let reference = issuer::native_inputs(&definition.reference_inputs)?;
    for (name, calculation) in &definition.calculations {
        for set in &calculation.curve_sets {
            used.insert(set);
        }
        materialize_with_reference(snapshot, name, reference.clone())?;
    }
    if used != plan.curve_sets.keys().collect() {
        return Err(invalid("unused or missing core curve selection"));
    }
    Ok(())
}
fn fill(inputs: &mut LibraryInputs, sets: &[String], snapshot: &CoreSnapshot) -> Result<()> {
    if !inputs.curves.is_empty() || !inputs.fixings.is_empty() || sets.len() != 1 {
        return Err(invalid(
            "core definitions must leave selected curve/fixing values empty",
        ));
    }
    inputs.curves = curves(snapshot, &sets[0])?;
    inputs.fixings = fixings(snapshot);
    Ok(())
}
fn curves(
    snapshot: &CoreSnapshot,
    name: &str,
) -> Result<Vec<convex_curves::contracts::CurveRecipe>> {
    snapshot
        .content
        .curves
        .get(name)
        .ok_or_else(|| invalid("missing named core curve set"))?
        .iter()
        .map(|r| {
            serde_json::from_value(r.recipe.clone())
                .map_err(|_| invalid("invalid full native curve recipe"))
        })
        .collect()
}
fn fixings(snapshot: &CoreSnapshot) -> Vec<FixingObservation> {
    snapshot
        .content
        .fixings
        .iter()
        .map(|f| FixingObservation {
            index_id: f.key.index_id.clone(),
            observation_date: f.key.observation_date.date(),
            rate: f.rate.value().decimal(),
            published_at: f.published_at,
            known_at: f.recorded_at,
            revision: f.version,
            status: if f.is_preliminary {
                FixingStatus::Preliminary
            } else {
                FixingStatus::Final
            },
            source_id: f.key.source.clone(),
        })
        .collect()
}
/// Materialize only the actual core workflow operations; no JSON-path/template language.
fn materialize(snapshot: &CoreSnapshot, name: &str) -> Result<LibraryRequest> {
    let reference = issuer::native_inputs(&definition(snapshot)?.reference_inputs)?;
    materialize_with_reference(snapshot, name, reference)
}
fn materialize_with_reference(
    snapshot: &CoreSnapshot,
    name: &str,
    reference: LibraryInputs,
) -> Result<LibraryRequest> {
    let definition = definition(snapshot)?;
    let calc = definition
        .calculations
        .get(name)
        .ok_or(CoreWorkflowError::NotFound)?;
    let mut request: LibraryRequest =
        serde_json::from_value(lifecycle_history::expand(snapshot, &calc.request)?)
            .map_err(|_| invalid("invalid native core definition"))?;
    if !reference.curves.is_empty() || !reference.fixings.is_empty() {
        return Err(invalid(
            "core reference inputs cannot contain market values",
        ));
    }
    if request.schema_version != convex::library::LIBRARY_SCHEMA_VERSION {
        return Err(invalid("unsupported native core schema"));
    }
    if let LibraryOperation::LifecycleHedgeRisk { history, .. } = &request.operation {
        if history.points.iter().any(|point| {
            point.known_at > snapshot.content.plan.known_at
                || point.valuation_date.as_naive_date()
                    > snapshot.content.plan.as_of.datetime().date_naive()
        }) {
            return Err(invalid(
                "lifecycle risk history is beyond the captured cutoffs",
            ));
        }
    }
    match &mut request.operation {
        LibraryOperation::GraphHedgeRisk { input, .. }
        | LibraryOperation::LifecycleHedgeRisk { input, .. } => {
            if !empty(&request.inputs) {
                return Err(invalid("core request must use shared reference inputs"));
            }
            request.inputs = reference;
            fill(&mut request.inputs, &calc.curve_sets, snapshot)?;
            // The whole book shares this real content identity. Risk from another
            // resolved input set cannot masquerade as the same hedge snapshot.
            input.snapshot_id = snapshot.id.clone();
        }
        LibraryOperation::BookPnl { input } => {
            if !empty(&request.inputs) || !empty(&input.resources) {
                return Err(invalid("core P&L must use shared reference inputs"));
            }
            input.resources = reference;
            if !request.inputs.curves.is_empty()
                || !request.inputs.fixings.is_empty()
                || calc.curve_sets.len() != input.markets.len()
                || !input.resources.curves.is_empty()
                || !input.resources.fixings.is_empty()
            {
                return Err(invalid("invalid core P&L resource bindings"));
            }
            let bindings: Vec<_> = std::iter::once(&input.before)
                .chain(input.steps.iter().map(|s| &s.after))
                .chain(input.standalone.iter().filter_map(|s| match s {
                    convex::library::CausalPnlCounterfactual::Available { information } => {
                        Some(information.as_ref())
                    }
                    _ => None,
                }))
                .collect();
            for binding in &bindings {
                // A frozen market can also be used for a later carry/fixing
                // counterfactual. Freshness belongs to its first selection;
                // availability still belongs to every actual point.
                let selected_at = bindings
                    .iter()
                    .filter(|b| b.market == binding.market)
                    .map(|b| b.point.known_at)
                    .min()
                    .ok_or_else(|| invalid("missing original market selection"))?;
                if !binding.additional_fixings.is_empty() {
                    return Err(invalid(
                        "core P&L fixing values must come from the fixing port",
                    ));
                }
                if binding.point.known_at > snapshot.content.plan.known_at
                    || binding.point.valuation_date.as_naive_date()
                        > snapshot.content.plan.as_of.datetime().date_naive()
                {
                    return Err(invalid("core P&L point is beyond the captured cutoffs"));
                }
                let set = calc
                    .curve_sets
                    .get(binding.market)
                    .ok_or_else(|| invalid("invalid core P&L market binding"))?;
                let records = snapshot
                    .content
                    .curves
                    .get(set)
                    .ok_or_else(|| invalid("missing core P&L market set"))?;
                if records.iter().any(|r| {
                    r.recorded_at > binding.point.known_at
                        || r.source_time > binding.point.known_at
                        || r.source_time.datetime().date_naive()
                            > binding.point.valuation_date.as_naive_date()
                        || (selected_at.datetime() - r.source_time.datetime()).num_milliseconds()
                            > r.max_age_ms
                        || (selected_at.datetime() - r.recorded_at.datetime()).num_milliseconds()
                            > r.max_age_ms
                }) {
                    return Err(invalid(
                        "core P&L market was not available to its original point",
                    ));
                }
            }
            let opening_cutoff = bindings
                .iter()
                .map(|b| b.point.known_at)
                .min()
                .ok_or_else(|| invalid("missing original fixing cutoff"))?;
            let captured = fixings(snapshot);
            let eligible =
                |f: &FixingObservation, cutoff| f.known_at <= cutoff && f.published_at <= cutoff;
            input.resources.fixings = captured
                .iter()
                .filter(|f| eligible(f, opening_cutoff))
                .cloned()
                .collect();
            // The fixing port owns every value. Historical points see only
            // revisions actually published and received at their own cutoff.
            for binding in std::iter::once(&mut input.before)
                .chain(input.steps.iter_mut().map(|s| &mut s.after))
                .chain(input.standalone.iter_mut().filter_map(|s| match s {
                    convex::library::CausalPnlCounterfactual::Available { information } => {
                        Some(information.as_mut())
                    }
                    _ => None,
                }))
            {
                binding.additional_fixings = captured
                    .iter()
                    .filter(|f| !eligible(f, opening_cutoff) && eligible(f, binding.point.known_at))
                    .cloned()
                    .collect();
            }
            for (market, set) in input.markets.iter_mut().zip(&calc.curve_sets) {
                if !market.curves.is_empty() {
                    return Err(invalid("core P&L curves must come from the curve port"));
                }
                market.curves = curves(snapshot, set)?;
            }
        }
        LibraryOperation::Hedge { .. } => {
            if !empty(&request.inputs)
                || !empty(&reference)
                || !calc.curve_sets.is_empty()
                || !snapshot.content.plan.fixings.is_empty()
            {
                return Err(invalid("pure core hedge does not consume market bindings"));
            }
        }
        _ => {
            return Err(TraitError::Unsupported(
                "managed core operation; use the full supplied native API for other operations",
            )
            .into())
        }
    }
    Ok(request)
}

fn empty(inputs: &LibraryInputs) -> bool {
    inputs.calendars.is_empty()
        && inputs.helper_calendars.is_empty()
        && inputs.curves.is_empty()
        && inputs.fixings.is_empty()
        && inputs.publications.is_empty()
        && inputs.calibration.is_none()
}
