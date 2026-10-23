//! Local configuration control use cases. Numerical fixtures reuse the actual snapshot/router path.
#![forbid(unsafe_code)]
use crate::{
    snapshot::{SnapshotAssembler, SnapshotValuator},
    RuntimePlan,
};
use convex_core::contracts::*;
use convex_ports::{
    configuration::*,
    fixing::FixingRepository,
    quote_event::QuoteEvent,
    reference_repository::ReferenceRepository,
    snapshot::{content_digest, SnapshotRepository},
    storage::{ConfigStore, Page, Pagination},
    TraitError,
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use thiserror::Error;

mod compile;
pub use compile::{assemble_from_repository, PlanSelection, MAX_PLAN_BOOKS};

/// Actual composition capabilities, supplied by the adapter owner rather than configuration content.
#[derive(Debug, Clone)]
pub struct ConfigurationCapabilities {
    /// Build actually executing the numerical fixture path.
    pub build_id: ModelId,
    /// Selected storage adapter identity.
    pub storage: SourceId,
    /// Synchronous durable acceptance in the selected failure domain.
    pub durable: bool,
    /// Atomic active pointer and immutable audit/CAS transactions.
    pub atomic_activation: bool,
    /// Shared-writer support; local redb is false.
    pub shared_writers: bool,
    /// Selected acknowledged result transport.
    pub result_transport: SourceId,
    /// Actually configured provider identities/capabilities and credential references.
    pub providers: Vec<ProviderBinding>,
    /// Explicit demonstration composition: every output is labelled demo, so inputs and
    /// results of `Demo` quality pass validation and readiness. Otherwise only `Valid` does.
    pub demo: bool,
}
impl ConfigurationCapabilities {
    /// `Valid` always; `Demo` only in an explicit demonstration composition.
    pub fn accepts(&self, quality: DataQuality) -> bool {
        quality == DataQuality::Valid || (self.demo && quality == DataQuality::Demo)
    }
}
/// Caller-supplied numerical regression assertion; no expectation is manufactured by the service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedConfigurationMeasure {
    /// Declared fixed-bullet identity.
    pub instrument_id: InstrumentId,
    /// Requested mid measure.
    pub measure: MeasureId,
    /// Finite expected value in the measure's existing unit.
    pub value: FiniteF64,
    /// Explicit absolute tolerance, bounded to 0..=1e-8.
    pub tolerance: FiniteF64,
}
/// Bounded control-plane fixture. This is offline/captured evidence, never live certification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationFixture {
    /// Regression case identity.
    pub id: SourceId,
    /// Supplied market instant.
    pub as_of: UtcTimestamp,
    /// Supplied information cutoff.
    pub known_at: UtcTimestamp,
    /// Actual canonical image/updates to evaluate.
    pub events: Vec<QuoteEvent>,
    /// Nonempty explicit regression assertions; every instrument needs at least one assertion.
    pub expected: Vec<ExpectedConfigurationMeasure>,
}
/// Stable validation diagnostics; raw credentials, paths and adapter errors are not exported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigurationIssue {
    /// Setting or dependency category.
    pub field: String,
    /// Stable diagnostic identifier.
    pub code: String,
    /// Safe actionable explanation.
    pub message: String,
}
/// One real numerical fixture run with immutable lineage and result hash.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigurationFixtureCheck {
    /// Original fixture identity.
    pub id: SourceId,
    /// Unpublished content-addressed input identity.
    pub snapshot_id: SnapshotId,
    /// Hash over complete actual numerical result records.
    pub results_digest: String,
}
/// Shared dry-run/activation report. Invalid reports cannot activate.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigurationValidation {
    /// All structural/capability/reference/fixture checks passed.
    pub valid: bool,
    /// Exact candidate identity, absent on invalid structure.
    pub document_digest: Option<String>,
    /// Redacted candidate content.
    pub configuration: serde_json::Value,
    /// Actual impact against the current active content.
    pub changes: Vec<SettingChange>,
    /// Failed checks with stable safe diagnostics.
    pub issues: Vec<ConfigurationIssue>,
    /// Completed real numerical regression cases.
    pub fixtures: Vec<ConfigurationFixtureCheck>,
}
/// Durable selection and separately acknowledged runtime application. Never roll back an ambiguous apply.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigurationActivationOutcome {
    /// Confirmed durable pointer/history commit.
    pub activation: ActivationRecord,
    /// Captured accepted work, absent if application requires restart/reconciliation.
    pub accepted_generation: Option<Version>,
    /// Actual accepted pointer; a concurrent later selection may supersede this command.
    pub accepted_activation_revision: Option<Version>,
    /// Stopped/new process needs startup to apply its selected configuration.
    pub restart_required: bool,
}
/// Typed control failure; Display/Debug never include provider error text or input documents.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ConfigurationError {
    /// Strict schema or metadata violation.
    #[error("CONFIGURATION_INVALID: {0}")]
    Invalid(&'static str),
    /// A revision or request identity changed.
    #[error("CONFIGURATION_CONFLICT")]
    Conflict,
    /// Fixtures/dependencies/capabilities failed validation.
    #[error("CONFIGURATION_VALIDATION_FAILED")]
    ValidationFailed,
    /// Runtime changes require an explicit stopped-process activation.
    #[error("CONFIGURATION_RESTART_REQUIRED")]
    RestartRequired,
    /// Capability/model not implemented by this composition.
    #[error("CONFIGURATION_UNSUPPORTED: {0}")]
    Unsupported(&'static str),
    /// Capacity bounds exceeded.
    #[error("CONFIGURATION_CAPACITY_EXCEEDED")]
    CapacityExceeded,
    /// Persisted outcome cannot be confirmed; reconcile the original request identity.
    #[error("CONFIGURATION_UNKNOWN_OUTCOME")]
    UnknownOutcome,
    /// Unavailable/corrupt I/O. Underlying details are deliberately not exposed.
    #[error("CONFIGURATION_STORAGE_UNAVAILABLE")]
    Storage,
}
impl From<TraitError> for ConfigurationError {
    fn from(error: TraitError) -> Self {
        match error {
            TraitError::InvalidInput(reason) => Self::Invalid(reason),
            TraitError::Conflict(_) => Self::Conflict,
            TraitError::Unsupported(reason) => Self::Unsupported(reason),
            TraitError::CapacityExceeded => Self::CapacityExceeded,
            TraitError::UnknownOutcome => Self::UnknownOutcome,
            _ => Self::Storage,
        }
    }
}
/// Narrow application service, reusable by the operator CLI and engine administration.
pub struct ConfigurationUseCases {
    repository: Arc<dyn ConfigurationRepository>,
    assembler: SnapshotAssembler,
    capabilities: ConfigurationCapabilities,
    admission: tokio::sync::Semaphore,
    warmed: parking_lot::Mutex<Option<Arc<SnapshotValuator>>>,
}
impl ConfigurationUseCases {
    /// Whether results of this quality count as ready in this composition.
    pub fn accepts(&self, quality: DataQuality) -> bool {
        self.capabilities.accepts(quality)
    }
    /// Compose genuine boundaries; no credentials are resolved inside analytics.
    pub fn new(
        repository: Arc<dyn ConfigurationRepository>,
        references: Arc<dyn ReferenceRepository>,
        configurations: Arc<dyn ConfigStore>,
        fixings: Arc<dyn FixingRepository>,
        snapshots: Arc<dyn SnapshotRepository>,
        capabilities: ConfigurationCapabilities,
    ) -> Self {
        Self {
            repository,
            assembler: SnapshotAssembler::new(references, configurations, fixings, snapshots),
            capabilities,
            admission: tokio::sync::Semaphore::new(1),
            warmed: parking_lot::Mutex::new(None),
        }
    }
    /// Configuration persistence boundary for inspection and immutable selection recovery.
    pub fn repository(&self) -> &Arc<dyn ConfigurationRepository> {
        &self.repository
    }
    /// Shared strict checks and real numerical fixtures. No content, snapshot or pointer is written.
    pub async fn validate(
        &self,
        document: &ConfigurationDocument,
        fixtures: &[ConfigurationFixture],
    ) -> Result<ConfigurationValidation, ConfigurationError> {
        let _permit = self
            .admission
            .try_acquire()
            .map_err(|_| ConfigurationError::CapacityExceeded)?;
        let mut report = ConfigurationValidation {
            valid: false,
            document_digest: None,
            configuration: document.redacted()?,
            changes: vec![],
            issues: vec![],
            fixtures: vec![],
        };
        if document.validate().is_err() {
            issue(
                &mut report,
                "configuration",
                "INVALID_SCHEMA",
                "Configuration structure, revision or bounds are invalid",
            );
            return Ok(report);
        }
        report.document_digest = Some(document.digest()?);
        let id = SourceId::new(&document.pricing.config_id)
            .map_err(|_| ConfigurationError::Invalid("invalid configuration identity"))?;
        if let Some(active) = io(self.repository.active(&id)).await? {
            report.changes = configuration_diff(&active.staged.document, document);
        }
        self.check_capabilities(document, &mut report);
        let plan = plan(document, vec![]);
        if plan.validate().is_err()
            || !matches!(
                document.pricing.pricing_spec,
                convex_ports::storage::PricingSpec::MarketQuote {
                    side: convex_ports::storage::QuoteSide::Mid
                }
            )
        {
            issue(&mut report, "runtime", "UNSUPPORTED_MODEL", "Configuration requires an explicit supported fixed-bullet mid model and numerical policy");
        }
        self.check_curves(document, &mut report);
        if !report.issues.is_empty() {
            return Ok(report);
        }
        if fixtures.is_empty()
            || fixtures.len() > 8
            || fixtures.iter().map(|f| f.events.len()).sum::<usize>() > 4096
            || fixtures.iter().map(|f| f.expected.len()).sum::<usize>() > 8192
        {
            issue(
                &mut report,
                "fixtures",
                "INVALID_FIXTURES",
                "One to eight bounded numerical fixtures with explicit expectations are required",
            );
            return Ok(report);
        }
        let mut fixture_ids = std::collections::BTreeSet::new();
        for fixture in fixtures {
            if !fixture_ids.insert(&fixture.id)
                || fixture.expected.is_empty()
                || fixture
                    .expected
                    .iter()
                    .any(|e| !(0.0..=1e-8).contains(&e.tolerance.get()))
            {
                issue(
                    &mut report,
                    "fixtures",
                    "INVALID_FIXTURES",
                    "Duplicate case identity, missing expectation or invalid tolerance",
                );
                continue;
            }
            let assembly =
                document.effective_assembly(fixture.as_of, fixture.known_at, version_one()?)?;
            let snapshot = match io(self.assembler.resolve_with_configuration(
                &assembly,
                &fixture.events,
                document.pricing.clone(),
            ))
            .await
            {
                Ok(snapshot) => snapshot,
                Err(ConfigurationError::Storage | ConfigurationError::UnknownOutcome) => {
                    return Err(ConfigurationError::Storage)
                }
                Err(_) => {
                    issue(&mut report, "assembly.references", "INVALID_REFERENCE", "An exact reference or calendar/convention is missing, later than cutoff or inconsistent");
                    continue;
                }
            };
            if !self.capabilities.accepts(snapshot.quality().quality) {
                issue(&mut report, "assembly", "INVALID_FIXTURE_INPUTS", "Required quotes/curves or feed continuity do not satisfy the supplied information cutoff");
                continue;
            }
            if !crate::optional_valuation::within_callable_bound(&snapshot) {
                issue(
                    &mut report,
                    "assembly.instruments",
                    "TOO_MANY_CALLABLES",
                    "A runtime plan values at most 8 callables",
                );
                continue;
            }
            let mut valuator = self.warmed.lock().clone();
            if valuator.is_none() {
                let initialized = Arc::new(SnapshotValuator::new(
                    self.capabilities.build_id.clone(),
                    256,
                )?);
                *self.warmed.lock() = Some(initialized.clone());
                valuator = Some(initialized);
            }
            let valuator = valuator.ok_or(ConfigurationError::ValidationFailed)?;
            let mut actual = Vec::new();
            let mut matched = std::collections::BTreeSet::new();
            for pin in &document.assembly.instruments {
                let context = plan
                    .context(&snapshot, &pin.instrument_id)
                    .map_err(|_| ConfigurationError::ValidationFailed)?;
                let result = match valuator.value(
                    &snapshot,
                    &context,
                    RequestId::new("configuration-fixture")
                        .map_err(|_| ConfigurationError::ValidationFailed)?,
                    fixture.known_at.max(fixture.as_of),
                ) {
                    Ok(result) => result,
                    Err(_) => {
                        issue(&mut report, "fixtures", "NUMERICAL_FAILURE", "Instrument, convention or required numerical calculation is unsupported or failed");
                        continue;
                    }
                };
                if result.envelope().status() != ResultStatus::Complete
                    || !self.capabilities.accepts(result.envelope().quality())
                {
                    issue(
                        &mut report,
                        "fixtures",
                        "NUMERICAL_FAILURE",
                        "Actual valuation did not produce every requested valid measure",
                    );
                }
                let expectations = fixture
                    .expected
                    .iter()
                    .filter(|e| e.instrument_id == pin.instrument_id)
                    .collect::<Vec<_>>();
                if expectations.is_empty() {
                    issue(
                        &mut report,
                        "fixtures.expected",
                        "MISSING_EXPECTATION",
                        "Every declared instrument requires a numerical regression assertion",
                    );
                }
                for expected in expectations {
                    if !matched.insert((&expected.instrument_id, &expected.measure)) {
                        issue(
                            &mut report,
                            "fixtures.expected",
                            "DUPLICATE_EXPECTATION",
                            "Duplicate instrument/measure regression assertion",
                        );
                    }
                    let value = result
                        .envelope()
                        .results()
                        .get(&expected.measure)
                        .and_then(|r| numeric(&r.value));
                    if value
                        .is_none_or(|v| (v - expected.value.get()).abs() > expected.tolerance.get())
                    {
                        issue(&mut report, "fixtures.expected", "FIXTURE_MISMATCH", "Actual numerical result differs from the explicit regression expectation");
                    }
                }
                actual.push(result);
            }
            if fixture.expected.iter().any(|e| {
                !document
                    .assembly
                    .instruments
                    .iter()
                    .any(|p| p.instrument_id == e.instrument_id)
            }) {
                issue(
                    &mut report,
                    "fixtures.expected",
                    "UNKNOWN_EXPECTATION",
                    "Expectation refers to an instrument outside the declared universe",
                );
            }
            let bytes =
                serde_json::to_vec(&actual).map_err(|_| ConfigurationError::ValidationFailed)?;
            if actual.len() == document.assembly.instruments.len() {
                report.fixtures.push(ConfigurationFixtureCheck {
                    id: fixture.id.clone(),
                    snapshot_id: snapshot.id().clone(),
                    results_digest: content_digest(b"convex.configuration-fixture.v1\0", &bytes),
                });
            }
        }
        report.valid = report.issues.is_empty();
        Ok(report)
    }
    fn check_capabilities(
        &self,
        document: &ConfigurationDocument,
        report: &mut ConfigurationValidation,
    ) {
        let cap = &self.capabilities;
        if document.runtime.build_id != cap.build_id {
            issue(
                report,
                "runtime.build_id",
                "UNSUPPORTED_BUILD",
                "Numerical fixtures must name the actual executable build identity",
            );
        }
        if document.process.storage != cap.storage
            || !cap.durable
            || !cap.atomic_activation
            || document.process.shared_writers && !cap.shared_writers
        {
            issue(report, "process.storage", "UNSUPPORTED_STORAGE", "Selected storage lacks required durable atomic activation or shared-writer semantics");
        }
        if document.process.result_transport != cap.result_transport {
            issue(
                report,
                "process.result_transport",
                "MISSING_TRANSPORT",
                "Selected result transport is not configured",
            );
        }
        for required in &document.providers {
            let available = cap.providers.iter().find(|p| p.source == required.source);
            if available.is_none_or(|p| {
                p.credential != required.credential
                    || required
                        .capabilities
                        .iter()
                        .any(|c| !p.capabilities.contains(c))
            }) {
                issue(report, "providers", "MISSING_PROVIDER", "Required provider, credentials or capabilities are unavailable in this composition");
            }
        }
        for quote in &document.assembly.quotes {
            let required_mode = match document.runtime.mode {
                RuntimeMode::Captured => ProviderCapability::CanonicalQuotes,
                RuntimeMode::Replay => ProviderCapability::Replay,
                RuntimeMode::Live => ProviderCapability::LiveImages,
            };
            if !document.providers.iter().any(|p| {
                p.source.as_str() == quote.key.source
                    && p.capabilities
                        .contains(&ProviderCapability::CanonicalQuotes)
                    && p.capabilities.contains(&required_mode)
            }) {
                issue(
                    report,
                    "assembly.quotes",
                    "MISSING_PROVIDER",
                    "Selected quote source is absent or lacks the explicit mode capability",
                );
            }
        }
    }
    fn check_curves(&self, document: &ConfigurationDocument, report: &mut ConfigurationValidation) {
        if crate::calibration_replay::rebuild_captured_curves(
            &document.assembly.curve_recipes,
            document.assembly.calibration.as_ref(),
            document.assembly.as_of,
            document.assembly.known_at,
        )
        .is_err()
        {
            if document.assembly.calibration.is_some() {
                issue(report, "assembly.calibration", "INVALID_CALIBRATION_CAPTURE", "Complete original calibration must rebuild and match all captured outputs/diagnostics");
            } else {
                issue(report, "assembly.curve_recipes", "INVALID_CURVE_RECIPE", "Complete native curve graph must reconstruct without missing versions or unsupported terms");
            }
        }
        let configured = &document.pricing.analytics_curves;
        let mut required = vec![configured.discount_curve.as_str()];
        required.extend(
            [
                &configured.government_curve,
                &configured.swap_curve,
                &configured.frn_reference_curve,
            ]
            .into_iter()
            .flatten()
            .map(|id| id.as_str()),
        );
        required.extend(
            document
                .assembly
                .required_curves
                .iter()
                .map(|id| id.as_str()),
        );
        for id in required {
            if !(document
                .assembly
                .curves
                .iter()
                .any(|c| c.curve_id.as_str() == id)
                || document
                    .assembly
                    .curve_recipes
                    .iter()
                    .any(|c| c.curve_id.as_str() == id)
                || document
                    .assembly
                    .calibration
                    .as_ref()
                    .is_some_and(|c| c.curves.iter().any(|p| p.curve_id.as_str() == id)))
                || !document
                    .assembly
                    .required_curves
                    .iter()
                    .any(|c| c.as_str() == id)
            {
                issue(
                    report,
                    "assembly.curves",
                    "MISSING_CURVE",
                    "Every configured curve role must have an explicit required supplied revision",
                );
            }
        }
    }
    /// Validate then atomically stage; ambiguous outcomes are reconciled against the exact record.
    pub async fn stage(
        &self,
        staged: &StagedConfiguration,
        fixtures: &[ConfigurationFixture],
    ) -> Result<ConfigurationValidation, ConfigurationError> {
        staged.validate()?;
        if let Some(retained) = io(self.repository.get_staged(
            &SourceId::new(&staged.document.pricing.config_id)
                .map_err(|_| ConfigurationError::Invalid("invalid configuration identity"))?,
            version(staged.document.pricing.version)?,
        ))
        .await?
        {
            if retained != *staged {
                return Err(ConfigurationError::Conflict);
            }
        }
        let report = self.validate(&staged.document, fixtures).await?;
        if !report.valid {
            return Err(ConfigurationError::ValidationFailed);
        }
        match io(self.repository.stage(staged)).await {
            Ok(()) => {}
            Err(ConfigurationError::UnknownOutcome) => {
                let retained = io(self.repository.get_staged(
                    &SourceId::new(&staged.document.pricing.config_id).map_err(|_| {
                        ConfigurationError::Invalid("invalid configuration identity")
                    })?,
                    version(staged.document.pricing.version)?,
                ))
                .await?;
                if retained.as_ref() != Some(staged) {
                    return Err(ConfigurationError::UnknownOutcome);
                }
            }
            Err(error) => return Err(error),
        }
        Ok(report)
    }
    /// Revalidate/warm actual inputs, then CAS. Running processes can accept only hot changes.
    pub async fn activate(
        &self,
        command: &ActivationCommand,
        fixtures: &[ConfigurationFixture],
        running: bool,
    ) -> Result<ActivationRecord, ConfigurationError> {
        command.validate()?;
        if let Some(retained) = io(self
            .repository
            .activation_for_request(&command.idempotency_key))
        .await?
        {
            self.verify_activation(command, &retained).await?;
            return Ok(retained);
        }
        if running && command.mode != ReloadClass::Hot {
            return Err(ConfigurationError::RestartRequired);
        }
        let staged = io(self
            .repository
            .get_staged(&command.id, command.configuration_version))
        .await?
        .ok_or(ConfigurationError::Invalid("missing staged configuration"))?;
        let report = self.validate(&staged.document, fixtures).await?;
        if !report.valid {
            return Err(ConfigurationError::ValidationFailed);
        }
        if command.mode == ReloadClass::Hot
            && (command.expected_revision == 0
                || report
                    .changes
                    .iter()
                    .any(|c| c.reload == ReloadClass::RestartRequired))
        {
            return Err(ConfigurationError::RestartRequired);
        }
        match io(self.repository.activate(command)).await {
            Ok(record) => {
                self.verify_activation(command, &record).await?;
                Ok(record)
            }
            Err(ConfigurationError::UnknownOutcome) => {
                let retained = io(self
                    .repository
                    .activation_for_request(&command.idempotency_key))
                .await?;
                match retained {
                    Some(record) => {
                        self.verify_activation(command, &record).await?;
                        Ok(record)
                    }
                    _ => Err(ConfigurationError::UnknownOutcome),
                }
            }
            Err(error) => Err(error),
        }
    }
    async fn verify_activation(
        &self,
        command: &ActivationCommand,
        record: &ActivationRecord,
    ) -> Result<(), ConfigurationError> {
        if record.command != *command {
            return Err(ConfigurationError::Conflict);
        }
        self.selection(record).await?;
        Ok(())
    }
    /// Exact current selection. An absent active pointer stays absent even when newer content is staged.
    pub async fn active(
        &self,
        id: &SourceId,
    ) -> Result<Option<ActiveConfiguration>, ConfigurationError> {
        io(self.repository.active(id)).await
    }
    /// Exact staged revision with actor/reason/override history; callers export its redacted view.
    pub async fn get_version(
        &self,
        id: &SourceId,
        version: Version,
    ) -> Result<Option<StagedConfiguration>, ConfigurationError> {
        io(self.repository.get_staged(id, version)).await
    }
    /// Restore one accepted selection after restart, never replacing it with the newest pointer.
    pub async fn selection(
        &self,
        record: &ActivationRecord,
    ) -> Result<ActiveConfiguration, ConfigurationError> {
        let activation = io(self
            .repository
            .get_activation(&record.command.id, record.revision))
        .await?
        .ok_or(ConfigurationError::Conflict)?;
        if activation != *record {
            return Err(ConfigurationError::Conflict);
        }
        let staged = io(self
            .repository
            .get_staged(&record.command.id, record.command.configuration_version))
        .await?
        .ok_or(ConfigurationError::Conflict)?;
        let selected = ActiveConfiguration { activation, staged };
        selected.validate()?;
        Ok(selected)
    }
    /// Bind a supplied snapshot's claimed activation/override policies to retained immutable content.
    /// Historical accepted selections remain valid even after the active pointer changes.
    pub async fn verify_snapshot_selection(
        &self,
        snapshot: &convex_ports::snapshot::MarketSnapshot,
    ) -> Result<(), ConfigurationError> {
        let content = snapshot.content();
        let capture = content
            .configuration_capture
            .as_ref()
            .ok_or(ConfigurationError::Invalid("no captured activation"))?;
        let selected = self.selection(&capture.activation).await?;
        let mut expected = selected.capture(content.as_of, content.known_at, content.generation)?;
        // Compare the retained selection in the same canonical order as immutable manifests.
        expected
            .instruments
            .sort_by(|a, b| a.instrument_id.cmp(&b.instrument_id));
        expected.quotes.sort_by(|a, b| {
            (a.key.instrument_id.as_str(), &a.key.source)
                .cmp(&(b.key.instrument_id.as_str(), &b.key.source))
        });
        for quote in &mut expected.quotes {
            quote.fields.sort();
        }
        expected.curves.sort_by(|a, b| a.curve_id.cmp(&b.curve_id));
        expected
            .curve_recipes
            .sort_by(|a, b| a.curve_id.cmp(&b.curve_id));
        for curve in &mut expected.curve_recipes {
            curve
                .input_versions
                .sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
        }
        for curve in &mut expected.curves {
            curve
                .input_versions
                .sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
        }
        if let Some(calibration) = &mut expected.calibration {
            calibration.normalize();
        }
        expected.required_curves.sort();
        expected.fixings.sort();
        if selected.staged.document.pricing != content.configuration
            || expected.configuration_capture != content.configuration_capture
            || expected.quotes
                != content
                    .quotes
                    .iter()
                    .map(|q| q.requirement.clone())
                    .collect::<Vec<_>>()
            || expected.curves != content.curves
            || expected.curve_recipes != content.curve_recipes
            || expected.calibration != content.calibration
            || expected.required_curves != content.required_curves
            || expected.max_source_skew_ms != content.max_source_skew_ms
            || expected.instruments.len() != content.references.len()
            || expected
                .instruments
                .iter()
                .zip(&content.references)
                .any(|(pin, reference)| {
                    pin.instrument_id.as_str() != reference.record.instrument_id.as_str()
                        || pin.version != reference.record.version
                })
            || expected.fixings
                != content
                    .fixings
                    .iter()
                    .map(|f| f.key.clone())
                    .collect::<Vec<_>>()
        {
            return Err(ConfigurationError::Conflict);
        }
        Ok(())
    }
    /// Bounded coherent activation audit; rollback records appear as new sequences.
    pub async fn history(
        &self,
        id: &SourceId,
        page: &Pagination,
    ) -> Result<Page<ActivationRecord>, ConfigurationError> {
        io(self.repository.history(id, page)).await
    }
    /// Reject a restarted/runtime activation that changes the process footprint under its old plan.
    pub fn compatible_runtime(
        &self,
        configured: &RuntimePlan,
        baseline: &convex_ports::storage::BondPricingConfig,
        concurrency: usize,
        selected: &ActiveConfiguration,
    ) -> Result<(), ConfigurationError> {
        selected.validate()?;
        let candidate = plan(&selected.staged.document, vec![]);
        let old = &configured.assembly;
        let new = &candidate.assembly;
        let pricing = &selected.staged.document.pricing;
        if baseline.pricing_spec != pricing.pricing_spec
            || baseline.applies_to != pricing.applies_to
            || baseline.analytics_curves != pricing.analytics_curves
            || baseline.bid_ask_spread != pricing.bid_ask_spread
            || configured.runtime_id != candidate.runtime_id
            || configured.mode != candidate.mode
            || configured.settlement_date != candidate.settlement_date
            || configured.build_id != candidate.build_id
            || configured.measures != candidate.measures
            || configured.numerical_settings != candidate.numerical_settings
            || old.instruments != new.instruments
            || old.fixings != new.fixings
            || old.curves != new.curves
            || old.curve_recipes != new.curve_recipes
            || old.calibration != new.calibration
            || old.required_curves != new.required_curves
            || old.configuration.id != new.configuration.id
            || old
                .quotes
                .iter()
                .map(|q| (&q.key, &q.fields))
                .collect::<Vec<_>>()
                != new
                    .quotes
                    .iter()
                    .map(|q| (&q.key, &q.fields))
                    .collect::<Vec<_>>()
            || concurrency != selected.staged.document.process.max_concurrent_calcs
        {
            return Err(ConfigurationError::RestartRequired);
        }
        let mut report = ConfigurationValidation {
            valid: false,
            document_digest: None,
            configuration: serde_json::Value::Null,
            changes: vec![],
            issues: vec![],
            fixtures: vec![],
        };
        self.check_capabilities(&selected.staged.document, &mut report);
        self.check_curves(&selected.staged.document, &mut report);
        if candidate.validate().is_err()
            || !matches!(
                pricing.pricing_spec,
                convex_ports::storage::PricingSpec::MarketQuote {
                    side: convex_ports::storage::QuoteSide::Mid
                }
            )
        {
            return Err(ConfigurationError::ValidationFailed);
        }
        if !report.issues.is_empty() {
            return Err(ConfigurationError::ValidationFailed);
        }
        Ok(())
    }
}
/// Translate explicit application content to the existing runtime contract without defaults.
pub fn plan(document: &ConfigurationDocument, seed_events: Vec<QuoteEvent>) -> RuntimePlan {
    RuntimePlan {
        schema_version: 1,
        runtime_id: document.runtime.runtime_id.clone(),
        mode: document.runtime.mode,
        assembly: document.assembly.clone(),
        seed_events,
        settlement_date: document.runtime.settlement_date,
        build_id: document.runtime.build_id.clone(),
        measures: document.runtime.measures.clone(),
        numerical_settings: document.runtime.numerical_settings.clone(),
    }
}
fn numeric(value: &MeasureValue) -> Option<f64> {
    match value {
        MeasureValue::Price(p) => p.value().to_model().ok().map(|v| v.get()),
        MeasureValue::ModelPrice(p) => Some(p.value().get()),
        MeasureValue::ModelRate(p) => Some(p.value().get()),
        MeasureValue::ModelBps(p) => Some(p.value().get()),
        MeasureValue::ModelYears(p) => Some(p.value().get()),
        MeasureValue::ModelYearsSquared(p) => Some(p.value().get()),
        _ => None,
    }
}
fn issue(report: &mut ConfigurationValidation, field: &str, code: &str, message: &str) {
    if report.issues.len() < 256 {
        report.issues.push(ConfigurationIssue {
            field: field.into(),
            code: code.into(),
            message: message.into(),
        });
    }
}
fn version(value: u64) -> Result<Version, ConfigurationError> {
    Version::new(value).map_err(|_| ConfigurationError::Invalid("zero configuration version"))
}
fn version_one() -> Result<Version, ConfigurationError> {
    version(1)
}
async fn io<T>(
    future: impl std::future::Future<Output = Result<T, TraitError>>,
) -> Result<T, ConfigurationError> {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .map_err(|_| ConfigurationError::UnknownOutcome)?
        .map_err(Into::into)
}
