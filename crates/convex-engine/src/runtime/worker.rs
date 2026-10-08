//! Frozen-input CPU work and checked graph commit. No repository/provider reads in workers.
use super::{RuntimeError, RuntimePlan};
use crate::{
    context::*,
    snapshot::{SnapshotValuationResult, SnapshotValuator, BOND_SNAPSHOT_MODEL},
    CalculationGraph, CommitOutcome, NodeId, NodeSpec, NodeValue,
};
use convex_core::contracts::*;
use convex_ports::{
    runtime::{PublishedValuation, RuntimeCheckpoint},
    snapshot::{MarketSnapshot, SnapshotIssueKind},
};
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn context(
    plan: &RuntimePlan,
    snapshot: &MarketSnapshot,
    instrument: &InstrumentId,
) -> Result<ValuationContext, RuntimeError> {
    let reference = snapshot
        .content()
        .references
        .iter()
        .find(|r| r.record.instrument_id.as_str() == instrument.as_str())
        .ok_or(RuntimeError::RecoveryConflict)?;
    if !plan
        .assembly
        .instruments
        .iter()
        .any(|pin| pin.instrument_id == *instrument && pin.version == reference.record.version)
    {
        return Err(RuntimeError::RecoveryConflict);
    }
    let bond = reference
        .record
        .bond
        .as_ref()
        .ok_or(RuntimeError::RecoveryConflict)?;
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
        model_id: ModelId::new(BOND_SNAPSHOT_MODEL)
            .map_err(|_| RuntimeError::Configuration("invalid compiled model identity"))?,
        build_id: plan.build_id.clone(),
        reporting_currency: bond.currency,
        fixing_known_at: snapshot.content().known_at,
        scenario: ValuationScenario::Base,
        numerical_settings: plan.numerical_settings.clone(),
        measures: plan.measures.clone(),
    })?;
    context.validate_snapshot(snapshot)?;
    Ok(context)
}
pub(super) fn activate(
    graph: &CalculationGraph,
    plan: &RuntimePlan,
    snapshot: Arc<MarketSnapshot>,
) -> Result<(), RuntimeError> {
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
    graph
        .activate_snapshot(nodes, snapshot)
        .map_err(|_| RuntimeError::Worker)?;
    Ok(())
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
    let reference = snapshot
        .content()
        .references
        .iter()
        .find(|r| {
            r.record.instrument_id.as_str()
                == match &spec.subject {
                    ValuationSubject::InstrumentVersion { instrument_id, .. } => {
                        instrument_id.as_str()
                    }
                    _ => "",
                }
        })
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
pub(super) fn value(
    valuator: &SnapshotValuator,
    snapshot: &MarketSnapshot,
    context: &ValuationContext,
    request: RequestId,
    calculated: UtcTimestamp,
) -> Result<SnapshotValuationResult, RuntimeError> {
    context.validate_snapshot(snapshot)?;
    if calculated < context.spec().valuation_time
        || calculated < context.spec().known_at
        || context.spec().model_id.as_str() != BOND_SNAPSHOT_MODEL
        || context.spec().numerical_settings.policy_id.as_str()
            != crate::snapshot::BOND_NUMERICAL_POLICY
        || context.spec().scenario != ValuationScenario::Base
    {
        return Err(RuntimeError::Configuration(
            "unsupported context/time/model policy",
        ));
    }
    if snapshot.quality().quality != DataQuality::Valid {
        failed(
            context,
            snapshot,
            request,
            calculated,
            quality_error(snapshot),
        )
    } else {
        match valuator.value(snapshot, context, request.clone(), calculated) {
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
    snapshot: Arc<MarketSnapshot>,
    checkpoint: &RuntimeCheckpoint,
    concurrency: usize,
) -> Result<Vec<PublishedValuation>, RuntimeError> {
    checkpoint
        .result_revision
        .checked_add(plan.assembly.instruments.len() as u64)
        .ok_or(RuntimeError::CapacityExceeded)?;
    let live_valid_until = if plan.mode == super::RuntimeMode::Live {
        expiry(&snapshot)?
    } else {
        None
    };
    let mut produced = Vec::with_capacity(plan.assembly.instruments.len());
    let mut revision = checkpoint.result_revision;
    // A maximum of concurrency blocking jobs is admitted at once, and every join is awaited.
    for chunk in plan.assembly.instruments.chunks(concurrency) {
        // Prepare the complete bounded chunk before spawning. An admission failure cancels every
        // ticket already captured, so no early-return can leave an unjoined blocking worker.
        let mut prepared = Vec::with_capacity(chunk.len());
        for pin in chunk {
            let preparation = (|| {
                let ctx = context(plan, &snapshot, &pin.instrument_id)?;
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
            match preparation {
                Ok(input) => prepared.push(input),
                Err(error) => {
                    for (ticket, _, _, _) in &prepared {
                        graph.cancel_attempt(ticket);
                    }
                    return Err(error);
                }
            }
        }
        let mut jobs = Vec::with_capacity(prepared.len());
        for (ticket, ctx, request, calculated) in prepared {
            let valuator = valuator.clone();
            let snapshot = snapshot.clone();
            jobs.push(tokio::task::spawn_blocking(move || {
                let result = value(&valuator, &snapshot, &ctx, request, calculated);
                (ticket, result)
            }));
        }
        // Never early-return with unjoined jobs, including on panic/deadline/commit failure.
        let mut failure = None;
        for job in jobs {
            match job.await {
                Ok((ticket, result)) => match result {
                    Ok(result) => {
                        let committed = graph.commit_result(
                            &ticket,
                            Ok(NodeValue::Valuation(Arc::new(result.clone()))),
                        );
                        if !matches!(committed, Ok(CommitOutcome::Committed { .. })) {
                            graph.cancel_attempt(&ticket);
                            failure = Some(RuntimeError::Worker);
                            continue;
                        }
                        revision = match revision.checked_add(1) {
                            Some(v) => v,
                            None => {
                                failure = Some(RuntimeError::CapacityExceeded);
                                continue;
                            }
                        };
                        let last_good = if result.envelope().status() == ResultStatus::Complete
                            && result.envelope().quality() == DataQuality::Valid
                        {
                            Some(result.envelope().clone())
                        } else {
                            checkpoint
                                .results
                                .iter()
                                .find(|r| {
                                    r.result.provenance().subject
                                        == result.envelope().provenance().subject
                                })
                                .and_then(|r| r.last_good.clone())
                        };
                        let retained_quality = last_good.as_ref().map(|good| {
                            if good == result.envelope() {
                                DataQuality::Valid
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
                    Err(error) => {
                        graph.cancel_attempt(&ticket);
                        failure = Some(error);
                    }
                },
                Err(_) => {
                    failure = Some(RuntimeError::Worker);
                }
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
    }
    Ok(produced)
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
        let ctx = context(plan, snapshot, &pin.instrument_id)?;
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
    let ctx = context(plan, snapshot, instrument_id)?;
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
