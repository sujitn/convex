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
    snapshot::{
        InputSet, MarketSnapshot, QualityWindow, SnapshotIssueKind, SnapshotQuality,
        SnapshotReferences,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ops::Bound,
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
    // Each instrument requests only the plan measures its own model publishes, and a
    // G-spread only when it is priced against a benchmark set it matures within.
    let model = product_model(bond);
    let measures: Vec<_> = plan
        .measures
        .iter()
        .filter(|measure| {
            product_measures(model).contains(&measure.as_str())
                || (measure.as_str() == crate::snapshot::G_SPREAD_MEASURE
                    && crate::level_pricing::is_g_spread_published(snapshot, reference))
        })
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
/// Build the snapshot's curves once; the activation's valuations share them.
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
/// every other pin keeps its committed result. The callable bound reads only the
/// reference section, so it is counted again only when the section is not `checked`, the
/// one last counted within it (P17c-iii).
pub(super) fn rebind(
    graph: &CalculationGraph,
    plan: &RuntimePlan,
    snapshot: Arc<MarketSnapshot>,
    selected: &[usize],
    checked: &mut Option<SnapshotReferences>,
) -> Result<Arc<BuiltSnapshot>, RuntimeError> {
    check_callables_once(&snapshot.content().references, checked)?;
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
/// What a Live tick keeps for the next one (D55): its snapshot, what each pin reads, and
/// each pin's derived quality with the window it holds in (D81), by plan index.
#[cfg_attr(test, derive(Clone))]
pub(super) struct Baseline {
    snapshot: Arc<MarketSnapshot>,
    reads: Reads,
    qualities: Vec<SnapshotQuality>,
    windows: Vec<QualityWindow>,
    /// The windows, by plan index, ordered by the millisecond each ends at and by the
    /// time each was judged at, so a tick finds the pins whose window it left without
    /// walking every window (P17c-iii).
    ends: BTreeSet<(i64, usize)>,
    starts: BTreeSet<(UtcTimestamp, usize)>,
}
/// What each pin's valuation reads while the frame and the pinned reference section hold
/// (G31): its curves, by plan index. They follow from the pin's model, the plan's
/// measures and the configured curves. With them, the plan indices sorted by instrument
/// id, so an instrument whose quotes changed finds its pin, and each pin's dependents:
/// the pins priced against it as a benchmark, whose inputs hold its quotes. The section
/// pins each rule, so the dependents are found again only when it changes, at each
/// activation.
#[cfg_attr(test, derive(Clone))]
struct Reads {
    curves: Vec<Vec<String>>,
    by_instrument: Vec<usize>,
    dependents: Vec<Vec<usize>>,
}
impl Reads {
    /// Each pin's context is built once here, so a pin that `context` refuses is refused
    /// here too.
    fn new(plan: &RuntimePlan, snapshot: &MarketSnapshot) -> Result<Self, RuntimeError> {
        let configured = &snapshot.content().configuration.analytics_curves;
        let curves = plan
            .assembly
            .instruments
            .iter()
            .map(|pin| {
                let ctx = context(plan, snapshot, pin)?;
                let spec = ctx.spec();
                let curves = crate::snapshot::read_curve_ids(
                    spec.model_id.as_str(),
                    &spec.measures,
                    snapshot
                        .pricing_rule(pin.instrument_id.as_str())
                        .map(|rule| &rule.method),
                    configured,
                );
                Ok(curves.into_iter().map(str::to_owned).collect())
            })
            .collect::<Result<_, RuntimeError>>()?;
        let mut reads = Self::with_curves(&plan.assembly.instruments, curves);
        reads.index_dependents(&plan.assembly.instruments, snapshot);
        Ok(reads)
    }
    /// `curves[i]` names the curves pin `i` reads.
    fn with_curves(pins: &[InstrumentPin], curves: Vec<Vec<String>>) -> Self {
        let mut by_instrument: Vec<usize> = (0..pins.len()).collect();
        by_instrument.sort_unstable_by(|&a, &b| {
            pins[a]
                .instrument_id
                .as_str()
                .cmp(pins[b].instrument_id.as_str())
        });
        Self {
            dependents: vec![Vec::new(); pins.len()],
            curves,
            by_instrument,
        }
    }
    /// Index each benchmark pin's dependents from the rules `snapshot`'s references pin, in
    /// plan order; a set's securities are distinct, so each dependent once. A benchmark
    /// the plan does not pin has none: the dependent's valuation refuses it by tenor.
    fn index_dependents(&mut self, pins: &[InstrumentPin], snapshot: &MarketSnapshot) {
        for (dependent, pin) in pins.iter().enumerate() {
            let Some(rule) = snapshot.pricing_rule(pin.instrument_id.as_str()) else {
                continue;
            };
            for security in snapshot.benchmark_securities(rule).unwrap_or_default() {
                if let Some(benchmark) = self.pin(pins, security.instrument_id.as_str()) {
                    self.dependents[benchmark].push(dependent);
                }
            }
        }
    }
    /// The plan index of `instrument`'s pin. A plan pins each instrument once and selects
    /// one quote for each pin (`RuntimePlan::validate`), and an activation keeps the plan's
    /// instruments and quote keys, so every snapshot quote names a pin.
    fn pin(&self, pins: &[InstrumentPin], instrument: &str) -> Option<usize> {
        self.by_instrument
            .binary_search_by(|&index| pins[index].instrument_id.as_str().cmp(instrument))
            .ok()
            .map(|found| self.by_instrument[found])
    }
    fn curves(&self, index: usize) -> Vec<&str> {
        self.curves[index].iter().map(String::as_str).collect()
    }
}
impl Baseline {
    /// A frame's first tick: every pin is judged, and every pin is valued.
    fn first(
        snapshot: &Arc<MarketSnapshot>,
        pins: &[InstrumentPin],
        reads: Reads,
    ) -> Result<Self, RuntimeError> {
        if reads.curves.len() != pins.len() {
            return Err(RuntimeError::RecoveryConflict);
        }
        let (qualities, windows): (_, Vec<QualityWindow>) = pins
            .iter()
            .enumerate()
            .map(|(index, pin)| {
                snapshot.judge(&snapshot.inputs(pin.instrument_id.as_str(), &reads.curves(index)))
            })
            .unzip();
        let mut baseline = Self {
            snapshot: snapshot.clone(),
            reads,
            qualities,
            windows: Vec::new(),
            ends: BTreeSet::new(),
            starts: BTreeSet::new(),
        };
        for window in windows {
            baseline.index(baseline.windows.len(), &window);
            baseline.windows.push(window);
        }
        Ok(baseline)
    }
    /// Order pin `index`'s `window` by its end and by when it was judged.
    fn index(&mut self, index: usize, window: &QualityWindow) {
        if let Some(until) = window.until_ms() {
            self.ends.insert((until, index));
        }
        self.starts.insert((window.judged_at(), index));
    }
    /// Give pin `index` its new `window`, ordered in place of its last one.
    fn set_window(&mut self, index: usize, window: QualityWindow) -> Result<(), RuntimeError> {
        let last = self
            .windows
            .get_mut(index)
            .ok_or(RuntimeError::RecoveryConflict)?;
        if *last == window {
            return Ok(());
        }
        let last = std::mem::replace(last, window);
        if let Some(until) = last.until_ms() {
            self.ends.remove(&(until, index));
        }
        self.starts.remove(&(last.judged_at(), index));
        self.index(index, &window);
        Ok(())
    }
    /// The pins whose window `snapshot`'s time has left, as `MarketSnapshot::within` judges
    /// it: their window ends by that millisecond, or was judged after it. Each is named
    /// once, by plan index.
    fn left(&self, snapshot: &MarketSnapshot) -> BTreeSet<usize> {
        let now = snapshot.quality_time();
        let ended = self
            .ends
            .range(..=(now.datetime().timestamp_millis(), usize::MAX))
            .map(|&(_, index)| index);
        let later = self
            .starts
            .range((Bound::Excluded((now, usize::MAX)), Bound::Unbounded))
            .map(|&(_, index)| index);
        ended
            .chain(later)
            .filter(|&index| {
                self.windows
                    .get(index)
                    .is_none_or(|window| !snapshot.within(window))
            })
            .collect()
    }
    /// Move to `snapshot`, which shares this baseline's frame (D55), and select the pins
    /// it values. With `listed`, the instruments whose quotes changed by
    /// [`MarketSnapshot::changed_quotes`], only their pins and the pins whose quality
    /// window the snapshot's time has left are judged (D81). Without it, every pin is.
    /// A judged pin is valued when its inputs or its quality differ, and keeps the new
    /// quality and window. `check` runs on every judged pin first. Returns the pins to
    /// value, by plan index in plan order.
    fn advance(
        &mut self,
        snapshot: &Arc<MarketSnapshot>,
        pins: &[InstrumentPin],
        listed: Option<Vec<&str>>,
        check: impl Fn(usize) -> Result<(), RuntimeError>,
    ) -> Result<Vec<usize>, RuntimeError> {
        let count = pins.len();
        if self.reads.curves.len() != count
            || self.qualities.len() != count
            || self.windows.len() != count
        {
            return Err(RuntimeError::RecoveryConflict);
        }
        // In plan order: every pin, or the listed ones and those whose window was left.
        let judged: Vec<usize> = match listed {
            None => (0..count).collect(),
            Some(instruments) => {
                let mut judge = self.left(snapshot);
                // Every test build checks the ordered windows against every window.
                debug_assert!(self
                    .windows
                    .iter()
                    .enumerate()
                    .filter(|(_, window)| !snapshot.within(window))
                    .map(|(index, _)| index)
                    .eq(judge.iter().copied()));
                for instrument in instruments {
                    let pin = self
                        .reads
                        .pin(pins, instrument)
                        .ok_or(RuntimeError::RecoveryConflict)?;
                    judge.insert(pin);
                    // A benchmark's quotes are its dependents' inputs too.
                    judge.extend(self.reads.dependents[pin].iter().copied());
                }
                judge.into_iter().collect()
            }
        };
        let mut valued = Vec::new();
        for index in judged {
            let pin = pins.get(index).ok_or(RuntimeError::RecoveryConflict)?;
            check(index)?;
            let instrument = pin.instrument_id.as_str();
            let curves = self.reads.curves(index);
            let (quality, window) = snapshot.judge(&snapshot.inputs(instrument, &curves));
            if self.qualities[index] != quality
                || !snapshot.same_inputs(&self.snapshot, instrument, &curves)
            {
                valued.push(index);
            }
            self.qualities[index] = quality;
            self.set_window(index, window)?;
        }
        self.snapshot = snapshot.clone();
        Ok(valued)
    }
}
/// The pins a Live tick values, by plan index, and the baseline for the next tick
/// (D55). Without a baseline, or when the frame changed, every pin is valued. With one,
/// a pin is valued when its own inputs or its derived quality differ; quality depends on
/// time, so a quote that ages out counts. Only the pins whose quotes changed, or whose
/// quality window the snapshot's time has left, are judged, unless something else a
/// valuation reads changed; then every pin is (D81). `results[i]` must be pin `i`'s
/// record. The baseline's reads are reused while the frame and the reference section
/// hold (G31). An error ends the runtime, so the baseline is taken, not borrowed.
pub(super) fn changed(
    plan: &RuntimePlan,
    snapshot: &Arc<MarketSnapshot>,
    baseline: Option<Baseline>,
    results: &[Arc<PublishedValuation>],
) -> Result<(Vec<usize>, Baseline), RuntimeError> {
    let pins = &plan.assembly.instruments;
    let Some(mut baseline) = baseline.filter(|baseline| snapshot.same_frame(&baseline.snapshot))
    else {
        let baseline = Baseline::first(snapshot, pins, Reads::new(plan, snapshot)?)?;
        return Ok(((0..pins.len()).collect(), baseline));
    };
    if results.len() != pins.len() {
        return Err(RuntimeError::RecoveryConflict);
    }
    let references = &baseline.snapshot.content().references;
    if !references.ptr_eq(&snapshot.content().references) {
        baseline.reads = Reads::new(plan, snapshot)?;
    }
    let listed = snapshot.changed_quotes(&baseline.snapshot);
    let selected = baseline.advance(snapshot, pins, listed, |index| {
        let pin = &pins[index];
        if subject_key(&results[index].result.provenance().subject)
            == Some((pin.instrument_id.as_str(), pin.version.get()))
        {
            Ok(())
        } else {
            Err(RuntimeError::RecoveryConflict)
        }
    })?;
    Ok((selected, baseline))
}
/// The plan pins only identities; product types are known once references resolve.
pub(super) fn check_callables(snapshot: &MarketSnapshot) -> Result<(), RuntimeError> {
    callable_bound(&snapshot.content().references)
}
fn callable_bound(references: &SnapshotReferences) -> Result<(), RuntimeError> {
    if crate::optional_valuation::references_within_callable_bound(references) {
        Ok(())
    } else {
        Err(RuntimeError::Configuration(
            "a runtime plan values at most 8 callables",
        ))
    }
}
/// [`check_callables`] for a Live tick: the bound reads only the reference section, so it
/// is counted only when `references` is not `checked`, the section it last held for
/// (P17c-iii). A refused section is not kept.
fn check_callables_once(
    references: &SnapshotReferences,
    checked: &mut Option<SnapshotReferences>,
) -> Result<(), RuntimeError> {
    if checked
        .as_ref()
        .is_some_and(|section| section.ptr_eq(references))
    {
        return Ok(());
    }
    *checked = None;
    callable_bound(references)?;
    *checked = Some(references.clone());
    Ok(())
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
pub(crate) fn accepted(quality: DataQuality, demo: bool) -> bool {
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
) -> Result<Vec<Arc<PublishedValuation>>, RuntimeError> {
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
                    ctx.cache_key(),
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
                // The graph and the published record share one result (G32).
                let result = Arc::new(result);
                let committed =
                    graph.commit_result(&ticket, Ok(NodeValue::Valuation(result.clone())));
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
        produced.push(Arc::new(PublishedValuation {
            runtime_id: plan.runtime_id.clone(),
            revision,
            generation: checkpoint.generation,
            context_key: result.cache_key().into(),
            live_valid_until,
            result: result.envelope().clone(),
            last_good,
            retained_quality,
        }));
    }
    Ok(produced)
}
/// Prior results by valued instrument revision, built at most once per tick.
fn by_subject(results: &[Arc<PublishedValuation>]) -> HashMap<(&str, u64), &PublishedValuation> {
    let mut index = HashMap::with_capacity(results.len());
    for record in results {
        if let Some(key) = subject_key(&record.result.provenance().subject) {
            index.entry(key).or_insert(record.as_ref());
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
    records: &[Arc<PublishedValuation>],
) -> Result<(), RuntimeError> {
    // Indexed once, so a restart is linear in the pins (G34). Each pin owns exactly one
    // record: one record per subject, and as many records as pins.
    let index = by_subject(records);
    if records.len() != plan.assembly.instruments.len() || index.len() != records.len() {
        return Err(RuntimeError::RecoveryConflict);
    }
    for pin in &plan.assembly.instruments {
        let ctx = context(plan, snapshot, pin)?;
        let record = index
            .get(&(pin.instrument_id.as_str(), pin.version.get()))
            .ok_or(RuntimeError::RecoveryConflict)?;
        if record.generation != snapshot.content().generation
            || record.context_key != ctx.cache_key()
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
                .filter(|o| inputs.quotes().any(|q| q.requirement.key == o.scope))
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

// The ports suites' snapshot fixtures, which load the reference terms the crate's own
// tests load too.
#[cfg(test)]
#[allow(clippy::duplicate_mod)]
#[path = "../../../convex-ports/tests/support/snapshot_fixtures.rs"]
mod fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use convex_ports::snapshot::{
        SnapshotContent, SnapshotFixing, SnapshotQuote, SnapshotReferences,
    };

    /// The fixture's day, `ms` milliseconds and `nanos` nanoseconds after its 15:00 as-of.
    fn after_as_of(ms: i64, nanos: i64) -> UtcTimestamp {
        let total = (15 * 3_600_000 + ms) * 1_000_000 + nanos;
        let (seconds, nanos) = (total / 1_000_000_000, total % 1_000_000_000);
        UtcTimestamp::parse(&format!(
            "2026-10-02T{:02}:{:02}:{:02}.{nanos:09}Z",
            seconds / 3_600,
            seconds / 60 % 60,
            seconds % 60
        ))
        .unwrap()
    }

    /// One quote observed again at `ms` after the as-of.
    fn observe(quote: &mut SnapshotQuote, ms: i64) {
        let at = after_as_of(ms, 0).datetime().timestamp_millis();
        let image = quote.image.as_mut().unwrap();
        image.watermark.sequence += 1;
        image.watermark.source_time_ms = at;
        image.watermark.receive_time_ms = at;
        quote.delivery_watermark = Some(image.watermark.clone());
        for field in image.fields.values_mut() {
            field.source_time_ms = at;
            field.receive_time_ms = at;
            field.sequence = image.watermark.sequence;
        }
    }

    /// P17c-iii: a Live tick counts the callables again whenever its reference section is
    /// not the one last found within the bound, even an equal copy of it, and a refused
    /// section is never kept as counted.
    #[test]
    fn the_callable_bound_is_counted_for_every_section_but_the_one_last_found_within_it() {
        let bullet = fixture::content().references[0].clone();
        let callable = |index: usize| {
            let mut reference = bullet.clone();
            reference.record.instrument_id =
                convex_core::ids::InstrumentId::new(format!("CALLABLE-{index}"));
            let bond = reference.record.bond.as_mut().unwrap();
            bond.instrument_id = reference.record.instrument_id.clone();
            bond.bond_type = convex_ports::reference_data::BondType::FixedCallable;
            reference
        };
        let bound = crate::optional_valuation::MAX_RUNTIME_CALLABLES;
        let within = SnapshotReferences::new((0..bound).map(callable).collect());
        let over = SnapshotReferences::new((0..=bound).map(callable).collect());
        let mut checked = None;

        check_callables_once(&within, &mut checked).unwrap();
        assert!(checked.as_ref().is_some_and(|s| s.ptr_eq(&within)));
        check_callables_once(&within, &mut checked).unwrap();
        // Another section is counted, and refused; it is not kept.
        assert!(check_callables_once(&over, &mut checked).is_err());
        assert!(checked.is_none());
        assert!(check_callables_once(&over, &mut checked).is_err());
        // An equal copy of the section within the bound is counted again.
        check_callables_once(&within, &mut checked).unwrap();
        let copy = SnapshotReferences::new(over.to_vec());
        assert!(check_callables_once(&copy, &mut checked).is_err());
        // A section within the bound after one over it is counted, and kept.
        check_callables_once(&within, &mut checked).unwrap();
        assert!(checked.as_ref().is_some_and(|s| s.ptr_eq(&within)));
    }

    /// A rule quoting its bond as a spread over the 2-year of the `UST-OTR` set.
    fn spread_rule() -> convex_ports::level_pricing::ResolvedPricingRule {
        convex_ports::level_pricing::ResolvedPricingRule {
            version: fixture::version(1),
            recorded_at: fixture::time("2025-01-01T00:00:00Z"),
            method: convex_ports::level_pricing::PricingMethod::BenchmarkSpread {
                tenor: "2Y".into(),
            },
            benchmark_set_id: Some(ReferenceDataId::new("UST-OTR").unwrap()),
        }
    }

    /// The `UST-OTR` set, holding `benchmark` at version 1 as its 2-year.
    fn set_of(benchmark: &str) -> convex_ports::level_pricing::ResolvedBenchmarkSet {
        serde_json::from_value(serde_json::json!({
            "benchmark_set_id": "UST-OTR",
            "version": 1,
            "recorded_at": "2025-01-01T00:00:00Z",
            "securities": [{"tenor": "2Y", "instrument_id": benchmark, "version": 1}],
        }))
        .unwrap()
    }

    /// A Live tick that judges only the pins whose quotes changed, their dependents and
    /// the pins whose quality window it left values exactly the pins that judging every
    /// pin values, with the same qualities. Covered: no change, a quote, a quote's age
    /// reaching its limit and passing it, a curve's age passing its limit, a benchmark's
    /// quote and its age, a fixing, a copied and a changed reference section, and a time
    /// before the last tick's. The plan's order is not the snapshot's.
    #[test]
    fn a_tick_that_judges_only_the_changed_pins_values_what_judging_every_pin_does() {
        let mut content = fixture::content();
        fixture::add_bond(&mut content, "BOND-002", 2_000);
        fixture::add_bond(&mut content, "BOND-003", 0);
        // BOND-001 is quoted as a spread over BOND-002.
        content.references.make_mut()[0].pricing_rule = Some(spread_rule());
        content.benchmark_sets = vec![set_of("BOND-002")].into();
        // BOND-001 reads the curve, 5 s old; each age limit is 10 s.
        let curve_time = fixture::time("2026-10-02T14:59:55Z");
        content.curves[0].source_time = curve_time;
        content.curves[0].recorded_at = curve_time;
        let mut pins: Vec<InstrumentPin> = content
            .references
            .iter()
            .map(|reference| InstrumentPin {
                instrument_id: InstrumentId::new(reference.record.instrument_id.as_str()).unwrap(),
                version: reference.record.version,
            })
            .collect();
        pins.reverse();
        let curves = pins
            .iter()
            .map(|pin| match pin.instrument_id.as_str() {
                "BOND-001" => vec!["USD-FIXTURE".to_owned()],
                _ => vec![],
            })
            .collect();
        let mut reads = Reads::with_curves(&pins, curves);
        let first = Arc::new(MarketSnapshot::new(content).unwrap());
        reads.index_dependents(&pins, &first);
        let mut baseline = Baseline::first(&first, &pins, reads).unwrap();
        let mut generation = 1;
        let mut tick = |ms: i64, nanos: i64, change: &dyn Fn(&mut SnapshotContent)| {
            let mut content = baseline.snapshot.content().clone();
            generation += 1;
            content.generation = fixture::version(generation);
            content.as_of = after_as_of(ms, nanos);
            content.known_at = content.as_of;
            change(&mut content);
            let snapshot = Arc::new(MarketSnapshot::new(content).unwrap());
            assert!(snapshot.same_frame(&baseline.snapshot));
            let listed = snapshot.changed_quotes(&baseline.snapshot);
            let gated = listed.is_some();
            let mut every = baseline.clone();
            let all = every.advance(&snapshot, &pins, None, |_| Ok(())).unwrap();
            let some = baseline
                .advance(&snapshot, &pins, listed, |_| Ok(()))
                .unwrap();
            assert_eq!(some, all, "at {ms} ms");
            assert_eq!(baseline.qualities, every.qualities, "at {ms} ms");
            let valued: Vec<_> = some
                .iter()
                .map(|&index| pins[index].instrument_id.as_str().to_owned())
                .collect();
            (gated, valued)
        };
        let names = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        // BOND-003 is index 2 in the snapshot; the plan lists it first.
        assert_eq!(tick(1_000, 0, &|_| {}), (true, names(&[])));
        assert_eq!(
            tick(1_500, 0, &|content| observe(
                content.quotes[2].make_mut(),
                1_500
            )),
            (true, names(&["BOND-003"]))
        );
        // The curve read by BOND-001 reaches its age limit at 5 s.
        assert_eq!(tick(5_000, 0, &|_| {}), (true, names(&[])));
        assert_eq!(tick(5_000, 1, &|_| {}), (true, names(&["BOND-001"])));
        // BOND-002's quote, observed 2 s before the as-of, reaches its limit at 8 s, and
        // makes BOND-001, quoted over it, stale too.
        assert_eq!(tick(8_000, 0, &|_| {}), (true, names(&[])));
        assert_eq!(
            tick(8_000, 1, &|_| {}),
            (true, names(&["BOND-002", "BOND-001"]))
        );
        // Anything but a quote: every pin is judged.
        assert_eq!(
            tick(8_100, 0, &|content| content.fixings.push(SnapshotFixing {
                key: fixture::fixing_key(),
                fixing: None,
            })),
            (false, names(&[]))
        );
        assert_eq!(
            tick(8_200, 0, &|content| {
                content.references = SnapshotReferences::new(content.references.to_vec())
            }),
            (false, names(&[]))
        );
        assert_eq!(
            tick(8_300, 0, &|content| {
                content.references.make_mut()[2].record.version = fixture::version(2)
            }),
            (false, names(&["BOND-003"]))
        );
        // A time before the last tick's leaves every window, so every pin is judged.
        assert_eq!(tick(8_250, 0, &|_| {}), (true, names(&[])));
        assert_eq!(
            tick(9_000, 0, &|content| observe(
                content.quotes[1].make_mut(),
                9_000
            )),
            (true, names(&["BOND-002", "BOND-001"]))
        );
    }
}
