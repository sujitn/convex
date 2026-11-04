//! Configuration inspection and operator changes over HTTP, and the local composition
//! capabilities the server and the offline CLI share.
use crate::access::AccessContext;
use crate::config::ServerProfile;
use crate::handlers::AppState;
use axum::{
    extract::{rejection::JsonRejection, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use convex_core::contracts::*;
use convex_engine::{
    configuration::{
        ConfigurationCapabilities, ConfigurationError, ConfigurationFixture, ConfigurationUseCases,
    },
    ports::{
        configuration::*,
        market_data::{QuoteEventSource, StreamError},
        quote_event::QuoteEvent,
        storage::Pagination,
    },
};
use convex_ext_file::FileQuoteReplay;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

/// Capabilities of the real local redb/JSONL/canonical-file composition. No live provider is certified.
pub fn local_replay_capabilities(
    sources: impl IntoIterator<Item = SourceId>,
) -> Result<ConfigurationCapabilities, ConfigurationError> {
    local_capabilities(sources, ProviderCapability::Replay, "jsonl-local")
}

/// Capabilities of the local live composition: canonical images over HTTP ingress, results
/// delivered for display only. No live provider is certified.
pub fn local_live_capabilities(
    sources: impl IntoIterator<Item = SourceId>,
) -> Result<ConfigurationCapabilities, ConfigurationError> {
    local_capabilities(sources, ProviderCapability::LiveImages, "display-only")
}

fn local_capabilities(
    sources: impl IntoIterator<Item = SourceId>,
    mode: ProviderCapability,
    result_transport: &str,
) -> Result<ConfigurationCapabilities, ConfigurationError> {
    let mut providers = Vec::new();
    for source in sources {
        if providers
            .iter()
            .any(|p: &ProviderBinding| p.source == source)
        {
            continue;
        }
        if providers.len() == 64 {
            return Err(ConfigurationError::CapacityExceeded);
        }
        providers.push(ProviderBinding {
            source,
            capabilities: vec![ProviderCapability::CanonicalQuotes, mode],
            credential: None,
        });
    }
    Ok(ConfigurationCapabilities {
        build_id: ModelId::new(crate::SERVER_BUILD_ID).map_err(|_| ConfigurationError::Storage)?,
        storage: SourceId::new("redb-local").map_err(|_| ConfigurationError::Storage)?,
        durable: true,
        atomic_activation: true,
        shared_writers: false,
        result_transport: SourceId::new(result_transport)
            .map_err(|_| ConfigurationError::Storage)?,
        providers,
        demo: false,
    })
}

/// Safe selection inspection. Eligible policy overrides are evaluated only against supplied cutoffs.
pub async fn effective_configuration(
    cases: &ConfigurationUseCases,
    id: &SourceId,
    as_of: UtcTimestamp,
    known_at: UtcTimestamp,
) -> Result<Value, ConfigurationError> {
    let active = cases
        .active(id)
        .await?
        .ok_or(ConfigurationError::Invalid("no active configuration"))?;
    let assembly = active.capture(
        as_of,
        known_at,
        Version::new(1).map_err(|_| ConfigurationError::Storage)?,
    )?;
    let overrides = active.staged.document.overrides.iter().map(|o| json!({"id":o.id,"scope":o.scope,"actor":o.actor,"reason":o.reason,"created_at":o.created_at,"approved_by":o.approved_by,"approved_at":o.approved_at,"expires_at":o.expires_at,"state":if o.applies_at(as_of, known_at) {"applied"} else if as_of >= o.expires_at {"expired"} else {"not_yet_eligible"}})).collect::<Vec<_>>();
    Ok(
        json!({"schema_version":1,"activation":active.activation,"staged_by":active.staged.actor,"staged_at":active.staged.staged_at,"staging_reason":active.staged.reason,"configuration":active.staged.document.redacted()?,"effective_assembly":assembly,"overrides":overrides}),
    )
}

/// Supported JSON document schema; cross-record/model checks remain application validation.
pub async fn schema() -> Json<Value> {
    Json(convex_engine::ports::configuration_schema::configuration_schema())
}
/// Explicit configuration time; HTTP never silently chooses today's model/override policy.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationTime {
    /// Economic instant.
    pub as_of: UtcTimestamp,
    /// Information cutoff.
    pub known_at: UtcTimestamp,
}
/// Read-only adapter to the same effective inspection used by the CLI.
pub async fn effective(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(time): Query<ConfigurationTime>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let error = |code: &str, status| {
        (
            status,
            Json(json!({"schema_version":1,"error":{"code":code}})),
        )
    };
    let id =
        SourceId::new(id).map_err(|_| error("CONFIGURATION_INVALID", StatusCode::BAD_REQUEST))?;
    let cases = state
        .engine
        .configuration_use_cases()
        .ok_or_else(|| error("CONFIGURATION_UNAVAILABLE", StatusCode::SERVICE_UNAVAILABLE))?;
    effective_configuration(cases, &id, time.as_of, time.known_at)
        .await
        .map(Json)
        .map_err(|e| match e {
            ConfigurationError::Invalid(_) => {
                error("CONFIGURATION_INVALID", StatusCode::BAD_REQUEST)
            }
            _ => error("CONFIGURATION_UNAVAILABLE", StatusCode::SERVICE_UNAVAILABLE),
        })
}

/// Replay quotes file that HTTP configuration fixtures are checked against, when the
/// composition runs one.
#[derive(Debug, Clone)]
pub struct ConfigurationTrace(pub PathBuf);

/// Capabilities the server composes for a document at start: its mode, the sources its
/// quotes name, and whether the profile is the demo.
pub fn document_capabilities(
    document: &ConfigurationDocument,
    demo: bool,
) -> Result<ConfigurationCapabilities, ConfigurationError> {
    let sources = document
        .assembly
        .quotes
        .iter()
        .map(|q| {
            SourceId::new(&q.key.source)
                .map_err(|_| ConfigurationError::Invalid("invalid quote provider identity"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    mode_capabilities(document.runtime.mode, sources, demo)
}

/// Local capabilities for a runtime mode over the named quote sources.
pub fn mode_capabilities(
    mode: RuntimeMode,
    sources: Vec<SourceId>,
    demo: bool,
) -> Result<ConfigurationCapabilities, ConfigurationError> {
    let mut capabilities = match mode {
        RuntimeMode::Live => local_live_capabilities(sources)?,
        RuntimeMode::Replay | RuntimeMode::Captured => local_replay_capabilities(sources)?,
    };
    capabilities.demo = demo;
    Ok(capabilities)
}

/// The first 4,096 events of the canonical replay, the most a fixture trace can prefix.
pub async fn replay_events(path: &FsPath) -> Result<Vec<QuoteEvent>, ConfigurationError> {
    let mut source = FileQuoteReplay::open(path).map_err(|_| {
        ConfigurationError::Invalid("canonical replay file is unavailable; path redacted")
    })?;
    let mut events = vec![];
    loop {
        match source.next_event().await {
            Ok(event) => {
                events.push(event);
                if events.len() == 4096 {
                    return Ok(events);
                }
            }
            Err(StreamError::Closed) => return Ok(events),
            Err(_) => {
                return Err(ConfigurationError::Invalid(
                    "canonical replay failed; provider details redacted",
                ))
            }
        }
    }
}

/// One to eight fixtures, each a prefix of the replay when there is one to check.
pub fn fixture_evidence(
    fixtures: &[ConfigurationFixture],
    replay: Option<&[QuoteEvent]>,
) -> Result<FixtureEvidence, ConfigurationError> {
    if fixtures.is_empty() || fixtures.len() > 8 {
        return Err(ConfigurationError::Invalid(
            "one to eight fixture cases required",
        ));
    }
    let Some(events) = replay else {
        return Ok(FixtureEvidence::OperatorSupplied);
    };
    if fixtures
        .iter()
        .any(|f| f.events.len() > events.len() || f.events != events[..f.events.len()])
    {
        return Err(ConfigurationError::Invalid(
            "fixture trace differs from actual canonical replay prefix",
        ));
    }
    Ok(FixtureEvidence::ReplayChecked)
}

/// Candidate document and the fixtures it must reproduce.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidateRequest {
    document: ConfigurationDocument,
    fixtures: Vec<ConfigurationFixture>,
}

/// Two documents to compare.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffRequest {
    previous: ConfigurationDocument,
    next: ConfigurationDocument,
}

/// A staging request. The actor is the verified caller and the key is the
/// `Idempotency-Key` header; a retry resends the same body.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageRequest {
    document: ConfigurationDocument,
    fixtures: Vec<ConfigurationFixture>,
    reason: String,
    staged_at: UtcTimestamp,
}

/// An activation of a staged version. `If-Match` carries the expected active revision
/// (0 when nothing is active).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivateRequest {
    configuration_version: Version,
    reason: String,
    activated_at: UtcTimestamp,
    mode: ReloadClass,
    fixtures: Vec<ConfigurationFixture>,
}

/// A rollback to the content of a retained activation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollbackRequest {
    to_revision: Version,
    reason: String,
    activated_at: UtcTimestamp,
    mode: ReloadClass,
    fixtures: Vec<ConfigurationFixture>,
}

/// History page bounds.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryPage {
    offset: usize,
    limit: usize,
}

/// Two retained versions to compare.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionRange {
    from: u64,
    to: u64,
}

enum Refusal {
    Unavailable,
    Configuration(ConfigurationError),
}

impl From<ConfigurationError> for Refusal {
    fn from(error: ConfigurationError) -> Self {
        Self::Configuration(error)
    }
}

impl From<convex_engine::ports::TraitError> for Refusal {
    fn from(error: convex_engine::ports::TraitError) -> Self {
        Self::Configuration(error.into())
    }
}

fn respond(result: Result<Value, Refusal>) -> Response {
    let error = match result {
        Ok(value) => return Json(value).into_response(),
        Err(Refusal::Unavailable) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"schema_version":1,"error":{"code":"CONFIGURATION_UNAVAILABLE"}})),
            )
                .into_response()
        }
        Err(Refusal::Configuration(error)) => error,
    };
    use ConfigurationError as E;
    let (status, code) = match error {
        E::Invalid(_) => (StatusCode::BAD_REQUEST, "CONFIGURATION_INVALID"),
        E::Conflict => (StatusCode::CONFLICT, "CONFIGURATION_CONFLICT"),
        E::ValidationFailed => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "CONFIGURATION_VALIDATION_FAILED",
        ),
        E::RestartRequired => (StatusCode::CONFLICT, "CONFIGURATION_RESTART_REQUIRED"),
        E::Unsupported(_) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "CONFIGURATION_UNSUPPORTED",
        ),
        E::CapacityExceeded => (
            StatusCode::TOO_MANY_REQUESTS,
            "CONFIGURATION_CAPACITY_EXCEEDED",
        ),
        E::UnknownOutcome => (
            StatusCode::SERVICE_UNAVAILABLE,
            "CONFIGURATION_UNKNOWN_OUTCOME",
        ),
        E::Storage => (
            StatusCode::SERVICE_UNAVAILABLE,
            "CONFIGURATION_STORAGE_UNAVAILABLE",
        ),
    };
    let reason = match error {
        E::Invalid(reason) | E::Unsupported(reason) => Some(reason),
        _ => None,
    };
    (
        status,
        Json(json!({"schema_version":1,"error":{"code":code,"reason":reason}})),
    )
        .into_response()
}

fn use_cases(state: &AppState) -> Result<&Arc<ConfigurationUseCases>, Refusal> {
    state
        .engine
        .configuration_use_cases()
        .ok_or(Refusal::Unavailable)
}

fn configuration_id(raw: String) -> Result<SourceId, ConfigurationError> {
    SourceId::new(raw).map_err(|_| ConfigurationError::Invalid("invalid configuration identity"))
}

fn body<T>(payload: Result<Json<T>, JsonRejection>) -> Result<T, ConfigurationError> {
    payload.map(|Json(value)| value).map_err(|_| {
        ConfigurationError::Invalid("request JSON violates the strict schema; values redacted")
    })
}

fn idempotency_key(headers: &HeaderMap) -> Result<RequestId, ConfigurationError> {
    crate::reference::key(headers)
        .map_err(|_| ConfigurationError::Invalid("one valid Idempotency-Key header required"))
}

/// The expected active revision: one canonical unsigned `If-Match`, 0 when nothing is active.
fn expected_revision(headers: &HeaderMap) -> Result<u64, ConfigurationError> {
    let invalid = ConfigurationError::Invalid("one canonical If-Match active revision required");
    let mut values = headers.get_all(axum::http::header::IF_MATCH).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return Err(invalid);
    };
    let raw = value.to_str().map_err(|_| invalid.clone())?;
    let raw = raw
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(raw);
    match raw.parse::<u64>() {
        Ok(revision) if revision.to_string() == raw => Ok(revision),
        _ => Err(invalid),
    }
}

/// The verified caller's pseudonymous audit identity; explicit local development has none.
fn actor(ctx: &Option<Extension<AccessContext>>) -> String {
    ctx.as_ref()
        .and_then(|Extension(ctx)| ctx.subject_fingerprint())
        .map_or_else(|| "local-development".to_owned(), |f| format!("access:{f}"))
}

fn is_demo(profile: &Option<Extension<ServerProfile>>) -> bool {
    matches!(profile, Some(Extension(ServerProfile::Demo)))
}

async fn evidence(
    trace: &Option<Extension<ConfigurationTrace>>,
    fixtures: &[ConfigurationFixture],
) -> Result<FixtureEvidence, ConfigurationError> {
    match trace {
        Some(Extension(trace)) => {
            let events = replay_events(&trace.0).await?;
            fixture_evidence(fixtures, Some(&events))
        }
        None => fixture_evidence(fixtures, None),
    }
}

/// Dry run under the capabilities a start composes for the document. Writes nothing.
pub async fn validate(
    State(state): State<Arc<AppState>>,
    profile: Option<Extension<ServerProfile>>,
    trace: Option<Extension<ConfigurationTrace>>,
    payload: Result<Json<ValidateRequest>, JsonRejection>,
) -> Response {
    respond(
        async {
            let request = body(payload)?;
            let cases = use_cases(&state)?;
            evidence(&trace, &request.fixtures).await?;
            let cases = cases
                .with_capabilities(document_capabilities(&request.document, is_demo(&profile))?);
            Ok::<_, Refusal>(json!(
                cases.validate(&request.document, &request.fixtures).await?
            ))
        }
        .await,
    )
}

/// Classified changes between two documents, in their redacted forms.
pub async fn diff(payload: Result<Json<DiffRequest>, JsonRejection>) -> Response {
    respond(
        async {
            let request = body(payload)?;
            request.previous.validate()?;
            request.next.validate()?;
            Ok::<_, Refusal>(json!({
                "schema_version": 1,
                "changes": configuration_diff(&request.previous, &request.next),
                "previous": request.previous.redacted()?,
                "next": request.next.redacted()?,
            }))
        }
        .await,
    )
}

/// Validate and retain an immutable staged version.
#[allow(clippy::too_many_arguments)]
pub async fn stage(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    ctx: Option<Extension<AccessContext>>,
    profile: Option<Extension<ServerProfile>>,
    trace: Option<Extension<ConfigurationTrace>>,
    headers: HeaderMap,
    payload: Result<Json<StageRequest>, JsonRejection>,
) -> Response {
    respond(
        async {
            let id = configuration_id(id)?;
            let idempotency_key = idempotency_key(&headers)?;
            let request = body(payload)?;
            if request.document.pricing.config_id != id.as_str() {
                return Err(ConfigurationError::Invalid(
                    "path and document configuration identities differ",
                )
                .into());
            }
            let cases = use_cases(&state)?;
            let fixture_evidence = evidence(&trace, &request.fixtures).await?;
            let cases = cases
                .with_capabilities(document_capabilities(&request.document, is_demo(&profile))?);
            let staged = StagedConfiguration {
                document: request.document,
                idempotency_key,
                actor: actor(&ctx),
                reason: request.reason,
                staged_at: request.staged_at,
                fixture_evidence,
            };
            Ok::<_, Refusal>(json!(cases.stage(&staged, &request.fixtures).await?))
        }
        .await,
    )
}

/// Select a staged version. A running runtime applies a hot selection; a
/// restart-required one stays committed and applies at the next start.
#[allow(clippy::too_many_arguments)]
pub async fn activate(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    ctx: Option<Extension<AccessContext>>,
    profile: Option<Extension<ServerProfile>>,
    trace: Option<Extension<ConfigurationTrace>>,
    headers: HeaderMap,
    payload: Result<Json<ActivateRequest>, JsonRejection>,
) -> Response {
    respond(
        async {
            let request = body(payload)?;
            let selection = Selection {
                configuration_version: request.configuration_version,
                reason: request.reason,
                activated_at: request.activated_at,
                mode: request.mode,
                action: ActivationKind::Activate,
            };
            select(
                &state,
                id,
                &ctx,
                &profile,
                &trace,
                &headers,
                selection,
                &request.fixtures,
            )
            .await
        }
        .await,
    )
}

/// Select the content of a retained activation again, as a new activation.
#[allow(clippy::too_many_arguments)]
pub async fn rollback(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    ctx: Option<Extension<AccessContext>>,
    profile: Option<Extension<ServerProfile>>,
    trace: Option<Extension<ConfigurationTrace>>,
    headers: HeaderMap,
    payload: Result<Json<RollbackRequest>, JsonRejection>,
) -> Response {
    respond(
        async {
            let request = body(payload)?;
            let target = configuration_id(id.clone())?;
            let retained = use_cases(&state)?
                .repository()
                .get_activation(&target, request.to_revision)
                .await
                .map_err(ConfigurationError::from)?
                .ok_or(ConfigurationError::Invalid("missing rollback target"))?;
            let selection = Selection {
                configuration_version: retained.command.configuration_version,
                reason: request.reason,
                activated_at: request.activated_at,
                mode: request.mode,
                action: ActivationKind::Rollback {
                    to_revision: request.to_revision,
                },
            };
            select(
                &state,
                id,
                &ctx,
                &profile,
                &trace,
                &headers,
                selection,
                &request.fixtures,
            )
            .await
        }
        .await,
    )
}

struct Selection {
    configuration_version: Version,
    reason: String,
    activated_at: UtcTimestamp,
    mode: ReloadClass,
    action: ActivationKind,
}

#[allow(clippy::too_many_arguments)]
async fn select(
    state: &AppState,
    id: String,
    ctx: &Option<Extension<AccessContext>>,
    profile: &Option<Extension<ServerProfile>>,
    trace: &Option<Extension<ConfigurationTrace>>,
    headers: &HeaderMap,
    selection: Selection,
    fixtures: &[ConfigurationFixture],
) -> Result<Value, Refusal> {
    let id = configuration_id(id)?;
    let idempotency_key = idempotency_key(headers)?;
    let expected_revision = expected_revision(headers)?;
    let staged = use_cases(state)?
        .get_version(&id, selection.configuration_version)
        .await?
        .ok_or(ConfigurationError::Invalid("missing staged configuration"))?;
    let fixture_evidence = evidence(trace, fixtures).await?;
    let command = ActivationCommand {
        id,
        configuration_version: selection.configuration_version,
        expected_revision,
        idempotency_key,
        actor: actor(ctx),
        reason: selection.reason,
        activated_at: selection.activated_at,
        mode: selection.mode,
        action: selection.action,
        fixture_evidence,
    };
    let capabilities = document_capabilities(&staged.document, is_demo(profile))?;
    let outcome = state
        .engine
        .activate_configuration_with(capabilities, &command, fixtures)
        .await?;
    Ok(json!({
        "schema_version": 1,
        "activation": outcome.activation,
        "accepted_generation": outcome.accepted_generation,
        "accepted_activation_revision": outcome.accepted_activation_revision,
        "restart_required": outcome.restart_required,
    }))
}

/// Bounded activation audit, rollbacks included as new revisions.
pub async fn history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(page): Query<HistoryPage>,
) -> Response {
    respond(
        async {
            let id = configuration_id(id)?;
            let page = use_cases(&state)?
                .history(&id, &Pagination::new(page.offset, page.limit))
                .await?;
            Ok::<_, Refusal>(json!({
                "schema_version": 1,
                "items": page.items,
                "total": page.total,
                "offset": page.offset,
                "limit": page.limit,
            }))
        }
        .await,
    )
}

/// Changed fields between two retained versions. Values stay on the server, so a
/// redacted credential never compares as a change.
pub async fn version_diff(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(range): Query<VersionRange>,
) -> Response {
    respond(
        async {
            let id = configuration_id(id)?;
            let cases = use_cases(&state)?;
            let mut documents = Vec::with_capacity(2);
            for version in [range.from, range.to] {
                let version = Version::new(version)
                    .map_err(|_| ConfigurationError::Invalid("invalid configuration version"))?;
                let staged = cases
                    .get_version(&id, version)
                    .await?
                    .ok_or(ConfigurationError::Invalid("missing staged configuration"))?;
                documents.push(staged.document);
            }
            Ok::<_, Refusal>(json!({
                "schema_version": 1,
                "from": range.from,
                "to": range.to,
                "changes": configuration_diff(&documents[0], &documents[1]),
            }))
        }
        .await,
    )
}

/// One retained staged version, redacted, with its operator metadata.
pub async fn version(
    State(state): State<Arc<AppState>>,
    Path((id, version)): Path<(String, u64)>,
) -> Response {
    respond(
        async {
            let id = configuration_id(id)?;
            let version = Version::new(version)
                .map_err(|_| ConfigurationError::Invalid("invalid configuration version"))?;
            let staged = use_cases(&state)?
                .get_version(&id, version)
                .await?
                .ok_or(ConfigurationError::Invalid("missing staged configuration"))?;
            Ok::<_, Refusal>(json!({
                "schema_version": 1,
                "configuration": staged.document.redacted()?,
                "digest": staged.document.digest()?,
                "actor": staged.actor,
                "reason": staged.reason,
                "staged_at": staged.staged_at,
                "idempotency_key": staged.idempotency_key,
                "fixture_evidence": staged.fixture_evidence,
            }))
        }
        .await,
    )
}
