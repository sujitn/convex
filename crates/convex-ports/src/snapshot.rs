//! Immutable resolved information sets with explicit schema-separated content identities.
#![forbid(unsafe_code)]

use async_trait::async_trait;
use convex_core::contracts::{
    BusinessDate, CurveId, DataQuality, FiniteF64, InputVersion, InstrumentId, ModelId,
    ReferenceDataId, SnapshotId, SourceId, UtcTimestamp, Version,
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::Write as _;
use std::sync::{Arc, OnceLock};

use crate::calibration::SnapshotCalibration;
use crate::fixing::{FixingKey, PublishedFixing};
use crate::level_pricing::{
    BenchmarkSecurity, ResolvedBenchmarkSet, ResolvedPricingRule, MAX_SNAPSHOT_BENCHMARK_SETS,
};
use crate::quote_event::{
    DataState, QuoteEvent, QuoteEventKind, QuoteField, QuoteKey, QuoteQuality, QuoteSnapshot,
};
#[cfg(doc)]
use crate::reference_master::validate_resolved_bond_at;
use crate::reference_master::{
    validate_resolved_bond, EffectiveInterval, MasterDefinition, ReferenceLookup,
    ResolvedBondReference,
};
use crate::storage::BondPricingConfig;
use crate::TraitError;

/// Maximum serialized snapshot; adapters must reject growth beyond this bound. Every
/// snapshot but a Live runtime's is identified within it too.
pub const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
/// Maximum selected instruments or quote keys in one information set, but a Live one.
pub const MAX_SNAPSHOT_INSTRUMENTS: usize = 256;
/// Selected instruments or quote keys in a Live runtime's snapshot, which is never
/// stored.
pub const MAX_LIVE_SNAPSHOT_INSTRUMENTS: usize = 4096;
/// The JSON a Live snapshot's identity hashes. 4,096 instruments with every text at the
/// bound its validator sets, each with a pricing rule, and 64 benchmark sets at theirs,
/// write 126.7 MiB with a small frame, which leaves 1.3 MiB for the rest of the frame; a
/// frame at its own bounds is not counted. Past
/// it, every tick fails with `CapacityExceeded`. Live snapshots are never stored, and
/// the JSON streams into the hash.
pub const MAX_LIVE_IDENTITY_BYTES: usize = 128 * 1024 * 1024;
/// Maximum explicitly required fixings in one information set.
pub const MAX_SNAPSHOT_FIXINGS: usize = 1024;
/// Maximum supplied curves in one information set.
pub const MAX_SNAPSHOT_CURVES: usize = 64;
/// Maximum total curve pillars, not a separate allowance for each curve.
pub const MAX_SNAPSHOT_PILLARS: usize = 4096;
/// The schema assembly writes: the sectioned identity, whatever the curves.
/// Schemas 1–3 still verify, so stored snapshots keep their ids.
pub const SNAPSHOT_SCHEMA: u32 = 4;

/// What a snapshot may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotBounds {
    /// Stored, replayed and request snapshots: [`MAX_SNAPSHOT_INSTRUMENTS`] instruments,
    /// identified within [`MAX_SNAPSHOT_BYTES`].
    Standard,
    /// A Live runtime's snapshots, never stored: [`MAX_LIVE_SNAPSHOT_INSTRUMENTS`]
    /// instruments, identified within [`MAX_LIVE_IDENTITY_BYTES`].
    Live,
}

impl SnapshotBounds {
    /// The most instruments, and quote keys, a snapshot holds.
    pub const fn instruments(self) -> usize {
        match self {
            Self::Standard => MAX_SNAPSHOT_INSTRUMENTS,
            Self::Live => MAX_LIVE_SNAPSHOT_INSTRUMENTS,
        }
    }
    /// The most JSON a snapshot's identity hashes.
    pub const fn identity_bytes(self) -> usize {
        match self {
            Self::Standard => MAX_SNAPSHOT_BYTES,
            Self::Live => MAX_LIVE_IDENTITY_BYTES,
        }
    }
}

/// Schema 4's domains: the id's, and one for each kind of section digest it hashes.
const SCHEMA_FOUR: &[u8] = b"convex.snapshot.v4\0";
const SCHEMA_FOUR_REFERENCES: &[u8] = b"convex.snapshot.v4.references\0";
const SCHEMA_FOUR_QUOTE: &[u8] = b"convex.snapshot.v4.quote\0";

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

/// One selected quote of a snapshot: a shared, immutable entry. A Live process
/// keeps one entry for each selected key, and every tick's snapshot shares it until that
/// key ingests an event or its state changes, so a tick copies a pointer, not the quote.
/// The entry keeps its schema-4 digest and the result of its checks that read no cutoff,
/// each computed once; the checks that read a cutoff run for every snapshot.
/// [`SharedQuote::make_mut`] is the only way to change it, and it drops both. It
/// serializes as the plain quote, so stored snapshots keep their shape.
#[derive(Clone)]
pub struct SharedQuote(Arc<QuoteEntry>);

struct QuoteEntry {
    quote: SnapshotQuote,
    /// Schema 4's digest of `quote`, computed by the first schema-4 identity.
    digest: OnceLock<SectionDigest>,
    /// Set once `quote` passes the checks that read no cutoff.
    checked: OnceLock<()>,
}

impl QuoteEntry {
    fn new(quote: SnapshotQuote) -> Self {
        Self {
            quote,
            digest: OnceLock::new(),
            checked: OnceLock::new(),
        }
    }
}

impl Clone for QuoteEntry {
    /// A copy is made only to change it, so it starts unchecked and unhashed.
    fn clone(&self) -> Self {
        Self::new(self.quote.clone())
    }
}

impl SharedQuote {
    /// A new entry, unchecked and unhashed.
    pub fn new(quote: SnapshotQuote) -> Self {
        Self(Arc::new(QuoteEntry::new(quote)))
    }
    /// The quote to change, copied first when another holder shares it. The checks and
    /// digest are dropped; the next snapshot checks and hashes it again.
    pub fn make_mut(&mut self) -> &mut SnapshotQuote {
        let entry = Arc::make_mut(&mut self.0);
        entry.digest.take();
        entry.checked.take();
        &mut entry.quote
    }
    /// Whether both hold one entry, so their quotes are the same without comparing them.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    /// The checks that read the quote alone, run once per entry.
    fn check(&self) -> Result<(), TraitError> {
        if self.0.checked.get().is_none() {
            check_quote(&self.0.quote)?;
            let _ = self.0.checked.set(());
        }
        Ok(())
    }
    /// The quote's schema-4 digest, computed once per entry, within the largest bound;
    /// each identity counts its length against its own.
    fn digest(&self) -> Result<&SectionDigest, TraitError> {
        if let Some(digest) = self.0.digest.get() {
            return Ok(digest);
        }
        let mut hash = Sha256::new();
        hash.update(SCHEMA_FOUR_QUOTE);
        let mut len = 0;
        let digest = hash_json(hash, &self.0.quote, &mut len, MAX_LIVE_IDENTITY_BYTES)?.finalize();
        Ok(self.0.digest.get_or_init(|| SectionDigest { digest, len }))
    }
}

impl From<SnapshotQuote> for SharedQuote {
    fn from(quote: SnapshotQuote) -> Self {
        Self::new(quote)
    }
}

impl std::ops::Deref for SharedQuote {
    type Target = SnapshotQuote;
    fn deref(&self) -> &SnapshotQuote {
        &self.0.quote
    }
}

impl PartialEq for SharedQuote {
    /// Equal quotes; the cached checks and digest are not compared.
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other) || self.0.quote == other.0.quote
    }
}

impl Eq for SharedQuote {}

impl std::fmt::Debug for SharedQuote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.quote.fmt(f)
    }
}

impl Serialize for SharedQuote {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.quote.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SharedQuote {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        SnapshotQuote::deserialize(deserializer).map(Self::new)
    }
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
    /// 1 for original zero records, 2 for direct recipes, 3 for product calibration,
    /// each hashed whole. 4 for any of them, hashed by section ([`SNAPSHOT_SCHEMA`]).
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
    /// Resolved instrument, issuer, calendar and convention revisions, one shared section.
    pub references: SnapshotReferences,
    /// Selected quote requirements, values and feed continuity, each a shared entry.
    pub quotes: Vec<SharedQuote>,
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
    /// Each benchmark set the references' pricing rules name and that is in force at the
    /// cutoff, once, sorted by set id; at most [`MAX_SNAPSHOT_BENCHMARK_SETS`]. A Live
    /// activation pins them with the references, and every tick's snapshot shares them.
    #[serde(default, skip_serializing_if = "holds_no_sets")]
    pub benchmark_sets: Arc<[ResolvedBenchmarkSet]>,
}

/// The resolved references of a snapshot: one shared, immutable section. A Live
/// selection pins its references once per activation, and every tick's snapshot shares
/// the section instead of copying it. The checks that read the records alone run once
/// per section, and its canonical JSON (schemas 1–3) or its digest (schema 4) is
/// computed once; the checks that read a cutoff run for every snapshot.
/// [`SnapshotReferences::make_mut`] is the only way to change the records, and it drops
/// all three. It serializes as the plain array of records, so stored snapshots keep
/// their shape.
#[derive(Clone)]
pub struct SnapshotReferences(Arc<ReferenceSection>);

struct ReferenceSection {
    records: Vec<ResolvedBondReference>,
    /// Canonical JSON of `records`, written by the first schema 1–3 identity that needs it.
    json: OnceLock<Box<RawValue>>,
    /// Schema 4's digest of `records`, computed by the first schema-4 identity.
    digest: OnceLock<SectionDigest>,
    /// Set once `records` pass the checks that read no cutoff.
    checked: OnceLock<()>,
    /// What the cutoff checks of checked `records` compare, computed once.
    bounds: OnceLock<Option<ReferenceBounds>>,
    /// The positions of the records that carry a floating index, found once.
    floating: OnceLock<Box<[usize]>>,
}

/// The times a checked section's cutoff checks compare. A record and each of
/// its resources must be recorded by the lookup's `known_at` and effective at its
/// `as_of`, so every one is exactly when the latest recording is by `known_at` and
/// `as_of` lies from the latest start to before the earliest end. `None` in a field means
/// no record bounds it. A section holding a record without terms has no bounds, since
/// that record fails at every lookup.
#[derive(Clone, Copy)]
struct ReferenceBounds {
    from: Option<UtcTimestamp>,
    until: Option<UtcTimestamp>,
    recorded_at: Option<UtcTimestamp>,
}

impl ReferenceBounds {
    fn of(records: &[ResolvedBondReference]) -> Option<Self> {
        let mut bounds = Self {
            from: None,
            until: None,
            recorded_at: None,
        };
        let mut add = |recorded_at: UtcTimestamp, effective: EffectiveInterval| {
            bounds.recorded_at = bounds.recorded_at.max(Some(recorded_at));
            bounds.from = bounds.from.max(Some(effective.from));
            if let Some(until) = effective.until {
                bounds.until = Some(bounds.until.map_or(until, |earliest| earliest.min(until)));
            }
        };
        for reference in records {
            let terms = reference.record.bond.as_ref()?.contractual_terms.as_ref()?;
            add(reference.record.recorded_at, terms.effective);
            let floating = reference
                .floating
                .iter()
                .flat_map(|floating| [&floating.index, &floating.calendar]);
            for record in [
                &reference.issuer,
                &reference.calendar,
                &reference.convention,
            ]
            .into_iter()
            .chain(floating)
            {
                add(record.recorded_at, record.definition.effective());
            }
        }
        // A pinned rule bounds only the cutoff it was recorded by.
        let rules = records
            .iter()
            .filter_map(|reference| reference.pricing_rule.as_ref())
            .map(|rule| rule.recorded_at)
            .max();
        bounds.recorded_at = bounds.recorded_at.max(rules);
        Some(bounds)
    }
    /// Whether every record passes [`validate_resolved_bond_at`] at `lookup`.
    fn hold_at(&self, lookup: ReferenceLookup) -> bool {
        self.from.is_none_or(|from| from <= lookup.as_of)
            && self.until.is_none_or(|until| lookup.as_of < until)
            && self
                .recorded_at
                .is_none_or(|recorded_at| recorded_at <= lookup.known_at)
    }
}

/// SHA-256 of a section's canonical JSON under its schema-4 domain, and the length of
/// that JSON, which counts against the snapshot's [`MAX_SNAPSHOT_BYTES`].
struct SectionDigest {
    digest: sha2::digest::Output<Sha256>,
    len: usize,
}

impl ReferenceSection {
    fn new(records: Vec<ResolvedBondReference>) -> Self {
        Self {
            records,
            json: OnceLock::new(),
            digest: OnceLock::new(),
            checked: OnceLock::new(),
            bounds: OnceLock::new(),
            floating: OnceLock::new(),
        }
    }
}

impl Clone for ReferenceSection {
    /// A copy is made only to change it, so it starts unchecked and unwritten.
    fn clone(&self) -> Self {
        Self::new(self.records.clone())
    }
}

impl SnapshotReferences {
    /// One section of `records`, sorted by instrument id as every snapshot holds them.
    pub fn new(mut records: Vec<ResolvedBondReference>) -> Self {
        records.sort_by(by_instrument);
        Self(Arc::new(ReferenceSection::new(records)))
    }
    /// The records to change, copied first when another holder shares them. The checks,
    /// their bounds, canonical JSON and digest are dropped; the next snapshot sorts,
    /// checks and hashes again.
    pub fn make_mut(&mut self) -> &mut Vec<ResolvedBondReference> {
        let section = Arc::make_mut(&mut self.0);
        section.json.take();
        section.digest.take();
        section.checked.take();
        section.bounds.take();
        section.floating.take();
        &mut section.records
    }
    /// Sort by instrument id, copying only when the records are out of order. A shared
    /// section is already sorted, so a Live tick never copies it.
    fn sort(&mut self) {
        if !self
            .0
            .records
            .is_sorted_by(|a, b| by_instrument(a, b).is_le())
        {
            self.make_mut().sort_by(by_instrument);
        }
    }
    /// The records that carry a floating index, in order. Their positions are found once
    /// per section, so a Live tick does not walk every record for them.
    pub fn floating(&self) -> impl Iterator<Item = &ResolvedBondReference> {
        let records = &self.0.records;
        self.0
            .floating
            .get_or_init(|| {
                records
                    .iter()
                    .enumerate()
                    .filter(|(_, record)| record.floating.is_some())
                    .map(|(position, _)| position)
                    .collect()
            })
            .iter()
            .filter_map(|&position| records.get(position))
    }
    /// Whether both hold one section, so their records are the same without comparing
    /// them. Every snapshot of a Live activation shares its pinned section.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    /// One instrument's reference; the records are sorted and distinct once checked.
    fn find(&self, instrument_id: &str) -> Option<&ResolvedBondReference> {
        let records = &self.0.records;
        records
            .binary_search_by(|r| r.record.instrument_id.as_str().cmp(instrument_id))
            .ok()
            .map(|i| &records[i])
    }
    /// Check sorted records at `lookup`. The checks that read the records alone run
    /// once per section. The cutoff checks run at every call, once checked in O(1) from
    /// the section's bounds.
    fn validate(&self, lookup: ReferenceLookup) -> Result<(), TraitError> {
        use crate::reference_repository::validate_reference_id;
        let invalid = |_| TraitError::InvalidInput("invalid resolved snapshot reference");
        if self.0.checked.get().is_some() {
            let bounds = self
                .0
                .bounds
                .get_or_init(|| ReferenceBounds::of(&self.0.records));
            return match bounds {
                Some(bounds) if bounds.hold_at(lookup) => Ok(()),
                _ => Err(TraitError::InvalidInput(
                    "invalid resolved snapshot reference",
                )),
            };
        }
        let mut resources = std::collections::BTreeMap::new();
        for (i, reference) in self.0.records.iter().enumerate() {
            validate_reference_id(&reference.record.instrument_id)
                .map_err(|_| TraitError::InvalidInput("invalid snapshot instrument"))?;
            if i > 0 && self.0.records[i - 1].record.instrument_id == reference.record.instrument_id
            {
                return Err(TraitError::InvalidInput("duplicate snapshot reference"));
            }
            validate_resolved_bond(reference, lookup).map_err(invalid)?;
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
        let _ = self.0.checked.set(());
        Ok(())
    }
    /// The records' canonical JSON, written once per section.
    fn json(&self) -> Result<&RawValue, TraitError> {
        if let Some(json) = self.0.json.get() {
            return Ok(json);
        }
        let json = serde_json::value::to_raw_value(&self.0.records)
            .map_err(|_| TraitError::InvalidInput("snapshot serialization failed"))?;
        Ok(self.0.json.get_or_init(|| json))
    }
    /// The records' schema-4 digest, computed once per section within the largest
    /// bound; each identity counts its length against its own. The JSON streams into the
    /// hash and is not kept.
    fn digest(&self) -> Result<&SectionDigest, TraitError> {
        if let Some(digest) = self.0.digest.get() {
            return Ok(digest);
        }
        let mut hash = Sha256::new();
        hash.update(SCHEMA_FOUR_REFERENCES);
        let mut len = 0;
        let digest =
            hash_json(hash, &self.0.records, &mut len, MAX_LIVE_IDENTITY_BYTES)?.finalize();
        Ok(self.0.digest.get_or_init(|| SectionDigest { digest, len }))
    }
}

fn by_instrument(a: &ResolvedBondReference, b: &ResolvedBondReference) -> std::cmp::Ordering {
    a.record
        .instrument_id
        .as_str()
        .cmp(b.record.instrument_id.as_str())
}

impl std::ops::Deref for SnapshotReferences {
    type Target = [ResolvedBondReference];
    fn deref(&self) -> &Self::Target {
        &self.0.records
    }
}

impl<'a> IntoIterator for &'a SnapshotReferences {
    type Item = &'a ResolvedBondReference;
    type IntoIter = std::slice::Iter<'a, ResolvedBondReference>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.records.iter()
    }
}

impl PartialEq for SnapshotReferences {
    /// Equal records; the cached checks and JSON are not compared.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.records == other.0.records
    }
}

impl std::fmt::Debug for SnapshotReferences {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.records.fmt(f)
    }
}

impl Serialize for SnapshotReferences {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.records.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SnapshotReferences {
    /// Records keep their wire order, so verification still refuses unsorted ones.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::deserialize(deserializer).map(|records| Self(Arc::new(ReferenceSection::new(records))))
    }
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

/// The inputs one dependent reads from a snapshot, in snapshot order. Quality over
/// a set judges only these inputs; source skew is measured among its quotes alone, a
/// level a bond's rule quotes it by left out.
#[derive(Debug, Clone)]
pub struct InputSet<'a> {
    own_quotes: &'a [SharedQuote],
    /// The field of the level its rule quotes it by, if any.
    own_level: OwnLevel,
    /// The quotes of the benchmark securities its pricing rule's set names, in set order;
    /// empty for a bond whose rule names no set.
    benchmark_quotes: Vec<&'a [SharedQuote]>,
    fixings: Vec<&'a SnapshotFixing>,
    curves: Vec<&'a CurveId>,
}
/// The quote field of the level a set's own quotes are quoted by: one bond's, or each
/// quote's instrument's for a whole snapshot. A quote is a level only when it carries
/// that field.
#[derive(Debug, Clone, Copy)]
enum OwnLevel {
    Field(Option<QuoteField>),
    ByRule,
}

impl<'a> InputSet<'a> {
    fn all(content: &'a SnapshotContent) -> Self {
        Self {
            own_quotes: &content.quotes,
            own_level: OwnLevel::ByRule,
            benchmark_quotes: Vec::new(),
            fixings: content.fixings.iter().collect(),
            curves: content.required_curves.iter().collect(),
        }
    }
    /// Selected quotes read: its own, then its benchmarks'.
    pub fn quotes(&self) -> impl Iterator<Item = &'a SharedQuote> + '_ {
        self.own_quotes
            .iter()
            .chain(self.benchmark_quotes.iter().copied().flatten())
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

/// The snapshot times over which one input set keeps the quality it was judged to have:
/// from the time it was judged at, up to but not including the first millisecond
/// at which one of quality's time comparisons could turn. Quality reads the time only to
/// compare a quote field's source and receive times with the snapshot's, and a quote
/// field's or a curve's age with its maximum. Each comparison turns at most once as time
/// moves forward, so the same inputs judged at any time in the window are judged the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityWindow {
    judged_at: UtcTimestamp,
    until_ms: Option<i64>,
}

impl QualityWindow {
    /// The snapshot time the inputs were judged at.
    pub fn judged_at(&self) -> UtcTimestamp {
        self.judged_at
    }
    /// The first millisecond since the Unix epoch at which their quality could differ;
    /// `None` when no comparison can turn. That millisecond itself is outside the window:
    /// an age comparison's sub-millisecond tie-break can turn within it.
    pub fn until_ms(&self) -> Option<i64> {
        self.until_ms
    }
}

/// Verified immutable content-addressed information set, schema 1 to 4. Every value
/// is verified when it is made: [`MarketSnapshot::new`] derives the identity,
/// [`MarketSnapshot::next`] returns what `new` would, deserialization re-derives the
/// identity and quality and compares them, and no method mutates one. Holders do not
/// re-verify it. The whole snapshot's quality is derived from its content when first
/// read: by [`MarketSnapshot::quality`], serializing, verifying or comparing.
#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "SnapshotWire", into = "SnapshotWire")]
pub struct MarketSnapshot(Snapshot);

/// A snapshot as held.
#[derive(Clone)]
struct Snapshot {
    snapshot_id: SnapshotId,
    content: SnapshotContent,
    /// The whole snapshot's quality, derived from `content` when first read. A
    /// deserialized snapshot holds its wire's, which `verify` compares.
    quality: OnceLock<SnapshotQuality>,
    /// Schema 4's quote digests in snapshot order, set by `new` and `next`; a
    /// deserialized snapshot has none, and its next is identified from every entry.
    digests: OnceLock<Arc<QuoteDigests>>,
    /// What it was made within; its next is made within the same.
    bounds: SnapshotBounds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotWire {
    snapshot_id: SnapshotId,
    content: SnapshotContent,
    quality: SnapshotQuality,
}

impl PartialEq for MarketSnapshot {
    /// Equal identities, content and quality. Comparing derives the quality of each.
    fn eq(&self, other: &Self) -> bool {
        self.0.snapshot_id == other.0.snapshot_id
            && self.0.content == other.0.content
            && self.quality() == other.quality()
    }
}

impl std::fmt::Debug for MarketSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MarketSnapshot")
            .field("snapshot_id", &self.0.snapshot_id)
            .field("content", &self.0.content)
            .field("quality", self.quality())
            .finish()
    }
}

impl MarketSnapshot {
    /// Normalize unordered manifests, validate supplied values and derive the identity,
    /// within [`SnapshotBounds::Standard`].
    pub fn new(content: SnapshotContent) -> Result<Self, TraitError> {
        Self::bounded(content, SnapshotBounds::Standard)
    }
    /// [`MarketSnapshot::new`] within `bounds`. A Live runtime's snapshots are made
    /// within [`SnapshotBounds::Live`], and so is each one's next.
    pub fn bounded(
        mut content: SnapshotContent,
        bounds: SnapshotBounds,
    ) -> Result<Self, TraitError> {
        normalize(&mut content, bounds)?;
        validate_content(&content)?;
        let (id, digests) = identity_and_digests(&content, bounds)?;
        Ok(Self::made(id, content, digests, bounds))
    }
    fn made(
        snapshot_id: SnapshotId,
        content: SnapshotContent,
        digests: Option<QuoteDigests>,
        bounds: SnapshotBounds,
    ) -> Self {
        Self(Snapshot {
            snapshot_id,
            content,
            quality: OnceLock::new(),
            digests: digests
                .map(Arc::new)
                .map_or_else(OnceLock::new, OnceLock::from),
            bounds,
        })
    }
    /// What this snapshot was made within.
    pub fn bounds(&self) -> SnapshotBounds {
        self.0.bounds
    }
    /// What [`MarketSnapshot::bounded`] returns for `content` within this snapshot's
    /// bounds, derived from this snapshot when `content` follows it ([`MarketSnapshot::follows`]). A Live
    /// tick's content is its last snapshot's but for the cutoffs, the generation, the
    /// curve records, the fixings and the entries rebuilt since.
    ///
    /// Then only what can differ is checked, as `new` checks it: the configuration and
    /// its capture at these cutoffs, the references at these cutoffs in O(1) from their
    /// section's bounds, each quote entry not shared with this snapshot, and the fixings
    /// and curves. A shared entry passed its cutoff checks here, at cutoffs no later than
    /// these, and they compare its times with the cutoffs only, so it passes them again.
    /// A rebuilt entry has its position's requirement, so the quotes keep their order and
    /// keys, each key its reference, and each override its quote. Otherwise, or when a
    /// check fails, this is `bounded`.
    pub fn next(&self, mut content: SnapshotContent) -> Result<Self, TraitError> {
        let bounds = self.bounds();
        let Some(rebuilt) = self.rebuilt(&content) else {
            return Self::bounded(content, bounds);
        };
        if derive(&mut content, &rebuilt).is_err() {
            return Self::bounded(content, bounds);
        }
        match self.derived_identity(&content, &rebuilt) {
            Ok((id, digests)) => Ok(Self::made(id, content, digests, bounds)),
            Err(_) => Self::bounded(content, bounds),
        }
    }
    /// `content`'s identity, as `new` derives it. Under schema 4 its quote digests are
    /// this snapshot's with the `rebuilt` positions' replaced, so the 32 bytes of each
    /// shared entry are copied, not read from the entry. The bytes hashed are
    /// the same, so the identity is.
    fn derived_identity(
        &self,
        content: &SnapshotContent,
        rebuilt: &[usize],
    ) -> Result<(SnapshotId, Option<QuoteDigests>), TraitError> {
        let bounds = self.bounds();
        let Some(previous) = self.0.digests.get().filter(|_| content.schema_version == 4) else {
            return identity_and_digests(content, bounds);
        };
        let digests = previous.patched(&self.0.content.quotes, &content.quotes, rebuilt)?;
        let id = hash_sections(content, &digests, bounds)?;
        // Every test build checks the patched digests against every entry's.
        debug_assert!(
            QuoteDigests::of(&content.quotes, &mut 0, bounds).is_ok_and(|all| all == digests)
        );
        Ok((id, Some(digests)))
    }
    /// Whether [`MarketSnapshot::next`] derives `content` from this snapshot: neither
    /// cutoff is earlier than this snapshot's, the reference section is this snapshot's
    /// and so is the configuration capture, and at each position the quote entry is this
    /// snapshot's or has its requirement.
    pub fn follows(&self, content: &SnapshotContent) -> bool {
        self.rebuilt(content).is_some()
    }
    /// The positions whose entries `content` does not share with this snapshot, when it
    /// follows it. Shared entries are found by pointer, so they are not read.
    fn rebuilt(&self, content: &SnapshotContent) -> Option<Vec<usize>> {
        let then = &self.0.content;
        if content.as_of < then.as_of
            || content.known_at < then.known_at
            || !content.references.ptr_eq(&then.references)
            || !same_sets(&content.benchmark_sets, &then.benchmark_sets)
            || content.quotes.len() != then.quotes.len()
            || content.configuration_capture != then.configuration_capture
        {
            return None;
        }
        let mut rebuilt = Vec::new();
        for (position, (quote, prior)) in content.quotes.iter().zip(&then.quotes).enumerate() {
            if quote.ptr_eq(prior) {
                continue;
            }
            if quote.requirement != prior.requirement {
                return None;
            }
            rebuilt.push(position);
        }
        Some(rebuilt)
    }
    /// Content hash identity.
    pub fn id(&self) -> &SnapshotId {
        &self.0.snapshot_id
    }
    /// The same content under `schema_version`, normalized, checked and identified as
    /// [`MarketSnapshot::new`] does, so a schema whose rules the content breaks is
    /// refused. Restore uses it to rebuild a checkpoint's snapshot under the schema an
    /// earlier build chose.
    pub fn with_schema(&self, schema_version: u32) -> Result<Self, TraitError> {
        let mut content = self.0.content.clone();
        content.schema_version = schema_version;
        Self::bounded(content, self.bounds())
    }
    /// Immutable resolved values, including policies and input generation.
    pub fn content(&self) -> &SnapshotContent {
        &self.0.content
    }
    /// Derived quality at capture time, never refreshed by reading a cache. It is derived
    /// from the content when first read, and kept.
    pub fn quality(&self) -> &SnapshotQuality {
        self.0.quality.get_or_init(|| {
            derive_quality(
                &self.0.content,
                self.quality_time(),
                &InputSet::all(&self.0.content),
            )
            .0
        })
    }
    /// The resolved reference of one instrument. Every snapshot holds its references
    /// sorted by instrument id without duplicates, so this is a binary search.
    pub fn reference(&self, instrument_id: &str) -> Option<&ResolvedBondReference> {
        self.0.content.references.find(instrument_id)
    }
    /// The selected quotes of one instrument, one per source, in source order. Every
    /// snapshot holds its quotes sorted by instrument id, then source.
    pub fn quotes_for(&self, instrument_id: &str) -> &[SharedQuote] {
        let quotes = &self.0.content.quotes;
        let start =
            quotes.partition_point(|q| q.requirement.key.instrument_id.as_str() < instrument_id);
        let end =
            quotes.partition_point(|q| q.requirement.key.instrument_id.as_str() <= instrument_id);
        &quotes[start..end]
    }
    /// What one instrument's valuation reads: its selected quotes, the quotes of
    /// the benchmark securities its pricing rule's set names, a floating note's fixings
    /// on its pinned index and fixing source, and the required curves named in `curves`,
    /// the ones its valuation reads. A named curve the snapshot does not require is not
    /// judged.
    pub fn inputs(&self, instrument_id: &str, curves: &[&str]) -> InputSet<'_> {
        let content = &self.0.content;
        let reference = self.reference(instrument_id);
        let index = reference
            .and_then(|reference| reference.floating.as_ref())
            .and_then(|floating| match &floating.index.definition {
                MasterDefinition::RateIndex(index) => Some(index),
                _ => None,
            });
        let rule = reference.and_then(|reference| reference.pricing_rule.as_ref());
        InputSet {
            own_quotes: self.quotes_for(instrument_id),
            own_level: OwnLevel::Field(rule.and_then(|rule| rule.method.level_field())),
            benchmark_quotes: rule.map_or_else(Vec::new, |rule| {
                self.benchmark_securities(rule)
                    .unwrap_or_default()
                    .iter()
                    .filter(|security| security.instrument_id.as_str() != instrument_id)
                    .map(|security| self.quotes_for(security.instrument_id.as_str()))
                    .collect()
            }),
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
    /// The benchmark set `id` names, when this snapshot holds it.
    pub fn benchmark_set(&self, id: &ReferenceDataId) -> Option<&ResolvedBenchmarkSet> {
        let sets = &self.0.content.benchmark_sets;
        sets.binary_search_by(|set| set.benchmark_set_id.cmp(id))
            .ok()
            .map(|position| &sets[position])
    }
    /// The securities of the set `rule` names; `None` when the rule names a set this
    /// snapshot does not hold, and none when it names no set.
    pub fn benchmark_securities(&self, rule: &ResolvedPricingRule) -> Option<&[BenchmarkSecurity]> {
        match &rule.benchmark_set_id {
            None => Some(&[]),
            Some(id) => self.benchmark_set(id).map(|set| set.securities.as_slice()),
        }
    }
    /// The pricing rule `instrument_id`'s reference pins; none for a bond quoted by price.
    pub fn pricing_rule(&self, instrument_id: &str) -> Option<&ResolvedPricingRule> {
        self.reference(instrument_id)?.pricing_rule.as_ref()
    }
    /// Whether this snapshot shares every valuation's frame with `previous`: the
    /// configuration and its capture, the skew policy, the schema and the valuation
    /// date. A floating note's valuation date and its due fixings follow the date.
    pub fn same_frame(&self, previous: &MarketSnapshot) -> bool {
        let (now, then) = (&self.0.content, &previous.0.content);
        now.schema_version == then.schema_version
            && now.max_source_skew_ms == then.max_source_skew_ms
            && now.as_of.datetime().date_naive() == then.as_of.datetime().date_naive()
            && now.configuration == then.configuration
            && now.configuration_capture == then.configuration_capture
            && (same_sets(&now.benchmark_sets, &then.benchmark_sets)
                || now.benchmark_sets == then.benchmark_sets)
    }
    /// Whether one instrument's valuation reads the same inputs here as in `previous`:
    /// its reference, its quotes, a floating note's fixings, and each curve in
    /// `curves` with its record, recipe or calibration. A recipe pins the versions of
    /// the curves it is built on. The frame is compared once, by `same_frame`. A shared
    /// reference section or quote entry is the same one, so it is not compared.
    pub fn same_inputs(
        &self,
        previous: &MarketSnapshot,
        instrument_id: &str,
        curves: &[&str],
    ) -> bool {
        let (now, then) = (
            self.inputs(instrument_id, curves),
            previous.inputs(instrument_id, curves),
        );
        let references = &self.0.content.references;
        (references.ptr_eq(&previous.0.content.references)
            || self.reference(instrument_id) == previous.reference(instrument_id))
            && now.own_quotes == then.own_quotes
            && now.benchmark_quotes == then.benchmark_quotes
            && now.fixings == then.fixings
            && now.curves == then.curves
            && now
                .curves
                .iter()
                .all(|id| self.curve_data(id) == previous.curve_data(id))
    }
    /// What one required curve resolves to: its record, its recipe and the calibration
    /// that holds it.
    fn curve_data(
        &self,
        id: &CurveId,
    ) -> (
        Option<&SnapshotCurve>,
        Option<&SnapshotCurveRecipe>,
        Option<&SnapshotCalibration>,
    ) {
        let content = &self.0.content;
        (
            content.curves.iter().find(|c| &c.curve_id == id),
            content.curve_recipes.iter().find(|c| &c.curve_id == id),
            content
                .calibration
                .as_ref()
                .filter(|c| c.curves.iter().any(|p| &p.curve_id == id)),
        )
    }
    /// Quality of this snapshot's inputs in `inputs` alone, at the snapshot's own time.
    pub fn quality_of(&self, inputs: &InputSet<'_>) -> SnapshotQuality {
        derive_quality(&self.0.content, self.quality_time(), inputs).0
    }
    /// [`MarketSnapshot::quality_of`], and the window of snapshot times over which the
    /// same inputs keep that quality.
    pub fn judge(&self, inputs: &InputSet<'_>) -> (SnapshotQuality, QualityWindow) {
        let judged_at = self.quality_time();
        let (quality, until_ms) = derive_quality(&self.0.content, judged_at, inputs);
        (
            quality,
            QualityWindow {
                judged_at,
                until_ms,
            },
        )
    }
    /// Whether this snapshot's time lies in `window`, so inputs this snapshot shares with
    /// the one judged there keep the quality they were judged to have. A time before
    /// the window's start is outside it.
    pub fn within(&self, window: &QualityWindow) -> bool {
        let now = self.quality_time();
        now >= window.judged_at
            && window
                .until_ms
                .is_none_or(|until| now.datetime().timestamp_millis() < until)
    }
    /// The time quality is judged at: the later of the two cutoffs.
    pub fn quality_time(&self) -> UtcTimestamp {
        self.0.content.as_of.max(self.0.content.known_at)
    }
    /// The instruments whose quote entries are not shared with `previous`'s, in snapshot
    /// order and each named once. `None` unless everything else a valuation reads is
    /// the same in both: the frame ([`MarketSnapshot::same_frame`]), a shared reference
    /// section, equal fixings, required curves, curve records, recipes and calibration,
    /// and the same quote keys in the same order. An instrument it does not list then has
    /// the same frame here as in `previous` and the same quotes of its own; it has the
    /// same inputs by [`MarketSnapshot::same_inputs`]'s rule unless it reads a listed
    /// benchmark's quotes, which its caller follows.
    pub fn changed_quotes(&self, previous: &MarketSnapshot) -> Option<Vec<&str>> {
        let (now, then) = (&self.0.content, &previous.0.content);
        if !self.same_frame(previous)
            || !now.references.ptr_eq(&then.references)
            || now.quotes.len() != then.quotes.len()
            || now.fixings != then.fixings
            || now.required_curves != then.required_curves
            || now.curves != then.curves
            || now.curve_recipes != then.curve_recipes
            || now.calibration != then.calibration
        {
            return None;
        }
        let mut changed: Vec<&str> = Vec::new();
        for (quote, prior) in now.quotes.iter().zip(&then.quotes) {
            if quote.ptr_eq(prior) {
                continue;
            }
            if quote.requirement.key != prior.requirement.key {
                return None;
            }
            // Quotes are sorted by instrument, so one instrument's are adjacent.
            let instrument = quote.requirement.key.instrument_id.as_str();
            if changed.last() != Some(&instrument) {
                changed.push(instrument);
            }
        }
        Some(changed)
    }
    /// Quality of one curve alone; a curve the snapshot does not require is `Invalid`.
    pub fn curve_quality(&self, curve_id: &str) -> DataQuality {
        let curves = &self.0.content.required_curves;
        match curves.iter().find(|id| id.as_str() == curve_id) {
            Some(id) => {
                self.quality_of(&InputSet {
                    own_quotes: &[],
                    own_level: OwnLevel::Field(None),
                    benchmark_quotes: Vec::new(),
                    fixings: Vec::new(),
                    curves: vec![id],
                })
                .quality
            }
            None => DataQuality::Invalid,
        }
    }
    /// Re-derive identity and quality from the content and compare. Deserialization runs
    /// this; a value made by [`MarketSnapshot::new`] passes by construction.
    pub fn verify(&self) -> Result<(), TraitError> {
        let verified = Self::bounded(self.0.content.clone(), self.bounds())?;
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
            || !self.quality().complete
            || self.quality().quality != DataQuality::Valid
            || !derive_quality(
                &self.0.content,
                decision_time,
                &InputSet::all(&self.0.content),
            )
            .0
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
/// Schemas 1–3: SHA-256 under the schema's domain over the canonical JSON of normalized
/// content, the bytes `serde_json::to_vec` writes for it. They stream into the hash,
/// bounded as they go, and the references are written from their section's cached JSON.
/// Schema 4 hashes by section ([`sectioned_identity`]), and returns its quote
/// digests for the snapshot to keep.
fn identity_and_digests(
    content: &SnapshotContent,
    bounds: SnapshotBounds,
) -> Result<(SnapshotId, Option<QuoteDigests>), TraitError> {
    let domain: &[u8] = match content.schema_version {
        1 => b"convex.snapshot.v1\0",
        2 => b"convex.snapshot.v2\0",
        3 => b"convex.snapshot.v3\0",
        4 => return sectioned_identity(content, bounds).map(|(id, digests)| (id, Some(digests))),
        _ => return Err(TraitError::InvalidInput("unsupported snapshot schema")),
    };
    let canonical = CanonicalContent::new(content, true)?;
    let mut hash = Sha256::new();
    hash.update(domain);
    let limit = bounds.identity_bytes();
    Ok((
        snapshot_id(hash_json(hash, &canonical, &mut 0, limit)?)?,
        None,
    ))
}

/// Schema 4: SHA-256 under `convex.snapshot.v4\0` over, in order,
/// 1. the reference section's digest, 32 bytes: SHA-256 under
///    `convex.snapshot.v4.references\0` over the records' canonical JSON, computed once
///    per section;
/// 2. the number of quotes, 8 bytes big-endian;
/// 3. each quote's digest in snapshot order, 32 bytes: SHA-256 under
///    `convex.snapshot.v4.quote\0` over the quote's canonical JSON, computed once per
///    entry;
/// 4. the frame's canonical JSON: the content without `references` and `quotes`, its
///    other fields in their order and under their skip rules.
///
/// Every part before the frame has a fixed or stated length, and the frame runs to the
/// end, so one byte string names one content. The JSON hashed for the three sections
/// stays within the bounds' [`SnapshotBounds::identity_bytes`] together, as schemas
/// 1–3's whole content does. The quote digests are gathered in order into one buffer,
/// which the snapshot keeps.
fn sectioned_identity(
    content: &SnapshotContent,
    bounds: SnapshotBounds,
) -> Result<(SnapshotId, QuoteDigests), TraitError> {
    let mut written = content.references.digest()?.len;
    let digests = QuoteDigests::of(&content.quotes, &mut written, bounds)?;
    Ok((hash_sections(content, &digests, bounds)?, digests))
}

/// Schema 4's identity of `content` over `digests`, its quote digests in order.
fn hash_sections(
    content: &SnapshotContent,
    digests: &QuoteDigests,
    bounds: SnapshotBounds,
) -> Result<SnapshotId, TraitError> {
    let references = content.references.digest()?;
    if digests.bytes.len() != content.quotes.len().saturating_mul(DIGEST_BYTES) {
        return Err(TraitError::InvalidInput(
            "quote digests do not match the quotes",
        ));
    }
    let limit = bounds.identity_bytes();
    let mut written = references.len.saturating_add(digests.len);
    if written > limit {
        return Err(TraitError::CapacityExceeded);
    }
    let mut hash = Sha256::new();
    hash.update(SCHEMA_FOUR);
    hash.update(references.digest);
    hash.update(
        u64::try_from(content.quotes.len())
            .map_err(|_| TraitError::CapacityExceeded)?
            .to_be_bytes(),
    );
    hash.update(&digests.bytes);
    let frame = CanonicalContent::new(content, false)?;
    snapshot_id(hash_json(hash, &frame, &mut written, limit)?)
}

/// The length of one SHA-256 digest.
const DIGEST_BYTES: usize = 32;

/// Schema 4's quote digests in snapshot order, one after another, and the summed length
/// of the JSON they hash. Identity hashes these exact bytes, so a snapshot
/// derived from another copies them and replaces only its rebuilt entries'.
#[derive(Debug, PartialEq)]
struct QuoteDigests {
    bytes: Box<[u8]>,
    len: usize,
}

impl QuoteDigests {
    /// Every entry's digest, in order. `written` counts the JSON one identity has hashed
    /// so far, and the walk fails once it passes the bounds', as identity does.
    fn of(
        quotes: &[SharedQuote],
        written: &mut usize,
        bounds: SnapshotBounds,
    ) -> Result<Self, TraitError> {
        let mut bytes = Vec::with_capacity(quotes.len().saturating_mul(DIGEST_BYTES));
        let mut len = 0usize;
        for quote in quotes {
            let digest = quote.digest()?;
            *written = written.saturating_add(digest.len);
            if *written > bounds.identity_bytes() {
                return Err(TraitError::CapacityExceeded);
            }
            len = len.saturating_add(digest.len);
            bytes.extend_from_slice(&digest.digest);
        }
        Ok(Self {
            bytes: bytes.into(),
            len,
        })
    }
    /// These digests, of `previous`, with the entries at `rebuilt` replaced by those of
    /// `quotes`, which holds `previous`'s entries everywhere else.
    fn patched(
        &self,
        previous: &[SharedQuote],
        quotes: &[SharedQuote],
        rebuilt: &[usize],
    ) -> Result<Self, TraitError> {
        let size = previous.len().saturating_mul(DIGEST_BYTES);
        if self.bytes.len() != size || quotes.len() != previous.len() {
            return Err(TraitError::InvalidInput(
                "quote digests do not match the quotes",
            ));
        }
        let mut bytes = self.bytes.clone();
        let mut len = self.len;
        for &position in rebuilt {
            let (Some(old), Some(new)) = (previous.get(position), quotes.get(position)) else {
                return Err(TraitError::InvalidInput("rebuilt position out of range"));
            };
            let (old, new) = (old.digest()?, new.digest()?);
            len = len
                .checked_sub(old.len)
                .and_then(|len| len.checked_add(new.len))
                .ok_or(TraitError::CapacityExceeded)?;
            let start = position.saturating_mul(DIGEST_BYTES);
            bytes
                .get_mut(start..start.saturating_add(DIGEST_BYTES))
                .ok_or(TraitError::InvalidInput("rebuilt position out of range"))?
                .copy_from_slice(&new.digest);
        }
        Ok(Self { bytes, len })
    }
}

/// Stream `value`'s canonical JSON into `hash`. `written` counts the JSON bytes one
/// identity has hashed so far, and the identity fails once they pass `limit`.
fn hash_json<T: Serialize + ?Sized>(
    hash: Sha256,
    value: &T,
    written: &mut usize,
    limit: usize,
) -> Result<Sha256, TraitError> {
    let mut writer = std::io::BufWriter::new(BoundedDigest {
        hash,
        len: *written,
        limit,
    });
    let result = serde_json::to_writer(&mut writer, value)
        .map_err(drop)
        .and_then(|()| writer.flush().map_err(drop));
    *written = writer.get_ref().len;
    if result.is_err() {
        return Err(if *written > limit {
            TraitError::CapacityExceeded
        } else {
            TraitError::InvalidInput("snapshot serialization failed")
        });
    }
    let (digest, _) = writer.into_parts();
    Ok(digest.hash)
}

fn snapshot_id(hash: Sha256) -> Result<SnapshotId, TraitError> {
    SnapshotId::new(format!("sha256:{:x}", hash.finalize()))
        .map_err(|_| TraitError::InvalidInput("invalid snapshot identity"))
}

/// A hash that refuses bytes beyond `limit`.
struct BoundedDigest {
    hash: Sha256,
    len: usize,
    limit: usize,
}

impl std::io::Write for BoundedDigest {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.len = self.len.saturating_add(bytes.len());
        if self.len > self.limit {
            return Err(std::io::Error::other("snapshot exceeds its bound"));
        }
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// [`SnapshotContent`] as identity writes it. Schemas 1–3 write every field, the
/// references from their cached JSON; schema 4's frame leaves out the references and the
/// quotes. The fields, their order and their skip rules mirror `SnapshotContent`; tests
/// compare both identities with SHA-256 over `serde_json::to_vec` of the content.
#[derive(Serialize)]
struct CanonicalContent<'a> {
    schema_version: u32,
    as_of: &'a UtcTimestamp,
    known_at: &'a UtcTimestamp,
    generation: &'a Version,
    #[serde(serialize_with = "snapshot_configuration::serialize")]
    configuration: &'a BondPricingConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    configuration_capture: Option<&'a crate::configuration::ConfigurationCapture>,
    #[serde(skip_serializing_if = "Option::is_none")]
    references: Option<&'a RawValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quotes: Option<&'a [SharedQuote]>,
    fixings: &'a [SnapshotFixing],
    required_curves: &'a [CurveId],
    curves: &'a [SnapshotCurve],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    curve_recipes: &'a [SnapshotCurveRecipe],
    #[serde(skip_serializing_if = "Option::is_none")]
    calibration: Option<&'a SnapshotCalibration>,
    max_source_skew_ms: i64,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    benchmark_sets: &'a [ResolvedBenchmarkSet],
}

impl<'a> CanonicalContent<'a> {
    /// The whole content, or schema 4's frame when `sections` is false.
    fn new(content: &'a SnapshotContent, sections: bool) -> Result<Self, TraitError> {
        Ok(Self {
            schema_version: content.schema_version,
            as_of: &content.as_of,
            known_at: &content.known_at,
            generation: &content.generation,
            configuration: &content.configuration,
            configuration_capture: content.configuration_capture.as_ref(),
            references: if sections {
                Some(content.references.json()?)
            } else {
                None
            },
            quotes: sections.then_some(content.quotes.as_slice()),
            fixings: &content.fixings,
            required_curves: &content.required_curves,
            curves: &content.curves,
            curve_recipes: &content.curve_recipes,
            calibration: content.calibration.as_ref(),
            max_source_skew_ms: content.max_source_skew_ms,
            benchmark_sets: &content.benchmark_sets,
        })
    }
}

impl TryFrom<SnapshotWire> for MarketSnapshot {
    type Error = TraitError;
    /// The wire's quality is kept, so `verify` compares it with the derived one.
    fn try_from(value: SnapshotWire) -> Result<Self, Self::Error> {
        let result = Self(Snapshot {
            snapshot_id: value.snapshot_id,
            content: value.content,
            quality: OnceLock::from(value.quality),
            digests: OnceLock::new(),
            bounds: SnapshotBounds::Standard,
        });
        result.verify()?;
        Ok(result)
    }
}
impl From<MarketSnapshot> for SnapshotWire {
    fn from(value: MarketSnapshot) -> Self {
        let quality = value.quality().clone();
        Self {
            snapshot_id: value.0.snapshot_id,
            content: value.0.content,
            quality,
        }
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

fn normalize(content: &mut SnapshotContent, bounds: SnapshotBounds) -> Result<(), TraitError> {
    if content.references.len() > bounds.instruments()
        || content.quotes.len() > bounds.instruments()
    {
        return Err(TraitError::CapacityExceeded);
    }
    normalize_inputs(content)?;
    content.references.sort();
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
        // Only an entry whose fields are out of order is copied; a shared one is sorted.
        if !quote.requirement.fields.is_sorted() {
            quote.make_mut().requirement.fields.sort();
        }
    }
    Ok(())
}

/// [`normalize`] for what does not grow with the instruments: the bounds and order of the
/// fixings, curves, recipes and calibration.
fn normalize_inputs(content: &mut SnapshotContent) -> Result<(), TraitError> {
    if content.fixings.len() > MAX_SNAPSHOT_FIXINGS
        || content.curves.len()
            + content.curve_recipes.len()
            + content.calibration.as_ref().map_or(0, |c| c.curves.len())
            > MAX_SNAPSHOT_CURVES
        || content.required_curves.len() > MAX_SNAPSHOT_CURVES
        || content.benchmark_sets.len() > MAX_SNAPSHOT_BENCHMARK_SETS
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
    content.fixings.sort_by(|a, b| a.key.cmp(&b.key));
    if !content
        .benchmark_sets
        .is_sorted_by(|a, b| a.benchmark_set_id <= b.benchmark_set_id)
    {
        let mut sets = content.benchmark_sets.to_vec();
        sets.sort_by(|a, b| a.benchmark_set_id.cmp(&b.benchmark_set_id));
        content.benchmark_sets = sets.into();
    }
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
    validate_configuration(content, true)?;
    content.references.validate(ReferenceLookup {
        as_of: content.as_of,
        known_at: content.known_at,
    })?;
    let as_of_ms = content.as_of.datetime().timestamp_millis();
    let known_at_ms = content.known_at.datetime().timestamp_millis();
    for (i, quote) in content.quotes.iter().enumerate() {
        quote.check()?;
        let key = &quote.requirement.key;
        // `normalize` sorted the quotes by instrument, then source, so a repeat is adjacent.
        if content
            .references
            .find(key.instrument_id.as_str())
            .is_none()
            || (i > 0 && content.quotes[i - 1].requirement.key == *key)
        {
            return Err(TraitError::InvalidInput(
                "invalid/duplicate quote requirement",
            ));
        }
        check_quote_at(quote, as_of_ms, known_at_ms)?;
    }
    validate_benchmark_sets(content)?;
    validate_inputs(content)
}

/// [`MarketSnapshot::next`]'s checks of `content`, whose quote entries at `rebuilt` are
/// not its last snapshot's: [`validate_content`]'s, in its order, but for what holds
/// because `content` follows that snapshot. It reads the references' bounds and the
/// rebuilt entries, and nothing else that grows with the instruments. The benchmark sets
/// are that snapshot's, checked at a cutoff no later than these.
fn derive(content: &mut SnapshotContent, rebuilt: &[usize]) -> Result<(), TraitError> {
    normalize_inputs(content)?;
    validate_configuration(content, false)?;
    content.references.validate(ReferenceLookup {
        as_of: content.as_of,
        known_at: content.known_at,
    })?;
    let as_of_ms = content.as_of.datetime().timestamp_millis();
    let known_at_ms = content.known_at.datetime().timestamp_millis();
    for &position in rebuilt {
        let quote = &content.quotes[position];
        quote.check()?;
        check_quote_at(quote, as_of_ms, known_at_ms)?;
    }
    validate_inputs(content)
}

/// Whether a snapshot holds no benchmark sets, so its content leaves them out.
fn holds_no_sets(sets: &Arc<[ResolvedBenchmarkSet]>) -> bool {
    sets.is_empty()
}

/// Whether two snapshots hold the same benchmark sets: one pinned section, or none.
fn same_sets(now: &Arc<[ResolvedBenchmarkSet]>, then: &Arc<[ResolvedBenchmarkSet]>) -> bool {
    Arc::ptr_eq(now, then) || (now.is_empty() && then.is_empty())
}

/// A snapshot's benchmark sets are each valid and recorded by the cutoff, with distinct
/// ids in order. They are pinned with the references, so their effective intervals are
/// not compared with later cutoffs.
fn validate_benchmark_sets(content: &SnapshotContent) -> Result<(), TraitError> {
    let invalid = |_| TraitError::InvalidInput("invalid snapshot benchmark set");
    for set in content.benchmark_sets.iter() {
        set.validate(content.known_at).map_err(invalid)?;
    }
    if content
        .benchmark_sets
        .windows(2)
        .any(|pair| pair[0].benchmark_set_id >= pair[1].benchmark_set_id)
    {
        return Err(TraitError::InvalidInput("duplicate snapshot benchmark set"));
    }
    Ok(())
}

/// The checks of the schema, the configuration and its capture at the cutoffs. With
/// `match_quotes`, each applied override must name a selected quote at its age limit, a
/// walk of every quote; [`derive`] skips it, since its quotes have their last snapshot's
/// requirements and its capture is that snapshot's.
fn validate_configuration(content: &SnapshotContent, match_quotes: bool) -> Result<(), TraitError> {
    if !((content.schema_version == 1
        && content.curve_recipes.is_empty()
        && content.calibration.is_none())
        || (content.schema_version == 2
            && !content.curve_recipes.is_empty()
            && content.calibration.is_none())
        || (content.schema_version == 3 && content.calibration.is_some())
        || content.schema_version == SNAPSHOT_SCHEMA)
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
                || (match_quotes
                    && !content.quotes.iter().any(|q| {
                        q.requirement.key == override_.scope
                            && q.requirement.max_age_ms == override_.max_age_ms
                    }))
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
        .map_err(TraitError::InvalidInput)
}

/// The checks of the fixings, required curves, curve records, recipes and calibration.
fn validate_inputs(content: &SnapshotContent) -> Result<(), TraitError> {
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

/// The checks that read the quote alone: its requirement's key, fields and age policy,
/// its events' structure and source, and how its image agrees with its delivery. Each
/// entry runs them once ([`SharedQuote`]).
fn check_quote(quote: &SnapshotQuote) -> Result<(), TraitError> {
    use crate::reference_repository::validate_reference_id;
    let requirement = &quote.requirement;
    validate_reference_id(&requirement.key.instrument_id)
        .map_err(|_| TraitError::InvalidInput("invalid quote instrument"))?;
    crate::quote_event::validate_text(&requirement.key.source, 128)?;
    // `normalize` sorted the fields, so a repeat is adjacent.
    if requirement.fields.is_empty()
        || requirement.fields.len() > 12
        || requirement.fields.windows(2).any(|w| w[0] == w[1])
        || !(1..=86_400_000).contains(&requirement.max_age_ms)
    {
        return Err(TraitError::InvalidInput(
            "invalid/duplicate quote requirement",
        ));
    }
    let selected = |event: &QuoteEvent| -> Result<(), TraitError> {
        event.validate()?;
        if event.key != requirement.key {
            return Err(TraitError::InvalidInput(
                "quote outside selected source/time scope",
            ));
        }
        Ok(())
    };
    if let Some(delivery) = &quote.delivery_watermark {
        selected(delivery)?;
    }
    if let Some(image) = &quote.image {
        selected(&image.watermark)?;
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

/// The checks that read the snapshot's cutoffs, run for every snapshot: nothing in the
/// quote was observed after `as_of` or received after `known_at`.
fn check_quote_at(
    quote: &SnapshotQuote,
    as_of_ms: i64,
    known_at_ms: i64,
) -> Result<(), TraitError> {
    let events = quote
        .delivery_watermark
        .iter()
        .chain(quote.image.iter().map(|image| &image.watermark));
    for event in events {
        if event.source_time_ms > as_of_ms || event.receive_time_ms > known_at_ms {
            return Err(TraitError::InvalidInput(
                "quote outside selected source/time scope",
            ));
        }
    }
    let fields = quote.image.iter().flat_map(|image| image.fields.values());
    for value in fields {
        if value.source_time_ms > as_of_ms || value.receive_time_ms > known_at_ms {
            return Err(TraitError::InvalidInput(
                "invalid snapshot field observation",
            ));
        }
    }
    Ok(())
}

/// The quality of `inputs` at `now`, and the first millisecond at which one of its time
/// comparisons could turn ([`QualityWindow`]). Every time comparison runs, so each
/// one's turning point counts, even where another already decides the issue.
fn derive_quality(
    content: &SnapshotContent,
    now: UtcTimestamp,
    inputs: &InputSet<'_>,
) -> (SnapshotQuality, Option<i64>) {
    let now_ms = now.datetime().timestamp_millis();
    let now_sub_ms = now.datetime().timestamp_subsec_nanos() % 1_000_000;
    use SnapshotIssueKind as K;
    let mut until = None;
    let mut issues = Vec::new();
    let mut complete = true;
    let mut source_times = Vec::new();
    let mut add = |dependency: String, reason| issues.push(SnapshotIssue { dependency, reason });
    // A quote of the level a bond's rule quotes it by is the desk's or a feed's level: a
    // manual one is the desk's mark, and only its age is judged, not its skew. A bond's
    // benchmark quotes are judged as quotes of their own.
    let is_level = |quote: &SharedQuote| {
        let field = match inputs.own_level {
            OwnLevel::Field(field) => field,
            OwnLevel::ByRule => content
                .references
                .find(quote.requirement.key.instrument_id.as_str())
                .and_then(|reference| reference.pricing_rule.as_ref())
                .and_then(|rule| rule.method.level_field()),
        };
        field.is_some_and(|field| quote.requirement.fields.contains(&field))
    };
    let own = inputs
        .own_quotes
        .iter()
        .map(|quote| (quote, is_level(quote)));
    let benchmarks = inputs
        .benchmark_quotes
        .iter()
        .flat_map(|quotes| quotes.iter())
        .map(|quote| (quote, false));
    for (quote, is_own_level) in own.chain(benchmarks) {
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
            DataState::Manual if is_own_level => None,
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
                    if !is_own_level {
                        source_times.push(value.source_time_ms);
                    }
                    let max_age_ms = quote.requirement.max_age_ms;
                    let ahead = before(now_ms, value.receive_time_ms, &mut until)
                        | before(now_ms, value.source_time_ms, &mut until);
                    let aged = aged(
                        now_ms,
                        now_sub_ms,
                        value.source_time_ms,
                        0,
                        max_age_ms,
                        &mut until,
                    ) | aged(
                        now_ms,
                        now_sub_ms,
                        value.receive_time_ms,
                        0,
                        max_age_ms,
                        &mut until,
                    );
                    if ahead || aged {
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
                if aged(
                    now_ms,
                    now_sub_ms,
                    source_time.datetime().timestamp_millis(),
                    source_time.datetime().timestamp_subsec_nanos() % 1_000_000,
                    max_age_ms,
                    &mut until,
                ) | aged(
                    now_ms,
                    now_sub_ms,
                    recorded_at.datetime().timestamp_millis(),
                    recorded_at.datetime().timestamp_subsec_nanos() % 1_000_000,
                    max_age_ms,
                    &mut until,
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
    (
        SnapshotQuality {
            complete,
            quality,
            issues,
        },
        until,
    )
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

/// `now_ms < time_ms`. While it holds, it stops holding at `time_ms`, which `until`
/// then counts.
fn before(now_ms: i64, time_ms: i64, until: &mut Option<i64>) -> bool {
    let before = now_ms < time_ms;
    if before {
        turns_at(until, time_ms);
    }
    before
}

/// [`age_exceeded`]. While it does not hold, it may start to hold within the millisecond
/// the age reaches its maximum, which `until` then counts.
fn aged(
    now_ms: i64,
    now_sub_ms: u32,
    source_ms: i64,
    source_sub_ms: u32,
    maximum_ms: i64,
    until: &mut Option<i64>,
) -> bool {
    let aged = age_exceeded(now_ms, now_sub_ms, source_ms, source_sub_ms, maximum_ms);
    if !aged {
        turns_at(until, source_ms.saturating_add(maximum_ms));
    }
    aged
}

/// Keep the earlier of `until` and `at_ms`.
fn turns_at(until: &mut Option<i64>, at_ms: i64) {
    *until = Some(until.map_or(at_ms, |until| until.min(at_ms)));
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
