//! Immutable resolved information sets with explicit schema-separated content identities.
#![forbid(unsafe_code)]

use async_trait::async_trait;
use convex_core::contracts::{
    BusinessDate, CurveId, DataQuality, FiniteF64, InputVersion, InstrumentId, ModelId, SnapshotId,
    SourceId, UtcTimestamp, Version,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use crate::calibration::SnapshotCalibration;
use crate::fixing::{FixingKey, PublishedFixing};
use crate::quote_event::{
    DataState, QuoteEvent, QuoteEventKind, QuoteField, QuoteKey, QuoteQuality, QuoteSnapshot,
};
use crate::reference_master::{
    validate_resolved_bond, MasterDefinition, ReferenceLookup, ResolvedBondReference,
};
use crate::storage::BondPricingConfig;
use crate::TraitError;

/// Maximum serialized snapshot; adapters must reject growth beyond this bound.
pub const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
/// Maximum selected instruments or quote keys in one information set.
pub const MAX_SNAPSHOT_INSTRUMENTS: usize = 256;
/// Maximum explicitly required fixings in one information set.
pub const MAX_SNAPSHOT_FIXINGS: usize = 1024;
/// Maximum supplied curves in one information set.
pub const MAX_SNAPSHOT_CURVES: usize = 64;
/// Maximum total curve pillars, not a separate allowance for each curve.
pub const MAX_SNAPSHOT_PILLARS: usize = 4096;

/// Stable content identity; domain separation prevents reuse as another record's hash.
pub fn content_digest(domain: &[u8], bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(bytes);
    format!("{:x}", hash.finalize())
}

/// Explicit supported representation of supplied zero curves. Calibration is a separate operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZeroInterpolation {
    /// Linear continuous-zero interpolation.
    Linear,
    /// Existing pure library monotone-convex interpolation.
    MonotoneConvex,
}

/// Explicit long-end policy; no policy is inferred on reconstruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZeroExtrapolation {
    /// Constant continuous-zero boundary value.
    FlatZero,
    /// Constant instantaneous forward beyond the boundary.
    FlatForward,
}

/// One finite ACT/365F year fraction and continuously compounded decimal zero rate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZeroRatePoint {
    /// Explicit positive year fraction; no lossy conversion through integer days.
    pub tenor_years: FiniteF64,
    /// Finite decimal zero rate, including negative rates.
    pub continuous_zero: FiniteF64,
}

/// Versioned supplied continuous-zero ACT/365F curve with full reconstruction policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotCurve {
    /// Internal curve identity; configuration determines its pricing role.
    pub curve_id: CurveId,
    /// Immutable supplied curve revision.
    pub version: Version,
    /// Original reference date.
    pub reference_date: BusinessDate,
    /// Economic source time.
    pub source_time: UtcTimestamp,
    /// Recording/publication time of this immutable curve.
    pub recorded_at: UtcTimestamp,
    /// Provider-neutral origin.
    pub source: SourceId,
    /// Supplied construction/model identity.
    pub model_id: ModelId,
    /// Actual input definition revisions, where this is a calibrated curve.
    pub input_versions: Vec<InputVersion>,
    /// Numerical construction quality; supplied nodes are not automatically certified calibration.
    pub quality: DataQuality,
    /// Explicit curve source/recording age policy, 1..=86,400,000 milliseconds.
    pub max_age_ms: i64,
    /// Interpolation policy.
    pub interpolation: ZeroInterpolation,
    /// Extrapolation policy.
    pub extrapolation: ZeroExtrapolation,
    /// Strictly increasing finite pillars, at least two.
    pub points: Vec<ZeroRatePoint>,
}

impl SnapshotCurve {
    /// Reject invalid/oversized pillars and duplicate consumed definition keys.
    pub fn validate(&self) -> Result<(), TraitError> {
        if !(2..=MAX_SNAPSHOT_PILLARS).contains(&self.points.len())
            || !(1..=86_400_000).contains(&self.max_age_ms)
            || self.source_time > self.recorded_at
            || self.input_versions.len() > 256
            || self.points.iter().any(|p| p.tenor_years.get() <= 0.0)
            || self
                .points
                .windows(2)
                .any(|p| p[0].tenor_years.get() >= p[1].tenor_years.get())
        {
            return Err(TraitError::InvalidInput(
                "invalid supplied zero-curve pillars",
            ));
        }
        let mut seen = BTreeSet::new();
        for input in &self.input_versions {
            if !seen.insert((&input.kind, &input.id)) {
                return Err(TraitError::InvalidInput("duplicate curve input revision"));
            }
        }
        Ok(())
    }
}

/// Complete versioned native curve recipe at an application boundary. Ports
/// retain JSON without depending on pricing crates; the engine must decode and
/// build the entire exact dependency graph before numerical use or publication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotCurveRecipe {
    /// Payload format, currently 1 (the complete native CurveRecipe JSON).
    pub schema_version: u32,
    /// Exact identity matching the native recipe key's id.
    pub curve_id: CurveId,
    /// Supplied immutable record revision; never inferred from a model version string.
    pub version: Version,
    /// Actual economic source timestamp.
    pub source_time: UtcTimestamp,
    /// Actual recording/publication timestamp.
    pub recorded_at: UtcTimestamp,
    /// Provider-neutral provenance.
    pub source: SourceId,
    /// Explicit supplied construction/model revision.
    pub model_id: ModelId,
    /// Original consumed definition revisions; not fabricated by replay.
    pub input_versions: Vec<InputVersion>,
    /// Supplied numerical/data quality, without a calibration certificate inference.
    pub quality: DataQuality,
    /// Source and recording freshness budget, 1..=86,400,000 milliseconds.
    pub max_age_ms: i64,
    /// Full native recipe, including key version, roles, units, model, original
    /// quotes/helper terms/settings and exact dependencies. No sampled nodes.
    pub recipe: serde_json::Value,
}

impl SnapshotCurveRecipe {
    /// Bounded structural admission. Financial validation belongs to the native
    /// engine decoder, which rejects unknown/unsupported economics atomically.
    pub fn validate(&self) -> Result<(), TraitError> {
        if self.schema_version != 1
            || !(1..=86_400_000).contains(&self.max_age_ms)
            || self.source_time > self.recorded_at
            || self.input_versions.len() > 256
            || !self.recipe.is_object()
            || self.recipe.pointer("/key/id").and_then(|v| v.as_str())
                != Some(self.curve_id.as_str())
        {
            return Err(TraitError::InvalidInput(
                "invalid complete curve recipe record",
            ));
        }
        let mut seen = BTreeSet::new();
        for input in &self.input_versions {
            if !seen.insert((&input.kind, &input.id)) {
                return Err(TraitError::InvalidInput("duplicate curve input revision"));
            }
        }
        let mut pending = vec![(&self.recipe, 0)];
        let mut visited = 0;
        while let Some((value, depth)) = pending.pop() {
            visited += 1;
            if visited > 32_768 || depth > 64 {
                return Err(TraitError::CapacityExceeded);
            }
            match value {
                serde_json::Value::Array(items) => {
                    if items.len() > MAX_SNAPSHOT_PILLARS {
                        return Err(TraitError::CapacityExceeded);
                    }
                    pending.extend(items.iter().map(|v| (v, depth + 1)));
                }
                serde_json::Value::Object(items) => {
                    if items.len() > MAX_SNAPSHOT_PILLARS || items.keys().any(|k| k.len() > 8192) {
                        return Err(TraitError::CapacityExceeded);
                    }
                    pending.extend(items.values().map(|v| (v, depth + 1)));
                }
                serde_json::Value::String(text) if text.len() > 8192 => {
                    return Err(TraitError::CapacityExceeded);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Original nodes/observations/helpers/segments counted against the shared
    /// snapshot pillar budget. Dependencies are separately retained records.
    pub fn input_count(&self) -> usize {
        Self::recipe_input_count(&self.recipe)
    }
    pub(crate) fn recipe_input_count(recipe: &serde_json::Value) -> usize {
        let Some(source) = recipe.get("source") else {
            return 0;
        };
        [
            "/Nodes",
            "/MoneyMarket",
            "/Helpers",
            "/MonotoneConvex/nodes",
            "/FlatForward/nodes",
            "/PiecewiseConstant/nodes",
            "/Parametric/observations",
            "/Segmented/segments",
        ]
        .iter()
        .filter_map(|path| source.pointer(path).and_then(|v| v.as_array()))
        .map(Vec::len)
        .sum()
    }
}

/// A required selected source and fields, with a versioned per-product age policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteRequirement {
    /// No implicit provider failover or cross-source bid/ask merge.
    pub key: QuoteKey,
    /// Fields needed for this snapshot's declared use.
    pub fields: Vec<QuoteField>,
    /// Maximum source/receipt age, 1..=86,400,000 UTC milliseconds.
    pub max_age_ms: i64,
}

/// Selected feed state and latest delivery watermark, including explicit loss/closure controls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotQuote {
    /// Required fields and freshness policy.
    pub requirement: QuoteRequirement,
    /// Last delivered eligible event, distinct from the last accepted data sequence.
    pub delivery_watermark: Option<QuoteEvent>,
    /// Retained field observations, original ages and accepted-data watermark.
    pub image: Option<QuoteSnapshot>,
}

/// Missing historic fixings remain explicit in the manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotFixing {
    /// Exact required source/index/date.
    pub key: FixingKey,
    /// Selected published revision, absent when none was known.
    pub fixing: Option<PublishedFixing>,
}

/// Resolved values and policies; no repository handles or mutable current pointers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotContent {
    /// 1 for original zero records, 2 for direct recipes, 3 for product calibration.
    pub schema_version: u32,
    /// Economic market/valuation instant.
    pub as_of: UtcTimestamp,
    /// Information cutoff, with nanosecond precision.
    pub known_at: UtcTimestamp,
    /// Captured application input generation. Zero cannot mean current.
    pub generation: Version,
    /// Exact active pricing configuration, retained rather than fetched on replay.
    #[serde(with = "snapshot_configuration")]
    pub configuration: BondPricingConfig,
    /// Optional atomic active selection and approved exceptions actually applied at these cutoffs.
    /// Omitted on older explicit-pin manifests to preserve their content identities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_capture: Option<crate::configuration::ConfigurationCapture>,
    /// Resolved instrument, issuer, calendar and convention revisions.
    pub references: Vec<ResolvedBondReference>,
    /// Selected quote requirements, values and feed continuity.
    pub quotes: Vec<SnapshotQuote>,
    /// Required known-at fixings, including explicit missing entries.
    pub fixings: Vec<SnapshotFixing>,
    /// Required curve IDs; absent supplied curves are visible quality failures.
    pub required_curves: Vec<CurveId>,
    /// Complete reconstructible curve values, never mutable builder lookups.
    pub curves: Vec<SnapshotCurve>,
    /// Schema-2 complete native recipes. Omitted for original schema-1 hashes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curve_recipes: Vec<SnapshotCurveRecipe>,
    /// Complete original product calibration, requiring schema 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<SnapshotCalibration>,
    /// Maximum skew among required quote source-field timestamps, 0..=86,400,000 ms.
    pub max_source_skew_ms: i64,
}

/// Machine-readable quality reason; identity is a selected dependency, never raw licensed data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotIssue {
    /// Internal dependency identity (quote, fixing or curve).
    pub dependency: String,
    /// Stable quality reason.
    pub reason: SnapshotIssueKind,
}

/// Explicit unavailable/unsafe input states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotIssueKind {
    /// Required observation absent.
    MissingQuote,
    /// Required field cleared or absent.
    MissingField,
    /// Delivery has no accepted image.
    AwaitingImage,
    /// Feed discontinuity.
    Gap,
    /// Feed ended.
    Closed,
    /// Invalid or quarantined input.
    Invalid,
    /// Source or receipt age exceeds policy.
    Stale,
    /// Provider reports delayed observations.
    Delayed,
    /// Indicative/manual/approximate inputs.
    Indicative,
    /// Required observation is not entitled.
    Unauthorized,
    /// Required fixing had not been published/recorded by the cutoff.
    MissingFixing,
    /// Required supplied curve is missing.
    MissingCurve,
    /// Explicit demonstration construction.
    Demo,
    /// Required selected source observations exceed skew policy.
    SourceSkew,
}

/// Derived information-set completeness and quality. It is not an execution permission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotQuality {
    /// Every required input is present (independent of timeliness/entitlements).
    pub complete: bool,
    /// Overall conservative classification.
    pub quality: DataQuality,
    /// Derived reasons; callers cannot stamp their own Valid label.
    pub issues: Vec<SnapshotIssue>,
}

/// The inputs one dependent reads from a snapshot, in snapshot order (D58). Quality over
/// a set judges only these inputs; source skew is measured among its quotes alone.
#[derive(Debug, Clone)]
pub struct InputSet<'a> {
    quotes: &'a [SnapshotQuote],
    fixings: Vec<&'a SnapshotFixing>,
    curves: Vec<&'a CurveId>,
}
impl<'a> InputSet<'a> {
    fn all(content: &'a SnapshotContent) -> Self {
        Self {
            quotes: &content.quotes,
            fixings: content.fixings.iter().collect(),
            curves: content.required_curves.iter().collect(),
        }
    }
    /// Selected quotes read.
    pub fn quotes(&self) -> &'a [SnapshotQuote] {
        self.quotes
    }
    /// Required fixings read.
    pub fn fixings(&self) -> &[&'a SnapshotFixing] {
        &self.fixings
    }
    /// Required curves read.
    pub fn curves(&self) -> &[&'a CurveId] {
        &self.curves
    }
}

/// Verified immutable content-addressed information set, schema 1, 2 or 3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SnapshotWire", into = "SnapshotWire")]
pub struct MarketSnapshot(SnapshotWire);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotWire {
    snapshot_id: SnapshotId,
    content: SnapshotContent,
    quality: SnapshotQuality,
}

impl MarketSnapshot {
    /// Normalize unordered manifests, validate supplied values and derive identity/quality.
    pub fn new(mut content: SnapshotContent) -> Result<Self, TraitError> {
        normalize(&mut content)?;
        validate_content(&content)?;
        let quality = derive_quality(
            &content,
            content.as_of.max(content.known_at),
            &InputSet::all(&content),
        );
        let bytes = serde_json::to_vec(&content)
            .map_err(|_| TraitError::InvalidInput("snapshot serialization failed"))?;
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        let id = SnapshotId::new(format!(
            "sha256:{}",
            content_digest(
                match content.schema_version {
                    1 => b"convex.snapshot.v1\0",
                    2 => b"convex.snapshot.v2\0",
                    _ => b"convex.snapshot.v3\0",
                },
                &bytes
            )
        ))
        .map_err(|_| TraitError::InvalidInput("invalid snapshot identity"))?;
        Ok(Self(SnapshotWire {
            snapshot_id: id,
            content,
            quality,
        }))
    }
    /// Content hash identity.
    pub fn id(&self) -> &SnapshotId {
        &self.0.snapshot_id
    }
    /// Immutable resolved values, including policies and input generation.
    pub fn content(&self) -> &SnapshotContent {
        &self.0.content
    }
    /// Derived quality at capture time, never refreshed by reading a cache.
    pub fn quality(&self) -> &SnapshotQuality {
        &self.0.quality
    }
    /// The resolved reference of one instrument. Every snapshot holds its references
    /// sorted by instrument id without duplicates, so this is a binary search.
    pub fn reference(&self, instrument_id: &str) -> Option<&ResolvedBondReference> {
        let references = &self.0.content.references;
        references
            .binary_search_by(|r| r.record.instrument_id.as_str().cmp(instrument_id))
            .ok()
            .map(|i| &references[i])
    }
    /// The selected quotes of one instrument, one per source, in source order. Every
    /// snapshot holds its quotes sorted by instrument id, then source.
    pub fn quotes_for(&self, instrument_id: &str) -> &[SnapshotQuote] {
        let quotes = &self.0.content.quotes;
        let start =
            quotes.partition_point(|q| q.requirement.key.instrument_id.as_str() < instrument_id);
        let end =
            quotes.partition_point(|q| q.requirement.key.instrument_id.as_str() <= instrument_id);
        &quotes[start..end]
    }
    /// What one instrument's valuation reads (D58): its selected quotes, a floating
    /// note's fixings on its pinned index and fixing source, and the required curves
    /// named in `curves`, the ones its valuation reads. A named curve the snapshot does
    /// not require is not judged.
    pub fn inputs(&self, instrument_id: &str, curves: &[&str]) -> InputSet<'_> {
        let content = &self.0.content;
        let index = self
            .reference(instrument_id)
            .and_then(|reference| reference.floating.as_ref())
            .and_then(|floating| match &floating.index.definition {
                MasterDefinition::RateIndex(index) => Some(index),
                _ => None,
            });
        InputSet {
            quotes: self.quotes_for(instrument_id),
            fixings: index.map_or_else(Vec::new, |index| {
                content
                    .fixings
                    .iter()
                    .filter(|f| {
                        f.key.index_id == index.index_id && f.key.source == index.fixing_source
                    })
                    .collect()
            }),
            curves: content
                .required_curves
                .iter()
                .filter(|id| curves.contains(&id.as_str()))
                .collect(),
        }
    }
    /// Quality of this snapshot's inputs in `inputs` alone, at the snapshot's own time.
    pub fn quality_of(&self, inputs: &InputSet<'_>) -> SnapshotQuality {
        let content = &self.0.content;
        derive_quality(content, content.as_of.max(content.known_at), inputs)
    }
    /// Quality of one curve alone; a curve the snapshot does not require is `Invalid`.
    pub fn curve_quality(&self, curve_id: &str) -> DataQuality {
        let curves = &self.0.content.required_curves;
        match curves.iter().find(|id| id.as_str() == curve_id) {
            Some(id) => {
                self.quality_of(&InputSet {
                    quotes: &[],
                    fixings: Vec::new(),
                    curves: vec![id],
                })
                .quality
            }
            None => DataQuality::Invalid,
        }
    }
    /// Verify content/quality against its identity (also required after deserialization).
    pub fn verify(&self) -> Result<(), TraitError> {
        let verified = Self::new(self.0.content.clone())?;
        if verified != *self {
            return Err(TraitError::Conflict(
                "snapshot content/quality/identity mismatch",
            ));
        }
        Ok(())
    }
    /// Canonical bytes for storage/replay. Serialized content is bounded.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, TraitError> {
        self.verify()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|_| TraitError::InvalidInput("snapshot serialization failed"))?;
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        Ok(bytes)
    }
    /// Necessary data-quality condition for executable hedging, evaluated at supplied decision time.
    /// This never grants trading, risk, ownership, venue or strategy authorization.
    pub fn require_execution_quality_at(
        &self,
        decision_time: UtcTimestamp,
    ) -> Result<(), TraitError> {
        if decision_time < self.0.content.known_at
            || decision_time < self.0.content.as_of
            || !self.0.quality.complete
            || self.0.quality.quality != DataQuality::Valid
            || !derive_quality(
                &self.0.content,
                decision_time,
                &InputSet::all(&self.0.content),
            )
            .issues
            .is_empty()
            || self
                .0
                .content
                .configuration_capture
                .as_ref()
                .is_some_and(|capture| {
                    capture
                        .applied_overrides
                        .iter()
                        .any(|o| decision_time >= o.expires_at)
                })
        {
            return Err(TraitError::InvalidInput(
                "snapshot cannot satisfy executable hedge data policy",
            ));
        }
        Ok(())
    }
}
impl TryFrom<SnapshotWire> for MarketSnapshot {
    type Error = TraitError;
    fn try_from(value: SnapshotWire) -> Result<Self, Self::Error> {
        let result = Self(value);
        result.verify()?;
        Ok(result)
    }
}
impl From<MarketSnapshot> for SnapshotWire {
    fn from(value: MarketSnapshot) -> Self {
        value.0
    }
}

/// Atomic immutable publication. Exact retries return the original content; never overwrite.
#[async_trait]
pub trait SnapshotRepository: Send + Sync {
    /// Publish a complete immutable core snapshot using its separate schema/content domain.
    async fn publish_core(
        &self,
        _snapshot: &crate::core_workflow::CoreSnapshot,
    ) -> Result<(), TraitError> {
        Err(TraitError::Unsupported("core snapshot persistence"))
    }
    /// Retrieve the complete original core inputs; corruption is never absence.
    async fn get_core(
        &self,
        _id: &SnapshotId,
    ) -> Result<Option<crate::core_workflow::CoreSnapshot>, TraitError> {
        Err(TraitError::Unsupported("core snapshot persistence"))
    }
    /// Verify identity, publish all resolved content, acknowledge only at the declared durability boundary.
    /// UnknownOutcome requires get(id) and reconciliation before retrying.
    async fn publish(&self, snapshot: &MarketSnapshot) -> Result<(), TraitError>;
    /// Retrieve exact immutable content and verify it before returning; never use latest as a substitute.
    async fn get(&self, id: &SnapshotId) -> Result<Option<MarketSnapshot>, TraitError>;
}

fn normalize(content: &mut SnapshotContent) -> Result<(), TraitError> {
    if content.references.len() > MAX_SNAPSHOT_INSTRUMENTS
        || content.quotes.len() > MAX_SNAPSHOT_INSTRUMENTS
        || content.fixings.len() > MAX_SNAPSHOT_FIXINGS
        || content.curves.len()
            + content.curve_recipes.len()
            + content.calibration.as_ref().map_or(0, |c| c.curves.len())
            > MAX_SNAPSHOT_CURVES
        || content.required_curves.len() > MAX_SNAPSHOT_CURVES
        || content.curves.iter().map(|c| c.points.len()).sum::<usize>()
            + content
                .curve_recipes
                .iter()
                .map(SnapshotCurveRecipe::input_count)
                .sum::<usize>()
            + content
                .calibration
                .as_ref()
                .map_or(0, SnapshotCalibration::input_count)
            > MAX_SNAPSHOT_PILLARS
    {
        return Err(TraitError::CapacityExceeded);
    }
    content.references.sort_by(|a, b| {
        a.record
            .instrument_id
            .as_str()
            .cmp(b.record.instrument_id.as_str())
    });
    content.quotes.sort_by(|a, b| {
        (
            a.requirement.key.instrument_id.as_str(),
            &a.requirement.key.source,
        )
            .cmp(&(
                b.requirement.key.instrument_id.as_str(),
                &b.requirement.key.source,
            ))
    });
    for quote in &mut content.quotes {
        quote.requirement.fields.sort();
    }
    content.fixings.sort_by(|a, b| a.key.cmp(&b.key));
    if let Some(calibration) = &mut content.calibration {
        calibration.normalize();
    }
    content.required_curves.sort();
    content.curves.sort_by(|a, b| a.curve_id.cmp(&b.curve_id));
    content
        .curve_recipes
        .sort_by(|a, b| a.curve_id.cmp(&b.curve_id));
    for curve in &mut content.curve_recipes {
        curve
            .input_versions
            .sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
    }
    for curve in &mut content.curves {
        curve
            .input_versions
            .sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
    }
    Ok(())
}

fn validate_content(content: &SnapshotContent) -> Result<(), TraitError> {
    use crate::reference_repository::validate_reference_id;
    if !((content.schema_version == 1
        && content.curve_recipes.is_empty()
        && content.calibration.is_none())
        || (content.schema_version == 2
            && !content.curve_recipes.is_empty()
            && content.calibration.is_none())
        || (content.schema_version == 3 && content.calibration.is_some()))
        || (content.references.is_empty() && content.calibration.is_none())
        || !(0..=86_400_000).contains(&content.max_source_skew_ms)
        || content.configuration.version == 0
        || !content.configuration.active
        || content.configuration.created_at < 0
        || content.configuration.updated_at < content.configuration.created_at
        || content.configuration.updated_at > content.known_at.datetime().timestamp_millis()
    {
        return Err(TraitError::InvalidInput(
            "invalid snapshot schema/configuration/policy",
        ));
    }
    crate::storage::validate_pricing_config(&content.configuration)?;
    if let Some(capture) = &content.configuration_capture {
        capture.activation.validate()?;
        let command = &capture.activation.command;
        if command.id.as_str() != content.configuration.config_id
            || command.configuration_version.get() != content.configuration.version
            || command.activated_at > content.known_at
            || capture.applied_overrides.len() > 256
        {
            return Err(TraitError::InvalidInput(
                "invalid captured configuration activation",
            ));
        }
        let mut scopes = std::collections::HashSet::new();
        for override_ in &capture.applied_overrides {
            override_.validate()?;
            if !override_.applies_at(content.as_of, content.known_at)
                || !scopes.insert(&override_.scope)
                || !content.quotes.iter().any(|q| {
                    q.requirement.key == override_.scope
                        && q.requirement.max_age_ms == override_.max_age_ms
                })
            {
                return Err(TraitError::InvalidInput(
                    "expired or mismatched captured override",
                ));
            }
        }
    }
    crate::quote_event::validate_text(&content.configuration.config_id, 128)?;
    content
        .configuration
        .applies_to
        .validate()
        .map_err(TraitError::InvalidInput)?;
    let mut instruments = BTreeSet::new();
    let mut resources = std::collections::BTreeMap::new();
    for reference in &content.references {
        validate_reference_id(&reference.record.instrument_id)
            .map_err(|_| TraitError::InvalidInput("invalid snapshot instrument"))?;
        if !instruments.insert(reference.record.instrument_id.as_str()) {
            return Err(TraitError::InvalidInput("duplicate snapshot reference"));
        }
        validate_resolved_bond(
            reference,
            ReferenceLookup {
                as_of: content.as_of,
                known_at: content.known_at,
            },
        )
        .map_err(|_| TraitError::InvalidInput("invalid resolved snapshot reference"))?;
        for record in [
            &reference.issuer,
            &reference.calendar,
            &reference.convention,
        ] {
            let key = (record.definition.key(), record.version);
            if resources
                .insert(key, record)
                .is_some_and(|prior| prior != record)
            {
                return Err(TraitError::Conflict(
                    "different content for one reference revision",
                ));
            }
        }
    }
    let mut quotes = BTreeSet::new();
    for quote in &content.quotes {
        let requirement = &quote.requirement;
        validate_reference_id(&requirement.key.instrument_id)
            .map_err(|_| TraitError::InvalidInput("invalid quote instrument"))?;
        crate::quote_event::validate_text(&requirement.key.source, 128)?;
        if !instruments.contains(requirement.key.instrument_id.as_str())
            || !quotes.insert((
                requirement.key.instrument_id.as_str(),
                &requirement.key.source,
            ))
            || requirement.fields.is_empty()
            || requirement.fields.len() > 12
            || requirement.fields.windows(2).any(|w| w[0] == w[1])
            || !(1..=86_400_000).contains(&requirement.max_age_ms)
        {
            return Err(TraitError::InvalidInput(
                "invalid/duplicate quote requirement",
            ));
        }
        validate_quote(quote, content)?;
    }
    let mut fixing_keys = BTreeSet::new();
    for selected in &content.fixings {
        if !fixing_keys.insert(&selected.key)
            || selected.key.observation_date.date().as_naive_date()
                > content.as_of.datetime().date_naive()
        {
            return Err(TraitError::InvalidInput(
                "duplicate or future required fixing",
            ));
        }
        if let Some(fixing) = &selected.fixing {
            fixing.validate()?;
            if fixing.key != selected.key
                || fixing.published_at > content.known_at
                || fixing.recorded_at > content.known_at
            {
                return Err(TraitError::InvalidInput(
                    "fixing was not known at snapshot cutoff",
                ));
            }
        }
    }
    if content.required_curves.windows(2).any(|w| w[0] == w[1]) {
        return Err(TraitError::InvalidInput("duplicate required curve"));
    }
    let mut curve_ids = BTreeSet::new();
    for curve in &content.curves {
        curve.validate()?;
        if !curve_ids.insert(&curve.curve_id)
            || !content.required_curves.contains(&curve.curve_id)
            || curve.source_time > content.as_of
            || curve.recorded_at > content.known_at
        {
            return Err(TraitError::InvalidInput(
                "duplicate/unrequested/later-known curve",
            ));
        }
    }
    for curve in &content.curve_recipes {
        curve.validate()?;
        if !curve_ids.insert(&curve.curve_id)
            || !content.required_curves.contains(&curve.curve_id)
            || curve.source_time > content.as_of
            || curve.recorded_at > content.known_at
        {
            return Err(TraitError::InvalidInput(
                "duplicate/unrequested/later-known curve recipe",
            ));
        }
    }
    if let Some(calibration) = &content.calibration {
        calibration.validate()?;
        if calibration.source_time > content.as_of || calibration.recorded_at > content.known_at {
            return Err(TraitError::InvalidInput("later-known calibration"));
        }
        for pin in &calibration.curves {
            if !curve_ids.insert(&pin.curve_id) || !content.required_curves.contains(&pin.curve_id)
            {
                return Err(TraitError::InvalidInput(
                    "duplicate/unrequested calibration curve",
                ));
            }
        }
    }

    Ok(())
}

fn validate_quote(quote: &SnapshotQuote, content: &SnapshotContent) -> Result<(), TraitError> {
    let eligible = |event: &QuoteEvent| -> Result<(), TraitError> {
        event.validate()?;
        if event.key != quote.requirement.key
            || event.source_time_ms > content.as_of.datetime().timestamp_millis()
            || event.receive_time_ms > content.known_at.datetime().timestamp_millis()
        {
            return Err(TraitError::InvalidInput(
                "quote outside selected source/time scope",
            ));
        }
        Ok(())
    };
    if let Some(delivery) = &quote.delivery_watermark {
        eligible(delivery)?;
    }
    if let Some(image) = &quote.image {
        eligible(&image.watermark)?;
        if !matches!(
            image.watermark.event,
            QuoteEventKind::Image { .. } | QuoteEventKind::Delta { .. }
        ) {
            return Err(TraitError::InvalidInput(
                "accepted quote watermark must be data",
            ));
        }
        if image.fields.len() > 12 || quote.delivery_watermark.is_none() {
            return Err(TraitError::InvalidInput(
                "missing delivery watermark/oversized image",
            ));
        }
        for (field, value) in &image.fields {
            if field.unit() != value.value.unit
                || value.sequence == 0
                || value.sequence > image.watermark.sequence
                || value.source_time_ms < 0
                || value.receive_time_ms < 0
                || value.source_time_ms > content.as_of.datetime().timestamp_millis()
                || value.receive_time_ms > content.known_at.datetime().timestamp_millis()
            {
                return Err(TraitError::InvalidInput(
                    "invalid snapshot field observation",
                ));
            }
        }
        if image.state == DataState::Live {
            let delivery = quote
                .delivery_watermark
                .as_ref()
                .ok_or(TraitError::InvalidInput("missing watermark"))?;
            if !delivery.entitled
                || !image.watermark.entitled
                || image.watermark.quality != QuoteQuality::Live
                || delivery.session != image.watermark.session
                || delivery.sequence != image.watermark.sequence
                || !matches!(
                    delivery.event,
                    QuoteEventKind::Image { .. }
                        | QuoteEventKind::Delta { .. }
                        | QuoteEventKind::Heartbeat
                )
            {
                return Err(TraitError::InvalidInput(
                    "live quote contradicts delivery watermark",
                ));
            }
        }
    }
    Ok(())
}

fn derive_quality(
    content: &SnapshotContent,
    now: UtcTimestamp,
    inputs: &InputSet<'_>,
) -> SnapshotQuality {
    let now_ms = now.datetime().timestamp_millis();
    let now_sub_ms = now.datetime().timestamp_subsec_nanos() % 1_000_000;
    use SnapshotIssueKind as K;
    let mut issues = Vec::new();
    let mut complete = true;
    let mut source_times = Vec::new();
    let mut add = |dependency: String, reason| issues.push(SnapshotIssue { dependency, reason });
    for quote in inputs.quotes {
        let id = format!(
            "quote:{}:{}",
            quote.requirement.key.instrument_id, quote.requirement.key.source
        );
        let Some(image) = &quote.image else {
            complete = false;
            add(id.clone(), K::MissingQuote);
            if let Some(delivery) = &quote.delivery_watermark {
                if !delivery.entitled {
                    add(id.clone(), K::Unauthorized);
                }
                match delivery.event {
                    QuoteEventKind::Gap => add(id, K::Gap),
                    QuoteEventKind::Closed => add(id, K::Closed),
                    QuoteEventKind::Error { .. } => add(id, K::Invalid),
                    QuoteEventKind::Reset => add(id, K::AwaitingImage),
                    _ => {}
                }
            }
            continue;
        };
        let reason = match image.state {
            DataState::Live => None,
            DataState::AwaitingImage => Some(K::AwaitingImage),
            DataState::Gap => Some(K::Gap),
            DataState::Closed => Some(K::Closed),
            DataState::Invalid => Some(K::Invalid),
            DataState::Stale => Some(K::Stale),
            DataState::Delayed => Some(K::Delayed),
            DataState::Indicative | DataState::Manual => Some(K::Indicative),
            DataState::Unauthorized => Some(K::Unauthorized),
            DataState::Incomplete => Some(K::MissingField),
        };
        if let Some(reason) = reason {
            add(id.clone(), reason);
        }
        for field in &quote.requirement.fields {
            match image.fields.get(field) {
                None => {
                    complete = false;
                    add(format!("{id}:{field:?}"), K::MissingField);
                }
                Some(value) => {
                    source_times.push(value.source_time_ms);
                    if now_ms < value.receive_time_ms
                        || now_ms < value.source_time_ms
                        || age_exceeded(
                            now_ms,
                            now_sub_ms,
                            value.source_time_ms,
                            0,
                            quote.requirement.max_age_ms,
                        )
                        || age_exceeded(
                            now_ms,
                            now_sub_ms,
                            value.receive_time_ms,
                            0,
                            quote.requirement.max_age_ms,
                        )
                    {
                        add(format!("{id}:{field:?}"), K::Stale);
                    }
                }
            }
        }
    }
    if let (Some(low), Some(high)) = (source_times.iter().min(), source_times.iter().max()) {
        if high.saturating_sub(*low) > content.max_source_skew_ms {
            add("selected_sources".into(), K::SourceSkew);
        }
    }
    for selected in &inputs.fixings {
        let id = format!(
            "fixing:{}:{}:{}",
            selected.key.index_id,
            selected.key.observation_date.date(),
            selected.key.source
        );
        match &selected.fixing {
            None => {
                complete = false;
                add(id, K::MissingFixing);
            }
            Some(fixing) if !fixing.entitled => add(id, K::Unauthorized),
            Some(fixing) if fixing.is_preliminary => add(id, K::Indicative),
            _ => {}
        }
    }
    for &id in &inputs.curves {
        let dependency = format!("curve:{id}");
        let retained = content
            .curves
            .iter()
            .find(|c| &c.curve_id == id)
            .map(|c| (c.source_time, c.recorded_at, c.max_age_ms, c.quality))
            .or_else(|| {
                content
                    .curve_recipes
                    .iter()
                    .find(|c| &c.curve_id == id)
                    .map(|c| (c.source_time, c.recorded_at, c.max_age_ms, c.quality))
            })
            .or_else(|| {
                content
                    .calibration
                    .as_ref()
                    .filter(|c| c.curves.iter().any(|p| &p.curve_id == id))
                    .map(|c| (c.source_time, c.recorded_at, c.max_age_ms, c.quality))
            });
        match retained {
            None => {
                complete = false;
                add(dependency, K::MissingCurve);
            }
            Some((source_time, recorded_at, max_age_ms, curve_quality)) => {
                if age_exceeded(
                    now_ms,
                    now_sub_ms,
                    source_time.datetime().timestamp_millis(),
                    source_time.datetime().timestamp_subsec_nanos() % 1_000_000,
                    max_age_ms,
                ) || age_exceeded(
                    now_ms,
                    now_sub_ms,
                    recorded_at.datetime().timestamp_millis(),
                    recorded_at.datetime().timestamp_subsec_nanos() % 1_000_000,
                    max_age_ms,
                ) {
                    add(dependency.clone(), K::Stale);
                }
                match curve_quality {
                    DataQuality::Valid => {}
                    DataQuality::Indicative => add(dependency, K::Indicative),
                    DataQuality::Delayed => add(dependency, K::Delayed),
                    DataQuality::Stale => add(dependency, K::Stale),
                    DataQuality::Invalid => add(dependency, K::Invalid),
                    DataQuality::Demo => add(dependency, K::Demo),
                }
            }
        }
    }
    let quality = if issues.is_empty() {
        DataQuality::Valid
    } else if issues.iter().any(|i| {
        matches!(
            i.reason,
            K::MissingQuote
                | K::MissingField
                | K::AwaitingImage
                | K::Gap
                | K::Closed
                | K::Invalid
                | K::Unauthorized
                | K::MissingFixing
                | K::MissingCurve
                | K::SourceSkew
        )
    }) {
        DataQuality::Invalid
    } else if issues.iter().any(|i| i.reason == K::Demo) {
        DataQuality::Demo
    } else if issues.iter().any(|i| i.reason == K::Stale) {
        DataQuality::Stale
    } else if issues.iter().any(|i| i.reason == K::Delayed) {
        DataQuality::Delayed
    } else {
        DataQuality::Indicative
    };
    SnapshotQuality {
        complete,
        quality,
        issues,
    }
}

// Snapshot persistence preserves exact configuration decimals even when serde-float is unified.
// Ordinary client configuration contracts retain their existing format.
pub(crate) mod snapshot_configuration {
    use super::*;
    use crate::storage::PricingSpec;
    use convex_core::contracts::ExactDecimal;
    pub fn serialize<S: serde::Serializer>(
        config: &BondPricingConfig,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut wire = serde_json::to_value(config).map_err(serde::ser::Error::custom)?;
        if let PricingSpec::RealYield { index_ratio, .. } = &config.pricing_spec {
            wire["pricing_spec"]["RealYield"]["index_ratio"] =
                serde_json::Value::String(index_ratio.to_string());
        }
        wire.serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BondPricingConfig, D::Error> {
        let wire = serde_json::Value::deserialize(deserializer)?;
        let ratio = wire
            .pointer("/pricing_spec/RealYield/index_ratio")
            .map(|value| {
                let text = value.as_str().ok_or_else(|| {
                    serde::de::Error::custom("snapshot index ratio requires exact decimal string")
                })?;
                ExactDecimal::parse(text).map_err(serde::de::Error::custom)
            })
            .transpose()?;
        let mut config: BondPricingConfig =
            serde_json::from_value(wire).map_err(serde::de::Error::custom)?;
        if let (PricingSpec::RealYield { index_ratio, .. }, Some(ratio)) =
            (&mut config.pricing_spec, ratio)
        {
            *index_ratio = ratio.decimal();
        }
        Ok(config)
    }
}

/// Checked conversion for callers starting with existing native instrument IDs.
pub fn snapshot_instrument_id(
    id: &convex_core::ids::InstrumentId,
) -> Result<InstrumentId, TraitError> {
    InstrumentId::new(id.as_str())
        .map_err(|_| TraitError::InvalidInput("invalid instrument identity"))
}

fn age_exceeded(
    now_ms: i64,
    now_sub_ms: u32,
    source_ms: i64,
    source_sub_ms: u32,
    maximum_ms: i64,
) -> bool {
    let age_ms = now_ms.saturating_sub(source_ms);
    age_ms > maximum_ms || (age_ms == maximum_ms && now_sub_ms > source_sub_ms)
}
