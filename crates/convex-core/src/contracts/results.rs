//! Stable errors, supplied provenance and checked result completeness.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::{
    ContractError, MeasureId, MeasureValue, ModelId, ObservationId, RequestId, SchemaVersion,
    SnapshotId, SourceId, UtcTimestamp, ValuationSubject, Version,
};

/// Stable failure codes. Human-readable messages do not define protocol behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ErrorCode {
    /// Structurally or numerically invalid request.
    InvalidInput,
    /// Unsupported product family or terms.
    UnsupportedProduct,
    /// Unsupported quotation/calendar/model convention.
    UnsupportedConvention,
    /// Required instrument, calendar or convention missing.
    MissingReference,
    /// Required known-at fixing missing.
    MissingFixing,
    /// Required market observation missing.
    MissingMarketData,
    /// Input exceeds freshness policy.
    StaleInput,
    /// Feed continuity is unknown.
    DataGap,
    /// Use of requested data is not permitted.
    EntitlementDenied,
    /// Calibration failed its declared contract.
    CalibrationFailed,
    /// Numerical iteration/resource bound reached.
    SolverLimit,
    /// Expected version no longer current.
    VersionConflict,
    /// Requested risk coverage is incomplete.
    IncompleteRisk,
    /// No feasible hedge under the hard constraints.
    InfeasibleHedge,
    /// Required risk/trading limit violated.
    LimitBreach,
    /// Authoritative ownership lost.
    OwnershipLost,
    /// Caller-supplied deadline exceeded.
    DeadlineExceeded,
    /// Requested operation cancelled.
    Cancelled,
    /// Dependency/service unavailable.
    Unavailable,
    /// Mutation outcome needs reconciliation before retry.
    UnknownOutcome,
    /// Measure has no economic meaning for the declared product.
    NotApplicable,
}

/// Structured failure without an invented numerical value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ErrorWire", into = "ErrorWire")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(with = "ErrorWire"))]
pub struct StructuredError(ErrorWire);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
struct ErrorWire {
    code: ErrorCode,
    #[cfg_attr(feature = "schemars", schemars(length(min = 1, max = 256)))]
    field: Option<String>,
    #[cfg_attr(feature = "schemars", schemars(length(min = 1, max = 1024)))]
    message: String,
    retryable: bool,
}
impl StructuredError {
    /// Bound diagnostic detail; caller decides retry policy using operation semantics.
    pub fn new(
        code: ErrorCode,
        field: Option<String>,
        message: impl Into<String>,
        retryable: bool,
    ) -> Result<Self, ContractError> {
        Self::try_from(ErrorWire {
            code,
            field,
            message: message.into(),
            retryable,
        })
    }
    /// Stable failure code.
    pub const fn code(&self) -> ErrorCode {
        self.0.code
    }
    /// Offending measure/field, when supplied.
    pub fn field(&self) -> Option<&str> {
        self.0.field.as_deref()
    }
    /// Bounded diagnostic message.
    pub fn message(&self) -> &str {
        &self.0.message
    }
    /// Whether retry is safe under the caller's operation policy.
    pub const fn retryable(&self) -> bool {
        self.0.retryable
    }
}
impl TryFrom<ErrorWire> for StructuredError {
    type Error = ContractError;
    fn try_from(value: ErrorWire) -> Result<Self, Self::Error> {
        if value.message.is_empty()
            || value.message.len() > 1024
            || value
                .field
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 256)
        {
            return Err(ContractError::new(
                "error",
                "diagnostic exceeds bounds or is empty",
            ));
        }
        // Unknown outcome must be reconciled, never advertised as safely retryable.
        if value.code == ErrorCode::UnknownOutcome && value.retryable {
            return Err(ContractError::new(
                "retryable",
                "unknown outcome requires reconciliation",
            ));
        }
        Ok(Self(value))
    }
}
impl From<StructuredError> for ErrorWire {
    fn from(value: StructuredError) -> Self {
        value.0
    }
}

/// Independent quality classification; completeness alone is not execution eligibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum DataQuality {
    /// Passed applicable data/model policy.
    Valid,
    /// Indicative input or explicitly permitted approximation.
    Indicative,
    /// Licensed or source delay.
    Delayed,
    /// Older result/input retained with a visible stale label.
    Stale,
    /// Fails declared data/model policy.
    Invalid,
    /// Explicit demonstration data or composition.
    Demo,
}

/// Completeness relative to the requested measure set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ResultStatus {
    /// Every requested measure succeeded; no errors.
    Complete,
    /// Some measures succeeded and some failed explicitly.
    Partial,
    /// No successful numerical measures.
    Failed,
}

/// Exact version of a consumed definition; full snapshot manifest remains elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InputVersion {
    /// Record kind (calendar, convention, curve, fixing, instrument, ...).
    pub kind: SourceId,
    /// Provider-neutral record identity.
    pub id: SourceId,
    /// Exact consumed revision.
    pub version: Version,
}

/// Reproducibility inputs supplied by the caller, never filled using current time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Provenance {
    /// Pinned instrument/trade definition.
    pub subject: ValuationSubject,
    /// Immutable information set consumed.
    pub snapshot_id: SnapshotId,
    /// Pinned configuration.
    pub configuration_version: Version,
    /// Economic valuation time.
    pub valuation_time: UtcTimestamp,
    /// Information publication/recording cutoff.
    pub known_at: UtcTimestamp,
    /// Supplied calculation completion time.
    pub calculated_at: UtcTimestamp,
    /// Quantitative model version.
    pub model_version: ModelId,
    /// Executable/library build identity.
    pub build_id: ModelId,
    /// Consumed reference/calibration/fixing definition versions.
    #[cfg_attr(feature = "schemars", schemars(length(max = 256)))]
    pub input_versions: Vec<InputVersion>,
}
impl Provenance {
    /// Check supplied chronology, bounded version manifest and duplicate records.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.known_at > self.calculated_at || self.input_versions.len() > 256 {
            return Err(ContractError::new(
                "provenance",
                "future information cutoff or oversized manifest",
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for input in &self.input_versions {
            if !seen.insert((&input.kind, &input.id)) {
                return Err(ContractError::new(
                    "input_versions",
                    "duplicate consumed record",
                ));
            }
        }
        Ok(())
    }
}

/// One successful measure with its own source/method/quality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MeasureResult {
    /// Explicit representation and unit.
    pub value: MeasureValue,
    /// Per-measure quality, independently of envelope completeness.
    pub quality: DataQuality,
    /// Observed-mark/model/bump method supplied by the caller.
    pub method: ModelId,
    /// Actual observation consumed, if applicable; never invented by an adapter.
    pub source_observation_id: Option<ObservationId>,
}

/// Shared envelope with checked status, errors and matching input provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "EnvelopeWire", into = "EnvelopeWire")]
pub struct ResultEnvelope(EnvelopeWire);

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for ResultEnvelope {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ResultEnvelope".into()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = <EnvelopeWire as schemars::JsonSchema>::json_schema(generator);
        schema.insert("allOf".into(), serde_json::json!([
            {"properties":{"results":{"maxProperties":4096,"propertyNames":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9][A-Za-z0-9_.:/-]*$"}}}},
            {"if":{"properties":{"status":{"const":"Complete"}}},"then":{"properties":{"results":{"minProperties":1},"errors":{"maxItems":0}}}},
            {"if":{"properties":{"status":{"const":"Partial"}}},"then":{"properties":{"results":{"minProperties":1},"errors":{"minItems":1}}}},
            {"if":{"properties":{"status":{"const":"Failed"}}},"then":{"properties":{"results":{"maxProperties":0},"errors":{"minItems":1}}}}
        ]));
        schema
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
struct EnvelopeWire {
    schema_version: SchemaVersion,
    request_id: RequestId,
    status: ResultStatus,
    quality: DataQuality,
    snapshot_id: SnapshotId,
    configuration_version: Version,
    #[serde(deserialize_with = "deserialize_results")]
    results: BTreeMap<MeasureId, MeasureResult>,
    #[cfg_attr(feature = "schemars", schemars(length(max = 256)))]
    errors: Vec<StructuredError>,
    provenance: Provenance,
}

fn deserialize_results<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<MeasureId, MeasureResult>, D::Error> {
    struct ResultsVisitor;
    impl<'de> serde::de::Visitor<'de> for ResultsVisitor {
        type Value = BTreeMap<MeasureId, MeasureResult>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a bounded map with unique measure identities")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            while let Some((id, result)) = map.next_entry()? {
                if values.len() == 4096 || values.insert(id, result).is_some() {
                    return Err(serde::de::Error::custom(
                        "duplicate measure or result bounds exceeded",
                    ));
                }
            }
            Ok(values)
        }
    }
    deserializer.deserialize_map(ResultsVisitor)
}
impl ResultEnvelope {
    /// Build a complete, partial or failed result. Caller checks requested coverage.
    pub fn new(
        request_id: RequestId,
        status: ResultStatus,
        quality: DataQuality,
        results: BTreeMap<MeasureId, MeasureResult>,
        errors: Vec<StructuredError>,
        provenance: Provenance,
    ) -> Result<Self, ContractError> {
        Self::try_from(EnvelopeWire {
            schema_version: SchemaVersion::V2,
            request_id,
            status,
            quality,
            snapshot_id: provenance.snapshot_id.clone(),
            configuration_version: provenance.configuration_version,
            results,
            errors,
            provenance,
        })
    }
    /// Validated completeness.
    pub const fn status(&self) -> ResultStatus {
        self.0.status
    }
    /// Successful measures only; absence must never be converted to zero.
    pub fn results(&self) -> &BTreeMap<MeasureId, MeasureResult> {
        &self.0.results
    }
    /// Explicit failures for missing/invalid/unsupported measures.
    pub fn errors(&self) -> &[StructuredError] {
        &self.0.errors
    }
    /// Input and calculation lineage.
    pub fn provenance(&self) -> &Provenance {
        &self.0.provenance
    }
    /// Request correlation identity.
    pub fn request_id(&self) -> &RequestId {
        &self.0.request_id
    }
    /// Independent data/model quality classification for native callers.
    pub const fn quality(&self) -> DataQuality {
        self.0.quality
    }
    /// Immutable information set consumed.
    pub fn snapshot_id(&self) -> &SnapshotId {
        &self.0.snapshot_id
    }
    /// Pinned configuration, matching provenance.
    pub const fn configuration_version(&self) -> Version {
        self.0.configuration_version
    }
    /// Canonical wire schema version.
    pub const fn schema_version(&self) -> SchemaVersion {
        self.0.schema_version
    }
}
impl TryFrom<EnvelopeWire> for ResultEnvelope {
    type Error = ContractError;
    fn try_from(value: EnvelopeWire) -> Result<Self, Self::Error> {
        value.provenance.validate()?;
        if value.snapshot_id != value.provenance.snapshot_id
            || value.configuration_version != value.provenance.configuration_version
        {
            return Err(ContractError::new(
                "provenance",
                "envelope input versions do not match lineage",
            ));
        }
        for result in value.results.values() {
            if let MeasureValue::Risk(risk) = &result.value {
                if risk.snapshot_id() != &value.snapshot_id {
                    return Err(ContractError::new(
                        "snapshot_id",
                        "risk measure belongs to another information set",
                    ));
                }
            }
        }
        let valid = match value.status {
            ResultStatus::Complete => !value.results.is_empty() && value.errors.is_empty(),
            ResultStatus::Partial => !value.results.is_empty() && !value.errors.is_empty(),
            ResultStatus::Failed => value.results.is_empty() && !value.errors.is_empty(),
        };
        if !valid || value.results.len() > 4096 || value.errors.len() > 256 {
            return Err(ContractError::new(
                "status",
                "status inconsistent with measures/errors, or result bounds exceeded",
            ));
        }
        Ok(Self(value))
    }
}
impl From<ResultEnvelope> for EnvelopeWire {
    fn from(value: ResultEnvelope) -> Self {
        value.0
    }
}
