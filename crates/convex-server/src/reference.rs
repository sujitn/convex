//! Thin reference handlers delegating to the engine's authoritative repository.

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{header::ETAG, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use convex_core::contracts::{RequestId, UtcTimestamp, Version};
use convex_core::ids::InstrumentId;
use convex_core::Currency;
use convex_engine::ports::reference_data::{BondReferenceData, BondType, IssuerType};
use convex_engine::ports::reference_master::{ExternalIdentifier, ReferenceLookup};
use convex_engine::ports::reference_repository::*;
use convex_engine::ports::storage::{BondFilter, Pagination};
use serde::Deserialize;

use crate::handlers::AppState;

pub(crate) fn reference_error(error: ReferenceError) -> Response {
    crate::error::ApiError::from(error).into_response()
}

pub(crate) async fn commit_reference_transaction(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    payload: Result<Json<ReferenceCommand>, JsonRejection>,
) -> Response {
    let command = match payload {
        Ok(Json(command)) => command,
        Err(_) => {
            return reference_error(ReferenceError::InvalidInput(
                "invalid reference command JSON",
            ))
        }
    };
    let header_key = match key(&headers) {
        Ok(key) => key,
        Err(error) => return reference_error(error),
    };
    if command.idempotency_key != header_key {
        return reference_error(ReferenceError::InvalidInput(
            "header/body idempotency keys differ",
        ));
    }
    let time = match now() {
        Ok(time) => time,
        Err(error) => return reference_error(error),
    };
    match state.engine.commit_reference(&command, time).await {
        Ok(receipt) => Json(receipt).into_response(),
        Err(error) => reference_error(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolveReferenceRequest {
    identifier: ExternalIdentifier,
    lookup: ReferenceLookup,
}

pub(crate) async fn resolve_reference_at(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<ResolveReferenceRequest>, JsonRejection>,
) -> Response {
    let request = match payload {
        Ok(Json(request)) => request,
        Err(_) => {
            return reference_error(ReferenceError::InvalidInput(
                "invalid reference resolution JSON",
            ))
        }
    };
    match state
        .engine
        .reference_repository()
        .resolve_at(&request.identifier, request.lookup)
        .await
    {
        Ok(Some(resolved)) => Json(resolved).into_response(),
        Ok(None) => reference_error(ReferenceError::NotFound),
        Err(error) => reference_error(error),
    }
}

fn one_header<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<&'a str, ReferenceError> {
    let mut values = headers.get_all(name).iter();
    let first = values
        .next()
        .ok_or(ReferenceError::InvalidInput("missing mutation header"))?;
    if values.next().is_some() {
        return Err(ReferenceError::InvalidInput("duplicate mutation header"));
    }
    first
        .to_str()
        .map_err(|_| ReferenceError::InvalidInput("invalid mutation header"))
}

pub(crate) fn key(headers: &HeaderMap) -> Result<RequestId, ReferenceError> {
    RequestId::new(one_header(headers, "idempotency-key")?)
        .map_err(|_| ReferenceError::InvalidInput("invalid idempotency key"))
}

pub(crate) fn expected_version(headers: &HeaderMap) -> Result<Version, ReferenceError> {
    let raw = one_header(headers, "if-match")?;
    let raw = raw
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(raw);
    let value: u64 = raw
        .parse()
        .map_err(|_| ReferenceError::InvalidInput("expected positive If-Match version"))?;
    if value.to_string() != raw {
        return Err(ReferenceError::InvalidInput(
            "expected canonical If-Match version",
        ));
    }
    Version::new(value)
        .map_err(|_| ReferenceError::InvalidInput("expected positive If-Match version"))
}

fn identity(raw: String) -> Result<InstrumentId, ReferenceError> {
    let id = InstrumentId::new(raw);
    validate_reference_id(&id)?;
    Ok(id)
}

pub(crate) fn now() -> Result<UtcTimestamp, ReferenceError> {
    UtcTimestamp::from_datetime(chrono::Utc::now()).map_err(|_| ReferenceError::Unavailable)
}

/// Record an input version without treating it as an ETag for a market-dependent quote.
pub(crate) fn with_consumed_reference_version(
    mut response: Response,
    version: Version,
) -> Response {
    match HeaderValue::from_str(&version.get().to_string()) {
        Ok(value) => {
            response
                .headers_mut()
                .insert("x-convex-reference-version", value);
            response
        }
        _ => reference_error(ReferenceError::Unavailable),
    }
}

/// Reference records themselves can use their immutable version as an ETag.
pub(crate) fn with_reference_version(response: Response, version: Version) -> Response {
    let mut response = with_consumed_reference_version(response, version);
    match HeaderValue::from_str(&format!("\"{}\"", version.get())) {
        Ok(etag) => {
            response.headers_mut().insert(ETAG, etag);
            response
        }
        Err(_) => reference_error(ReferenceError::Unavailable),
    }
}

fn bond_response(record: ReferenceRecord, status: StatusCode) -> Response {
    match record.bond {
        Some(bond) => with_reference_version((status, Json(bond)).into_response(), record.version),
        None => reference_error(ReferenceError::NotFound),
    }
}

async fn mutate(
    state: &AppState,
    headers: &HeaderMap,
    changes: Vec<ReferenceChange>,
    status: StatusCode,
) -> Response {
    let command = match key(headers) {
        Ok(idempotency_key) => ReferenceCommand {
            master_changes: Vec::new(),
            idempotency_key,
            changes,
        },
        Err(error) => return reference_error(error),
    };
    let recorded_at = match now() {
        Ok(time) => time,
        Err(error) => return reference_error(error),
    };
    match state.engine.commit_reference(&command, recorded_at).await {
        Ok(result) => match result.records.into_iter().next() {
            Some(record) if status == StatusCode::NO_CONTENT => {
                with_reference_version(status.into_response(), record.version)
            }
            Some(record) => bond_response(record, status),
            None => reference_error(ReferenceError::Unavailable),
        },
        Err(error) => reference_error(error),
    }
}

pub(crate) async fn create_bond(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    payload: Result<Json<BondReferenceData>, JsonRejection>,
) -> Response {
    let bond = match payload {
        Ok(Json(bond)) => bond,
        Err(_) => {
            return reference_error(ReferenceError::InvalidInput(
                "invalid complete bond reference JSON",
            ))
        }
    };
    mutate(
        &state,
        &headers,
        vec![ReferenceChange::Create {
            bond: Box::new(bond),
        }],
        StatusCode::CREATED,
    )
    .await
}

pub(crate) async fn update_bond(
    State(state): State<Arc<AppState>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<BondReferenceData>, JsonRejection>,
) -> Response {
    let bond = match payload {
        Ok(Json(bond)) => bond,
        Err(_) => {
            return reference_error(ReferenceError::InvalidInput(
                "invalid complete bond reference JSON",
            ))
        }
    };
    let expected_version = match expected_version(&headers) {
        Ok(v) => v,
        Err(error) => return reference_error(error),
    };
    let id = match identity(raw) {
        Ok(id) => id,
        Err(error) => return reference_error(error),
    };
    if id != bond.instrument_id {
        return reference_error(ReferenceError::InvalidInput(
            "path and body instrument identities differ",
        ));
    }
    mutate(
        &state,
        &headers,
        vec![ReferenceChange::Update {
            bond: Box::new(bond),
            expected_version,
        }],
        StatusCode::OK,
    )
    .await
}

pub(crate) async fn delete_bond(
    State(state): State<Arc<AppState>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
) -> Response {
    let expected_version = match expected_version(&headers) {
        Ok(v) => v,
        Err(error) => return reference_error(error),
    };
    let instrument_id = match identity(raw) {
        Ok(id) => id,
        Err(error) => return reference_error(error),
    };
    mutate(
        &state,
        &headers,
        vec![ReferenceChange::Delete {
            instrument_id,
            expected_version,
        }],
        StatusCode::NO_CONTENT,
    )
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BatchBondCreateRequest {
    bonds: Vec<BondReferenceData>,
}

pub(crate) async fn batch_create_bonds(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    payload: Result<Json<BatchBondCreateRequest>, JsonRejection>,
) -> Response {
    let request = match payload {
        Ok(Json(request)) => request,
        Err(_) => {
            return reference_error(ReferenceError::InvalidInput(
                "invalid complete bond reference batch JSON",
            ))
        }
    };
    let idempotency_key = match key(&headers) {
        Ok(key) => key,
        Err(error) => return reference_error(error),
    };
    let command = ReferenceCommand {
        master_changes: Vec::new(),
        idempotency_key,
        changes: request
            .bonds
            .into_iter()
            .map(|bond| ReferenceChange::Create {
                bond: Box::new(bond),
            })
            .collect(),
    };
    let time = match now() {
        Ok(time) => time,
        Err(error) => return reference_error(error),
    };
    match state.engine.commit_reference(&command, time).await {
        Ok(receipt) => Json(serde_json::json!({"created":receipt.records.len(), "commit":receipt}))
            .into_response(),
        Err(error) => reference_error(error),
    }
}

pub(crate) async fn get_bond(
    State(state): State<Arc<AppState>>,
    Path(raw): Path<String>,
) -> Response {
    let id = match identity(raw) {
        Ok(id) => id,
        Err(error) => return reference_error(error),
    };
    match state.engine.reference_for_pricing(&id).await {
        Ok(Some(record)) => bond_response(record, StatusCode::OK),
        Ok(None) => reference_error(ReferenceError::NotFound),
        Err(error) => reference_error(error),
    }
}

pub(crate) async fn get_bond_version(
    State(state): State<Arc<AppState>>,
    Path((raw, version)): Path<(String, u64)>,
) -> Response {
    let id = match identity(raw) {
        Ok(id) => id,
        Err(error) => return reference_error(error),
    };
    let version = match Version::new(version) {
        Ok(v) => v,
        Err(_) => return reference_error(ReferenceError::InvalidInput("invalid version")),
    };
    match state
        .engine
        .reference_repository()
        .get_version(&id, version)
        .await
    {
        Ok(Some(record)) => with_reference_version(Json(record).into_response(), version),
        Ok(None) => reference_error(ReferenceError::NotFound),
        Err(error) => reference_error(error),
    }
}

pub(crate) async fn get_reference_commit(
    State(state): State<Arc<AppState>>,
    Path(raw): Path<String>,
) -> Response {
    let key = match RequestId::new(raw) {
        Ok(key) => key,
        Err(_) => return reference_error(ReferenceError::InvalidInput("invalid idempotency key")),
    };
    match state.engine.reference_repository().get_commit(&key).await {
        Ok(Some(receipt)) => Json(receipt).into_response(),
        Ok(None) => reference_error(ReferenceError::NotFound),
        Err(error) => reference_error(error),
    }
}

async fn resolve(state: &AppState, identifier: ExternalIdentifier) -> Response {
    match state
        .engine
        .reference_repository()
        .resolve(&identifier)
        .await
    {
        Ok(Some(record)) => bond_response(record, StatusCode::OK),
        Ok(None) => reference_error(ReferenceError::NotFound),
        Err(error) => reference_error(error),
    }
}

pub(crate) async fn get_bond_by_isin(
    State(state): State<Arc<AppState>>,
    Path(value): Path<String>,
) -> Response {
    resolve(&state, ExternalIdentifier::Isin { value }).await
}
pub(crate) async fn get_bond_by_cusip(
    State(state): State<Arc<AppState>>,
    Path(value): Path<String>,
) -> Response {
    resolve(&state, ExternalIdentifier::Cusip { value }).await
}

fn default_limit() -> usize {
    100
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BondListQuery {
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
    currency: Option<String>,
    issuer_type: Option<String>,
    bond_type: Option<String>,
    country: Option<String>,
    sector: Option<String>,
    issuer_id: Option<String>,
    q: Option<String>,
    is_callable: Option<bool>,
    is_floating: Option<bool>,
    is_inflation_linked: Option<bool>,
}

impl BondListQuery {
    fn filter(&self) -> Result<BondFilter, ReferenceError> {
        let currency = self
            .currency
            .as_ref()
            .map(|value| {
                Currency::from_code(value)
                    .ok_or(ReferenceError::InvalidInput("unknown currency filter"))
            })
            .transpose()?;
        let issuer_type = self
            .issuer_type
            .as_ref()
            .map(|value| match value.to_lowercase().as_str() {
                "sovereign" => Ok(IssuerType::Sovereign),
                "agency" => Ok(IssuerType::Agency),
                "supranational" => Ok(IssuerType::Supranational),
                "corporateig" | "corporate_ig" => Ok(IssuerType::CorporateIG),
                "corporatehy" | "corporate_hy" => Ok(IssuerType::CorporateHY),
                "financial" => Ok(IssuerType::Financial),
                "municipal" => Ok(IssuerType::Municipal),
                _ => Err(ReferenceError::InvalidInput("unknown issuer type filter")),
            })
            .transpose()?;
        let bond_type = self
            .bond_type
            .as_ref()
            .map(|value| match value.to_lowercase().as_str() {
                "fixedbullet" | "fixed_bullet" => Ok(BondType::FixedBullet),
                "fixedcallable" | "fixed_callable" => Ok(BondType::FixedCallable),
                "fixedputable" | "fixed_putable" => Ok(BondType::FixedPutable),
                "floatingrate" | "floating_rate" | "frn" => Ok(BondType::FloatingRate),
                "zerocoupon" | "zero_coupon" => Ok(BondType::ZeroCoupon),
                "inflationlinked" | "inflation_linked" | "linker" => Ok(BondType::InflationLinked),
                "amortizing" => Ok(BondType::Amortizing),
                "convertible" => Ok(BondType::Convertible),
                _ => Err(ReferenceError::InvalidInput("unknown bond type filter")),
            })
            .transpose()?;
        Ok(BondFilter {
            currency,
            issuer_type,
            bond_type,
            country: self.country.clone(),
            sector: self.sector.clone(),
            issuer_id: self.issuer_id.clone(),
            text_search: self.q.clone(),
            is_callable: self.is_callable,
            is_floating: self.is_floating,
            is_inflation_linked: self.is_inflation_linked,
            ..BondFilter::default()
        })
    }
}

pub(crate) async fn list_bonds(
    State(state): State<Arc<AppState>>,
    query: Result<Query<BondListQuery>, QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) => query,
        Err(_) => return reference_error(ReferenceError::InvalidInput("invalid reference query")),
    };
    let filter = match query.filter() {
        Ok(filter) => filter,
        Err(error) => return reference_error(error),
    };
    match state
        .engine
        .reference_repository()
        .list(&filter, &Pagination::new(query.offset, query.limit))
        .await
    {
        Ok(page) => {
            let versions: Vec<_> = page.items.iter().map(|record| serde_json::json!({"instrument_id":record.instrument_id,"version":record.version})).collect();
            let bonds: Vec<_> = page
                .items
                .into_iter()
                .filter_map(|record| record.bond)
                .collect();
            Json(serde_json::json!({"bonds":bonds,"versions":versions,"total":page.total,"limit":page.limit,"offset":page.offset})).into_response()
        }
        Err(error) => reference_error(error),
    }
}
