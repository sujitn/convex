//! Snapshot assembly and valuation use cases. All numerical inputs/time are supplied.
#![forbid(unsafe_code)]

use convex_core::contracts::*;
use convex_ports::{
    fixing::FixingRepository,
    quote_event::{QuoteEvent, QuoteField},
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

pub use convex_ports::configuration::{InstrumentPin, SnapshotAssembly};

/// Assemble then publish one complete immutable manifest. This creates no scheduler or background task.
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
        let configuration = self
            .configurations
            .get_version(
                request.configuration.id.as_str(),
                request.configuration.version.get(),
            )
            .await?
            .ok_or(TraitError::InvalidInput(
                "missing exact snapshot configuration",
            ))?;
        self.resolve_with_configuration(request, events, configuration)
            .await
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
            references.push(ResolvedBondReference {
                record,
                issuer,
                calendar,
                convention,
            });
        }
        let mut assembler = MarketDataAssembler::new(IngestionPolicy {
            max_keys: MAX_SNAPSHOT_INSTRUMENTS,
            stale_after_ms: 86_400_000,
            future_tolerance_ms: 0,
        })?;
        let capture = request
            .as_of
            .max(request.known_at)
            .datetime()
            .timestamp_millis();
        let mut delivered = HashMap::new();
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
            let outcome = assembler.ingest(event.clone(), capture);
            if !matches!(
                outcome.outcome,
                IngestionOutcome::Duplicate | IngestionOutcome::OutOfOrder
            ) {
                delivered.insert(event.key.clone(), event.clone());
            }
        }
        let quotes = request
            .quotes
            .iter()
            .map(|requirement| SnapshotQuote {
                requirement: requirement.clone(),
                delivery_watermark: delivered.remove(&requirement.key),
                image: assembler.snapshot(&requirement.key, capture),
            })
            .collect();
        let mut fixings = Vec::with_capacity(request.fixings.len());
        for key in &request.fixings {
            fixings.push(SnapshotFixing {
                key: key.clone(),
                fixing: self.fixings.get_at(key, request.known_at).await?,
            });
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
    pub fn value(
        &self,
        snapshot: &MarketSnapshot,
        context: &ValuationContext,
        request_id: RequestId,
        calculated_at: UtcTimestamp,
    ) -> Result<SnapshotValuationResult, TraitError> {
        context.validate_snapshot(snapshot)?;
        let recipe_curves =
            crate::curve_replay::replay_snapshot_curves(snapshot).map_err(TraitError::from)?;
        let spec = context.spec();
        if calculated_at < spec.known_at || calculated_at < spec.valuation_time {
            return Err(TraitError::InvalidInput(
                "calculation precedes supplied information/valuation time",
            ));
        }
        if spec.build_id != self.build_id
            || spec.model_id.as_str() != BOND_SNAPSHOT_MODEL
            || spec.numerical_settings.policy_id.as_str() != BOND_NUMERICAL_POLICY
            || spec.scenario != ValuationScenario::Base
        {
            return Err(TraitError::Unsupported(
                "unsupported build/model/numerical/scenario policy",
            ));
        }
        if let Some(cached) = self.cache.get(context)? {
            let envelope = cached.envelope();
            return SnapshotValuationResult::new(
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
            );
        }
        let ValuationSubject::InstrumentVersion { instrument_id, .. } = &spec.subject else {
            return Err(TraitError::Unsupported("trade economics unavailable"));
        };
        let reference = snapshot
            .content()
            .references
            .iter()
            .find(|r| r.record.instrument_id.as_str() == instrument_id.as_str())
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
        let selected = snapshot
            .content()
            .quotes
            .iter()
            .filter(|q| q.requirement.key.instrument_id.as_str() == instrument_id.as_str())
            .collect::<Vec<_>>();
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
        let input = PricingInput {
            bond: bond.clone(),
            settlement_date: spec.settlement_date.date(),
            market_price_bid: mark(QuoteField::BidPrice),
            market_price_mid: mark(QuoteField::MidPrice),
            market_price_ask: mark(QuoteField::AskPrice),
            discount_curve: curve(&configuration.analytics_curves.discount_curve)?,
            benchmark_curve: configuration
                .analytics_curves
                .swap_curve
                .as_ref()
                .map(curve)
                .transpose()?
                .flatten(),
            government_curve: None,
            volatility: None,
            bid_ask_config: configuration.bid_ask_spread.clone(),
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
                            quality: snapshot.quality().quality,
                            method: spec.model_id.clone(),
                            source_observation_id: None,
                        },
                    );
                }
                None => {
                    let supported = matches!(
                        measure.as_str(),
                        "clean_price_mid" | "dirty_price_mid" | "accrued_interest" | "ytm_mid"
                    );
                    errors.push(StructuredError::new(if !supported { ErrorCode::InvalidInput } else if pricing_failed { ErrorCode::SolverLimit } else if input.market_price_mid.is_none() { ErrorCode::MissingMarketData } else { ErrorCode::SolverLimit }, Some(measure.as_str().into()), if supported { "Required mark or numerical calculation is unavailable; no value substituted" } else { "Requested measure is outside this snapshot valuation slice" }, false).map_err(contract_error)?);
                }
            }
        }
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
            snapshot.quality().quality,
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
        "accrued_interest" => output
            .accrued_interest
            .map(|value| {
                model(value).map(|finite| MeasureValue::ModelPrice(ModelPricePer100::new(finite)))
            })
            .transpose(),
        "ytm_mid" => output
            .ytm_mid
            .map(|value| {
                model(value).map(|finite| MeasureValue::ModelRate(ModelDecimalRate::new(finite)))
            })
            .transpose(),
        _ => Ok(None),
    }
}
