//! Durable core calculation inputs and results; native economics stay opaque at ports.
#![forbid(unsafe_code)]
#![deny(unused_must_use)]

use crate::{fixing::*, reference_master::*, snapshot::*, TraitError};
use convex_core::contracts::{ModelId, ReferenceDataId, SnapshotId, UtcTimestamp};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Bounded named native calculation with explicit resource-set bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreCalculation {
    /// Full native LibraryRequest, with curve/fixing values absent until assembly.
    pub request: serde_json::Value,
    /// One outer set, or each BookPnl market in order; pure hedge uses none.
    pub curve_sets: Vec<String>,
}

/// Versioned full native contracts/settings, retained in the existing reference master.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreDefinition {
    /// Internal definition identity.
    pub id: ReferenceDataId,
    /// Shared native reference inputs (calendars/publication rules), without curve/fixing values.
    /// Optional reserved `issuer_scope` selects an exact master resource; the
    /// engine captures it separately before decoding the native reference inputs.
    pub reference_inputs: serde_json::Value,
    /// Explicit named calculations; no script or financial dispatch at this port.
    pub calculations: BTreeMap<String, CoreCalculation>,
    /// Economic validity of the supplied definition.
    pub effective: EffectiveInterval,
    /// Provider-neutral provenance.
    pub provenance: ReferenceProvenance,
}
impl CoreDefinition {
    /// Explicit saved operational profile; absent definitions keep their original bounds.
    pub fn expanded_capacity(&self) -> Result<bool, crate::reference_repository::ReferenceError> {
        match self.reference_inputs.get("core_capacity") {
            None => Ok(false),
            Some(serde_json::Value::String(value)) if value == "book_128" => Ok(true),
            _ => Err(crate::reference_repository::ReferenceError::InvalidInput(
                "unknown core capacity profile",
            )),
        }
    }
    /// Structural limits only. The application decodes actual native contracts.
    pub fn validate(&self) -> Result<(), crate::reference_repository::ReferenceError> {
        use crate::reference_repository::ReferenceError as E;
        self.effective.validate()?;
        let expanded = self.expanded_capacity()?;
        let (calculations, nodes, bytes) = if expanded {
            (130, 524_288, 8 * 1024 * 1024)
        } else {
            (32, 65_536, 1024 * 1024)
        };
        if self.calculations.is_empty() || self.calculations.len() > calculations {
            if expanded {
                return Err(E::CapacityExceeded);
            }
            return Err(E::InvalidInput("expected 1..32 core calculations"));
        }
        if !self.reference_inputs.is_object() {
            return Err(E::InvalidInput("core reference input object required"));
        }
        let mut pending = vec![(&self.reference_inputs, 0usize)];
        pending.extend(self.calculations.values().map(|c| (&c.request, 0)));
        let mut visited = 0usize;
        while let Some((value, depth)) = pending.pop() {
            visited += 1;
            if visited > nodes || depth > 64 {
                return Err(E::CapacityExceeded);
            }
            match value {
                serde_json::Value::Object(values) => {
                    pending.extend(values.values().map(|v| (v, depth + 1)))
                }
                serde_json::Value::Array(values) => {
                    pending.extend(values.iter().map(|v| (v, depth + 1)))
                }
                _ => {}
            }
        }
        for (name, calc) in &self.calculations {
            if !valid_name(name)
                || !calc.request.is_object()
                || calc.curve_sets.len() > 64
                || calc.curve_sets.iter().any(|s| !valid_name(s))
            {
                return Err(E::InvalidInput("invalid core calculation definition"));
            }
        }
        let encoded =
            serde_json::to_vec(self).map_err(|_| E::InvalidInput("core definition JSON"))?;
        if encoded.len() > bytes {
            return Err(E::CapacityExceeded);
        }
        Ok(())
    }
}
/// Bounded identifier safe in native result keys and HTTP path segments.
pub fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// Exact immutable source selection. Updates select new versions, never overwrite a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreSnapshotPlan {
    /// Exact complete native definition revision.
    pub definition: ReferencePin,
    /// Economic source cutoff.
    pub as_of: UtcTimestamp,
    /// Publication/receipt cutoff.
    pub known_at: UtcTimestamp,
    /// Named complete native curve sets, each with exact stored revisions.
    pub curve_sets: BTreeMap<String, Vec<ReferencePin>>,
    /// Selected index/date/source keys. All eligible revisions are retained.
    pub fixings: Vec<FixingKey>,
}
impl CoreSnapshotPlan {
    /// Check bounds/duplicates before performing any adapter call.
    pub fn validate(&self) -> Result<(), TraitError> {
        if self.curve_sets.len() > 64
            || self.fixings.len() > MAX_SNAPSHOT_FIXINGS
            || self
                .curve_sets
                .iter()
                .any(|(n, p)| !valid_name(n) || p.is_empty() || p.len() > 64)
            || self.curve_sets.values().map(Vec::len).sum::<usize>() > 256
        {
            return Err(TraitError::CapacityExceeded);
        }
        let keys: std::collections::BTreeSet<_> = self.fixings.iter().collect();
        let observations: std::collections::BTreeSet<_> = self
            .fixings
            .iter()
            .map(|k| (&k.index_id, k.observation_date))
            .collect();
        if keys.len() != self.fixings.len() || observations.len() != self.fixings.len() {
            return Err(TraitError::InvalidInput("duplicate core fixing key"));
        }
        for pins in self.curve_sets.values() {
            let ids: std::collections::BTreeSet<_> = pins.iter().map(|p| &p.id).collect();
            if ids.len() != pins.len() {
                return Err(TraitError::InvalidInput("duplicate core curve key"));
            }
        }
        Ok(())
    }
}
/// Fully resolved, immutable inputs; replay never rereads mutable adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreSnapshotContent {
    /// Exactly 1.
    pub schema_version: u32,
    /// Exact original selection/cutoffs.
    pub plan: CoreSnapshotPlan,
    /// Complete original reference revision.
    pub definition: MasterRecord,
    /// Whole original native recipes and source metadata, in selected order.
    pub curves: BTreeMap<String, Vec<SnapshotCurveRecipe>>,
    /// Every eligible published revision, including original status/provenance.
    pub fixings: Vec<PublishedFixing>,
    /// Exact resolved issuer directory when explicitly selected; absent on prior snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer_scope: Option<MasterRecord>,
    /// Engine-checked carry from an exact successful saved closing window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<CoreCheckpoint>,
}
/// Explicit reserved `reference_inputs.pnl_checkpoint` selection. No latest lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreCheckpointSource {
    /// Exact immutable predecessor snapshot.
    pub snapshot_id: SnapshotId,
    /// Prior and current native book P&L calculation name.
    pub calculation: String,
    /// Build that produced the successful source result.
    pub build_id: ModelId,
}
/// Captured checkpoint; ports retain opaque financial totals, the native owner checks them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreCheckpoint {
    /// Exact audited predecessor, never a mutable latest book.
    pub source: CoreCheckpointSource,
    /// Deterministic original result identity.
    pub result_id: String,
    /// Original exact stored output digest.
    pub output_digest: String,
    /// Complete expanded opening inputs, preserving lifecycle replay and claims.
    pub opening_digest: String,
    /// Actual completed windows, excluding the current window.
    pub completed_windows: u64,
    /// Actual completed causal phases, excluding the current window.
    pub completed_causes: u64,
    /// Consumed interval fill IDs; they cannot be replayed in a later window.
    pub retired_fill_ids: Vec<String>,
    /// Checked native per-currency/per-basis interval totals.
    pub local_totals: serde_json::Value,
    /// Checked reporting totals, with PV and futures quote meaning kept separate.
    pub reporting_totals: serde_json::Value,
}
/// Content-addressed core snapshot, separate from the existing fixed-bullet schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreSnapshot {
    /// SHA256 content identity.
    pub id: SnapshotId,
    /// Complete captured inputs.
    pub content: CoreSnapshotContent,
}
impl CoreSnapshot {
    /// Validate and bind canonical content, without a clock or backend.
    pub fn new(content: CoreSnapshotContent) -> Result<Self, TraitError> {
        content.plan.validate()?;
        if content.schema_version != 1 || content.fixings.len() > 100_000 {
            return Err(TraitError::InvalidInput(
                "invalid core snapshot schema/size",
            ));
        }
        let bytes = serde_json::to_vec(&content)
            .map_err(|_| TraitError::InvalidInput("core snapshot JSON"))?;
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        let id = SnapshotId::new(format!(
            "sha256:{}",
            content_digest(b"convex.core-snapshot.v1\0", &bytes)
        ))
        .map_err(|_| TraitError::InvalidInput("core snapshot identity"))?;
        Ok(Self { id, content })
    }
    /// Reject corrupt identity/content rather than returning a different snapshot.
    pub fn verify(&self) -> Result<(), TraitError> {
        if Self::new(self.content.clone())?.id != self.id {
            return Err(TraitError::Conflict("core snapshot content mismatch"));
        }
        Ok(())
    }
}
/// Stored exact native envelope, keyed by complete snapshot/calculation/build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreResult {
    /// Deterministic complete-context result key.
    pub id: String,
    /// Exact consumed input set.
    pub snapshot_id: SnapshotId,
    /// Named calculation within that definition.
    pub calculation: String,
    /// Composition-root build identity, never supplied by an HTTP caller.
    pub build_id: ModelId,
    /// Exact native JSON bytes represented as a UTF8 string, including failure envelopes.
    pub output: String,
    /// Digest of the exact native output, verified on every read.
    pub output_digest: String,
    /// Larger bounded storage selected by the captured definition. Old rows omit it.
    #[serde(default, skip_serializing_if = "CoreResult::ordinary_capacity")]
    pub book_capacity: bool,
}
impl CoreResult {
    fn ordinary_capacity(value: &bool) -> bool {
        !*value
    }
    /// Complete-context storage key, stable before calculation and on retry.
    pub fn key(snapshot: &SnapshotId, calculation: &str, build: &ModelId) -> String {
        content_digest(
            b"convex.core-result-key.v1\0",
            &serde_json::json!([snapshot, calculation, build])
                .to_string()
                .into_bytes(),
        )
    }
    /// Bind exact output bytes; no financial reserialization or rounding.
    pub fn new(
        snapshot_id: SnapshotId,
        calculation: String,
        build_id: ModelId,
        output: String,
    ) -> Result<Self, TraitError> {
        Self::with_capacity(snapshot_id, calculation, build_id, output, false)
    }
    /// Bind the existing native 16 MiB output ceiling to an explicit book profile.
    /// The application checks this selection against the immutable source snapshot.
    pub fn new_book(
        snapshot_id: SnapshotId,
        calculation: String,
        build_id: ModelId,
        output: String,
    ) -> Result<Self, TraitError> {
        Self::with_capacity(snapshot_id, calculation, build_id, output, true)
    }
    fn with_capacity(
        snapshot_id: SnapshotId,
        calculation: String,
        build_id: ModelId,
        output: String,
        book_capacity: bool,
    ) -> Result<Self, TraitError> {
        let result = Self {
            id: Self::key(&snapshot_id, &calculation, &build_id),
            snapshot_id,
            calculation,
            build_id,
            output_digest: content_digest(b"convex.core-result-output.v1\0", output.as_bytes()),
            output,
            book_capacity,
        };
        result.verify()?;
        Ok(result)
    }
    /// Bounded, checked persistence. Native financial validation remains in the library.
    pub fn verify(&self) -> Result<(), TraitError> {
        let maximum = if self.book_capacity { 16 } else { 4 } * 1024 * 1024;
        if !valid_name(&self.calculation) || self.output.len() > maximum {
            return Err(TraitError::CapacityExceeded);
        }
        if self.id != Self::key(&self.snapshot_id, &self.calculation, &self.build_id)
            || self.output_digest
                != content_digest(b"convex.core-result-output.v1\0", self.output.as_bytes())
        {
            return Err(TraitError::Conflict("core result content mismatch"));
        }
        let value: serde_json::Value = serde_json::from_str(&self.output)
            .map_err(|_| TraitError::InvalidInput("invalid core result JSON"))?;
        let valid = match value.get("ok").and_then(|v| v.as_str()) {
            Some("true") => {
                value.get("result").is_some_and(|v| v.is_object()) && value.get("error").is_none()
            }
            Some("false") => {
                value.get("error").is_some_and(|v| v.is_object()) && value.get("result").is_none()
            }
            _ => false,
        };
        if !valid {
            return Err(TraitError::InvalidInput(
                "core result must retain a native envelope",
            ));
        }
        Ok(())
    }
}
