//! Full original product graphs at the application snapshot boundary. Pure
//! library owners perform all economics, calibration and risk calculations.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use convex::library::{
    decode_calibration_inputs, CalibrationOperation, CalibrationReplay, LibraryError,
    LibraryErrorCode, LibraryInputs,
};
use convex_core::contracts::{SnapshotId, UtcTimestamp};
use convex_curves::contracts::{CurveBuildOutput, CurveKey};
use convex_ports::{
    calibration::SnapshotCalibration,
    snapshot::{MarketSnapshot, SnapshotCurveRecipe, MAX_SNAPSHOT_CURVES, MAX_SNAPSHOT_PILLARS},
    TraitError,
};
use serde_json::Value;

/// Lossless native failure at capture/replay; no partially accepted curve set.
#[derive(Debug, thiserror::Error)]
pub enum CalibrationReplayError {
    /// Original structural/identity/storage-boundary error.
    #[error(transparent)]
    Record(#[from] TraitError),
    /// Original complete library admission/calibration/product/risk diagnostic.
    #[error(transparent)]
    Native(#[from] LibraryError),
    /// Complete direct recipe dependency error.
    #[error(transparent)]
    Curve(#[from] crate::curve_replay::CurveReplayError),
    /// Native diagnostic serialization failure.
    #[error(transparent)]
    Payload(#[from] serde_json::Error),
}
impl From<CalibrationReplayError> for TraitError {
    fn from(error: CalibrationReplayError) -> Self {
        match error {
            CalibrationReplayError::Record(e) => e,
            CalibrationReplayError::Curve(e) => e.into(),
            CalibrationReplayError::Native(e) if e.code == LibraryErrorCode::ResourceLimit => {
                Self::CapacityExceeded
            }
            CalibrationReplayError::Native(e) if e.code == LibraryErrorCode::Unsupported => {
                Self::Unsupported("complete native calibration/risk model unsupported")
            }
            _ => Self::InvalidInput(
                "complete calibration replay rejected original inputs or captured output",
            ),
        }
    }
}
fn time_scope(
    inputs: &LibraryInputs,
    as_of: UtcTimestamp,
    known_at: UtcTimestamp,
) -> Result<(), CalibrationReplayError> {
    let json = serde_json::to_value(inputs)?;
    scope_value(&json, as_of, known_at)?;
    if inputs
        .calibration
        .as_ref()
        .and_then(|c| c.rate_context.as_ref())
        .is_some_and(|c| c.as_of != as_of)
    {
        return Err(TraitError::InvalidInput(
            "original rate calibration instant differs from snapshot",
        )
        .into());
    }
    Ok(())
}
fn scope_value(
    json: &Value,
    as_of: UtcTimestamp,
    known_at: UtcTimestamp,
) -> Result<(), CalibrationReplayError> {
    let date = as_of.datetime().date_naive().to_string();
    let mut pending = vec![json];
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(fields) => {
                for name in ["known_at", "published_at", "as_of"] {
                    if let Some(value) = fields.get(name) {
                        let time = value
                            .as_str()
                            .and_then(|s| UtcTimestamp::parse(s).ok())
                            .ok_or(TraitError::InvalidInput("invalid native calibration time"))?;
                        let cutoff = if name == "as_of" { as_of } else { known_at };
                        if time > cutoff {
                            return Err(TraitError::InvalidInput(
                                "calibration input is later than captured cutoff",
                            )
                            .into());
                        }
                    }
                }
                if fields
                    .get("valuation_date")
                    .is_some_and(|v| v.as_str() != Some(&date))
                {
                    return Err(TraitError::InvalidInput(
                        "calibration valuation date differs from snapshot",
                    )
                    .into());
                }
                pending.extend(fields.values());
            }
            Value::Array(items) => pending.extend(items),
            _ => {}
        }
    }
    Ok(())
}
fn check_pins(
    record: &SnapshotCalibration,
    built: &CalibrationReplay,
) -> Result<(), CalibrationReplayError> {
    if record.curves.len() != built.curves().len() {
        return Err(TraitError::InvalidInput(
            "calibration capture omits original output/dependency pins",
        )
        .into());
    }
    for pin in &record.curves {
        let key = CurveKey {
            id: pin.curve_id.as_str().into(),
            version: pin.definition_version.clone(),
        };
        built.curves().get(&key).map_err(|_| {
            TraitError::InvalidInput("captured calibration exact curve pin differs")
        })?;
    }
    Ok(())
}
fn build(
    record: &SnapshotCalibration,
    as_of: UtcTimestamp,
    known_at: UtcTimestamp,
) -> Result<CalibrationReplay, CalibrationReplayError> {
    record.validate()?;
    if record.source_time > as_of || record.recorded_at > known_at {
        return Err(TraitError::InvalidInput("calibration record outside captured times").into());
    }
    let inputs = decode_calibration_inputs(&record.inputs)?;
    time_scope(&inputs, as_of, known_at)?;
    let built = CalibrationReplay::build(&inputs)?;
    check_pins(record, &built)?;
    Ok(built)
}
/// Seal a new capture with actual native output. All metadata/pins are supplied;
/// an existing expected output cannot be overwritten through this constructor.
pub fn capture_calibration(
    mut record: SnapshotCalibration,
    as_of: UtcTimestamp,
    known_at: UtcTimestamp,
) -> Result<SnapshotCalibration, CalibrationReplayError> {
    if record.expected != serde_json::json!({}) {
        return Err(
            TraitError::Conflict("capture cannot replace existing expected calibration").into(),
        );
    }
    let built = build(&record, as_of, known_at)?;
    record.expected = serde_json::from_slice(&serde_json::to_vec(built.diagnostics())?)?;
    record.validate()?;
    Ok(record)
}
/// Rebuild original complete inputs; retained fitted nodes never supply a graph.
/// Exact canonical diagnostics detect incomplete residual or source coverage.
pub fn replay_calibration(
    record: &SnapshotCalibration,
    as_of: UtcTimestamp,
    known_at: UtcTimestamp,
) -> Result<CalibrationReplay, CalibrationReplayError> {
    let built = build(record, as_of, known_at)?;
    let actual: Value = serde_json::from_slice(&serde_json::to_vec(built.diagnostics())?)?;
    if actual != record.expected {
        return Err(
            TraitError::Conflict("original calibration output/diagnostic replay mismatch").into(),
        );
    }
    Ok(built)
}
/// Reconstruct independent complete graphs, refusing identity collisions and
/// cross-boundary dependency substitution. Each graph owns its full inputs.
pub fn rebuild_captured_curves(
    records: &[SnapshotCurveRecipe],
    calibration: Option<&SnapshotCalibration>,
    as_of: UtcTimestamp,
    known_at: UtcTimestamp,
) -> Result<CurveBuildOutput, CalibrationReplayError> {
    if records.len() + calibration.map_or(0, |c| c.curves.len()) > MAX_SNAPSHOT_CURVES
        || records
            .iter()
            .map(SnapshotCurveRecipe::input_count)
            .sum::<usize>()
            + calibration.map_or(0, SnapshotCalibration::input_count)
            > MAX_SNAPSHOT_PILLARS
    {
        return Err(TraitError::CapacityExceeded.into());
    }
    let mut direct = crate::curve_replay::rebuild_curve_recipes(records)?;
    if let Some(record) = calibration {
        let built = replay_calibration(record, as_of, known_at)?;
        for key in built.curves().keys() {
            if direct.curves.keys().any(|k| k.id == key.id) {
                return Err(
                    TraitError::Conflict("duplicate captured native curve identity").into(),
                );
            }
        }
        // Keys are sorted by identity, not dependency. In particular a fitted
        // tail can sort before its original supplied base. Install the complete
        // already-validated native graph in bounded dependency order; neither
        // independent captured records nor a sampled cache may supply its pins.
        let mut pending: std::collections::BTreeSet<_> = built.curves().keys().cloned().collect();
        for _ in 0..built.curves().len() {
            let key = pending
                .iter()
                .find(|key| {
                    built.curves().get(key).is_ok_and(|curve| {
                        curve
                            .recipe()
                            .dependencies
                            .iter()
                            .all(|dependency| direct.curves.get(dependency).is_ok())
                    })
                })
                .cloned()
                .ok_or(TraitError::InvalidInput(
                    "captured native graph dependency order unresolved",
                ))?;
            pending.remove(&key);
            direct.curves = direct
                .curves
                .with_curve(
                    built
                        .curves()
                        .get(&key)
                        .map_err(|_| TraitError::InvalidInput("missing rebuilt curve"))?
                        .as_ref()
                        .clone(),
                )
                .map_err(crate::curve_replay::CurveReplayError::Native)?;
        }
        let order: Vec<CurveKey> =
            serde_json::from_value(built.diagnostics()["build_order"].clone())?;
        direct.build_order.extend(order);
    }
    Ok(direct)
}
fn check_snapshot_id(action: &CalibrationOperation, id: &SnapshotId) -> Result<(), TraitError> {
    let claimed = match action {
        CalibrationOperation::ProductRisk { snapshot_id, .. }
        | CalibrationOperation::ProductScenario { snapshot_id, .. } => Some(snapshot_id),
        CalibrationOperation::ReconcileBuckets { input } => Some(&input.snapshot_id),
        _ => None,
    };
    if claimed.is_some_and(|claimed| claimed != id) {
        return Err(TraitError::Conflict(
            "risk operation claims a different snapshot",
        ));
    }
    Ok(())
}
/// Execute one original full calibration/product/risk operation on a verified
/// snapshot. Original curve/quote revisions, model/response and numerical guards
/// remain authoritative. This function grants no execution authorization.
pub fn replay_calibration_operation(
    snapshot: &MarketSnapshot,
    action: &CalibrationOperation,
) -> Result<Value, CalibrationReplayError> {
    snapshot.verify()?;
    let content = snapshot.content();
    // Verify every retained native graph, including independent records that
    // this action does not consume. No invalid snapshot receives a risk result.
    for curve in &content.curves {
        crate::BuiltCurve::from_snapshot(curve)?;
    }
    crate::curve_replay::rebuild_curve_recipes(&content.curve_recipes)?;
    let record = content
        .calibration
        .as_ref()
        .ok_or(TraitError::InvalidInput(
            "snapshot has no complete product calibration",
        ))?;
    check_snapshot_id(action, snapshot.id())?;
    scope_value(
        &serde_json::to_value(action)?,
        content.as_of,
        content.known_at,
    )?;
    let built = replay_calibration(record, content.as_of, content.known_at)?;
    let result = built.evaluate(action)?;
    Ok(
        serde_json::json!({"snapshot_id":snapshot.id(),"graph_id":record.graph_id,"graph_version":record.version,"model_id":record.model_id,"quality":snapshot.quality(),"curves":record.curves,"input_versions":record.input_versions,"result":result}),
    )
}
