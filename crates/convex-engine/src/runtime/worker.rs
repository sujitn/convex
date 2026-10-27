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
    snapshot::{MarketSnapshot, SnapshotIssueKind},
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
        snapshot.quality().quality,
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
fn quality_error(snapshot: &MarketSnapshot) -> ErrorCode {
    use SnapshotIssueKind as K;
    match snapshot.quality().issues.first().map(|i| i.reason) {
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
    if !accepted(snapshot.quality().quality, demo) {
        failed(
            context,
            snapshot,
            request,
            calculated,
            quality_error(snapshot),
        )
    } else {
        let valued = match input {
            Input::Built(built) => {
                valuator.value_built(built, context, request.clone(), calculated)
            }
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
                match error {
                    convex_ports::TraitError::Unsupported(_) => ErrorCode::UnsupportedConvention,
                    convex_ports::TraitError::Conflict(_) => ErrorCode::VersionConflict,
                    _ => ErrorCode::InvalidInput,
                },
            ),
        }
    }
}
pub(super) async fn calculate(
    graph: &Arc<CalculationGraph>,
    valuator: &Arc<SnapshotValuator>,
    plan: &RuntimePlan,
    built: Arc<BuiltSnapshot>,
    checkpoint: &RuntimeCheckpoint,
    concurrency: usize,
    demo: bool,
) -> Result<Vec<PublishedValuation>, RuntimeError> {
    let snapshot = built.snapshot();
    checkpoint
        .result_revision
        .checked_add(plan.assembly.instruments.len() as u64)
        .ok_or(RuntimeError::CapacityExceeded)?;
    let live_valid_until = if plan.mode == super::RuntimeMode::Live {
        expiry(snapshot)?
    } else {
        None
    };
    let pins = &plan.assembly.instruments;
    // At most `concurrency` blocking jobs run at once. Each joined job is committed and
    // the next prepared at once, so no job waits for a slower one (G26). Tickets stay
    // here by plan index, so a panicked job's attempt is still cancelled.
    let mut tickets = Vec::with_capacity(pins.len());
    let mut valued = vec![None; pins.len()];
    let mut jobs = tokio::task::JoinSet::new();
    let mut failure = None;
    loop {
        while failure.is_none() && jobs.len() < concurrency && tickets.len() < pins.len() {
            let index = tickets.len();
            let pin = &pins[index];
            let preparation = (|| {
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
            Ok(result) => {
                let committed = graph
                    .commit_result(&ticket, Ok(NodeValue::Valuation(Arc::new(result.clone()))));
                if matches!(committed, Ok(CommitOutcome::Committed { .. })) {
                    valued[index] = Some(result);
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
    let mut produced = Vec::with_capacity(pins.len());
    let mut revision = checkpoint.result_revision;
    let mut prior = None;
    for result in valued {
        let result = result.ok_or(RuntimeError::Worker)?;
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

pub(super) fn expiry(snapshot: &MarketSnapshot) -> Result<Option<UtcTimestamp>, RuntimeError> {
    let mut deadlines = Vec::new();
    if let Some(capture) = &snapshot.content().configuration_capture {
        deadlines.extend(capture.applied_overrides.iter().map(|o| o.expires_at));
    }
    for curve in &snapshot.content().curves {
        let deadline = curve
            .source_time
            .datetime()
            .checked_add_signed(chrono::Duration::milliseconds(curve.max_age_ms))
            .ok_or(RuntimeError::Configuration("curve deadline overflow"))?;
        deadlines.push(
            UtcTimestamp::from_datetime(deadline)
                .map_err(|_| RuntimeError::Configuration("curve deadline overflow"))?,
        );
    }
    for curve in &snapshot.content().curve_recipes {
        let deadline = curve
            .source_time
            .datetime()
            .checked_add_signed(chrono::Duration::milliseconds(curve.max_age_ms))
            .ok_or(RuntimeError::Configuration("curve deadline overflow"))?;
        deadlines.push(
            UtcTimestamp::from_datetime(deadline)
                .map_err(|_| RuntimeError::Configuration("curve deadline overflow"))?,
        );
    }
    if let Some(calibration) = &snapshot.content().calibration {
        let deadline = calibration
            .source_time
            .datetime()
            .checked_add_signed(chrono::Duration::milliseconds(calibration.max_age_ms))
            .ok_or(RuntimeError::Configuration("calibration deadline overflow"))?;
        deadlines.push(
            UtcTimestamp::from_datetime(deadline)
                .map_err(|_| RuntimeError::Configuration("calibration deadline overflow"))?,
        );
    }
    for quote in &snapshot.content().quotes {
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
