//! Snapshot assembly and valuation use cases. All numerical inputs/time are supplied.
#![forbid(unsafe_code)]

use convex_core::contracts::*;
use convex_ports::{
    configuration::ConfigurationPin,
    fixing::FixingRepository,
    quote_event::{QuoteEvent, QuoteField, QuoteKey},
    reference_master::{ReferenceLookup, ResolvedBondReference},
    reference_repository::ReferenceRepository,
    snapshot::*,
    storage::{ConfigStore, PricingSpec},
    TraitError,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
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
/// Measures read from curves: spreads and curve risk. The others read the mid quote's
/// own yield, so a plan that asks for none of these prices without curves.
fn curve_measure(measure: &str) -> bool {
    matches!(measure, "z_spread_mid" | "i_spread_mid" | "asw_mid") || curve_risk_measure(measure)
}
/// The configured curves one valuation reads (D58): a callable's discount curve, a
/// floating rate note's discount and reference curves, and another bond's discount and
/// swap curves when it asks for a spread or curve-risk measure. No snapshot valuation
/// reads the government curve.
fn read_curves<'c>(
    spec: &ValuationContextSpec,
    curves: &'c convex_ports::storage::AnalyticsCurves,
) -> Vec<&'c str> {
    let model = spec.model_id.as_str();
    let second = if model == crate::optional_valuation::OPTIONAL_SNAPSHOT_MODEL {
        None
    } else if model == crate::floating_valuation::FLOATING_SNAPSHOT_MODEL {
        curves.frn_reference_curve.as_ref()
    } else if spec.measures.iter().any(|m| curve_measure(m.as_str())) {
        curves.swap_curve.as_ref()
    } else {
        return Vec::new();
    };
    std::iter::once(&curves.discount_curve)
        .chain(second)
        .map(|id| id.as_str())
        .collect()
}
/// The snapshot inputs one valuation context reads (D58).
pub(crate) fn valuation_inputs<'a>(
    snapshot: &'a MarketSnapshot,
    spec: &ValuationContextSpec,
) -> Result<convex_ports::snapshot::InputSet<'a>, TraitError> {
    let ValuationSubject::InstrumentVersion { instrument_id, .. } = &spec.subject else {
        return Err(TraitError::Unsupported("trade economics unavailable"));
    };
    let curves = read_curves(spec, &snapshot.content().configuration.analytics_curves);
    Ok(snapshot.inputs(instrument_id.as_str(), &curves))
}
/// Whether one valuation context reads the same inputs in `snapshot` as in `previous`
/// (D55). The caller compares the frame (`MarketSnapshot::same_frame`) first, so the
/// configured curves are the same in both.
pub(crate) fn same_valuation_inputs(
    snapshot: &MarketSnapshot,
    previous: &MarketSnapshot,
    spec: &ValuationContextSpec,
) -> Result<bool, TraitError> {
    let ValuationSubject::InstrumentVersion { instrument_id, .. } = &spec.subject else {
        return Err(TraitError::Unsupported("trade economics unavailable"));
    };
    let curves = read_curves(spec, &snapshot.content().configuration.analytics_curves);
    Ok(snapshot.same_inputs(previous, instrument_id.as_str(), &curves))
}
/// Quality of the inputs one valuation context reads, never the whole snapshot's.
pub(crate) fn valuation_quality(
    snapshot: &MarketSnapshot,
    spec: &ValuationContextSpec,
) -> Result<convex_ports::snapshot::SnapshotQuality, TraitError> {
    Ok(snapshot.quality_of(&valuation_inputs(snapshot, spec)?))
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
    /// Resolve without publishing; shared by dry-run configuration validation.
    pub async fn resolve(
        &self,
        request: &SnapshotAssembly,
        events: &[QuoteEvent],
    ) -> Result<MarketSnapshot, TraitError> {
        validate_request(request, events)?;
        let configuration = self.configuration(request).await?;
        self.resolve_with_configuration(request, events, configuration)
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
    /// Resolve supplied immutable quantitative content without staging or publishing it.
    /// The same reference, curve and quality checks govern actual assembly and dry runs.
    pub async fn resolve_with_configuration(
        &self,
        request: &SnapshotAssembly,
        events: &[QuoteEvent],
        configuration: convex_ports::storage::BondPricingConfig,
    ) -> Result<MarketSnapshot, TraitError> {
        validate_request(request, events)?;
        let references = self.references(request, &configuration).await?;
        let capture = capture_ms(request);
        let mut quotes = IngestedQuotes::new()?;
        for event in events {
            if event.source_time_ms > request.as_of.datetime().timestamp_millis()
                || event.receive_time_ms > request.known_at.datetime().timestamp_millis()
                || !request
                    .quotes
                    .iter()
                    .any(|requirement| requirement.key == event.key)
            {
                continue;
            }
            quotes.ingest(event.clone(), capture);
        }
        self.build(request, configuration, references, quotes.quotes(request))
            .await
    }
    async fn references(
        &self,
        request: &SnapshotAssembly,
        configuration: &convex_ports::storage::BondPricingConfig,
    ) -> Result<Vec<ResolvedBondReference>, TraitError> {
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
            references.push(ResolvedBondReference {
                record,
                issuer,
                calendar,
                convention,
                floating,
            });
        }
        Ok(references)
    }
    /// Resolve the exact configuration and the pinned references `request` selects, at its
    /// cutoff, for reuse by later snapshots of the same selection (D56).
    pub(crate) async fn pin(&self, request: &SnapshotAssembly) -> Result<PinnedInputs, TraitError> {
        validate_request(request, &[])?;
        let configuration = self.configuration(request).await?;
        let references = self.references(request, &configuration).await?;
        Ok(PinnedInputs {
            activation: activation_revision(request),
            configuration_pin: request.configuration.clone(),
            instruments: request.instruments.clone(),
            configuration,
            references,
        })
    }
    /// Resolve over quote state the caller already ingested and inputs it pinned, without
    /// publishing. Live mode keeps both in memory; market data is not stored. Fixings are
    /// read at this request's cutoff.
    pub(crate) async fn resolve_ingested(
        &self,
        request: &SnapshotAssembly,
        pinned: &PinnedInputs,
        quotes: &IngestedQuotes,
    ) -> Result<MarketSnapshot, TraitError> {
        validate_request(request, &[])?;
        if !pinned.serves(request) {
            return Err(TraitError::Conflict(
                "pinned inputs belong to another selection",
            ));
        }
        self.build(
            request,
            pinned.configuration.clone(),
            pinned.references.clone(),
            quotes.quotes(request),
        )
        .await
    }
    async fn build(
        &self,
        request: &SnapshotAssembly,
        configuration: convex_ports::storage::BondPricingConfig,
        references: Vec<ResolvedBondReference>,
        quotes: Vec<SnapshotQuote>,
    ) -> Result<MarketSnapshot, TraitError> {
        let keys = required_fixings(request, &references)?;
        let mut fixings = Vec::with_capacity(keys.len());
        for key in keys {
            let fixing = self.fixings.get_at(&key, request.known_at).await?;
            fixings.push(SnapshotFixing { key, fixing });
        }
        let snapshot = MarketSnapshot::new(SnapshotContent {
            schema_version: if request.calibration.is_some() {
                3
            } else if request.curve_recipes.is_empty() {
                1
            } else {
                2
            },
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
        })?;
        Ok(snapshot)
    }
}

/// Quote state ingested once per event in delivery order. Replay rebuilds it from a
/// trace for each snapshot; Live keeps one instance for the life of the process.
pub(crate) struct IngestedQuotes {
    assembler: MarketDataAssembler,
    delivered: HashMap<QuoteKey, QuoteEvent>,
}
impl IngestedQuotes {
    pub(crate) fn new() -> Result<Self, TraitError> {
        Ok(Self {
            assembler: MarketDataAssembler::new(IngestionPolicy {
                max_keys: MAX_SNAPSHOT_INSTRUMENTS,
                stale_after_ms: 86_400_000,
                future_tolerance_ms: 0,
            })?,
            delivered: HashMap::new(),
        })
    }
    /// Apply one event at the capture time it was accepted under.
    pub(crate) fn ingest(&mut self, event: QuoteEvent, capture_ms: i64) {
        let outcome = self.assembler.ingest(event.clone(), capture_ms);
        if !matches!(
            outcome.outcome,
            IngestionOutcome::Duplicate | IngestionOutcome::OutOfOrder
        ) {
            self.delivered.insert(event.key.clone(), event);
        }
    }
    /// Latest delivery and retained image for each requirement, aged at the request cutoff.
    fn quotes(&self, request: &SnapshotAssembly) -> Vec<SnapshotQuote> {
        let capture = capture_ms(request);
        request
            .quotes
            .iter()
            .map(|requirement| SnapshotQuote {
                requirement: requirement.clone(),
                delivery_watermark: self.delivered.get(&requirement.key).cloned(),
                image: self.assembler.snapshot(&requirement.key, capture),
            })
            .collect()
    }
}

/// The pricing configuration and pinned references of one selection, resolved once at
/// the cutoff of its first snapshot (D56). Every record is an exact pinned version, so
/// none changes while the selection holds; each snapshot still checks them at its own
/// cutoff (`MarketSnapshot::new`).
pub(crate) struct PinnedInputs {
    activation: Option<Version>,
    configuration_pin: ConfigurationPin,
    instruments: Vec<InstrumentPin>,
    configuration: convex_ports::storage::BondPricingConfig,
    references: Vec<ResolvedBondReference>,
}
impl PinnedInputs {
    /// Whether these inputs were resolved for `request`'s activation, configuration and
    /// instrument pins.
    pub(crate) fn serves(&self, request: &SnapshotAssembly) -> bool {
        self.activation == activation_revision(request)
            && self.configuration_pin == request.configuration
            && self.instruments == request.instruments
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

fn validate_request(request: &SnapshotAssembly, events: &[QuoteEvent]) -> Result<(), TraitError> {
    if request.instruments.is_empty()
        || request.instruments.len() > MAX_SNAPSHOT_INSTRUMENTS
        || request.quotes.len() > MAX_SNAPSHOT_INSTRUMENTS
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

/// Checked application result. Canonical numeric envelope remains compatible with CVX-007.
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
            cache_key: context.cache_key()?,
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
        if self.0.cache_key != self.0.context.cache_key()?
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
}
impl SnapshotValuator {
    /// Composition root supplies actual build identity and an explicit bounded cache.
    pub fn new(build_id: ModelId, cache_capacity: usize) -> Result<Self, TraitError> {
        Ok(Self {
            build_id,
            cache: ValuationCache::new(cache_capacity)?,
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
    /// [`Self::value_built`] for a caller that already derived the instrument's quality
    /// (D58), so one valuation derives it once.
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
        if crate::optional_valuation::product_model(bond) != spec.model_id.as_str() {
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
        if !selected[0]
            .requirement
            .fields
            .contains(&QuoteField::MidPrice)
        {
            return Err(TraitError::InvalidInput(
                "mid valuation must declare a required mid-price field",
            ));
        }
        let quote = selected[0].image.as_ref();
        let mark = |field| {
            quote
                .and_then(|q| q.fields.get(&field))
                .map(|value| value.value.value)
        };
        let (results, errors) = if spec.model_id.as_str()
            == crate::optional_valuation::OPTIONAL_SNAPSHOT_MODEL
        {
            callable_measures(
                snapshot,
                spec,
                reference,
                recipe_curves,
                mark(QuoteField::MidPrice),
                quality,
            )?
        } else if spec.model_id.as_str() == crate::floating_valuation::FLOATING_SNAPSHOT_MODEL {
            floating_measures(
                snapshot,
                spec,
                reference,
                recipe_curves,
                mark(QuoteField::MidPrice),
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
            let input = PricingInput {
                bond: bond.clone(),
                settlement_date: spec.settlement_date.date(),
                market_price_bid: None,
                market_price_mid: mark(QuoteField::MidPrice),
                market_price_ask: None,
                discount_curve: discount.filter(|_| curves),
                benchmark_curve: benchmark.filter(|_| curves),
                government_curve: None,
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
                        let supported = RUNTIME_MEASURES.contains(&measure.as_str());
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

/// The plan's explicit fixings plus each floating rate note's observations that are due by
/// the cutoff, derived from its built contract (D33). A note the adapter cannot build adds
/// none; its valuation reports why.
fn required_fixings(
    request: &SnapshotAssembly,
    references: &[ResolvedBondReference],
) -> Result<Vec<convex_ports::fixing::FixingKey>, TraitError> {
    let as_of = convex_core::Date::from(request.as_of.datetime().date_naive());
    let mut keys = request.fixings.iter().cloned().collect::<BTreeSet<_>>();
    for reference in references.iter().filter(|r| r.floating.is_some()) {
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

/// Floating rate note measures at the observed clean mid, from `convex::library` (D26, D34).
fn floating_measures(
    snapshot: &MarketSnapshot,
    spec: &ValuationContextSpec,
    reference: &ResolvedBondReference,
    recipe_curves: &convex_curves::contracts::CurveBuildOutput,
    clean_mid: Option<rust_decimal::Decimal>,
    quality: DataQuality,
) -> Result<Measures, TraitError> {
    use crate::optional_valuation::{failure, Failure};
    let configuration = &snapshot.content().configuration;
    let bond = reference
        .record
        .bond
        .as_ref()
        .ok_or(TraitError::InvalidInput("tombstone reference"))?;
    let clean = clean_mid.ok_or_else(|| {
        failure(
            ErrorCode::MissingMarketData,
            "Required mid mark is unavailable",
        )
    });
    let quoted = clean.clone().and_then(|clean| {
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
            clean,
        )
    });
    let finite = |value: f64| {
        FiniteF64::new(value)
            .map_err(|_| failure(ErrorCode::InvalidInput, "nonfinite floating measure"))
    };
    let mut results = BTreeMap::new();
    let mut errors = Vec::new();
    for measure in &spec.measures {
        let value = quoted.as_ref().map_err(Clone::clone).and_then(|quote| {
            let clean = clean.clone()?;
            Ok(match measure.as_str() {
                "clean_price_mid" => {
                    MeasureValue::Price(PricePer100::new(ExactDecimal::new(clean)))
                }
                "dirty_price_mid" => {
                    MeasureValue::ModelPrice(ModelPricePer100::new(finite(quote.dirty)?))
                }
                "accrued_interest" => {
                    MeasureValue::ModelPrice(ModelPricePer100::new(finite(quote.accrued)?))
                }
                // Over the frozen projection, discounted on the collateral curve.
                "discount_margin_mid" => MeasureValue::ModelBps(ModelBasisPoints::new(finite(
                    quote.discount_margin_bps,
                )?)),
                _ => {
                    return Err(failure(
                        ErrorCode::InvalidInput,
                        "Requested measure is outside this snapshot valuation slice",
                    ))
                }
            })
        });
        match value {
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

/// Callable measures at the observed clean mid, from `convex::library` (D26). A plan measure
/// the optional model does not publish never reaches this context (D29).
fn callable_measures(
    snapshot: &MarketSnapshot,
    spec: &ValuationContextSpec,
    reference: &ResolvedBondReference,
    recipe_curves: &convex_curves::contracts::CurveBuildOutput,
    clean_mid: Option<rust_decimal::Decimal>,
    quality: DataQuality,
) -> Result<Measures, TraitError> {
    use crate::optional_valuation::{self as optional, Failure};
    let failure = |code, message: &str| Failure {
        code,
        message: message.into(),
    };
    let configuration = &snapshot.content().configuration;
    let bond = reference
        .record
        .bond
        .as_ref()
        .ok_or(TraitError::InvalidInput("tombstone reference"))?;
    let clean = clean_mid.ok_or_else(|| {
        failure(
            ErrorCode::MissingMarketData,
            "Required mid mark is unavailable",
        )
    });
    let quoted = clean.clone().and_then(|clean| {
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
        let convex_ports::reference_master::MasterDefinition::Calendar(calendar) =
            &reference.calendar.definition
        else {
            return Err(failure(ErrorCode::InvalidInput, "wrong calendar resource"));
        };
        optional::value_callable(
            bond,
            calendar,
            reference.calendar.version,
            &recipes,
            key,
            configuration,
            spec.settlement_date.date(),
            snapshot.content().known_at,
            clean,
        )
    });
    let finite = |value: f64| {
        FiniteF64::new(value)
            .map_err(|_| failure(ErrorCode::InvalidInput, "nonfinite callable measure"))
    };
    let mut results = BTreeMap::new();
    let mut errors = Vec::new();
    for measure in &spec.measures {
        let value = quoted.as_ref().map_err(Clone::clone).and_then(|quote| {
            let clean = clean.clone()?;
            let risk = || quote.risk.as_ref().map_err(Clone::clone);
            let model = ExactDecimal::new(clean)
                .to_model()
                .map_err(|_| failure(ErrorCode::InvalidInput, "unrepresentable clean mid"))?;
            Ok(match measure.as_str() {
                "clean_price_mid" => {
                    MeasureValue::Price(PricePer100::new(ExactDecimal::new(clean)))
                }
                "dirty_price_mid" => MeasureValue::ModelPrice(ModelPricePer100::new(finite(
                    model.get() + quote.accrued,
                )?)),
                "accrued_interest" => {
                    MeasureValue::ModelPrice(ModelPricePer100::new(finite(quote.accrued)?))
                }
                "oas_mid" => MeasureValue::ModelBps(ModelBasisPoints::new(finite(quote.oas_bps)?)),
                // Parallel discount shift with the OAS frozen.
                "effective_duration_mid" => {
                    MeasureValue::ModelYears(ModelYears::new(finite(risk()?.duration)?))
                }
                "effective_convexity_mid" => MeasureValue::ModelYearsSquared(
                    ModelYearsSquared::new(finite(risk()?.convexity)?),
                ),
                // Loss per 100 face for +1 bp of the discount curve.
                "pv01_mid" => {
                    MeasureValue::ModelPrice(ModelPricePer100::new(finite(risk()?.pv01)?))
                }
                _ => {
                    return Err(failure(
                        ErrorCode::InvalidInput,
                        "Requested measure is outside this snapshot valuation slice",
                    ))
                }
            })
        });
        match value {
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
