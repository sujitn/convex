//! Read-only configuration inspection and shared local composition capabilities.
use crate::handlers::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use convex_core::contracts::*;
use convex_engine::{
    configuration::{ConfigurationCapabilities, ConfigurationError, ConfigurationUseCases},
    ports::configuration::*,
};
use serde::Deserialize;
use serde_json::{json, Value};
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
