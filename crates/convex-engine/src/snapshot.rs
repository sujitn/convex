//! Snapshot assembly and valuation use cases. All numerical inputs/time are supplied.
#![forbid(unsafe_code)]

use convex_core::contracts::*;
use convex_ports::{
    configuration::ConfigurationPin,
    fixing::FixingRepository,
    level_pricing::{PricingMethod, MAX_SNAPSHOT_BENCHMARK_SETS, RULE_DOES_NOT_APPLY},
    quote_event::{QuoteEvent, QuoteField, QuoteKey},
    reference_master::{ReferenceLookup, ResolvedBondReference},
    reference_repository::ReferenceRepository,
    snapshot::*,
    storage::{ConfigStore, PricingSpec},
    TraitError,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
};

use crate::{
    cache::ValuationCache,
    context::*,
    market_ingestion::{IngestionOutcome, IngestionPolicy, MarketDataAssembler},
    pricing_router::{PricingInput, PricingRouter},
    BuiltCurve,
};

/// Compiled fixed-bullet numerical policy; existing solver tolerances/limits remain unchanged.
pub const BOND_NUMERICAL_POLICY: &str = "convex-bond-router-v1";
/// Quantitative model implemented by this application slice.
pub const BOND_SNAPSHOT_MODEL: &str = "convex-fixed-bullet-router-v1";
/// Measures this slice publishes, each read from the router's quote at the mid mark.
/// Key-rate measures follow the router's standard tenors and labels.
pub const RUNTIME_MEASURES: [&str; 26] = [
    "clean_price_mid",
    "dirty_price_mid",
    "accrued_interest",
    "ytm_mid",
    "modified_duration_mid",
    "convexity_mid",
    "dv01_mid",
    "z_spread_mid",
    "i_spread_mid",
    "asw_mid",
    "macaulay_duration_mid",
    "effective_duration_mid",
    "effective_convexity_mid",
    "spread_duration_mid",
    "pv01_mid",
    "cs01_mid",
    "krd_3m_mid",
    "krd_6m_mid",
    "krd_1y_mid",
    "krd_2y_mid",
    "krd_3y_mid",
    "krd_5y_mid",
    "krd_7y_mid",
    "krd_10y_mid",
    "krd_20y_mid",
    "krd_30y_mid",
];
/// The spread to the benchmark set's interpolated yield, published only for a fixed bullet
/// whose pricing rule names a set it matures within.
pub const G_SPREAD_MEASURE: &str = "g_spread_mid";
/// Whether a runtime publishes `measure` for some bond: a measure of its fixed bullet
/// slice, or the G-spread.
pub(crate) fn is_runtime_measure(measure: &str) -> bool {
    RUNTIME_MEASURES.contains(&measure) || measure == G_SPREAD_MEASURE
}
/// Measures read from curves: spreads and curve risk. The others read the mid quote's
/// own yield, so a plan that asks for none of these prices without curves.
fn curve_measure(measure: &str) -> bool {
    matches!(measure, "z_spread_mid" | "i_spread_mid" | "asw_mid") || curve_risk_measure(measure)
}
/// The configured curves one valuation reads: a callable's discount curve on HW1F trees,
/// a floating rate note's discount and reference curves, and another bond's discount and
/// swap curves when it asks for a spread or curve-risk measure, or its discount curve
/// when its rule's `method` prices a level over it. A callable on its workouts reads
/// none. No snapshot valuation reads the government curve: a G-spread reads the
/// benchmark set's quotes.
pub(crate) fn read_curve_ids<'c>(
    model: &str,
    measures: &[MeasureId],
    method: Option<&PricingMethod>,
    curves: &'c convex_ports::storage::AnalyticsCurves,
) -> Vec<&'c str> {
    let second = if model == crate::optional_valuation::OPTIONAL_SNAPSHOT_MODEL {
        None
    } else if model == crate::floating_valuation::FLOATING_SNAPSHOT_MODEL {
        curves.frn_reference_curve.as_ref()
    } else if measures.iter().any(|m| curve_measure(m.as_str())) {
        curves.swap_curve.as_ref()
    } else if method.is_some_and(PricingMethod::reads_discount_curve) {
        None
    } else {
        return Vec::new();
    };
    std::iter::once(&curves.discount_curve)
        .chain(second)
        .map(|id| id.as_str())
        .collect()
}
/// The snapshot inputs one valuation context reads.
pub(crate) fn valuation_inputs<'a>(
    snapshot: &'a MarketSnapshot,
    spec: &ValuationContextSpec,
) -> Result<convex_ports::snapshot::InputSet<'a>, TraitError> {
    let ValuationSubject::InstrumentVersion { instrument_id, .. } = &spec.subject else {
        return Err(TraitError::Unsupported("trade economics unavailable"));
    };
    let curves = read_curve_ids(
        spec.model_id.as_str(),
        &spec.measures,
        snapshot
            .pricing_rule(instrument_id.as_str())
            .map(|rule| &rule.method),
        &snapshot.content().configuration.analytics_curves,
    );
    Ok(snapshot.inputs(instrument_id.as_str(), &curves))
}
/// Quality of the inputs one valuation context reads, never the whole snapshot's.
pub(crate) fn valuation_quality(
    snapshot: &MarketSnapshot,
    spec: &ValuationContextSpec,
) -> Result<convex_ports::snapshot::SnapshotQuality, TraitError> {
    Ok(snapshot.quality_of(&valuation_inputs(snapshot, spec)?))
}
/// Why each measure of one valuation context fails whatever its market data: the bond's
/// terms changed since its rule was committed so that the rule no longer applies, or it is
/// a callable beyond the plan's bound on HW1F trees.
pub(crate) fn valuation_refusal(
    snapshot: &MarketSnapshot,
    spec: &ValuationContextSpec,
) -> Option<(ErrorCode, &'static str)> {
    let ValuationSubject::InstrumentVersion { instrument_id, .. } = &spec.subject else {
        return None;
    };
    let instrument_id = instrument_id.as_str();
    if snapshot.reference(instrument_id)?.is_rule_misapplied() {
        Some((ErrorCode::InvalidInput, RULE_DOES_NOT_APPLY))
    } else if spec.model_id.as_str() == crate::optional_valuation::OPTIONAL_SNAPSHOT_MODEL
        && crate::optional_valuation::callables_beyond_bound(&snapshot.content().references)
            .any(|beyond| beyond == instrument_id)
    {
        Some((
            ErrorCode::SolverLimit,
            crate::optional_valuation::BEYOND_CALLABLE_BOUND,
        ))
    } else {
        None
    }
}
/// Measures read from the router's frozen-Z curve risk, which fails as one `curve_risk` field.
fn curve_risk_measure(measure: &str) -> bool {
    matches!(
        measure,
        "effective_duration_mid"
            | "effective_convexity_mid"
            | "spread_duration_mid"
            | "pv01_mid"
            | "cs01_mid"
    ) || (measure.starts_with("krd_") && RUNTIME_MEASURES.contains(&measure))
}

pub use convex_ports::configuration::{InstrumentPin, SnapshotAssembly};

/// Assemble then publish one complete immutable manifest. This creates no scheduler or background task.
#[derive(Clone)]
pub struct SnapshotAssembler {
    references: Arc<dyn ReferenceRepository>,
    configurations: Arc<dyn ConfigStore>,
    fixings: Arc<dyn FixingRepository>,
    snapshots: Arc<dyn SnapshotRepository>,
}
impl SnapshotAssembler {
    /// Genuine I/O boundaries; adapters choose their backend without entering pure pricing.
    pub fn new(
        references: Arc<dyn ReferenceRepository>,
        configurations: Arc<dyn ConfigStore>,
        fixings: Arc<dyn FixingRepository>,
        snapshots: Arc<dyn SnapshotRepository>,
    ) -> Self {
        Self {
            references,
            configurations,
            fixings,
            snapshots,
        }
    }
    /// Replay a bounded normalized event trace in recorded delivery order, filtering both cutoffs.
    /// No current/live source is queried. Invalid traces fail before publication.
    pub async fn assemble(
        &self,
        request: &SnapshotAssembly,
        events: &[QuoteEvent],
    ) -> Result<MarketSnapshot, TraitError> {
        let snapshot = self.resolve(request, events).await?;
        self.snapshots.publish(&snapshot).await?;
        Ok(snapshot)
    }
    /// Rebuild a committed checkpoint's snapshot `id` from its trace and publish it, as
    /// `assemble` does: an exact retry when it is stored, stored again when it is not. A
    /// snapshot is built under schema 4 now. A checkpoint written by an earlier build
    /// names the schema that build chose: 3 with a calibration, 2 with recipes, 1
    /// otherwise. When the schema-4 id differs, the rebuild under that schema must have
    /// `id` exactly. `None` when neither does.
    pub async fn restore(
        &self,
        request: &SnapshotAssembly,
        events: &[QuoteEvent],
        id: &SnapshotId,
    ) -> Result<Option<MarketSnapshot>, TraitError> {
        let mut snapshot = self.resolve(request, events).await?;
        if snapshot.id() != id {
            let content = snapshot.content();
            let earlier = if content.calibration.is_some() {
                3
            } else if content.curve_recipes.is_empty() {
                1
            } else {
                2
            };
            snapshot = snapshot.with_schema(earlier)?;
            if snapshot.id() != id {
                return Ok(None);
            }
        }
        self.snapshots.publish(&snapshot).await?;
        Ok(Some(snapshot))
    }
    /// Resolve without publishing, within [`SnapshotBounds::Standard`].
    pub async fn resolve(
        &self,
        request: &SnapshotAssembly,
        events: &[QuoteEvent],
    ) -> Result<MarketSnapshot, TraitError> {
        validate_request(request, events, SnapshotBounds::Standard)?;
        let configuration = self.configuration(request).await?;
        self.resolve_with_configuration(request, events, configuration, SnapshotBounds::Standard)
            .await
    }
    async fn configuration(
        &self,
        request: &SnapshotAssembly,
    ) -> Result<convex_ports::storage::BondPricingConfig, TraitError> {
        self.configurations
            .get_version(
                request.configuration.id.as_str(),
                request.configuration.version.get(),
            )
            .await?
            .ok_or(TraitError::InvalidInput(
                "missing exact snapshot configuration",
            ))
    }
    /// Resolve supplied immutable quantitative content without staging or publishing it,
    /// within `bounds`: a dry run of a Live configuration resolves within the Live ones.
    /// The same reference, curve and quality checks govern actual assembly and dry runs.
    pub async fn resolve_with_configuration(
        &self,
        request: &SnapshotAssembly,
        events: &[QuoteEvent],
        configuration: convex_ports::storage::BondPricingConfig,
        bounds: SnapshotBounds,
    ) -> Result<MarketSnapshot, TraitError> {
        validate_request(request, events, bounds)?;
        let references = self.references(request, &configuration).await?;
        let capture = capture_ms(request);
        let selected: BTreeSet<_> = request
            .quotes
            .iter()
            .map(|requirement| {
                (
                    requirement.key.instrument_id.as_str(),
                    &requirement.key.source,
                )
            })
            .collect();
        let mut quotes = IngestedQuotes::new(bounds)?;
        for event in events {
            if event.source_time_ms > request.as_of.datetime().timestamp_millis()
                || event.receive_time_ms > request.known_at.datetime().timestamp_millis()
                || !selected.contains(&(event.key.instrument_id.as_str(), &event.key.source))
            {
                continue;
            }
            quotes.ingest(event.clone(), capture);
        }
        let quotes = quotes.quotes(&request.quotes, capture);
        self.build(request, configuration, references, quotes, bounds)
            .await
    }
    /// The pinned references `request` selects, each with its pricing rule in force, and
    /// each benchmark set the rules name that is in force too, once.
    async fn references(
        &self,
        request: &SnapshotAssembly,
        configuration: &convex_ports::storage::BondPricingConfig,
    ) -> Result<ResolvedReferences, TraitError> {
        if configuration.config_id != request.configuration.id.as_str()
            || configuration.version != request.configuration.version.get()
        {
            return Err(TraitError::Conflict(
                "configuration adapter returned another revision",
            ));
        }
        let lookup = ReferenceLookup {
            as_of: request.as_of,
            known_at: request.known_at,
        };
        let mut references = Vec::with_capacity(request.instruments.len());
        for pin in &request.instruments {
            let id = convex_core::ids::InstrumentId::from(pin.instrument_id.clone());
            let record = self
                .references
                .get_version(&id, pin.version)
                .await
                .map_err(reference_error)?
                .ok_or(TraitError::InvalidInput(
                    "missing exact snapshot instrument",
                ))?;
            if record.instrument_id != id
                || record.version != pin.version
                || record.recorded_at > request.known_at
            {
                return Err(TraitError::Conflict(
                    "reference adapter returned another/later revision",
                ));
            }
            let bond = record.bond.as_ref().ok_or(TraitError::InvalidInput(
                "snapshot reference is a tombstone",
            ))?;
            let (issuer, calendar, convention) = self
                .references
                .resources_for_valuation(bond, lookup)
                .await
                .map_err(reference_error)?;
            let floating = self
                .references
                .floating_resources_for_valuation(bond)
                .await
                .map_err(reference_error)?;
            // The rule in force at the cutoff, pinned with the references.
            let pricing_rule = self
                .references
                .pricing_rule_for_valuation(&id, lookup)
                .await
                .map_err(reference_error)?;
            references.push(ResolvedBondReference {
                record,
                issuer,
                calendar,
                convention,
                floating,
                pricing_rule,
            });
        }
        let named: BTreeSet<_> = references
            .iter()
            .filter_map(|reference| reference.pricing_rule.as_ref()?.benchmark_set_id.clone())
            .collect();
        if named.len() > MAX_SNAPSHOT_BENCHMARK_SETS {
            return Err(TraitError::CapacityExceeded);
        }
        let mut benchmark_sets = Vec::with_capacity(named.len());
        for id in named {
            // A set not in force is left out, and only the bonds quoted to it fail.
            if let Some(set) = self
                .references
                .benchmark_set_for_valuation(&id, lookup)
                .await
                .map_err(reference_error)?
            {
                benchmark_sets.push(set);
            }
        }
        Ok(ResolvedReferences {
            references: SnapshotReferences::new(references),
            benchmark_sets: benchmark_sets.into(),
        })
    }
    /// Resolve the exact configuration and the pinned references `request` selects, at its
    /// cutoff, for reuse by later snapshots of the same selection. Only a Live
    /// runtime pins, so the request is within the Live bounds.
    pub(crate) async fn pin(&self, request: &SnapshotAssembly) -> Result<PinnedInputs, TraitError> {
        validate_request(request, &[], SnapshotBounds::Live)?;
        let configuration = self.configuration(request).await?;
        let references = self.references(request, &configuration).await?;
        Ok(PinnedInputs {
            activation: activation_revision(request),
            configuration_pin: request.configuration.clone(),
            instruments: request.instruments.clone(),
            configuration,
            references,
            epoch: None,
            last: None,
        })
    }
    /// Resolve over quote state the caller already ingested and inputs it pinned, without
    /// publishing. Live mode keeps both in memory; market data is not stored. Fixings are
    /// read at this request's cutoff. The quote entries that did not change since the
    /// last snapshot are shared with it.
    ///
    /// `epoch` names `request`'s selection. The caller changes it whenever the request's
    /// instruments, requirements, configuration or capture may have changed; otherwise
    /// only the request's cutoffs, generation and curve records change. Within
    /// one epoch, the instruments the pinned inputs were checked against are not compared
    /// again, and only the quote entries that may have changed are looked at.
    ///
    /// The snapshot is derived from the last one these inputs resolved, which is what
    /// `MarketSnapshot::new` would return for it.
    pub(crate) async fn resolve_ingested(
        &self,
        request: &SnapshotAssembly,
        epoch: u64,
        pinned: &mut PinnedInputs,
        quotes: &mut IngestedQuotes,
    ) -> Result<Arc<MarketSnapshot>, TraitError> {
        // The instruments are distinct: `pin` checked them, and the pinned inputs serve
        // only the same instruments.
        validate_frame(request, &[], SnapshotBounds::Live)?;
        if !pinned.serves_epoch(request, epoch) {
            return Err(TraitError::Conflict(
                "pinned inputs belong to another selection",
            ));
        }
        let content = self
            .content(
                request,
                pinned.configuration.clone(),
                pinned.references.clone(),
                quotes.live_quotes(&request.quotes, epoch, capture_ms(request)),
            )
            .await?;
        let snapshot = Arc::new(match &pinned.last {
            Some(last) => last.next(content)?,
            None => MarketSnapshot::bounded(content, SnapshotBounds::Live)?,
        });
        pinned.last = Some(snapshot.clone());
        Ok(snapshot)
    }
    async fn build(
        &self,
        request: &SnapshotAssembly,
        configuration: convex_ports::storage::BondPricingConfig,
        references: ResolvedReferences,
        quotes: Vec<SharedQuote>,
        bounds: SnapshotBounds,
    ) -> Result<MarketSnapshot, TraitError> {
        MarketSnapshot::bounded(
            self.content(request, configuration, references, quotes)
                .await?,
            bounds,
        )
    }
    /// A snapshot's content: the request's frame, the given inputs and quote entries, and
    /// the fixings its references require, read at its cutoff.
    async fn content(
        &self,
        request: &SnapshotAssembly,
        configuration: convex_ports::storage::BondPricingConfig,
        resolved: ResolvedReferences,
        quotes: Vec<SharedQuote>,
    ) -> Result<SnapshotContent, TraitError> {
        let ResolvedReferences {
            references,
            benchmark_sets,
        } = resolved;
        let keys = required_fixings(request, &references)?;
        let mut fixings = Vec::with_capacity(keys.len());
        for key in keys {
            let fixing = self.fixings.get_at(&key, request.known_at).await?;
            fixings.push(SnapshotFixing { key, fixing });
        }
        Ok(SnapshotContent {
            // The sectioned identity, whatever the curves.
            schema_version: SNAPSHOT_SCHEMA,
            as_of: request.as_of,
            known_at: request.known_at,
            generation: request.generation,
            configuration,
            configuration_capture: request.configuration_capture.clone(),
            references,
            quotes,
            fixings,
            required_curves: request.required_curves.clone(),
            curves: request.curves.clone(),
            curve_recipes: request.curve_recipes.clone(),
            calibration: request.calibration.clone(),
            max_source_skew_ms: request.max_source_skew_ms,
            benchmark_sets,
        })
    }
}

/// A selection's resolved references and the benchmark sets their rules name: what a
/// Live activation pins.
#[derive(Clone)]
pub(crate) struct ResolvedReferences {
    references: SnapshotReferences,
    benchmark_sets: Arc<[convex_ports::level_pricing::ResolvedBenchmarkSet]>,
}

/// Quote state ingested once per event in delivery order. Replay rebuilds it from a
/// trace for each snapshot; Live keeps one instance for the life of the process.
pub(crate) struct IngestedQuotes {
    assembler: MarketDataAssembler,
    /// How long the assembler keeps an observation fresh. A key's state changes without
    /// an event of its own only as its observations age past it, or by a capacity overflow.
    stale_after_ms: i64,
    delivered: HashMap<QuoteKey, QuoteEvent>,
    /// The snapshot entry each selected key last resolved to, shared by every snapshot
    /// until it changes. At most one per requirement of the last request.
    entries: HashMap<QuoteKey, SharedQuote>,
    /// The keys an event changed since the last Live list, while there is one. Live admits
    /// only selected keys, so at most one per requirement.
    changed: HashSet<QuoteKey>,
    /// The last list a Live selection resolved, so the next looks only at what may have
    /// changed. None before the first, and after a capacity overflow.
    last: Option<LiveList>,
}

/// One Live selection's last entries, in snapshot order, and when each may next change
/// without an event of its own.
struct LiveList {
    epoch: u64,
    capture_ms: i64,
    /// By instrument, then source, as `MarketSnapshot::new` orders them, so the snapshot
    /// keeps each entry at its position and `MarketSnapshot::next` finds the rebuilt ones
    /// by position.
    quotes: Vec<SharedQuote>,
    /// Each position's index in the request's requirements.
    requirements: Vec<usize>,
    positions: HashMap<QuoteKey, usize>,
    /// Each position's next turn, if it has one ([`turn`]), and the same pairs ordered by
    /// time: at most one each per requirement.
    turns: Vec<Option<i64>>,
    due: BTreeSet<(i64, usize)>,
}

impl LiveList {
    /// The list of `quotes`, given in requirement order and resolved at `capture_ms`, or
    /// None when two requirements share a key; such a selection walks every requirement at
    /// every call.
    fn new(
        epoch: u64,
        capture_ms: i64,
        quotes: &[SharedQuote],
        stale_after_ms: i64,
    ) -> Option<Self> {
        let mut requirements: Vec<usize> = (0..quotes.len()).collect();
        requirements.sort_by(|&a, &b| {
            let (a, b) = (&quotes[a].requirement.key, &quotes[b].requirement.key);
            (a.instrument_id.as_str(), &a.source).cmp(&(b.instrument_id.as_str(), &b.source))
        });
        let mut list = Self {
            epoch,
            capture_ms,
            quotes: requirements.iter().map(|&i| quotes[i].clone()).collect(),
            requirements,
            positions: HashMap::with_capacity(quotes.len()),
            turns: vec![None; quotes.len()],
            due: BTreeSet::new(),
        };
        for position in 0..list.quotes.len() {
            let quote = list.quotes[position].clone();
            if list
                .positions
                .insert(quote.requirement.key.clone(), position)
                .is_some()
            {
                return None;
            }
            list.set(position, quote, capture_ms, stale_after_ms);
        }
        Some(list)
    }
    /// Put `quote`, resolved at `capture_ms`, at `position`, and queue its next turn.
    fn set(&mut self, position: usize, quote: SharedQuote, capture_ms: i64, stale_after_ms: i64) {
        if let Some(at) = self.turns[position].take() {
            self.due.remove(&(at, position));
        }
        if let Some(at) = turn(&quote, capture_ms, stale_after_ms) {
            self.turns[position] = Some(at);
            self.due.insert((at, position));
        }
        self.quotes[position] = quote;
    }
}

/// The first capture time after `capture_ms` at which the state of `quote`'s key may
/// differ from its image's without an event, by `effective_state`: when a field received
/// after the capture is no longer ahead of it, or when a field's observation first ages
/// past `stale_after_ms`. None without an image, and once a field has aged, since the
/// state then stays stale until an event. A turn may come early; it must never come late.
fn turn(quote: &SnapshotQuote, capture_ms: i64, stale_after_ms: i64) -> Option<i64> {
    let image = quote.image.as_ref()?;
    let mut turn: Option<i64> = None;
    for field in image.fields.values() {
        let aged = field
            .source_time_ms
            .min(field.receive_time_ms)
            .saturating_add(stale_after_ms)
            .saturating_add(1);
        if aged <= capture_ms {
            return None;
        }
        let next = if field.receive_time_ms > capture_ms {
            field.receive_time_ms.min(aged)
        } else {
            aged
        };
        turn = Some(turn.map_or(next, |at| at.min(next)));
    }
    turn
}

impl IngestedQuotes {
    /// Quote state for the keys of snapshots within `bounds`.
    pub(crate) fn new(bounds: SnapshotBounds) -> Result<Self, TraitError> {
        Self::with_policy(IngestionPolicy {
            max_keys: bounds.instruments(),
            stale_after_ms: 86_400_000,
            future_tolerance_ms: 0,
        })
    }
    fn with_policy(policy: IngestionPolicy) -> Result<Self, TraitError> {
        Ok(Self {
            assembler: MarketDataAssembler::new(policy)?,
            stale_after_ms: policy.stale_after_ms,
            delivered: HashMap::new(),
            entries: HashMap::new(),
            changed: HashSet::new(),
            last: None,
        })
    }
    /// Apply one event at the capture time it was accepted under. A duplicate or
    /// out-of-order event changes neither its key's state nor its delivery, so its entry
    /// stands. Any other rebuilds its key's entry at the next snapshot; a full key set's
    /// transport gap blocks every key, so then every entry is.
    pub(crate) fn ingest(&mut self, event: QuoteEvent, capture_ms: i64) {
        let outcome = self.assembler.ingest(event.clone(), capture_ms);
        match outcome.outcome {
            IngestionOutcome::Duplicate | IngestionOutcome::OutOfOrder => return,
            IngestionOutcome::CapacityExceeded => {
                self.entries.clear();
                self.changed.clear();
                self.last = None;
            }
            _ => {
                self.entries.remove(&event.key);
                if self.last.is_some() {
                    self.changed.insert(event.key.clone());
                }
            }
        }
        self.delivered.insert(event.key.clone(), event);
    }
    /// Latest delivery and retained image for each requirement, aged at `capture_ms`. A
    /// key's entry is shared with the last snapshot unless the key ingested an event since,
    /// its requirement changed or its state at `capture_ms` differs (a day's staleness or
    /// a clock regression); only then is it built again, unchecked and unhashed. An entry
    /// holds its fields in the snapshot's order, so normalizing a snapshot copies none.
    fn quotes(&mut self, requirements: &[QuoteRequirement], capture_ms: i64) -> Vec<SharedQuote> {
        let quotes: Vec<_> = requirements
            .iter()
            .map(|requirement| self.resolve(requirement, capture_ms))
            .collect();
        // Keys the selection no longer holds are dropped, so at most one entry is kept for
        // each requirement.
        if self.entries.len() > requirements.len() {
            self.entries
                .retain(|key, _| requirements.iter().any(|r| &r.key == key));
        }
        quotes
    }
    /// What [`IngestedQuotes::quotes`] returns, in snapshot order, for a Live selection
    /// whose requirements do not change while `epoch` does not. Only the entries
    /// of keys that ingested an event since the last call, and of keys whose state may
    /// have turned with time, are looked at again; `quotes` would keep every other entry,
    /// so it is kept without a look. A new epoch, a capture earlier than the last or a
    /// capacity overflow walks every requirement, as `quotes` does. A selection with two
    /// requirements of one key keeps no list, so its entries come in requirement order.
    pub(crate) fn live_quotes(
        &mut self,
        requirements: &[QuoteRequirement],
        epoch: u64,
        capture_ms: i64,
    ) -> Vec<SharedQuote> {
        let current = self.last.take().filter(|last| {
            last.epoch == epoch
                && capture_ms >= last.capture_ms
                && last.quotes.len() == requirements.len()
        });
        let Some(mut last) = current else {
            let quotes = self.quotes(requirements, capture_ms);
            self.changed.clear();
            self.last = LiveList::new(epoch, capture_ms, &quotes, self.stale_after_ms);
            return match &self.last {
                Some(last) => last.quotes.clone(),
                None => quotes,
            };
        };
        let mut looked: Vec<usize> = self
            .changed
            .drain()
            .filter_map(|key| last.positions.get(&key).copied())
            .collect();
        while let Some(&(at, position)) = last.due.first() {
            if at > capture_ms {
                break;
            }
            last.due.pop_first();
            last.turns[position] = None;
            looked.push(position);
        }
        looked.sort_unstable();
        looked.dedup();
        for position in looked {
            let requirement = &requirements[last.requirements[position]];
            let quote = self.resolve(requirement, capture_ms);
            last.set(position, quote, capture_ms, self.stale_after_ms);
        }
        last.capture_ms = capture_ms;
        let quotes = last.quotes.clone();
        self.last = Some(last);
        quotes
    }
    /// One requirement's entry at `capture_ms`: the kept one while it holds, else one built
    /// from the state, then kept.
    fn resolve(&mut self, requirement: &QuoteRequirement, capture_ms: i64) -> SharedQuote {
        let key = &requirement.key;
        let current = self.entries.get(key).filter(|entry| {
            same_requirement(&entry.requirement, requirement)
                && entry
                    .image
                    .as_ref()
                    .is_none_or(|image| self.assembler.state(key, capture_ms) == Some(image.state))
        });
        if let Some(entry) = current {
            return entry.clone();
        }
        let mut requirement = requirement.clone();
        requirement.fields.sort();
        let entry = SharedQuote::new(SnapshotQuote {
            delivery_watermark: self.delivered.get(key).cloned(),
            image: self.assembler.snapshot(key, capture_ms),
            requirement,
        });
        self.entries.insert(key.clone(), entry.clone());
        entry
    }
}

/// Whether an entry's requirement, its fields sorted, is `requirement` with its fields in
/// any order. A configuration may list fields in any order; the snapshot sorts them.
fn same_requirement(entry: &QuoteRequirement, requirement: &QuoteRequirement) -> bool {
    entry.key == requirement.key
        && entry.max_age_ms == requirement.max_age_ms
        && if requirement.fields.is_sorted() {
            entry.fields == requirement.fields
        } else {
            same_fields(&entry.fields, &requirement.fields)
        }
}

/// Whether `fields` holds each field of `sorted` as often as `sorted` does, and nothing
/// else, without allocating. Lists longer than a quote's field limit, which the snapshot
/// refuses, are never the same, so the count stays bounded.
fn same_fields(sorted: &[QuoteField], fields: &[QuoteField]) -> bool {
    let count =
        |list: &[QuoteField], field: &QuoteField| list.iter().filter(|f| *f == field).count();
    sorted.len() == fields.len()
        && sorted.len() <= convex_ports::quote_event::MAX_QUOTE_FIELDS
        && sorted
            .iter()
            .all(|field| count(sorted, field) == count(fields, field))
}

/// The pricing configuration and pinned references of one selection, resolved once at
/// the cutoff of its first snapshot. Every record is an exact pinned version, so
/// none changes while the selection holds; each snapshot still checks them at its own
/// cutoff (`MarketSnapshot::new`). Every snapshot shares the one references section, so
/// its checks that read no cutoff and its digest are done once.
pub(crate) struct PinnedInputs {
    activation: Option<Version>,
    configuration_pin: ConfigurationPin,
    instruments: Vec<InstrumentPin>,
    configuration: convex_ports::storage::BondPricingConfig,
    references: ResolvedReferences,
    /// The Live selection epoch these inputs last served.
    epoch: Option<u64>,
    /// The last Live snapshot these inputs resolved; the next is derived from it.
    /// Any earlier snapshot serves, since `MarketSnapshot::next` compares
    /// what it shares with it.
    last: Option<Arc<MarketSnapshot>>,
}
impl PinnedInputs {
    /// Whether these inputs were resolved for `request`'s activation, configuration and
    /// instrument pins.
    pub(crate) fn serves(&self, request: &SnapshotAssembly) -> bool {
        self.activation == activation_revision(request)
            && self.configuration_pin == request.configuration
            && self.instruments == request.instruments
    }
    /// [`PinnedInputs::serves`] for a Live request of selection `epoch`. A request of the
    /// epoch these inputs last served has the same activation, configuration and pins, so
    /// it is not compared again.
    pub(crate) fn serves_epoch(&mut self, request: &SnapshotAssembly, epoch: u64) -> bool {
        if self.epoch == Some(epoch) {
            return true;
        }
        let serves = self.serves(request);
        if serves {
            self.epoch = Some(epoch);
        }
        serves
    }
}

fn activation_revision(request: &SnapshotAssembly) -> Option<Version> {
    request
        .configuration_capture
        .as_ref()
        .map(|capture| capture.activation.revision)
}

fn capture_ms(request: &SnapshotAssembly) -> i64 {
    request
        .as_of
        .max(request.known_at)
        .datetime()
        .timestamp_millis()
}

fn reference_error(error: convex_ports::reference_repository::ReferenceError) -> TraitError {
    use convex_ports::reference_repository::ReferenceError as E;
    match error {
        E::Unavailable => {
            TraitError::SourceNotAvailable("snapshot reference read unavailable".into())
        }
        E::CapacityExceeded => TraitError::CapacityExceeded,
        E::UnknownOutcome => TraitError::UnknownOutcome,
        E::Unsupported(reason) => TraitError::Unsupported(reason),
        _ => TraitError::InvalidInput("invalid/missing exact snapshot reference"),
    }
}

fn validate_request(
    request: &SnapshotAssembly,
    events: &[QuoteEvent],
    bounds: SnapshotBounds,
) -> Result<(), TraitError> {
    validate_frame(request, events, bounds)?;
    let mut ids = BTreeSet::new();
    for pin in &request.instruments {
        if !ids.insert(&pin.instrument_id) {
            return Err(TraitError::InvalidInput("duplicate requested reference"));
        }
    }
    let mut prior_receive = None;
    for event in events {
        event.validate()?;
        if prior_receive.is_some_and(|time| time > event.receive_time_ms) {
            return Err(TraitError::InvalidInput(
                "replay must retain recorded delivery order",
            ));
        }
        prior_receive = Some(event.receive_time_ms);
    }
    Ok(())
}

/// What [`validate_request`] checks except that the instruments are distinct and the
/// events valid and in order: the request's bounds, within `bounds`, and its curves at
/// its cutoffs.
fn validate_frame(
    request: &SnapshotAssembly,
    events: &[QuoteEvent],
    bounds: SnapshotBounds,
) -> Result<(), TraitError> {
    if request.instruments.is_empty()
        || request.instruments.len() > bounds.instruments()
        || request.quotes.len() > bounds.instruments()
        || request.fixings.len() > MAX_SNAPSHOT_FIXINGS
        || request.required_curves.len() > MAX_SNAPSHOT_CURVES
        || request.curves.len()
            + request.curve_recipes.len()
            + request.calibration.as_ref().map_or(0, |c| c.curves.len())
            > MAX_SNAPSHOT_CURVES
        || events.len() > 100_000
        || request.curves.iter().map(|c| c.points.len()).sum::<usize>()
            + request
                .curve_recipes
                .iter()
                .map(SnapshotCurveRecipe::input_count)
                .sum::<usize>()
            + request.calibration.as_ref().map_or(0, |c| c.input_count())
            > MAX_SNAPSHOT_PILLARS
    {
        return Err(TraitError::CapacityExceeded);
    }
    crate::calibration_replay::rebuild_captured_curves(
        &request.curve_recipes,
        request.calibration.as_ref(),
        request.as_of,
        request.known_at,
    )
    .map_err(TraitError::from)?;
    for record in &request.curves {
        BuiltCurve::from_snapshot(record)?;
    }
    Ok(())
}

/// Checked application result. Its numeric envelope is the canonical `ResultEnvelope`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ValuationResultWire", into = "ValuationResultWire")]
pub struct SnapshotValuationResult(ValuationResultWire);
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ValuationResultWire {
    cache_key: String,
    context: ValuationContext,
    envelope: ResultEnvelope,
}
impl SnapshotValuationResult {
    /// Validate exact context/provenance and complete requested coverage, including explicit failures.
    pub fn new(context: ValuationContext, envelope: ResultEnvelope) -> Result<Self, TraitError> {
        let result = Self(ValuationResultWire {
            cache_key: context.cache_key().to_owned(),
            context,
            envelope,
        });
        result.validate()?;
        Ok(result)
    }
    /// Full consumed context, including captured generation.
    pub fn context(&self) -> &ValuationContext {
        &self.0.context
    }
    /// Complete/partial/failed numeric results with original supplied calculation time.
    pub fn envelope(&self) -> &ResultEnvelope {
        &self.0.envelope
    }
    /// Complete-context digest, never a node ID or formatted input length.
    pub fn cache_key(&self) -> &str {
        &self.0.cache_key
    }
    /// Reject incompatible lineage, unrequested results and silent missing measures.
    pub fn validate(&self) -> Result<(), TraitError> {
        let spec = self.0.context.spec();
        let provenance = self.0.envelope.provenance();
        if self.0.cache_key != self.0.context.cache_key()
            || provenance.subject != spec.subject
            || provenance.snapshot_id != spec.snapshot_id
            || provenance.configuration_version != spec.configuration.version
            || provenance.valuation_time != spec.valuation_time
            || provenance.known_at != spec.known_at
            || provenance.model_version != spec.model_id
            || provenance.build_id != spec.build_id
        {
            return Err(TraitError::Conflict(
                "result lineage differs from valuation context",
            ));
        }
        let mut coverage: BTreeSet<&str> = self
            .0
            .envelope
            .results()
            .keys()
            .map(|id| id.as_str())
            .collect();
        for error in self.0.envelope.errors() {
            let field = error.field().ok_or(TraitError::InvalidInput(
                "result error must identify requested measure",
            ))?;
            if !coverage.insert(field) {
                return Err(TraitError::InvalidInput(
                    "duplicate success/failure measure",
                ));
            }
        }
        if coverage != spec.measures.iter().map(|id| id.as_str()).collect() {
            return Err(TraitError::InvalidInput(
                "result does not account for requested measures",
            ));
        }
        Ok(())
    }
}
impl TryFrom<ValuationResultWire> for SnapshotValuationResult {
    type Error = TraitError;
    fn try_from(value: ValuationResultWire) -> Result<Self, Self::Error> {
        let result = Self(value);
        result.validate()?;
        Ok(result)
    }
}
impl From<SnapshotValuationResult> for ValuationResultWire {
    fn from(value: SnapshotValuationResult) -> Self {
        value.0
    }
}

/// Snapshot valuation use case: existing router mathematics over resolved immutable values.
/// The owned runtime commits these results through captured snapshot/context graph tickets. This use case owns no scheduling loop.
pub struct SnapshotValuator {
    build_id: ModelId,
    cache: ValuationCache,
    /// Benchmark yields shared by the bonds quoted to them.
    benchmark_yield_cache: crate::level_pricing::BenchmarkYieldCache,
}
impl SnapshotValuator {
    /// Composition root supplies actual build identity and an explicit bounded cache.
    pub fn new(build_id: ModelId, cache_capacity: usize) -> Result<Self, TraitError> {
        Ok(Self {
            build_id,
            cache: ValuationCache::new(cache_capacity)?,
            benchmark_yield_cache: Default::default(),
        })
    }
    /// Explicit administrative cache release; published snapshots/results remain unchanged.
    pub fn clear_cache(&self) {
        self.cache.clear();
    }
    /// Number of retained complete contexts, for capacity diagnostics.
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
    /// Price the declared R0 mid measures. No reference/data/configuration/clock access occurs here.
    /// Cache hits preserve the original result; request correlation is supplied on the returned envelope.
    /// The cache is looked up before any build. A context names its snapshot by content digest,
    /// and every retained result was valued from that verified content, so a hit needs neither
    /// verification nor curves. A miss verifies the snapshot and builds its curves.
    pub fn value(
        &self,
        snapshot: &MarketSnapshot,
        context: &ValuationContext,
        request_id: RequestId,
        calculated_at: UtcTimestamp,
    ) -> Result<SnapshotValuationResult, TraitError> {
        self.admit(snapshot, context, calculated_at)?;
        if let Some(cached) = self.cache.get(context)? {
            return rebound(context, &cached, request_id);
        }
        let started = std::time::Instant::now();
        let recipe_curves = crate::curve_replay::replay_snapshot_curves(snapshot);
        crate::telemetry::observe_curve_build(crate::telemetry::CurvePath::Valuation, started);
        let recipe_curves = recipe_curves.map_err(TraitError::from)?;
        let quality = valuation_quality(snapshot, context.spec())?.quality;
        self.price(
            snapshot,
            &recipe_curves,
            context,
            request_id,
            calculated_at,
            quality,
        )
    }
    /// [`Self::value`] over a snapshot verified and built once for its whole activation, so
    /// valuing each instrument neither re-hashes the snapshot nor rebuilds its curves.
    pub fn value_built(
        &self,
        built: &crate::curve_replay::BuiltSnapshot,
        context: &ValuationContext,
        request_id: RequestId,
        calculated_at: UtcTimestamp,
    ) -> Result<SnapshotValuationResult, TraitError> {
        let quality = valuation_quality(built.snapshot(), context.spec())?.quality;
        self.value_built_judged(built, context, request_id, calculated_at, quality)
    }
    /// [`Self::value_built`] for a caller that already derived the instrument's quality,
    /// so one valuation derives it once.
    pub(crate) fn value_built_judged(
        &self,
        built: &crate::curve_replay::BuiltSnapshot,
        context: &ValuationContext,
        request_id: RequestId,
        calculated_at: UtcTimestamp,
        quality: DataQuality,
    ) -> Result<SnapshotValuationResult, TraitError> {
        self.admit(built.snapshot(), context, calculated_at)?;
        if let Some(cached) = self.cache.get(context)? {
            return rebound(context, &cached, request_id);
        }
        self.price(
            built.snapshot(),
            built.curves(),
            context,
            request_id,
            calculated_at,
            quality,
        )
    }
    fn admit(
        &self,
        snapshot: &MarketSnapshot,
        context: &ValuationContext,
        calculated_at: UtcTimestamp,
    ) -> Result<(), TraitError> {
        context.validate_snapshot(snapshot)?;
        let spec = context.spec();
        if calculated_at < spec.known_at || calculated_at < spec.valuation_time {
            return Err(TraitError::InvalidInput(
                "calculation precedes supplied information/valuation time",
            ));
        }
        if spec.build_id != self.build_id
            || !crate::optional_valuation::PRODUCT_MODELS.contains(&spec.model_id.as_str())
            || spec.numerical_settings.policy_id.as_str() != BOND_NUMERICAL_POLICY
            || spec.scenario != ValuationScenario::Base
        {
            return Err(TraitError::Unsupported(
                "unsupported build/model/numerical/scenario policy",
            ));
        }
        Ok(())
    }
    fn price(
        &self,
        snapshot: &MarketSnapshot,
        recipe_curves: &convex_curves::contracts::CurveBuildOutput,
        context: &ValuationContext,
        request_id: RequestId,
        calculated_at: UtcTimestamp,
        quality: DataQuality,
    ) -> Result<SnapshotValuationResult, TraitError> {
        let spec = context.spec();
        let ValuationSubject::InstrumentVersion { instrument_id, .. } = &spec.subject else {
            return Err(TraitError::Unsupported("trade economics unavailable"));
        };
        let reference = snapshot
            .reference(instrument_id.as_str())
            .ok_or(TraitError::InvalidInput("missing pinned reference"))?;
        let bond = reference
            .record
            .bond
            .as_ref()
            .ok_or(TraitError::InvalidInput("tombstone reference"))?;
        if spec.reporting_currency != bond.currency {
            return Err(TraitError::Unsupported(
                "snapshot FX reporting conversion is not implemented",
            ));
        }
        let configuration = &snapshot.content().configuration;
        if !configuration.applies_to.matches(bond) {
            return Err(TraitError::InvalidInput(
                "configuration does not apply to pinned instrument",
            ));
        }
        let rule = reference.pricing_rule.as_ref();
        if crate::optional_valuation::product_model(bond, reference.pricing_method())
            != spec.model_id.as_str()
        {
            return Err(TraitError::Unsupported(
                "context model does not value the pinned product",
            ));
        }
        if !matches!(
            configuration.pricing_spec,
            PricingSpec::Auto
                | PricingSpec::MarketQuote {
                    side: convex_ports::storage::QuoteSide::Mid
                }
        ) {
            return Err(TraitError::Unsupported(
                "snapshot pricing currently requires observed clean marks",
            ));
        }
        let selected = snapshot.quotes_for(instrument_id.as_str());
        if selected.len() != 1 {
            return Err(TraitError::InvalidInput(
                "pricing requires one explicitly selected quote source",
            ));
        }
        let model = spec.model_id.as_str();
        // A bond refused whatever its market data fails each measure, saying why; the
        // runtime reports it ahead of the inputs' quality, and a request is refused before
        // it is valued.
        let refusal = valuation_refusal(snapshot, spec);
        // A bond is read from the field its rule quotes it by: its level, or a price. A
        // callable and a floating rate note value their rule's level themselves, and a
        // price too, so when their quote declares no field of their rule's they are read
        // from a price: a request may send one, and a Live plan compiled before the rule
        // moved to a level still carries one ([`PricingMethod::declared_level_field`]).
        let declared = &selected[0].requirement.fields;
        let price = PricingMethod::Price {};
        let quoted_method = rule
            .map(|rule| &rule.method)
            .filter(|method| {
                !crate::optional_valuation::values_own_level(model)
                    || method.declared_level_field(declared).is_some()
            })
            .unwrap_or(&price);
        let quoted_field = quoted_method.quote_field();
        if refusal.is_none() && !declared.contains(&quoted_field) {
            return Err(TraitError::InvalidInput(
                "mid valuation must declare the field the bond is quoted by",
            ));
        }
        let quote = selected[0].image.as_ref();
        let mark = |field| {
            quote
                .and_then(|q| q.fields.get(&field))
                .map(|value| value.value.value)
        };
        let (results, errors) = if let Some((code, reason)) = refusal {
            let errors = spec
                .measures
                .iter()
                .map(|measure| {
                    StructuredError::new(code, Some(measure.as_str().into()), reason, false)
                        .map_err(contract_error)
                })
                .collect::<Result<Vec<_>, _>>()?;
            (BTreeMap::new(), errors)
        } else if model == crate::optional_valuation::OPTIONAL_SNAPSHOT_MODEL {
            callable_measures(
                snapshot,
                spec,
                reference,
                recipe_curves,
                quoted_method,
                mark(quoted_field),
                quality,
            )?
        } else if model == crate::workout_valuation::WORKOUT_SNAPSHOT_MODEL {
            workout_measures(spec, reference, quoted_method, mark(quoted_field), quality)?
        } else if model == crate::floating_valuation::FLOATING_SNAPSHOT_MODEL {
            floating_measures(
                snapshot,
                spec,
                reference,
                recipe_curves,
                quoted_method,
                mark(quoted_field),
                quality,
            )?
        } else {
            let curve = |id: &convex_core::ids::CurveId| {
                if let Some(key) = recipe_curves
                    .curves
                    .keys()
                    .find(|key| key.id == id.as_str())
                {
                    let selected = recipe_curves
                        .curves
                        .get(key)
                        .map_err(|_| TraitError::InvalidInput("missing exact complete curve"))?;
                    if !crate::curve_resolution::pricing_role_matches(
                        selected.recipe(),
                        id == &configuration.analytics_curves.discount_curve,
                        bond.currency,
                    ) {
                        return Err(TraitError::InvalidInput(
                            "snapshot pricing curve role/currency mismatch",
                        ));
                    }
                    if let Some(record) = snapshot
                        .content()
                        .curve_recipes
                        .iter()
                        .find(|r| r.curve_id.as_str() == id.as_str())
                    {
                        return BuiltCurve::from_recipe(record, selected.clone()).map(Some);
                    }
                    let capture = snapshot
                        .content()
                        .calibration
                        .as_ref()
                        .ok_or(TraitError::InvalidInput("missing complete calibration"))?;
                    let pin = capture
                        .curves
                        .iter()
                        .find(|p| p.curve_id.as_str() == id.as_str())
                        .ok_or(TraitError::InvalidInput("missing calibration curve pin"))?;
                    return BuiltCurve::from_calibration(capture, pin, selected.clone()).map(Some);
                }
                snapshot
                    .content()
                    .curves
                    .iter()
                    .find(|c| c.curve_id.as_str() == id.as_str())
                    .map(BuiltCurve::from_snapshot)
                    .transpose()
            };
            // Every runtime measure reads the mid quote, so bid and ask are not priced.
            // The configured curves are still resolved and checked, but reach the router
            // only when a spread or curve-risk measure is asked for.
            let discount = curve(&configuration.analytics_curves.discount_curve)?;
            let benchmark = configuration
                .analytics_curves
                .swap_curve
                .as_ref()
                .map(curve)
                .transpose()?
                .flatten();
            let curves = spec.measures.iter().any(|m| curve_measure(m.as_str()));
            // Under a rule, the level stage turns the quote into the clean price the
            // router values, and gives the set's securities for a G-spread asked of it.
            let quoted = mark(quoted_field);
            let staged = rule.zip(quoted).map(|(_, quoted)| {
                crate::level_pricing::price_in_snapshot(
                    snapshot,
                    reference,
                    quoted,
                    spec.settlement_date.date(),
                    discount.as_ref(),
                    &self.benchmark_yield_cache,
                )
            });
            let g_spread = spec.measures.iter().any(|m| m.as_str() == G_SPREAD_MEASURE);
            let (market_price_mid, government_curve, refusal) = match staged {
                None => (quoted.filter(|_| rule.is_none()), None, None),
                Some(Ok(pricing)) => (
                    Some(pricing.clean_price_mid),
                    pricing.government_curve.filter(|_| g_spread),
                    None,
                ),
                Some(Err(error)) => (None, None, Some(error)),
            };
            let input = PricingInput {
                bond: bond.clone(),
                settlement_date: spec.settlement_date.date(),
                market_price_bid: None,
                market_price_mid,
                market_price_ask: None,
                discount_curve: discount.filter(|_| curves),
                benchmark_curve: benchmark.filter(|_| curves),
                government_curve,
                volatility: None,
                bid_ask_config: None,
            };
            let router = PricingRouter {
                bump_bps: spec.numerical_settings.bump_bps.get(),
            };
            let (priced, pricing_failed) = if input.market_price_mid.is_none() {
                (None, false)
            } else {
                match router.price_at(&input, calculated_at.datetime().timestamp_millis()) {
                    Ok(value) => (Some(value), false),
                    Err(_) => (None, true),
                }
            };
            let mut results = BTreeMap::new();
            let mut errors = Vec::new();
            for measure in &spec.measures {
                let value = priced
                    .as_ref()
                    .map(|output| snapshot_mid_value(output, measure.as_str()))
                    .transpose()?
                    .flatten();
                match value {
                    Some(value) => {
                        results.insert(
                            measure.clone(),
                            MeasureResult {
                                value,
                                quality,
                                method: spec.model_id.clone(),
                                source_observation_id: None,
                            },
                        );
                    }
                    None => {
                        // The stage's refusal names what is missing, a benchmark by tenor.
                        if let Some(refusal) = &refusal {
                            errors.push(
                                StructuredError::new(
                                    refusal.error_code(),
                                    Some(measure.as_str().into()),
                                    refusal.to_string(),
                                    false,
                                )
                                .map_err(contract_error)?,
                            );
                            continue;
                        }
                        let supported = is_runtime_measure(measure.as_str());
                        // The router names the measures it could not produce; keep its code and reason.
                        let field = if curve_risk_measure(measure.as_str()) {
                            "curve_risk_mid"
                        } else {
                            measure.as_str()
                        };
                        let routed = priced.as_ref().and_then(|output| {
                            output
                                .measure_errors
                                .iter()
                                .find(|error| error.field() == Some(field))
                        });
                        if let Some(error) = routed {
                            errors.push(
                                StructuredError::new(
                                    error.code(),
                                    Some(measure.as_str().into()),
                                    error.message(),
                                    false,
                                )
                                .map_err(contract_error)?,
                            );
                            continue;
                        }
                        errors.push(StructuredError::new(if !supported { ErrorCode::InvalidInput } else if pricing_failed { ErrorCode::SolverLimit } else if input.market_price_mid.is_none() { ErrorCode::MissingMarketData } else { ErrorCode::SolverLimit }, Some(measure.as_str().into()), if supported { "Required mark or numerical calculation is unavailable; no value substituted" } else { "Requested measure is outside this snapshot valuation slice" }, false).map_err(contract_error)?);
                    }
                }
            }
            (results, errors)
        };
        let status = if results.is_empty() {
            ResultStatus::Failed
        } else if errors.is_empty() {
            ResultStatus::Complete
        } else {
            ResultStatus::Partial
        };
        let input_versions = consumed_versions(snapshot, reference)?;
        let envelope = ResultEnvelope::new(
            request_id.clone(),
            status,
            quality,
            results,
            errors,
            Provenance {
                subject: spec.subject.clone(),
                snapshot_id: snapshot.id().clone(),
                configuration_version: spec.configuration.version,
                valuation_time: spec.valuation_time,
                known_at: spec.known_at,
                calculated_at,
                model_version: spec.model_id.clone(),
                build_id: spec.build_id.clone(),
                input_versions,
            },
        )
        .map_err(contract_error)?;
        let result = SnapshotValuationResult::new(context.clone(), envelope)?;
        let retained = self.cache.insert(result)?;
        rebound(context, &retained, request_id)
    }
}

/// A retained result with this request's correlation.
fn rebound(
    context: &ValuationContext,
    retained: &SnapshotValuationResult,
    request_id: RequestId,
) -> Result<SnapshotValuationResult, TraitError> {
    let envelope = retained.envelope();
    SnapshotValuationResult::new(
        context.clone(),
        ResultEnvelope::new(
            request_id,
            envelope.status(),
            envelope.quality(),
            envelope.results().clone(),
            envelope.errors().to_vec(),
            envelope.provenance().clone(),
        )
        .map_err(contract_error)?,
    )
}

type Measures = (BTreeMap<MeasureId, MeasureResult>, Vec<StructuredError>);

/// Each requested measure from `value`, under the context's model and `quality`; a
/// measure `value` cannot give is reported with its code and reason.
fn model_measures(
    spec: &ValuationContextSpec,
    quality: DataQuality,
    value: impl Fn(&str) -> Result<MeasureValue, crate::optional_valuation::Failure>,
) -> Result<Measures, TraitError> {
    let mut results = BTreeMap::new();
    let mut errors = Vec::new();
    for measure in &spec.measures {
        match value(measure.as_str()) {
            Ok(value) => {
                results.insert(
                    measure.clone(),
                    MeasureResult {
                        value,
                        quality,
                        method: spec.model_id.clone(),
                        source_observation_id: None,
                    },
                );
            }
            Err(error) => errors.push(
                StructuredError::new(
                    error.code,
                    Some(measure.as_str().into()),
                    error.message,
                    false,
                )
                .map_err(contract_error)?,
            ),
        }
    }
    Ok((results, errors))
}

/// A callable's measures on its workouts, from its `mark` quoted by `method`: a clean
/// price, or a yield to worst. A plan measure the workout model does not publish never
/// reaches this context.
fn workout_measures(
    spec: &ValuationContextSpec,
    reference: &ResolvedBondReference,
    method: &PricingMethod,
    mark: Option<rust_decimal::Decimal>,
    quality: DataQuality,
) -> Result<Measures, TraitError> {
    use crate::optional_valuation::{failure, level_failure, Failure};
    use crate::workout_valuation::{value_on_workouts, WorkoutQuote};
    let bond = reference
        .record
        .bond
        .as_ref()
        .ok_or(TraitError::InvalidInput("tombstone reference"))?;
    let calendar = reference
        .calendar_definition()
        .ok_or(TraitError::InvalidInput("wrong calendar resource"))?;
    let valued = mark
        .ok_or_else(|| {
            failure(
                ErrorCode::MissingMarketData,
                "Required mid mark is unavailable",
            )
        })
        .and_then(|mark| {
            let quote = WorkoutQuote::quoted_by(method, mark).map_err(level_failure)?;
            value_on_workouts(bond, calendar, spec.settlement_date.date(), quote)
        });
    let finite = |value: f64| {
        FiniteF64::new(value)
            .map_err(|_| failure(ErrorCode::InvalidInput, "nonfinite workout measure"))
    };
    model_measures(spec, quality, |measure| {
        let valued = valued.as_ref().map_err(Failure::clone)?;
        Ok(match measure {
            "clean_price_mid" => {
                MeasureValue::Price(PricePer100::new(ExactDecimal::new(valued.clean_price)))
            }
            "dirty_price_mid" => {
                MeasureValue::ModelPrice(ModelPricePer100::new(finite(valued.dirty_price)?))
            }
            "accrued_interest" => {
                MeasureValue::ModelPrice(ModelPricePer100::new(finite(valued.accrued)?))
            }
            "ytw_mid" => {
                MeasureValue::ModelRate(ModelDecimalRate::new(finite(valued.yield_to_worst)?))
            }
            "years_to_worst_mid" => {
                MeasureValue::ModelYears(ModelYears::new(finite(valued.years_to_worst)?))
            }
            _ => {
                return Err(failure(
                    ErrorCode::InvalidInput,
                    "Requested measure is outside this snapshot valuation slice",
                ))
            }
        })
    })
}

/// The plan's explicit fixings plus each floating rate note's observations that are due by
/// the cutoff, derived from its built contract. A note the adapter cannot build adds
/// none; its valuation reports why.
fn required_fixings(
    request: &SnapshotAssembly,
    references: &SnapshotReferences,
) -> Result<Vec<convex_ports::fixing::FixingKey>, TraitError> {
    let as_of = convex_core::Date::from(request.as_of.datetime().date_naive());
    let mut keys = request.fixings.iter().cloned().collect::<BTreeSet<_>>();
    for reference in references.floating() {
        let due = crate::floating_valuation::FloatingNote::new(reference)
            .and_then(|note| note.due_fixings(as_of, request.known_at));
        if let Ok(due) = due {
            keys.extend(due);
        }
        if keys.len() > convex_ports::snapshot::MAX_SNAPSHOT_FIXINGS {
            return Err(TraitError::CapacityExceeded);
        }
    }
    Ok(keys.into_iter().collect())
}

/// Floating rate note measures from `convex::library`, at its `mark` quoted by `method`:
/// a clean price, or a discount margin.
#[allow(clippy::too_many_arguments)]
fn floating_measures(
    snapshot: &MarketSnapshot,
    spec: &ValuationContextSpec,
    reference: &ResolvedBondReference,
    recipe_curves: &convex_curves::contracts::CurveBuildOutput,
    method: &PricingMethod,
    mark: Option<rust_decimal::Decimal>,
    quality: DataQuality,
) -> Result<Measures, TraitError> {
    use crate::floating_valuation::FloatingQuote;
    use crate::optional_valuation::{failure, level_failure, Failure};
    let configuration = &snapshot.content().configuration;
    let bond = reference
        .record
        .bond
        .as_ref()
        .ok_or(TraitError::InvalidInput("tombstone reference"))?;
    let missing = || {
        failure(
            ErrorCode::MissingMarketData,
            "Required mid mark is unavailable",
        )
    };
    let valued = mark.ok_or_else(missing).and_then(|mark| {
        let quote = FloatingQuote::quoted_by(method, mark).map_err(level_failure)?;
        let note = crate::floating_valuation::FloatingNote::new(reference)?;
        // The library builds curves from recipes: the discount role must be a collateral
        // recipe and `frn_reference_curve` a projection recipe of the note's own index.
        let recipe = |id: &str, discount: bool| -> Result<convex_curves::contracts::CurveKey, Failure> {
            let unsupported = || {
                failure(
                    ErrorCode::UnsupportedConvention,
                    "floating rate notes need their discount and projection curves as stored curve recipes",
                )
            };
            if !snapshot
                .content()
                .curve_recipes
                .iter()
                .any(|r| r.curve_id.as_str() == id)
            {
                return Err(unsupported());
            }
            let key = recipe_curves
                .curves
                .keys()
                .find(|key| key.id == id)
                .ok_or_else(unsupported)?;
            let built = recipe_curves
                .curves
                .get(key)
                .map_err(|_| failure(ErrorCode::InvalidInput, "missing exact complete curve"))?;
            let role = crate::curve_resolution::pricing_role_matches(
                built.recipe(),
                discount,
                bond.currency,
            ) && (discount
                || matches!(
                    &built.recipe().role,
                    convex_curves::contracts::CurveRole::Projection { index_id }
                        if index_id.as_str() == note.index_id().as_str()
                ));
            if !role {
                return Err(failure(
                    ErrorCode::InvalidInput,
                    "snapshot pricing curve role/currency/index mismatch",
                ));
            }
            Ok(key.clone())
        };
        let discount = recipe(configuration.analytics_curves.discount_curve.as_str(), true)?;
        let projection = recipe(
            configuration
                .analytics_curves
                .frn_reference_curve
                .as_ref()
                .ok_or_else(|| {
                    failure(
                        ErrorCode::UnsupportedConvention,
                        "floating rate notes need the configuration's frn_reference_curve",
                    )
                })?
                .as_str(),
            false,
        )?;
        let recipes = snapshot
            .content()
            .curve_recipes
            .iter()
            .map(|r| serde_json::from_value(r.recipe.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| failure(ErrorCode::InvalidInput, "undecodable curve recipe"))?;
        crate::floating_valuation::value_floating(
            &note,
            &recipes,
            &discount,
            &projection,
            spec.settlement_date.date(),
            convex_core::Date::from(snapshot.content().as_of.datetime().date_naive()),
            snapshot.content().known_at,
            &snapshot.content().fixings,
            quote,
        )
    });
    let finite = |value: f64| {
        FiniteF64::new(value)
            .map_err(|_| failure(ErrorCode::InvalidInput, "nonfinite floating measure"))
    };
    model_measures(spec, quality, |measure| {
        let valued = valued.as_ref().map_err(Failure::clone)?;
        Ok(match measure {
            "clean_price_mid" => {
                MeasureValue::Price(PricePer100::new(ExactDecimal::new(valued.clean)))
            }
            "dirty_price_mid" => {
                MeasureValue::ModelPrice(ModelPricePer100::new(finite(valued.dirty)?))
            }
            "accrued_interest" => {
                MeasureValue::ModelPrice(ModelPricePer100::new(finite(valued.accrued)?))
            }
            // Over the frozen projection, discounted on the collateral curve.
            "discount_margin_mid" => {
                MeasureValue::ModelBps(ModelBasisPoints::new(finite(valued.discount_margin_bps)?))
            }
            _ => {
                return Err(failure(
                    ErrorCode::InvalidInput,
                    "Requested measure is outside this snapshot valuation slice",
                ))
            }
        })
    })
}

/// Callable measures on HW1F trees from `convex::library`, at its `mark` quoted by
/// `method`: a clean price, or an OAS. A plan measure the optional model does not publish
/// never reaches this context.
fn callable_measures(
    snapshot: &MarketSnapshot,
    spec: &ValuationContextSpec,
    reference: &ResolvedBondReference,
    recipe_curves: &convex_curves::contracts::CurveBuildOutput,
    method: &PricingMethod,
    mark: Option<rust_decimal::Decimal>,
    quality: DataQuality,
) -> Result<Measures, TraitError> {
    use crate::optional_valuation::{
        self as optional, failure, level_failure, CallableQuote, Failure,
    };
    let configuration = &snapshot.content().configuration;
    let bond = reference
        .record
        .bond
        .as_ref()
        .ok_or(TraitError::InvalidInput("tombstone reference"))?;
    let missing = || {
        failure(
            ErrorCode::MissingMarketData,
            "Required mid mark is unavailable",
        )
    };
    let valued = mark.ok_or_else(missing).and_then(|mark| {
        let quote = CallableQuote::quoted_by(method, mark).map_err(level_failure)?;
        // The library builds curves from recipes, so the discount curve must be a stored recipe.
        let id = configuration.analytics_curves.discount_curve.as_str();
        let unsupported = || {
            failure(
                ErrorCode::UnsupportedConvention,
                "callables need the discount curve as a stored curve recipe",
            )
        };
        if !snapshot
            .content()
            .curve_recipes
            .iter()
            .any(|r| r.curve_id.as_str() == id)
        {
            return Err(unsupported());
        }
        let key = recipe_curves
            .curves
            .keys()
            .find(|key| key.id == id)
            .ok_or_else(unsupported)?;
        let built = recipe_curves
            .curves
            .get(key)
            .map_err(|_| failure(ErrorCode::InvalidInput, "missing exact complete curve"))?;
        if !crate::curve_resolution::pricing_role_matches(built.recipe(), true, bond.currency) {
            return Err(failure(
                ErrorCode::InvalidInput,
                "snapshot pricing curve role/currency mismatch",
            ));
        }
        let recipes = snapshot
            .content()
            .curve_recipes
            .iter()
            .map(|r| serde_json::from_value(r.recipe.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| failure(ErrorCode::InvalidInput, "undecodable curve recipe"))?;
        let calendar = reference
            .calendar_definition()
            .ok_or_else(|| failure(ErrorCode::InvalidInput, "wrong calendar resource"))?;
        optional::value_callable(
            bond,
            calendar,
            reference.calendar.version,
            &recipes,
            key,
            configuration,
            spec.settlement_date.date(),
            snapshot.content().known_at,
            quote,
        )
    });
    let finite = |value: f64| {
        FiniteF64::new(value)
            .map_err(|_| failure(ErrorCode::InvalidInput, "nonfinite callable measure"))
    };
    model_measures(spec, quality, |measure| {
        let valued = valued.as_ref().map_err(Failure::clone)?;
        let risk = || valued.risk.as_ref().map_err(Failure::clone);
        Ok(match measure {
            "clean_price_mid" => {
                MeasureValue::Price(PricePer100::new(ExactDecimal::new(valued.clean)))
            }
            "dirty_price_mid" => {
                MeasureValue::ModelPrice(ModelPricePer100::new(finite(valued.dirty)?))
            }
            "accrued_interest" => {
                MeasureValue::ModelPrice(ModelPricePer100::new(finite(valued.accrued)?))
            }
            "oas_mid" => {
                let oas_bps = valued.oas_bps.as_ref().map_err(Failure::clone)?;
                MeasureValue::ModelBps(ModelBasisPoints::new(finite(*oas_bps)?))
            }
            // Parallel discount shift with the OAS frozen.
            "effective_duration_mid" => {
                MeasureValue::ModelYears(ModelYears::new(finite(risk()?.duration)?))
            }
            "effective_convexity_mid" => {
                MeasureValue::ModelYearsSquared(ModelYearsSquared::new(finite(risk()?.convexity)?))
            }
            // Loss per 100 face for +1 bp of the discount curve.
            "pv01_mid" => MeasureValue::ModelPrice(ModelPricePer100::new(finite(risk()?.pv01)?)),
            _ => {
                return Err(failure(
                    ErrorCode::InvalidInput,
                    "Requested measure is outside this snapshot valuation slice",
                ))
            }
        })
    })
}

pub(crate) fn consumed_versions(
    snapshot: &MarketSnapshot,
    reference: &ResolvedBondReference,
) -> Result<Vec<InputVersion>, TraitError> {
    let mut versions = Vec::new();
    let mut push = |kind: &str, id: &str, version| -> Result<(), TraitError> {
        versions.push(InputVersion {
            kind: SourceId::new(kind).map_err(contract_error)?,
            id: SourceId::new(id).map_err(contract_error)?,
            version,
        });
        Ok(())
    };
    push(
        "instrument",
        reference.record.instrument_id.as_str(),
        reference.record.version,
    )?;
    for record in [
        &reference.issuer,
        &reference.calendar,
        &reference.convention,
    ] {
        let (kind, id) = match &record.definition {
            convex_ports::reference_master::MasterDefinition::Entity(entity) => {
                ("entity", entity.entity_id.as_str())
            }
            convex_ports::reference_master::MasterDefinition::Calendar(calendar) => {
                ("calendar", calendar.calendar_id.as_str())
            }
            convex_ports::reference_master::MasterDefinition::Convention(convention) => {
                ("convention", convention.convention_id.as_str())
            }
            _ => return Err(TraitError::InvalidInput("wrong reference resource")),
        };
        push(kind, id, record.version)?;
    }
    if let Some(floating) = &reference.floating {
        use convex_ports::reference_master::MasterDefinition as M;
        let (M::RateIndex(index), M::Calendar(calendar)) =
            (&floating.index.definition, &floating.calendar.definition)
        else {
            return Err(TraitError::InvalidInput(
                "wrong floating reference resource",
            ));
        };
        push(
            "rate_index",
            index.rate_index_id.as_str(),
            floating.index.version,
        )?;
        push(
            "calendar",
            calendar.calendar_id.as_str(),
            floating.calendar.version,
        )?;
    }
    // A bond priced under a rule read the rule and its set; the set's version pins each
    // benchmark security's.
    if let Some(rule) = &reference.pricing_rule {
        push(
            "pricing_rule",
            reference.record.instrument_id.as_str(),
            rule.version,
        )?;
        if let Some(set) = rule
            .benchmark_set_id
            .as_ref()
            .and_then(|id| snapshot.benchmark_set(id))
        {
            push("benchmark_set", set.benchmark_set_id.as_str(), set.version)?;
        }
    }
    if let Some(capture) = &snapshot.content().configuration_capture {
        push(
            "configuration_activation",
            &snapshot.content().configuration.config_id,
            capture.activation.revision,
        )?;
        for override_ in &capture.applied_overrides {
            push(
                "policy_override",
                override_.id.as_str(),
                Version::new(snapshot.content().configuration.version).map_err(contract_error)?,
            )?;
        }
    }
    push(
        "configuration",
        &snapshot.content().configuration.config_id,
        Version::new(snapshot.content().configuration.version).map_err(contract_error)?,
    )?;
    for curve in &snapshot.content().curves {
        push("curve", curve.curve_id.as_str(), curve.version)?;
    }
    for curve in &snapshot.content().curve_recipes {
        push("curve", curve.curve_id.as_str(), curve.version)?;
    }
    if let Some(calibration) = &snapshot.content().calibration {
        push(
            "calibration",
            calibration.graph_id.as_str(),
            calibration.version,
        )?;
        for pin in &calibration.curves {
            push("curve", pin.curve_id.as_str(), pin.version)?;
        }
    }
    let mut exact = BTreeMap::new();
    for version in versions
        .into_iter()
        .chain(
            snapshot
                .content()
                .curves
                .iter()
                .flat_map(|c| c.input_versions.iter().cloned()),
        )
        .chain(
            snapshot
                .content()
                .curve_recipes
                .iter()
                .flat_map(|c| c.input_versions.iter().cloned()),
        )
        .chain(
            snapshot
                .content()
                .calibration
                .iter()
                .flat_map(|c| c.input_versions.iter().cloned()),
        )
    {
        let key = (version.kind.clone(), version.id.clone());
        if exact
            .insert(key, version.version)
            .is_some_and(|prior| prior != version.version)
        {
            return Err(TraitError::Conflict(
                "incompatible consumed input revisions",
            ));
        }
    }
    Ok(exact
        .into_iter()
        .map(|((kind, id), version)| InputVersion { kind, id, version })
        .collect())
}
fn contract_error(_: ContractError) -> TraitError {
    TraitError::InvalidInput("invalid canonical valuation value/lineage")
}

fn snapshot_mid_value(
    output: &convex_ports::output::BondQuoteOutput,
    measure: &str,
) -> Result<Option<MeasureValue>, TraitError> {
    let model =
        |value: rust_decimal::Decimal| ExactDecimal::new(value).to_model().map_err(contract_error);
    let years = |value: Option<rust_decimal::Decimal>| {
        value
            .map(|value| {
                model(value).map(|finite| MeasureValue::ModelYears(ModelYears::new(finite)))
            })
            .transpose()
    };
    let bps = |value: Option<rust_decimal::Decimal>| {
        value
            .map(|value| {
                model(value).map(|finite| MeasureValue::ModelBps(ModelBasisPoints::new(finite)))
            })
            .transpose()
    };
    let years_squared = |value: Option<rust_decimal::Decimal>| {
        value
            .map(|value| {
                model(value)
                    .map(|finite| MeasureValue::ModelYearsSquared(ModelYearsSquared::new(finite)))
            })
            .transpose()
    };
    let price = |value: Option<rust_decimal::Decimal>| {
        value
            .map(|value| {
                model(value).map(|finite| MeasureValue::ModelPrice(ModelPricePer100::new(finite)))
            })
            .transpose()
    };
    // Observed clean marks retain exact contract precision. Calculated outputs stay model values;
    // no hidden ledger/output rounding is performed to force an exact dirty-price decimal sum.
    match measure {
        "clean_price_mid" => Ok(output
            .clean_price_mid
            .map(|value| MeasureValue::Price(PricePer100::new(ExactDecimal::new(value))))),
        "dirty_price_mid" => match (output.clean_price_mid, output.accrued_interest) {
            (Some(clean), Some(accrued)) => {
                Ok(Some(MeasureValue::ModelPrice(ModelPricePer100::new(
                    FiniteF64::new(model(clean)?.get() + model(accrued)?.get())
                        .map_err(contract_error)?,
                ))))
            }
            _ => Ok(None),
        },
        "accrued_interest" => price(output.accrued_interest),
        "ytm_mid" => output
            .ytm_mid
            .map(|value| {
                model(value).map(|finite| MeasureValue::ModelRate(ModelDecimalRate::new(finite)))
            })
            .transpose(),
        "modified_duration_mid" => years(output.modified_duration),
        "convexity_mid" => years_squared(output.convexity),
        // Loss per 100 face for +1 bp of the bond's own quoted yield; not factor risk.
        "dv01_mid" => price(output.dv01),
        "z_spread_mid" => bps(output.z_spread_mid),
        "g_spread_mid" => bps(output.g_spread_mid),
        // Over the configuration's swap curve; absent without that role.
        "i_spread_mid" => bps(output.i_spread_mid),
        "asw_mid" => bps(output.asw_mid),
        "macaulay_duration_mid" => years(output.macaulay_duration),
        // Curve risk: central differences of the discount curve at the solved, frozen Z-spread.
        "effective_duration_mid" => years(output.effective_duration),
        "effective_convexity_mid" => years_squared(output.effective_convexity),
        "spread_duration_mid" => years(output.spread_duration),
        // Loss per 100 face for +1 bp in continuous zero rates, and in the Z-spread.
        "pv01_mid" => price(output.pv01),
        "cs01_mid" => price(output.cs01),
        // `krd_10y_mid` reads the router's "10Y" triangular zero duration.
        key if curve_risk_measure(key) => {
            let label = key
                .strip_prefix("krd_")
                .and_then(|key| key.strip_suffix("_mid"))
                .map(str::to_ascii_uppercase);
            years(output.key_rate_durations.as_ref().and_then(|durations| {
                durations
                    .iter()
                    .find(|(tenor, _)| Some(tenor) == label.as_ref())
                    .map(|(_, value)| *value)
            }))
        }
        _ => Ok(None),
    }
}

// The ports suites' snapshot fixtures, as the worker's tests load them.
#[cfg(test)]
#[allow(clippy::duplicate_mod)]
#[path = "../../convex-ports/tests/support/snapshot_fixtures.rs"]
mod snapshot_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use convex_ports::quote_event::{
        DataState, FieldUpdate, QuoteChange, QuoteEventKind, QuoteQuality, QuoteUnit, QuoteValue,
    };

    const START_MS: i64 = 1_790_000_000_000;
    const DAY_MS: i64 = 86_400_000;

    /// A callable on HW1F trees reads the discount curve whatever it is quoted by, so in
    /// Live a curve revision values it again from an OAS as from a price.
    #[test]
    fn a_callable_on_trees_reads_the_discount_curve_by_price_or_oas() {
        let curves = convex_ports::storage::AnalyticsCurves::default();
        let model = crate::optional_valuation::OPTIONAL_SNAPSHOT_MODEL;
        for method in [None, Some(PricingMethod::Oas {})] {
            assert_eq!(
                read_curve_ids(model, &[], method.as_ref(), &curves),
                [curves.discount_curve.as_str()]
            );
        }
    }

    fn key(instrument: &str) -> QuoteKey {
        QuoteKey {
            instrument_id: convex_core::ids::InstrumentId::new(instrument),
            source: "feed-a".into(),
        }
    }

    fn requirement(instrument: &str) -> QuoteRequirement {
        QuoteRequirement {
            key: key(instrument),
            fields: vec![QuoteField::MidPrice],
            max_age_ms: 60_000,
        }
    }

    fn image(instrument: &str, sequence: u64, at_ms: i64) -> QuoteEvent {
        QuoteEvent {
            schema_version: 1,
            key: key(instrument),
            session: "session-1".into(),
            sequence,
            source_time_ms: at_ms,
            receive_time_ms: at_ms,
            quality: QuoteQuality::Live,
            entitled: true,
            entitlement_tags: vec![],
            evidence_reference: None,
            event: QuoteEventKind::Image {
                fields: vec![QuoteChange {
                    field: QuoteField::MidPrice,
                    update: FieldUpdate::Set(QuoteValue {
                        value: rust_decimal::Decimal::new(9_925 + sequence as i64, 2),
                        unit: QuoteUnit::PricePer100,
                    }),
                }],
            },
        }
    }

    /// The entries `quotes` returns, and whether each is the one the previous call
    /// returned. Each must equal the quote built from the state without a cache, its
    /// fields in the snapshot's order.
    fn resolve(
        ingested: &mut IngestedQuotes,
        requirements: &[QuoteRequirement],
        capture_ms: i64,
        previous: &[SharedQuote],
    ) -> (Vec<SharedQuote>, Vec<bool>) {
        let quotes = ingested.quotes(requirements, capture_ms);
        for (quote, requirement) in quotes.iter().zip(requirements) {
            let mut requirement = requirement.clone();
            requirement.fields.sort();
            let uncached = SnapshotQuote {
                delivery_watermark: ingested.delivered.get(&requirement.key).cloned(),
                image: ingested.assembler.snapshot(&requirement.key, capture_ms),
                requirement,
            };
            assert_eq!(**quote, uncached);
        }
        let shared = quotes
            .iter()
            .map(|quote| previous.iter().any(|p| p.ptr_eq(quote)))
            .collect();
        (quotes, shared)
    }

    /// A Live tick shares every quote entry that did not change, so its digest
    /// and checks are kept. An entry is built again when its key ingests an event that
    /// is not a retry or out of order, its requirement changes, its state at the cutoff
    /// changes, or a full key set's transport gap blocks every key. A requirement whose
    /// fields are out of order is still shared. Keys the selection drops are forgotten.
    #[test]
    fn ingested_quotes_rebuild_only_the_entries_that_changed() {
        let mut ingested = IngestedQuotes::new(SnapshotBounds::Standard).unwrap();
        let mut requirements = vec![requirement("BOND-A"), requirement("BOND-B")];
        ingested.ingest(image("BOND-A", 1, START_MS), START_MS);
        ingested.ingest(image("BOND-B", 1, START_MS), START_MS);
        let (first, _) = resolve(&mut ingested, &requirements, START_MS, &[]);
        let (second, shared) = resolve(&mut ingested, &requirements, START_MS + 1, &first);
        assert_eq!(shared, [true, true]);

        // One key's event.
        ingested.ingest(image("BOND-A", 2, START_MS + 2), START_MS + 2);
        let (third, shared) = resolve(&mut ingested, &requirements, START_MS + 2, &second);
        assert_eq!(shared, [false, true]);

        // A retry of that event and an earlier one change nothing.
        ingested.ingest(image("BOND-A", 2, START_MS + 2), START_MS + 3);
        ingested.ingest(image("BOND-A", 1, START_MS), START_MS + 3);
        let (third, shared) = resolve(&mut ingested, &requirements, START_MS + 3, &third);
        assert_eq!(shared, [true, true]);

        // A day after B's observation, but not A's: B's state is stale.
        let later = START_MS + DAY_MS + 1;
        let (fourth, shared) = resolve(&mut ingested, &requirements, later, &third);
        assert_eq!(shared, [true, false]);
        assert_eq!(fourth[1].image.as_ref().unwrap().state, DataState::Stale);

        // B's requirement.
        requirements[1].max_age_ms = 30_000;
        let (fifth, shared) = resolve(&mut ingested, &requirements, later, &fourth);
        assert_eq!(shared, [true, false]);

        // B's fields out of order: built sorted once, then shared. Fields that sort to
        // another list are another requirement.
        requirements[1].fields = vec![QuoteField::MidPrice, QuoteField::BidPrice];
        let (sorted, shared) = resolve(&mut ingested, &requirements, later, &fifth);
        assert_eq!(shared, [true, false]);
        assert!(sorted[1].requirement.fields.is_sorted());
        let (again, shared) = resolve(&mut ingested, &requirements, later, &sorted);
        assert_eq!(shared, [true, true]);
        requirements[1].fields = vec![
            QuoteField::MidPrice,
            QuoteField::BidPrice,
            QuoteField::BidPrice,
        ];
        let (again, shared) = resolve(&mut ingested, &requirements, later, &again);
        assert_eq!(shared, [true, false]);
        requirements[1].fields = vec![
            QuoteField::MidPrice,
            QuoteField::MidPrice,
            QuoteField::BidPrice,
        ];
        let (fifth, shared) = resolve(&mut ingested, &requirements, later, &again);
        assert_eq!(shared, [true, false]);

        // A selection without B forgets B's entry.
        let (sixth, shared) = resolve(&mut ingested, &requirements[..1], later, &fifth);
        assert_eq!((shared, ingested.entries.len()), (vec![true], 1));

        // The key set is full: the next new key's gap blocks every key.
        for i in 0..MAX_SNAPSHOT_INSTRUMENTS {
            ingested.ingest(image(&format!("BOND-{i:03}"), 1, later), later);
        }
        let (gap, shared) = resolve(&mut ingested, &requirements[..1], later, &sixth);
        assert_eq!(shared, [false]);
        assert_eq!(gap[0].image.as_ref().unwrap().state, DataState::Gap);
    }

    /// A small deterministic generator, so every run follows the same script.
    struct Script(u64);
    impl Script {
        fn next(&mut self, bound: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) % bound
        }
        fn price(&mut self) -> FieldUpdate {
            FieldUpdate::Set(QuoteValue {
                value: rust_decimal::Decimal::new(9_900 + self.next(100) as i64, 2),
                unit: QuoteUnit::PricePer100,
            })
        }
    }

    fn event(
        instrument: &str,
        session: &str,
        sequence: u64,
        times: (i64, i64),
        kind: QuoteEventKind,
    ) -> QuoteEvent {
        let mut event = image(instrument, sequence, times.1);
        event.session = session.into();
        event.source_time_ms = times.0;
        event.event = kind;
        event
    }

    /// A Live selection's list, looking again only at the entries that may have
    /// changed, equals a full walk's at every capture, entry by entry, and shares exactly
    /// the entries the full walk shares, so the snapshots it makes are the same. The
    /// script mixes images, deltas, retries, earlier events, heartbeats, gaps, closures
    /// and resets; captures that hold, advance a little, pass every field's freshness, or
    /// fall below the latest receipts; a capacity overflow; and a new epoch with another
    /// requirement. The requirements are not in snapshot order, and the list is.
    ///
    /// Each tick's snapshot of the list, derived from the last one, is the
    /// snapshot `new` builds, value or error.
    #[test]
    fn live_quotes_equal_a_full_walk_at_every_capture() {
        let policy = IngestionPolicy {
            max_keys: 6,
            stale_after_ms: 1_000,
            future_tolerance_ms: 0,
        };
        let mut live = IngestedQuotes::with_policy(policy).unwrap();
        let mut full = IngestedQuotes::with_policy(policy).unwrap();
        let instruments = ["BOND-C", "BOND-A", "BOND-E", "BOND-B", "BOND-D"];
        let mut requirements: Vec<_> = instruments.iter().map(|i| requirement(i)).collect();
        let mut frame = super::snapshot_fixture::content();
        frame.schema_version = SNAPSHOT_SCHEMA;
        (frame.required_curves, frame.curves) = (Vec::new(), Vec::new());
        let reference = frame.references[0].clone();
        frame.references = SnapshotReferences::new(
            instruments
                .iter()
                .map(|id| {
                    let mut reference = reference.clone();
                    reference.record.instrument_id = convex_core::ids::InstrumentId::new(*id);
                    let bond = reference.record.bond.as_mut().unwrap();
                    bond.instrument_id = reference.record.instrument_id.clone();
                    reference
                })
                .collect(),
        );
        let mut snapshot: Option<MarketSnapshot> = None;
        let (mut derived, mut refused) = (0, 0);
        let mut script = Script(17);
        let mut sequences = [0u64; 5];
        let mut sessions = [0u32; 5];
        let mut sent: Vec<Option<QuoteEvent>> = vec![None; 5];
        let (mut capture, mut epoch) = (START_MS, 1);
        let (mut live_last, mut full_last) = (Vec::new(), Vec::new());
        let (mut looked_again, mut walked, mut stale, mut fresh) = (0, 0, 0, 0);
        for step in 0..4_000 {
            for _ in 0..script.next(4) {
                let k = script.next(5) as usize;
                let session = format!("session-{}", sessions[k]);
                let times = (capture - script.next(600) as i64, capture);
                let event = match script.next(10) {
                    0..=2 => {
                        sequences[k] += 1;
                        let mut fields = vec![QuoteChange {
                            field: QuoteField::MidPrice,
                            update: script.price(),
                        }];
                        if script.next(2) == 0 {
                            fields.push(QuoteChange {
                                field: QuoteField::BidPrice,
                                update: script.price(),
                            });
                        }
                        let kind = QuoteEventKind::Image { fields };
                        event(instruments[k], &session, sequences[k], times, kind)
                    }
                    3 | 4 => {
                        sequences[k] += 1;
                        let fields = vec![QuoteChange {
                            field: QuoteField::MidPrice,
                            update: script.price(),
                        }];
                        let kind = QuoteEventKind::Delta { fields };
                        event(instruments[k], &session, sequences[k], times, kind)
                    }
                    5 => match &sent[k] {
                        Some(retry) => retry.clone(),
                        None => continue,
                    },
                    6 => match &sent[k] {
                        Some(earlier) => {
                            let mut earlier = earlier.clone();
                            earlier.sequence = earlier.sequence.saturating_sub(1).max(1);
                            earlier
                        }
                        None => continue,
                    },
                    7 => {
                        let kind = QuoteEventKind::Heartbeat;
                        event(instruments[k], &session, sequences[k], times, kind)
                    }
                    8 => {
                        sequences[k] += 1;
                        let kind = if script.next(2) == 0 {
                            QuoteEventKind::Gap
                        } else {
                            QuoteEventKind::Closed
                        };
                        event(instruments[k], &session, sequences[k], times, kind)
                    }
                    _ => {
                        sessions[k] += 1;
                        sequences[k] = 0;
                        let session = format!("session-{}", sessions[k]);
                        event(instruments[k], &session, 0, times, QuoteEventKind::Reset)
                    }
                };
                sent[k] = Some(event.clone());
                live.ingest(event.clone(), capture);
                full.ingest(event, capture);
            }
            if step == 2_000 {
                // A sixth key fills the key set and a seventh overflows it.
                for (i, other) in ["BOND-X", "BOND-Y"].iter().enumerate() {
                    let event = image(other, i as u64 + 1, capture);
                    live.ingest(event.clone(), capture);
                    full.ingest(event, capture);
                }
            }
            if step == 3_000 {
                epoch += 1;
                requirements[2].max_age_ms = 30_000;
            }
            let list_capture = match script.next(12) {
                0 => capture,
                1 => capture + 1_000 + script.next(500) as i64,
                2 => capture - 1 - script.next(300) as i64,
                _ => capture + script.next(400) as i64,
            };
            capture = capture.max(list_capture);
            let incremental = live
                .last
                .as_ref()
                .is_some_and(|last| last.epoch == epoch && list_capture >= last.capture_ms);
            if incremental {
                looked_again += 1;
            } else {
                walked += 1;
            }
            let live_now = live.live_quotes(&requirements, epoch, list_capture);
            let mut full_now = full.quotes(&requirements, list_capture);
            full_now.sort_by(|a, b| {
                let (a, b) = (&a.requirement.key, &b.requirement.key);
                (a.instrument_id.as_str(), &a.source).cmp(&(b.instrument_id.as_str(), &b.source))
            });
            assert_eq!(live_now.len(), full_now.len());
            for (i, (now, walk)) in live_now.iter().zip(&full_now).enumerate() {
                assert_eq!(**now, **walk, "step {step}, position {i}");
                if let (Some(before), Some(walked_before)) = (live_last.get(i), full_last.get(i)) {
                    assert_eq!(
                        now.ptr_eq(before),
                        walk.ptr_eq(walked_before),
                        "sharing at step {step}, position {i}"
                    );
                }
                match walk.image.as_ref().map(|image| image.state) {
                    Some(DataState::Stale) => stale += 1,
                    Some(DataState::Live) => fresh += 1,
                    _ => {}
                }
            }
            let cutoff = UtcTimestamp::from_datetime(
                chrono::DateTime::from_timestamp_millis(list_capture).unwrap(),
            )
            .unwrap();
            let content = SnapshotContent {
                as_of: cutoff,
                known_at: cutoff,
                generation: Version::new(step + 1).unwrap(),
                quotes: live_now.clone(),
                ..frame.clone()
            };
            let built = MarketSnapshot::new(content.clone());
            if let Some(last) = &snapshot {
                match (last.follows(&content), &built) {
                    (true, Ok(_)) => derived += 1,
                    (true, Err(_)) => refused += 1,
                    _ => {}
                }
                let next = last.next(content);
                let id = |result: &Result<MarketSnapshot, TraitError>| {
                    format!("{:?}", result.as_ref().map(|s| s.id().clone()))
                };
                assert_eq!(id(&next), id(&built), "snapshot at step {step}");
                if let (Ok(next), Ok(built)) = (&next, &built) {
                    assert_eq!(next, built, "snapshot at step {step}");
                }
                if let Ok(next) = next {
                    snapshot = Some(next);
                }
            } else {
                snapshot = built.ok();
            }
            (live_last, full_last) = (live_now, full_now);
        }
        assert!(
            derived > 2_000 && refused > 100,
            "{derived} derived, {refused} refused"
        );
        assert!(looked_again > 3_000, "{looked_again} incremental lists");
        assert!(walked > 10, "{walked} full walks");
        assert!(
            stale > 1_000 && fresh > 1_000,
            "{stale} stale, {fresh} live"
        );
    }
}
