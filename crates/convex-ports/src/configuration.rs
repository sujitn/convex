//! Immutable staged configuration, active-pointer CAS and retained operator provenance.
#![forbid(unsafe_code)]

use crate::{
    fixing::FixingKey,
    snapshot::{content_digest, QuoteRequirement, SnapshotCurve, SnapshotCurveRecipe},
    storage::{BondPricingConfig, Page, Pagination},
    TraitError,
};
use async_trait::async_trait;
use convex_core::contracts::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Exact immutable pricing configuration selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationPin {
    /// Internal configuration identity.
    pub id: SourceId,
    /// Positive immutable version.
    pub version: Version,
}

/// Numerical compatibility settings; compiled policy identifies solver limits/tolerances.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumericalSettings {
    /// Versioned compiled numerical policy, not an undocumented default.
    pub policy_id: ModelId,
    /// Finite positive bump in basis points, where the router performs numerical risk.
    pub bump_bps: FiniteF64,
}

/// Exact immutable instrument reference selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentPin {
    /// Provider-neutral instrument identity.
    pub instrument_id: InstrumentId,
    /// Exact immutable reference revision.
    pub version: Version,
}

/// Bounded supplied assembly request. Source selection and requirements are explicit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotAssembly {
    /// Economic market time.
    pub as_of: UtcTimestamp,
    /// Publication/recording cutoff.
    pub known_at: UtcTimestamp,
    /// Captured application input generation.
    pub generation: Version,
    /// Exact immutable quantitative configuration.
    pub configuration: ConfigurationPin,
    /// Optional audited active selection. Older explicit pins retain their original wire content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_capture: Option<ConfigurationCapture>,
    /// Exact reference revisions to resolve, 1..=256.
    pub instruments: Vec<InstrumentPin>,
    /// Explicit selected sources/fields and per-product age policy.
    pub quotes: Vec<QuoteRequirement>,
    /// Required published fixing keys, at most 1,024.
    pub fixings: Vec<FixingKey>,
    /// Required curve identities, at most 64.
    pub required_curves: Vec<CurveId>,
    /// Explicit resolved supplied curve revisions (not current builder pointers).
    pub curves: Vec<SnapshotCurve>,
    /// Complete schema-2 native curve recipe records; omitted by older requests. A Live
    /// assembly that pins no curve data reads `required_curves` from the curve store at
    /// each snapshot's cutoff instead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curve_recipes: Vec<SnapshotCurveRecipe>,
    /// Optional complete original product-calibration capture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<crate::calibration::SnapshotCalibration>,
    /// Maximum selected quote-field source skew.
    pub max_source_skew_ms: i64,
}

/// Explicit interpretation of data time; historical replay never represents live strategy readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    /// Caller supplies exact event/cutoff times.
    Captured,
    /// File/pull replay retains original delivery and economic times.
    Replay,
    /// Subscribe to the configured canonical quote source; require new images after restart.
    Live,
}

/// Restart-bound process requirements; no database/MQ handle enters the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessRequirements {
    /// Selected adapter identity, validated against actual composition capabilities.
    pub storage: SourceId,
    /// Selected result transport identity.
    pub result_transport: SourceId,
    /// Bounded numerical concurrency, 1..=64.
    pub max_concurrent_calcs: usize,
    /// Shared writer/ownership capability required by managed configurations.
    pub shared_writers: bool,
}

/// Explicit selected quote-source requirements. These are capability names, never vendor fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapability {
    /// Normalized image/delta/control records.
    CanonicalQuotes,
    /// Deterministic finite replay with prefix verification.
    Replay,
    /// Recoverable subscription delivery.
    LiveImages,
}

/// Credential location only. Inline credentials have no representation in this schema.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SecretReference(String);
impl SecretReference {
    /// Accept env://NAME or secret://namespace/key; reject values, URLs and query strings.
    pub fn new(value: String) -> Result<Self, TraitError> {
        let valid = value.strip_prefix("env://").is_some_and(|name| {
            !name.is_empty()
                && name.len() <= 128
                && name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        }) || value.strip_prefix("secret://").is_some_and(|name| {
            !name.is_empty()
                && name.len() <= 256
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/' | b'.'))
        });
        if !valid {
            return Err(TraitError::InvalidInput("invalid secret reference"));
        }
        Ok(Self(value))
    }
}
impl std::fmt::Debug for SecretReference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}
impl TryFrom<String> for SecretReference {
    type Error = TraitError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<SecretReference> for String {
    fn from(value: SecretReference) -> String {
        value.0
    }
}

/// Integration requirements; actual adapter capabilities are supplied separately by composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderBinding {
    /// Internal provider identity selected by the quote keys.
    pub source: SourceId,
    /// Required advertised operations.
    pub capabilities: Vec<ProviderCapability>,
    /// Referenced credential; never resolved by quantitative code or exported by inspection.
    pub credential: Option<SecretReference>,
}

/// Explicit runtime model/operation choices. Changes require a new process activation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSettings {
    /// Durable runtime namespace.
    pub runtime_id: SourceId,
    /// Captured, replay or live source-time policy.
    pub mode: RuntimeMode,
    /// Explicit settlement date.
    pub settlement_date: BusinessDate,
    /// Exact executable identity.
    pub build_id: ModelId,
    /// Requested measures.
    pub measures: Vec<MeasureId>,
    /// Explicit existing numerical policy/settings.
    pub numerical_settings: NumericalSettings,
}

/// Approved freshness-policy exception. Model price overrides and market corrections are distinct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyOverride {
    /// Stable exception identity; revision is the containing immutable configuration version.
    pub id: SourceId,
    /// One selected instrument/source scope.
    pub scope: crate::quote_event::QuoteKey,
    /// Override freshness limit, 1..=86,400,000 UTC milliseconds.
    pub max_age_ms: i64,
    /// Operator creating the exception.
    pub actor: String,
    /// Required reason.
    pub reason: String,
    /// Immutable creation timestamp.
    pub created_at: UtcTimestamp,
    /// Required finite expiry; boundary is exclusive.
    pub expires_at: UtcTimestamp,
    /// Approval provenance. Authentication/role authorization belongs to the boundary.
    pub approved_by: String,
    /// Approval timestamp.
    pub approved_at: UtcTimestamp,
}
impl PolicyOverride {
    /// Validate finite bounds and retained actor/reason/approval history.
    pub fn validate(&self) -> Result<(), TraitError> {
        text(&self.actor, 128)?;
        text(&self.approved_by, 128)?;
        text(&self.reason, 4096)?;
        if !(1..=86_400_000).contains(&self.max_age_ms)
            || self.created_at > self.approved_at
            || self.approved_at >= self.expires_at
        {
            return Err(TraitError::InvalidInput(
                "invalid override bounds or approval times",
            ));
        }
        crate::quote_event::validate_text(self.scope.instrument_id.as_str(), 128)?;
        crate::quote_event::validate_text(self.scope.source.as_str(), 128)
    }
    /// Supplied valuation time and publication cutoff govern replay; no system clock lookup.
    pub fn applies_at(&self, as_of: UtcTimestamp, known_at: UtcTimestamp) -> bool {
        self.approved_at <= as_of && self.approved_at <= known_at && as_of < self.expires_at
    }
}

/// Complete, immutable schema-1 application settings. Quantitative inputs have no defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationDocument {
    /// Must be 1.
    pub schema_version: u32,
    /// Exact quantitative configuration content and revision.
    #[serde(with = "crate::snapshot::snapshot_configuration")]
    pub pricing: BondPricingConfig,
    /// Separate process/backend requirements.
    pub process: ProcessRequirements,
    /// Separate provider-neutral requirements and credential references.
    pub providers: Vec<ProviderBinding>,
    /// Explicit model/operation settings.
    pub runtime: RuntimeSettings,
    /// Exact reference/curve pins and base snapshot policies. Initial generation must be one.
    pub assembly: SnapshotAssembly,
    /// Controlled exceptions; expiration does not delete their immutable history.
    pub overrides: Vec<PolicyOverride>,
}
impl ConfigurationDocument {
    /// Structural validation only. Actual references/providers and numerical fixtures are engine checks.
    pub fn validate(&self) -> Result<(), TraitError> {
        crate::storage::validate_pricing_config(&self.pricing)?;
        if self.schema_version != 1
            || !self.pricing.active
            || self.pricing.created_at < 0
            || self.assembly.configuration.id.as_str() != self.pricing.config_id
            || self.assembly.configuration.version.get() != self.pricing.version
            || self.assembly.configuration_capture.is_some()
            || self.assembly.generation.get() != 1
            || !(1..=64).contains(&self.process.max_concurrent_calcs)
            || self.providers.is_empty()
            || self.providers.len() > 64
            || self.overrides.len() > 256
            || self.assembly.instruments.is_empty()
            || self.assembly.instruments.len() > 256
            || self.assembly.quotes.len() != self.assembly.instruments.len()
            || self.assembly.curves.len()
                + self.assembly.curve_recipes.len()
                + self
                    .assembly
                    .calibration
                    .as_ref()
                    .map_or(0, |c| c.curves.len())
                > 64
            || self.assembly.required_curves.len() > 64
            || self.assembly.fixings.len() > 1024
            || !(0..=86_400_000).contains(&self.assembly.max_source_skew_ms)
            || self.runtime.measures.is_empty()
            || self.runtime.measures.len() > 64
        {
            return Err(TraitError::InvalidInput(
                "invalid configuration schema, pins or bounds",
            ));
        }
        let mut providers = BTreeSet::new();
        for binding in &self.providers {
            let mut capabilities = BTreeSet::new();
            if !providers.insert(&binding.source)
                || binding.capabilities.is_empty()
                || binding.capabilities.iter().any(|c| !capabilities.insert(c))
            {
                return Err(TraitError::InvalidInput(
                    "duplicate/empty provider requirement",
                ));
            }
        }
        let mut instruments = BTreeSet::new();
        let mut keys = std::collections::HashSet::new();
        for pin in &self.assembly.instruments {
            if !instruments.insert(&pin.instrument_id) {
                return Err(TraitError::InvalidInput("duplicate instrument pin"));
            }
        }
        for quote in &self.assembly.quotes {
            if !keys.insert(&quote.key)
                || !(1..=86_400_000).contains(&quote.max_age_ms)
                || !instruments
                    .iter()
                    .any(|i| i.as_str() == quote.key.instrument_id.as_str())
                || quote.fields.is_empty()
                || quote.fields.len() > 12
            {
                return Err(TraitError::InvalidInput("invalid quote requirement"));
            }
            let mut fields = BTreeSet::new();
            if quote.fields.iter().any(|f| !fields.insert(f)) {
                return Err(TraitError::InvalidInput("duplicate quote field"));
            }
        }
        let mut curves = BTreeSet::new();
        for curve in &self.assembly.curves {
            curve.validate()?;
            if !curves.insert(&curve.curve_id) {
                return Err(TraitError::InvalidInput("duplicate curve pin"));
            }
        }
        for curve in &self.assembly.curve_recipes {
            curve.validate()?;
            if !curves.insert(&curve.curve_id) {
                return Err(TraitError::InvalidInput("duplicate curve pin"));
            }
        }
        if let Some(calibration) = &self.assembly.calibration {
            calibration.validate()?;
            for pin in &calibration.curves {
                if !curves.insert(&pin.curve_id) {
                    return Err(TraitError::InvalidInput("duplicate calibration curve pin"));
                }
            }
        }
        if self
            .assembly
            .curves
            .iter()
            .map(|c| c.points.len())
            .sum::<usize>()
            + self
                .assembly
                .curve_recipes
                .iter()
                .map(SnapshotCurveRecipe::input_count)
                .sum::<usize>()
            + self
                .assembly
                .calibration
                .as_ref()
                .map_or(0, |c| c.input_count())
            > crate::snapshot::MAX_SNAPSHOT_PILLARS
        {
            return Err(TraitError::CapacityExceeded);
        }
        let mut ids = BTreeSet::new();
        let mut scopes = std::collections::HashSet::new();
        for override_ in &self.overrides {
            override_.validate()?;
            if !ids.insert(&override_.id)
                || !scopes.insert(&override_.scope)
                || !keys.contains(&override_.scope)
                || override_.approved_at.datetime().timestamp_millis() > self.pricing.updated_at
            {
                return Err(TraitError::InvalidInput(
                    "invalid/duplicate override scope or revision time",
                ));
            }
        }
        Ok(())
    }
    /// Domain-separated identity over exact stored settings, including credential locations.
    pub fn digest(&self) -> Result<String, TraitError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|_| TraitError::InvalidInput("configuration encoding failed"))?;
        if bytes.len() > MAX_CONFIGURATION_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        Ok(content_digest(b"convex.configuration.v1\0", &bytes))
    }
    /// Safe inspection/export. Credential locations are always replaced, including before logging.
    pub fn redacted(&self) -> Result<serde_json::Value, TraitError> {
        let mut value = serde_json::to_value(self)
            .map_err(|_| TraitError::InvalidInput("configuration inspection failed"))?;
        if let Some(bindings) = value.get_mut("providers").and_then(|v| v.as_array_mut()) {
            for binding in bindings {
                if binding.get("credential").is_some_and(|v| !v.is_null()) {
                    binding["credential"] = serde_json::json!("[REDACTED]");
                }
            }
        }
        Ok(value)
    }
    /// Base policies followed by eligible approved overrides. Expired values are never applied.
    pub fn effective_assembly(
        &self,
        as_of: UtcTimestamp,
        known_at: UtcTimestamp,
        generation: Version,
    ) -> Result<SnapshotAssembly, TraitError> {
        self.validate()?;
        let mut assembly = self.assembly.clone();
        assembly.as_of = as_of;
        assembly.known_at = known_at;
        assembly.generation = generation;
        for override_ in &self.overrides {
            if override_.applies_at(as_of, known_at) {
                for quote in &mut assembly.quotes {
                    if quote.key == override_.scope {
                        quote.max_age_ms = override_.max_age_ms;
                    }
                }
            }
        }
        Ok(assembly)
    }
}
/// Maximum one configuration document, including supplied curves.
pub const MAX_CONFIGURATION_BYTES: usize = 2 * 1024 * 1024;

/// Setting activation requirement. The enum has no implicit best-effort reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReloadClass {
    /// Safe for the current declared runtime footprint.
    Hot,
    /// Requires stopped-runtime validation and a compatible new process/plan.
    RestartRequired,
}
/// Redacted field-level impact; values stay in the explicit inspection view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingChange {
    /// Declared setting path.
    pub field: String,
    /// Required activation mode.
    pub reload: ReloadClass,
}
/// Compute the closed initial reload contract, ignoring only immutable revision metadata.
pub fn configuration_diff(
    previous: &ConfigurationDocument,
    next: &ConfigurationDocument,
) -> Vec<SettingChange> {
    let mut changes = Vec::new();
    let mut check = |field: &str, changed, reload| {
        if changed {
            changes.push(SettingChange {
                field: field.into(),
                reload,
            });
        }
    };
    use ReloadClass::*;
    check("process", previous.process != next.process, RestartRequired);
    check(
        "providers",
        previous.providers != next.providers,
        RestartRequired,
    );
    check("runtime", previous.runtime != next.runtime, RestartRequired);
    check(
        "assembly.instruments",
        previous.assembly.instruments != next.assembly.instruments,
        RestartRequired,
    );
    check(
        "assembly.fixings",
        previous.assembly.fixings != next.assembly.fixings,
        RestartRequired,
    );
    check(
        "assembly.curves",
        previous.assembly.curves != next.assembly.curves
            || previous.assembly.curve_recipes != next.assembly.curve_recipes
            || previous.assembly.calibration != next.assembly.calibration
            || previous.assembly.required_curves != next.assembly.required_curves,
        RestartRequired,
    );
    let selections = |document: &ConfigurationDocument| {
        document
            .assembly
            .quotes
            .iter()
            .map(|q| (q.key.clone(), q.fields.clone()))
            .collect::<Vec<_>>()
    };
    check(
        "assembly.quote_sources_and_fields",
        selections(previous) != selections(next),
        RestartRequired,
    );
    check(
        "pricing.method_and_conventions",
        previous.pricing.pricing_spec != next.pricing.pricing_spec
            || previous.pricing.applies_to != next.pricing.applies_to
            || previous.pricing.analytics_curves != next.pricing.analytics_curves
            || previous.pricing.bid_ask_spread != next.pricing.bid_ask_spread,
        RestartRequired,
    );
    check(
        "pricing.description_and_priority",
        previous.pricing.description != next.pricing.description
            || previous.pricing.priority != next.pricing.priority,
        Hot,
    );
    check(
        "assembly.quote_freshness",
        previous
            .assembly
            .quotes
            .iter()
            .map(|q| q.max_age_ms)
            .collect::<Vec<_>>()
            != next
                .assembly
                .quotes
                .iter()
                .map(|q| q.max_age_ms)
                .collect::<Vec<_>>(),
        Hot,
    );
    check(
        "assembly.max_source_skew_ms",
        previous.assembly.max_source_skew_ms != next.assembly.max_source_skew_ms,
        Hot,
    );
    check("overrides", previous.overrides != next.overrides, Hot);
    changes
}

/// Where the regression fixtures that validated a staging or activation came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureEvidence {
    /// Checked against the composition's canonical replay prefix.
    #[default]
    ReplayChecked,
    /// Supplied by an operator with no replay trace to check them against.
    OperatorSupplied,
}
impl FixtureEvidence {
    fn is_replay_checked(&self) -> bool {
        *self == Self::ReplayChecked
    }
}
/// Operator metadata attached atomically to staged immutable content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedConfiguration {
    /// Complete typed settings.
    pub document: ConfigurationDocument,
    /// Unique staging request; exact retry includes this metadata.
    pub idempotency_key: RequestId,
    /// Named operator at the trusted boundary.
    pub actor: String,
    /// Required change reason.
    pub reason: String,
    /// Recording time, independent of fixture/economic time.
    pub staged_at: UtcTimestamp,
    /// Fixture provenance, omitted when replay-checked so earlier records keep their bytes.
    #[serde(default, skip_serializing_if = "FixtureEvidence::is_replay_checked")]
    pub fixture_evidence: FixtureEvidence,
}
impl StagedConfiguration {
    /// Validate operator metadata and immutable settings.
    pub fn validate(&self) -> Result<(), TraitError> {
        self.document.validate()?;
        text(&self.actor, 128)?;
        text(&self.reason, 4096)?;
        if self.staged_at.datetime().timestamp_millis() < self.document.pricing.updated_at {
            return Err(TraitError::InvalidInput(
                "staging precedes configuration revision",
            ));
        }
        Ok(())
    }
}
/// Activation intent; rollback retains the target's immutable content and appends another record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActivationKind {
    /// Select a staged revision.
    Activate,
    /// Select content from a retained activation.
    Rollback {
        /// Exact prior activation sequence.
        to_revision: Version,
    },
}
/// Durable compare-and-set command. Zero expected revision means no active selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationCommand {
    /// Configuration identity.
    pub id: SourceId,
    /// Exact staged content revision.
    pub configuration_version: Version,
    /// Expected active pointer sequence, independent of staged version.
    pub expected_revision: u64,
    /// Stable identity for outcome reconciliation and identical retries.
    pub idempotency_key: RequestId,
    /// Named operator.
    pub actor: String,
    /// Required reason.
    pub reason: String,
    /// Recording/effective selection time; supplied, never manufactured on retry.
    pub activated_at: UtcTimestamp,
    /// Requested reload contract.
    pub mode: ReloadClass,
    /// Activation or retained rollback target.
    pub action: ActivationKind,
    /// Fixture provenance, omitted when replay-checked so earlier records keep their bytes.
    #[serde(default, skip_serializing_if = "FixtureEvidence::is_replay_checked")]
    pub fixture_evidence: FixtureEvidence,
}
impl ActivationCommand {
    /// Validate bounded metadata and CAS revision capacity.
    pub fn validate(&self) -> Result<(), TraitError> {
        text(&self.actor, 128)?;
        text(&self.reason, 4096)?;
        if self.expected_revision == u64::MAX {
            return Err(TraitError::CapacityExceeded);
        }
        Ok(())
    }
}
/// Atomic pointer/audit history record. Sequence increases even when rolling back content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationRecord {
    /// Durable active selection revision.
    pub revision: Version,
    /// Original command/actor/reason/time; never refreshed on retry.
    pub command: ActivationCommand,
    /// Previously selected content, if any.
    pub previous_version: Option<Version>,
    /// Exact selected document digest.
    pub configuration_digest: String,
}
impl ActivationRecord {
    /// Validate structural pointer lineage before consumption.
    pub fn validate(&self) -> Result<(), TraitError> {
        self.command.validate()?;
        if self.command.expected_revision.checked_add(1) != Some(self.revision.get())
            || self.configuration_digest.len() != 64
            || !self
                .configuration_digest
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(TraitError::InvalidInput("invalid activation lineage"));
        }
        Ok(())
    }
}
/// Coherent selection read from one storage transaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveConfiguration {
    /// Active pointer and audit record.
    pub activation: ActivationRecord,
    /// Exact immutable content selected by that pointer.
    pub staged: StagedConfiguration,
}
impl ActiveConfiguration {
    /// Verify adapter consistency without looking at a mutable latest-version pointer.
    pub fn validate(&self) -> Result<(), TraitError> {
        self.activation.validate()?;
        self.staged.validate()?;
        let doc = &self.staged.document;
        if self.activation.command.id.as_str() != doc.pricing.config_id
            || self.activation.command.configuration_version.get() != doc.pricing.version
            || self.activation.configuration_digest != doc.digest()?
            || self.activation.command.activated_at < self.staged.staged_at
        {
            return Err(TraitError::Conflict(
                "activation and staged configuration disagree",
            ));
        }
        Ok(())
    }
    /// Freeze the selection and applied exceptions into the numerical information set.
    pub fn capture(
        &self,
        as_of: UtcTimestamp,
        known_at: UtcTimestamp,
        generation: Version,
    ) -> Result<SnapshotAssembly, TraitError> {
        self.validate()?;
        if self.activation.command.activated_at > known_at {
            return Err(TraitError::InvalidInput(
                "active configuration is later than information cutoff",
            ));
        }
        let mut assembly = self
            .staged
            .document
            .effective_assembly(as_of, known_at, generation)?;
        assembly.configuration_capture = Some(ConfigurationCapture {
            activation: self.activation.clone(),
            applied_overrides: self
                .staged
                .document
                .overrides
                .iter()
                .filter(|o| o.applies_at(as_of, known_at))
                .cloned()
                .collect(),
        });
        Ok(assembly)
    }
}
/// Snapshot provenance, without credential references or copied curve/model settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationCapture {
    /// Original immutable selection record.
    pub activation: ActivationRecord,
    /// Only exceptions actually applied at this information set's supplied cutoffs.
    pub applied_overrides: Vec<PolicyOverride>,
}

/// Atomic staged content, active pointer and activation audit. Backend replacement implements these semantics.
#[async_trait]
pub trait ConfigurationRepository: Send + Sync {
    /// Atomically retain immutable document, staging metadata and exact ConfigStore pricing version.
    /// Staging must not change the active pointer; exact retries succeed, changed identities conflict.
    async fn stage(&self, staged: &StagedConfiguration) -> Result<(), TraitError>;
    /// Exact retained document; no mutable current-version fallback.
    async fn get_staged(
        &self,
        id: &SourceId,
        version: Version,
    ) -> Result<Option<StagedConfiguration>, TraitError>;
    /// Coherent active pointer/content read in one storage snapshot.
    async fn active(&self, id: &SourceId) -> Result<Option<ActiveConfiguration>, TraitError>;
    /// Atomic CAS plus append-only activation record; exact retry returns its original record.
    /// Hot mode must reject restart-required differences; rollback verifies its retained target.
    async fn activate(&self, command: &ActivationCommand) -> Result<ActivationRecord, TraitError>;
    /// Resolve ambiguous outcomes before retrying the identical command.
    async fn activation_for_request(
        &self,
        key: &RequestId,
    ) -> Result<Option<ActivationRecord>, TraitError>;
    /// Exact retained activation, including a rollback's original target.
    async fn get_activation(
        &self,
        id: &SourceId,
        revision: Version,
    ) -> Result<Option<ActivationRecord>, TraitError>;
    /// Bounded activation history in sequence order with a snapshot-consistent total.
    async fn history(
        &self,
        id: &SourceId,
        pagination: &Pagination,
    ) -> Result<Page<ActivationRecord>, TraitError>;
}

fn text(value: &str, max: usize) -> Result<(), TraitError> {
    crate::quote_event::validate_text(value, max)
}
