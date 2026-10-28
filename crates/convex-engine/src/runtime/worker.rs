//! Frozen-input CPU work and checked graph commit. No repository/provider reads in workers.
use super::{RuntimeError, RuntimePlan, NO_APPLICABLE_MEASURE};
use crate::{
    context::*,
    curve_replay::BuiltSnapshot,
    optional_valuation::{product_measures, product_model},
    snapshot::{SnapshotValuationResult, SnapshotValuator},
    CalculationGraph, CommitOutcome, NodeId, NodeSpec, NodeValue,
};
use convex_core::contracts::*;
use convex_ports::{
    configuration::InstrumentPin,
    runtime::{PublishedValuation, RuntimeCheckpoint},
    snapshot::{InputSet, MarketSnapshot, SnapshotIssueKind, SnapshotQuality},
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

/// The context of one of the plan's own pins; the snapshot must hold that revision.
pub(super) fn context(
    plan: &RuntimePlan,
    snapshot: &MarketSnapshot,
    pin: &InstrumentPin,
) -> Result<ValuationContext, RuntimeError> {
    let instrument = &pin.instrument_id;
    let reference = snapshot
        .reference(instrument.as_str())
        .filter(|reference| reference.record.version == pin.version)
        .ok_or(RuntimeError::RecoveryConflict)?;
    let bond = reference
        .record
        .bond
        .as_ref()
        .ok_or(RuntimeError::RecoveryConflict)?;
    // Each instrument requests only the plan measures its own model publishes.
    let model = product_model(bond);
    let measures: Vec<_> = plan
        .measures
        .iter()
        .filter(|measure| product_measures(model).contains(&measure.as_str()))
        .cloned()
        .collect();
    if measures.is_empty() {
        return Err(RuntimeError::Configuration(NO_APPLICABLE_MEASURE));
    }
    let context = ValuationContext::new(ValuationContextSpec {
        snapshot_id: snapshot.id().clone(),
        generation: snapshot.content().generation,
        subject: ValuationSubject::InstrumentVersion {
            instrument_id: instrument.clone(),
            version: reference.record.version,
        },
        valuation_time: snapshot.content().as_of,
        known_at: snapshot.content().known_at,
        settlement_date: plan.settlement_date,
        configuration: ConfigurationPin {
            id: SourceId::new(&snapshot.content().configuration.config_id)
                .map_err(|_| RuntimeError::RecoveryConflict)?,
            version: Version::new(snapshot.content().configuration.version)
                .map_err(|_| RuntimeError::RecoveryConflict)?,
        },
        model_id: ModelId::new(model)
            .map_err(|_| RuntimeError::Configuration("invalid compiled model identity"))?,
        build_id: plan.build_id.clone(),
        reporting_currency: bond.currency,
        fixing_known_at: snapshot.content().known_at,
        scenario: ValuationScenario::Base,
        numerical_settings: plan.numerical_settings.clone(),
        measures,
    })?;
    context.validate_snapshot(snapshot)?;
    Ok(context)
}
/// The plan's pin for one instrument, for callers that start from an id rather than the plan.
pub(super) fn plan_pin<'a>(
    plan: &'a RuntimePlan,
    instrument: &InstrumentId,
) -> Result<&'a InstrumentPin, RuntimeError> {
    plan.assembly
        .instruments
        .iter()
        .find(|pin| pin.instrument_id == *instrument)
        .ok_or(RuntimeError::RecoveryConflict)
}
/// Verify the snapshot and build its curves once; the activation's valuations share them.
pub(super) fn activate(
    graph: &CalculationGraph,
    plan: &RuntimePlan,
    snapshot: Arc<MarketSnapshot>,
) -> Result<Arc<BuiltSnapshot>, RuntimeError> {
    check_callables(&snapshot)?;
    // Each context contains the complete validated manifest. No mutable compatibility roots are admitted.
    let nodes = plan
        .assembly
        .instruments
        .iter()
        .map(|pin| {
            NodeSpec::calculation(
                NodeId::BondPrice {
                    instrument_id: pin.instrument_id.clone().into(),
                },
                vec![],
            )
        })
        .collect();
    let built = Arc::new(BuiltSnapshot::build(snapshot).map_err(|_| RuntimeError::Worker)?);
    graph
        .activate_built(nodes, &built)
        .map_err(|_| RuntimeError::Worker)?;
    Ok(built)
}
/// Bind a Live tick's snapshot, built once, and reset only the pins it values (D55);
/// every other pin keeps its committed result.
pub(super) fn rebind(
    graph: &CalculationGraph,
    plan: &RuntimePlan,
    snapshot: Arc<MarketSnapshot>,
    selected: &[usize],
) -> Result<Arc<BuiltSnapshot>, RuntimeError> {
    check_callables(&snapshot)?;
    let revalue: Vec<_> = selected
        .iter()
        .map(|&index| NodeId::BondPrice {
            instrument_id: plan.assembly.instruments[index]
                .instrument_id
                .clone()
                .into(),
        })
        .collect();
    let built = Arc::new(BuiltSnapshot::build(snapshot).map_err(|_| RuntimeError::Worker)?);
    graph
        .rebind_built(&built, &revalue)
        .map_err(|_| RuntimeError::Worker)?;
    Ok(built)
}
/// What a Live tick keeps for the next one (D55): its snapshot and each pin's derived
/// quality, by plan index.
pub(super) struct Baseline {
    snapshot: Arc<MarketSnapshot>,
    qualities: Vec<SnapshotQuality>,
}
/// The pins a Live tick values, by plan index, and the baseline for the next tick
/// (D55). Without a baseline every pin is valued. With one, a pin is valued when the
/// frame, its own inputs or its derived quality differ; quality depends on time, so a
/// quote that ages out counts. `results[i]` must be pin `i`'s record.
pub(super) fn changed(
    plan: &RuntimePlan,
    snapshot: &Arc<MarketSnapshot>,
    baseline: Option<&Baseline>,
    results: &[PublishedValuation],
) -> Result<(Vec<usize>, Baseline), RuntimeError> {
    let pins = &plan.assembly.instruments;
    let frame = baseline.filter(|baseline| snapshot.same_frame(&baseline.snapshot));
    if frame.is_some_and(|b| results.len() != pins.len() || b.qualities.len() != pins.len()) {
        return Err(RuntimeError::RecoveryConflict);
    }
    let mut selected = Vec::new();
    let mut qualities = Vec::with_capacity(pins.len());
    for (index, pin) in pins.iter().enumerate() {
        let ctx = context(plan, snapshot, pin)?;
        let quality = crate::snapshot::valuation_quality(snapshot, ctx.spec())?;
        let kept = match frame {
            Some(baseline) => {
                if subject_key(&results[index].result.provenance().subject)
                    != Some((pin.instrument_id.as_str(), pin.version.get()))
                {
                    return Err(RuntimeError::RecoveryConflict);
                }
                baseline.qualities[index] == quality
                    && crate::snapshot::same_valuation_inputs(
                        snapshot,
                        &baseline.snapshot,
                        ctx.spec(),
                    )?
            }
            None => false,
        };
        if !kept {
            selected.push(index);
        }
        qualities.push(quality);
    }
    let baseline = Baseline {
        snapshot: snapshot.clone(),
        qualities,
    };
    Ok((selected, baseline))
}
/// The plan pins only identities; product types are known once references resolve.
pub(super) fn check_callables(snapshot: &MarketSnapshot) -> Result<(), RuntimeError> {
    if crate::optional_valuation::within_callable_bound(snapshot) {
        Ok(())
    } else {
        Err(RuntimeError::Configuration(
            "a runtime plan values at most 8 callables",
        ))
    }
}
fn failed(
    context: &ValuationContext,
    snapshot: &MarketSnapshot,
    request: RequestId,
    calculated: UtcTimestamp,
    quality: DataQuality,
    code: ErrorCode,
) -> Result<SnapshotValuationResult, RuntimeError> {
    let spec = context.spec();
    let errors = spec
        .measures
        .iter()
        .map(|measure| {
            StructuredError::new(
                code,
                Some(measure.as_str().to_string()),
                "Required runtime inputs or valuation are unavailable; no value substituted",
                false,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| RuntimeError::Worker)?;
    let ValuationSubject::InstrumentVersion { instrument_id, .. } = &spec.subject else {
        return Err(RuntimeError::RecoveryConflict);
    };
    let reference = snapshot
        .reference(instrument_id.as_str())
        .ok_or(RuntimeError::RecoveryConflict)?;
    let input_versions = crate::snapshot::consumed_versions(snapshot, reference)?;
    let envelope = ResultEnvelope::new(
        request,
        ResultStatus::Failed,
        quality,
        BTreeMap::new(),
        errors,
        Provenance {
            subject: spec.subject.clone(),
            snapshot_id: spec.snapshot_id.clone(),
            configuration_version: spec.configuration.version,
            valuation_time: spec.valuation_time,
            known_at: spec.known_at,
            calculated_at: calculated,
            model_version: spec.model_id.clone(),
            build_id: spec.build_id.clone(),
            input_versions,
        },
    )
    .map_err(|_| RuntimeError::Worker)?;
    Ok(SnapshotValuationResult::new(context.clone(), envelope)?)
}
fn quality_error(quality: &SnapshotQuality) -> ErrorCode {
    use SnapshotIssueKind as K;
    match quality.issues.first().map(|i| i.reason) {
        Some(K::Stale) => ErrorCode::StaleInput,
        Some(K::Gap | K::Closed | K::AwaitingImage) => ErrorCode::DataGap,
        Some(K::Unauthorized) => ErrorCode::EntitlementDenied,
        Some(K::MissingCurve) => ErrorCode::CalibrationFailed,
        _ => ErrorCode::MissingMarketData,
    }
}
/// `Valid`, or `Demo` in an explicit demonstration composition.
pub(super) fn accepted(quality: DataQuality, demo: bool) -> bool {
    quality == DataQuality::Valid || (demo && quality == DataQuality::Demo)
}
/// What one valuation reads: the activation's snapshot, verified and built once, or a
/// supplied snapshot whose curves are built only on a cache miss.
#[derive(Clone, Copy)]
pub(super) enum Input<'a> {
    Built(&'a BuiltSnapshot),
    Supplied(&'a MarketSnapshot),
}
pub(super) fn value(
    valuator: &SnapshotValuator,
    input: Input<'_>,
    context: &ValuationContext,
    request: RequestId,
    calculated: UtcTimestamp,
    demo: bool,
) -> Result<SnapshotValuationResult, RuntimeError> {
    let snapshot = match input {
        Input::Built(built) => built.snapshot().as_ref(),
        Input::Supplied(snapshot) => snapshot,
    };
    context.validate_snapshot(snapshot)?;
    if calculated < context.spec().valuation_time
        || calculated < context.spec().known_at
        || !crate::optional_valuation::PRODUCT_MODELS.contains(&context.spec().model_id.as_str())
        || context.spec().numerical_settings.policy_id.as_str()
            != crate::snapshot::BOND_NUMERICAL_POLICY
        || context.spec().scenario != ValuationScenario::Base
    {
        return Err(RuntimeError::Configuration(
            "unsupported context/time/model policy",
        ));
    }
    // Each instrument is judged only on the inputs its own valuation reads (D58).
    let quality = crate::snapshot::valuation_quality(snapshot, context.spec())?;
    if !accepted(quality.quality, demo) {
        failed(
            context,
            snapshot,
            request,
            calculated,
            quality.quality,
            quality_error(&quality),
        )
    } else {
        let valued = match input {
            Input::Built(built) => valuator.value_built_judged(
                built,
                context,
                request.clone(),
                calculated,
                quality.quality,
            ),
            Input::Supplied(snapshot) => {
                valuator.value(snapshot, context, request.clone(), calculated)
            }
        };
        match valued {
            Ok(result) => Ok(result),
            Err(error) => failed(
                context,
                snapshot,
                request,
                calculated,
                quality.quality,
                match error {
                    convex_ports::TraitError::Unsupported(_) => ErrorCode::UnsupportedConvention,
                    convex_ports::TraitError::Conflict(_) => ErrorCode::VersionConflict,
                    _ => ErrorCode::InvalidInput,
                },
            ),
        }
    }
}
/// Value the `selected` pins (plan indices, ascending) and return their records in that
/// order. A Live tick selects only the pins whose inputs changed (D55).
#[allow(clippy::too_many_arguments)]
pub(super) async fn calculate(
    graph: &Arc<CalculationGraph>,
    valuator: &Arc<SnapshotValuator>,
    plan: &RuntimePlan,
    built: Arc<BuiltSnapshot>,
    checkpoint: &RuntimeCheckpoint,
    selected: &[usize],
    concurrency: usize,
    demo: bool,
) -> Result<Vec<PublishedValuation>, RuntimeError> {
    let snapshot = built.snapshot();
    checkpoint
        .result_revision
        .checked_add(selected.len() as u64)
        .ok_or(RuntimeError::CapacityExceeded)?;
    let live = plan.mode == super::RuntimeMode::Live;
    let pins = &plan.assembly.instruments;
    // At most `concurrency` blocking jobs run at once. Each joined job is committed and
    // the next prepared at once, so no job waits for a slower one (G26). Tickets stay
    // here by position in `selected`, so a panicked job's attempt is still cancelled.
    let mut tickets = Vec::with_capacity(selected.len());
    let mut valued = vec![None; selected.len()];
    let mut jobs = tokio::task::JoinSet::new();
    let mut failure = None;
    loop {
        while failure.is_none() && jobs.len() < concurrency && tickets.len() < selected.len() {
            let index = tickets.len();
            let preparation = (|| {
                let pin = pins.get(selected[index]).ok_or(RuntimeError::Worker)?;
                let ctx = context(plan, snapshot, pin)?;
                let request = RequestId::new(format!(
                    "valuation-{}-{}",
                    ctx.cache_key()?,
                    checkpoint.generation.get()
                ))
                .map_err(|_| RuntimeError::Worker)?;
                let calculated = UtcTimestamp::from_datetime(chrono::Utc::now())
                    .map_err(|_| RuntimeError::Worker)?
                    .max(snapshot.content().as_of)
                    .max(snapshot.content().known_at);
                let node = NodeId::BondPrice {
                    instrument_id: pin.instrument_id.clone().into(),
                };
                let ticket = graph
                    .begin_valuation(&node, ctx.clone())
                    .map_err(|_| RuntimeError::Worker)?;
                Ok::<_, RuntimeError>((ticket, ctx, request, calculated))
            })();
            let (ticket, ctx, request, calculated) = match preparation {
                Ok(prepared) => prepared,
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            };
            tickets.push(Some(ticket));
            let valuator = valuator.clone();
            let built = built.clone();
            jobs.spawn_blocking(move || {
                let started = std::time::Instant::now();
                let result = value(
                    &valuator,
                    Input::Built(&built),
                    &ctx,
                    request,
                    calculated,
                    demo,
                );
                crate::telemetry::observe(crate::telemetry::VALUATION_SECONDS, started);
                // Each Live record expires with its own instrument's inputs (D58).
                let result = result.and_then(|result| {
                    let until = if live {
                        let snapshot = built.snapshot();
                        expiry(
                            snapshot,
                            &crate::snapshot::valuation_inputs(snapshot, ctx.spec())?,
                        )?
                    } else {
                        None
                    };
                    Ok((result, until))
                });
                (index, result)
            });
        }
        // Never return with a job unjoined, including on panic or commit failure.
        let Some(joined) = jobs.join_next().await else {
            break;
        };
        let Ok((index, result)) = joined else {
            failure.get_or_insert(RuntimeError::Worker);
            continue;
        };
        let Some(ticket) = tickets.get_mut(index).and_then(Option::take) else {
            failure.get_or_insert(RuntimeError::Worker);
            continue;
        };
        match result {
            Ok((result, until)) => {
                let committed = graph
                    .commit_result(&ticket, Ok(NodeValue::Valuation(Arc::new(result.clone()))));
                if matches!(committed, Ok(CommitOutcome::Committed { .. })) {
                    valued[index] = Some((result, until));
                } else {
                    graph.cancel_attempt(&ticket);
                    failure.get_or_insert(RuntimeError::Worker);
                }
            }
            Err(error) => {
                graph.cancel_attempt(&ticket);
                failure.get_or_insert(error);
            }
        }
    }
    // Only a panicked job leaves its ticket here.
    for ticket in tickets.iter().flatten() {
        graph.cancel_attempt(ticket);
    }
    if let Some(error) = failure {
        return Err(error);
    }
    // Revisions follow plan order, never completion order.
    let mut produced = Vec::with_capacity(selected.len());
    let mut revision = checkpoint.result_revision;
    let mut prior = None;
    for valued in valued {
        let (result, live_valid_until) = valued.ok_or(RuntimeError::Worker)?;
        revision = revision
            .checked_add(1)
            .ok_or(RuntimeError::CapacityExceeded)?;
        let last_good = if result.envelope().status() == ResultStatus::Complete
            && accepted(result.envelope().quality(), demo)
        {
            Some(result.envelope().clone())
        } else {
            subject_key(&result.envelope().provenance().subject)
                .and_then(|key| {
                    prior
                        .get_or_insert_with(|| by_subject(&checkpoint.results))
                        .get(&key)
                        .copied()
                })
                .and_then(|r| r.last_good.clone())
        };
        let retained_quality = last_good.as_ref().map(|good| {
            if good == result.envelope() {
                good.quality()
            } else {
                DataQuality::Stale
            }
        });
        produced.push(PublishedValuation {
            runtime_id: plan.runtime_id.clone(),
            revision,
            generation: checkpoint.generation,
            context_key: result.cache_key().into(),
            live_valid_until,
            result: result.envelope().clone(),
            last_good,
            retained_quality,
        });
    }
    Ok(produced)
}
/// Prior results by valued instrument revision, built at most once per tick.
fn by_subject(results: &[PublishedValuation]) -> HashMap<(&str, u64), &PublishedValuation> {
    let mut index = HashMap::with_capacity(results.len());
    for record in results {
        if let Some(key) = subject_key(&record.result.provenance().subject) {
            index.entry(key).or_insert(record);
        }
    }
    index
}
fn subject_key(subject: &ValuationSubject) -> Option<(&str, u64)> {
    match subject {
        ValuationSubject::InstrumentVersion {
            instrument_id,
            version,
        } => Some((instrument_id.as_str(), version.get())),
        _ => None,
    }
}
pub(super) fn restore_results(
    graph: &CalculationGraph,
    plan: &RuntimePlan,
    snapshot: &MarketSnapshot,
    records: &[PublishedValuation],
) -> Result<(), RuntimeError> {
    if records.len() != plan.assembly.instruments.len() {
        return Err(RuntimeError::RecoveryConflict);
    }
    for pin in &plan.assembly.instruments {
        let ctx = context(plan, snapshot, pin)?;
        let matches: Vec<_> = records
            .iter()
            .filter(|r| r.result.provenance().subject == ctx.spec().subject)
            .collect();
        if matches.len() != 1 {
            return Err(RuntimeError::RecoveryConflict);
        }
        let record = matches[0];
        if record.generation != snapshot.content().generation
            || record.context_key != ctx.cache_key()?
        {
            return Err(RuntimeError::RecoveryConflict);
        }
        let result = SnapshotValuationResult::new(ctx.clone(), record.result.clone())?;
        let node = NodeId::BondPrice {
            instrument_id: pin.instrument_id.clone().into(),
        };
        let ticket = graph
            .begin_valuation(&node, ctx)
            .map_err(|_| RuntimeError::Worker)?;
        if !matches!(
            graph.commit_result(&ticket, Ok(NodeValue::Valuation(Arc::new(result)))),
            Ok(CommitOutcome::Committed { .. })
        ) {
            return Err(RuntimeError::RecoveryConflict);
        }
    }
    Ok(())
}

/// When one instrument's inputs stop being fresh: its quote fields, the overrides on its
/// quote keys, and the curves or calibration it reads.
pub(super) fn expiry(
    snapshot: &MarketSnapshot,
    inputs: &InputSet<'_>,
) -> Result<Option<UtcTimestamp>, RuntimeError> {
    let content = snapshot.content();
    let after = |time: UtcTimestamp, max_age_ms: i64| {
        let deadline = time
            .datetime()
            .checked_add_signed(chrono::Duration::milliseconds(max_age_ms))
            .ok_or(RuntimeError::Configuration("curve deadline overflow"))?;
        UtcTimestamp::from_datetime(deadline)
            .map_err(|_| RuntimeError::Configuration("curve deadline overflow"))
    };
    let mut deadlines = Vec::new();
    if let Some(capture) = &content.configuration_capture {
        deadlines.extend(
            capture
                .applied_overrides
                .iter()
                .filter(|o| inputs.quotes().iter().any(|q| q.requirement.key == o.scope))
                .map(|o| o.expires_at),
        );
    }
    for &id in inputs.curves() {
        if let Some(curve) = content.curves.iter().find(|c| &c.curve_id == id) {
            deadlines.push(after(curve.source_time, curve.max_age_ms)?);
        } else if let Some(curve) = content.curve_recipes.iter().find(|c| &c.curve_id == id) {
            deadlines.push(after(curve.source_time, curve.max_age_ms)?);
        } else if let Some(calibration) = content
            .calibration
            .as_ref()
            .filter(|c| c.curves.iter().any(|p| &p.curve_id == id))
        {
            deadlines.push(after(calibration.source_time, calibration.max_age_ms)?);
        }
    }
    for quote in inputs.quotes() {
        if let Some(image) = &quote.image {
            for field in &quote.requirement.fields {
                if let Some(value) = image.fields.get(field) {
                    let epoch = value
                        .source_time_ms
                        .min(value.receive_time_ms)
                        .checked_add(quote.requirement.max_age_ms)
                        .ok_or(RuntimeError::Configuration("quote deadline overflow"))?;
                    let date = chrono::DateTime::from_timestamp_millis(epoch)
                        .ok_or(RuntimeError::Configuration("quote deadline overflow"))?;
                    deadlines.push(
                        UtcTimestamp::from_datetime(date)
                            .map_err(|_| RuntimeError::Configuration("quote deadline overflow"))?,
                    );
                }
            }
        }
    }
    Ok(deadlines.into_iter().min())
}
pub(super) fn restore_good(
    graph: &CalculationGraph,
    plan: &RuntimePlan,
    snapshot: &MarketSnapshot,
    envelope: &ResultEnvelope,
) -> Result<(), RuntimeError> {
    let ValuationSubject::InstrumentVersion { instrument_id, .. } = &envelope.provenance().subject
    else {
        return Err(RuntimeError::RecoveryConflict);
    };
    let ctx = context(plan, snapshot, plan_pin(plan, instrument_id)?)?;
    let node = NodeId::BondPrice {
        instrument_id: instrument_id.clone().into(),
    };
    let result = SnapshotValuationResult::new(ctx.clone(), envelope.clone())?;
    if envelope.status() != ResultStatus::Complete || envelope.quality() != DataQuality::Valid {
        return Err(RuntimeError::RecoveryConflict);
    }
    let ticket = graph
        .begin_valuation(&node, ctx)
        .map_err(|_| RuntimeError::RecoveryConflict)?;
    if !matches!(
        graph.commit_result(&ticket, Ok(NodeValue::Valuation(Arc::new(result)))),
        Ok(CommitOutcome::Committed { .. })
    ) {
        return Err(RuntimeError::RecoveryConflict);
    }
    Ok(())
}
pub(super) fn lost_record(
    prior: &PublishedValuation,
    revision: u64,
) -> Result<PublishedValuation, RuntimeError> {
    let errors = prior
        .result
        .results()
        .keys()
        .map(|m| m.as_str().to_string())
        .chain(
            prior
                .result
                .errors()
                .iter()
                .filter_map(|e| e.field().map(str::to_string)),
        )
        .map(|field| {
            StructuredError::new(
                ErrorCode::DataGap,
                Some(field),
                "Delivery continuity lost; last good is stale and requires a new verified image",
                true,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| RuntimeError::Worker)?;
    let mut provenance = prior.result.provenance().clone();
    provenance.calculated_at = UtcTimestamp::from_datetime(chrono::Utc::now())
        .map_err(|_| RuntimeError::Worker)?
        .max(provenance.calculated_at);
    let result = ResultEnvelope::new(
        prior.result.request_id().clone(),
        ResultStatus::Failed,
        DataQuality::Stale,
        BTreeMap::new(),
        errors,
        provenance,
    )
    .map_err(|_| RuntimeError::Worker)?;
    Ok(PublishedValuation {
        revision,
        result,
        retained_quality: prior.last_good.as_ref().map(|_| DataQuality::Stale),
        ..prior.clone()
    })
}
