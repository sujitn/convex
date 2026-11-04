//! Bounded complete product-calibration capture. Native financial validation
//! belongs to the pure library, called by the engine before numerical use.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::{
    snapshot::{SnapshotCurveRecipe, MAX_SNAPSHOT_CURVES, MAX_SNAPSHOT_PILLARS},
    TraitError,
};
use convex_core::contracts::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Original native key and separately supplied immutable capture revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationCurvePin {
    /// Exact original native identity.
    pub curve_id: CurveId,
    /// Exact opaque native definition revision, never inferred from record revision.
    pub definition_version: String,
    /// Caller-supplied positive immutable capture revision for this curve.
    pub version: Version,
}
/// One entire original product graph; independent supplied dependencies are
/// inside `inputs`, with one exact pin per resulting curve, including views.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotCalibration {
    /// Complete capture payload format, currently 1.
    pub schema_version: u32,
    /// Caller-supplied graph identity.
    pub graph_id: SourceId,
    /// Immutable graph capture revision.
    pub version: Version,
    /// Original source time, no refresh on replay.
    pub source_time: UtcTimestamp,
    /// Original recording/publication time.
    pub recorded_at: UtcTimestamp,
    /// Provider-neutral provenance.
    pub source: SourceId,
    /// Original graph/model policy revision.
    pub model_id: ModelId,
    /// All original caller-supplied numeric definition revisions.
    pub input_versions: Vec<InputVersion>,
    /// Supplied graph quality; native fit success is not a certificate.
    pub quality: DataQuality,
    /// Conservative age budget for every retained curve/dependency.
    pub max_age_ms: i64,
    /// Explicit native/capture pins for every supplied, fitted and view curve.
    pub curves: Vec<CalibrationCurvePin>,
    /// Full original strict `LibraryInputs`, including its complete calibration.
    pub inputs: Value,
    /// Original actual native fitted curves, full helper valuations and all
    /// residual/solver/rank/work/publication diagnostics. Never decoded as a cache.
    pub expected: Value,
}
impl SnapshotCalibration {
    /// Structural integrity only; rebuild/compare through the original native owner.
    pub fn validate(&self) -> Result<(), TraitError> {
        if self.schema_version != 1
            || self.source_time > self.recorded_at
            || !(1..=86_400_000).contains(&self.max_age_ms)
            || self.curves.is_empty()
            || self.curves.len() > MAX_SNAPSHOT_CURVES
            || self.input_versions.len() > 256
            || !self.inputs.is_object()
            || !self.inputs.get("calibration").is_some_and(Value::is_object)
            || !self.expected.is_object()
        {
            return Err(TraitError::InvalidInput(
                "invalid complete calibration capture",
            ));
        }
        validate_payload(&self.inputs, 32_768)?;
        validate_payload(&self.expected, 262_144)?;
        let mut ids = BTreeSet::new();
        for pin in &self.curves {
            crate::quote_event::validate_text(&pin.definition_version, 128)?;
            if !ids.insert(&pin.curve_id) {
                return Err(TraitError::InvalidInput("duplicate calibration curve pin"));
            }
        }
        let mut inputs = BTreeSet::new();
        for pin in &self.input_versions {
            if !inputs.insert((&pin.kind, &pin.id)) {
                return Err(TraitError::InvalidInput(
                    "duplicate calibration input revision",
                ));
            }
        }
        if self.input_count() > MAX_SNAPSHOT_PILLARS {
            return Err(TraitError::CapacityExceeded);
        }
        Ok(())
    }
    /// Original direct source inputs plus full product helpers, sharing the
    /// existing snapshot budget; no allowance per family or output reset.
    pub fn input_count(&self) -> usize {
        let direct = self
            .inputs
            .get("curves")
            .and_then(Value::as_array)
            .map_or(0, |curves| {
                curves
                    .iter()
                    .map(SnapshotCurveRecipe::recipe_input_count)
                    .sum()
            });
        direct
            + ["rates", "credit", "cash"]
                .iter()
                .filter_map(|kind| {
                    self.inputs
                        .pointer(&format!("/calibration/{kind}"))
                        .and_then(Value::as_array)
                })
                .flatten()
                .filter_map(|r| {
                    r.get("recipe")
                        .unwrap_or(r)
                        .get("helpers")
                        .and_then(Value::as_array)
                })
                .map(Vec::len)
                .sum::<usize>()
    }
    /// Canonical metadata order; original native input arrays remain untouched.
    pub fn normalize(&mut self) {
        self.curves.sort_by(|a, b| a.curve_id.cmp(&b.curve_id));
        self.input_versions
            .sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
    }
}
fn validate_payload(root: &Value, limit: usize) -> Result<(), TraitError> {
    let mut pending = vec![(root, 0)];
    let mut visited = 0;
    while let Some((value, depth)) = pending.pop() {
        visited += 1;
        if visited > limit || depth > 64 {
            return Err(TraitError::CapacityExceeded);
        }
        match value {
            Value::Array(items) => {
                if items.len() > 4096 {
                    return Err(TraitError::CapacityExceeded);
                }
                pending.extend(items.iter().map(|v| (v, depth + 1)));
            }
            Value::Object(items) => {
                if items.len() > 4096 || items.keys().any(|k| k.len() > 8192) {
                    return Err(TraitError::CapacityExceeded);
                }
                pending.extend(items.values().map(|v| (v, depth + 1)));
            }
            Value::String(text) if text.len() > 8192 => return Err(TraitError::CapacityExceeded),
            _ => {}
        }
    }
    Ok(())
}
