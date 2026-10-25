//! One fail-closed HTTP boundary. Endpoint classification is deliberately a closed table.
use super::{
    AccessContext, AccessControl, AccessError, Action, Admission, BearerToken, DataScope, Resource,
    ResourceKind,
};
use axum::{
    body::{to_bytes, Body},
    extract::{FromRequestParts, MatchedPath, RawPathParams, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

const MAX_BODY: usize = 2 * 1024 * 1024;
/// A configuration document (at most 2 MiB) with its fixtures and operator metadata.
pub(crate) const MAX_CONFIGURATION_REQUEST: usize = 4 * 1024 * 1024;
pub(crate) const MAX_RESPONSE: usize = 4 * 1024 * 1024;
// Existing book/reference ports carry a whole captured definition. Other routes
// keep their original transport bounds; authorization still scans every value.
fn book_transport(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/reference/transactions"
            | "/api/v1/reference/import"
            | "/api/v1/reference/commits/:key"
            | "/api/v1/analytics/snapshots/:snapshot_id"
            | "/api/v1/analytics/results/:result_id"
            | "/api/v1/books/:book_id"
    )
}
/// Retained by the upgrade callback; quota is released even if upgrade fails.
#[derive(Clone)]
pub(crate) struct WsAccess {
    pub access: Arc<AccessControl>,
    pub context: AccessContext,
    pub _admission: Arc<Admission>,
}
/// Exact normalized allowed origins installed by the composition root.
#[derive(Clone, Default)]
pub(crate) struct AllowedOrigins(pub Vec<HeaderValue>);
/// Outer origin/preflight guard also runs before CorsLayer can short-circuit OPTIONS.
pub(crate) async fn guard_origin(
    State((access, origins)): State<(Arc<AccessControl>, AllowedOrigins)>,
    request: Request,
    next: Next,
) -> Response {
    if let Err(error) = check_origin(request.headers(), &origins) {
        return response(error);
    }
    match preflight(&request) {
        Err(error) => response(error),
        Ok(true) => match access.admit_attempt() {
            Ok(_permit) => next.run(request).await,
            Err(error) => response(error),
        },
        Ok(false) => next.run(request).await,
    }
}

pub(crate) fn response(error: AccessError) -> Response {
    let status = match error {
        AccessError::Unauthenticated => StatusCode::UNAUTHORIZED,
        AccessError::Forbidden => StatusCode::FORBIDDEN,
        AccessError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        AccessError::Quota => StatusCode::TOO_MANY_REQUESTS,
        _ => StatusCode::BAD_REQUEST,
    };
    tracing::warn!(target: "convex_server::access_audit", code = %error, status = status.as_u16(), "Access denied");
    let mut response = (
        status,
        Json(serde_json::json!({"error":error.to_string(),"code":error.to_string()})),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if status == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
    }
    response
}
fn bearer(headers: &HeaderMap) -> Result<Option<BearerToken>, AccessError> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let Some(first) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(AccessError::Unauthenticated);
    }
    let value = first.to_str().map_err(|_| AccessError::Unauthenticated)?;
    let (scheme, token) = value.split_once(' ').ok_or(AccessError::Unauthenticated)?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(AccessError::Unauthenticated);
    }
    Ok(Some(BearerToken::parse(token)?))
}
fn check_origin(headers: &HeaderMap, allowed: &AllowedOrigins) -> Result<(), AccessError> {
    let mut origins = headers.get_all(header::ORIGIN).iter();
    if let Some(origin) = origins.next() {
        if origins.next().is_some() || !allowed.0.contains(origin) {
            return Err(AccessError::Forbidden);
        }
    }
    Ok(())
}
fn preflight(request: &Request) -> Result<bool, AccessError> {
    if request.method() != Method::OPTIONS {
        return Ok(false);
    }
    if request.headers().get(header::ORIGIN).is_none() {
        return Err(AccessError::Forbidden);
    }
    let method = request
        .headers()
        .get(header::ACCESS_CONTROL_REQUEST_METHOD)
        .and_then(|v| v.to_str().ok())
        .ok_or(AccessError::Forbidden)?;
    if !["GET", "POST", "PUT", "DELETE", "HEAD"].contains(&method) {
        return Err(AccessError::Forbidden);
    }
    if let Some(value) = request
        .headers()
        .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
    {
        let value = value.to_str().map_err(|_| AccessError::Forbidden)?;
        if value.split(',').any(|h| {
            ![
                "authorization",
                "content-type",
                "idempotency-key",
                "if-match",
            ]
            .contains(&h.trim().to_ascii_lowercase().as_str())
        }) {
            return Err(AccessError::Forbidden);
        }
    }
    Ok(true)
}
struct Requirement {
    action: Action,
    data: Vec<DataScope>,
    resources: Vec<Resource>,
    body_required: bool,
    inspect_output_resources: bool,
}
fn requirement(
    path: &str,
    method: &Method,
    params: &BTreeMap<String, String>,
) -> Result<Requirement, AccessError> {
    use {Action::*, DataScope as D, ResourceKind as R};
    let read = method == Method::GET || method == Method::HEAD;
    let mut req = Requirement {
        action: Calculate,
        data: vec![D::Market, D::Results],
        resources: Vec::new(),
        body_required: !read,
        inspect_output_resources: true,
    };
    let mut exact = |kind, name: &str| -> Result<(), AccessError> {
        let value = params.get(name).ok_or(AccessError::Forbidden)?;
        req.resources.push(Resource::new(kind, value));
        Ok(())
    };
    match path {
        "/api/v1/bonds" | "/api/v1/bonds/batch" => {
            req.action = if read { ReferenceRead } else { ReferenceWrite };
            req.data = vec![D::Reference];
            if read {
                req.resources.push(Resource::new(R::Instrument, "*"));
            }
        }
        "/api/v1/bonds/:instrument_id" | "/api/v1/bonds/:instrument_id/versions/:version" => {
            exact(R::Instrument, "instrument_id")?;
            req.action = if read { ReferenceRead } else { ReferenceWrite };
            req.data = vec![D::Reference];
            req.body_required = method == Method::PUT;
        }
        "/api/v1/bonds/isin/:isin"
        | "/api/v1/bonds/cusip/:cusip"
        | "/api/v1/reference/resolve"
        | "/api/v1/reference/commits/:key" => {
            req.action = ReferenceRead;
            req.data = vec![D::Reference];
            req.resources.push(Resource::new(R::Instrument, "*"));
        }
        "/api/v1/reference/transactions" | "/api/v1/reference/import" => {
            req.action = ReferenceWrite;
            req.data = vec![D::Reference];
        }
        // Books keep their own listing and scope.
        "/api/v1/reference/masters/:kind" => {
            req.action = ReferenceRead;
            req.data = vec![D::Reference];
            req.resources.push(Resource::new(R::Instrument, "*"));
        }
        "/api/v1/quotes/:instrument_id" => {
            exact(R::Instrument, "instrument_id")?;
            req.action = MarketRead;
            req.data = vec![D::Reference, D::Market, D::Results];
            req.resources.push(Resource::new(R::Source, "*"));
            req.resources.push(Resource::new(R::Curve, "*"));
        }
        // They read the activated configuration's curves unless the request supplies them.
        "/api/v1/quote" | "/api/v1/batch/price" => {
            req.data.push(D::Reference);
            req.data.push(D::Market);
            req.resources.push(Resource::new(R::Source, "*"));
            req.resources.push(Resource::new(R::Curve, "*"));
        }
        // Stateless calculator: stores nothing, so it is a calculation, not a curve
        // write. Its `curve_id` is an echoed label, not a scoped curve.
        "/api/v1/curves/bootstrap" => {
            req.data = vec![D::Market];
            req.inspect_output_resources = false;
        }
        "/api/v1/curves" => {
            req.action = if read { MarketRead } else { CurveWrite };
            req.data = vec![D::Market];
            req.resources.push(Resource::new(R::Source, "*"));
            if read {
                req.resources.push(Resource::new(R::Curve, "*"));
            }
        }
        "/api/v1/curves/:curve_id"
        | "/api/v1/curves/:curve_id/zero/:tenor"
        | "/api/v1/curves/:curve_id/discount/:tenor"
        | "/api/v1/curves/:curve_id/forward/:t1/:t2" => {
            exact(R::Curve, "curve_id")?;
            req.action = if read { MarketRead } else { CurveWrite };
            req.data = vec![D::Market];
            req.resources.push(Resource::new(R::Source, "*"));
            req.body_required = false;
        }
        "/api/v1/runtime" | "/api/v1/dependencies" | "/api/v1/ws/status" => {
            req.action = Operate;
            req.data = vec![D::Operations];
            req.body_required = false;
        }
        "/api/v1/runtime/results" => {
            req.action = ResultRead;
            req.data = vec![D::Results, D::Market, D::Reference];
            req.resources.extend([
                Resource::new(R::Instrument, "*"),
                Resource::new(R::Curve, "*"),
                Resource::new(R::Source, "*"),
            ]);
        }
        "/api/v1/runtime/valuation" => {
            req.data.push(D::Reference);
        }
        // Feed input, scoped by each event's instrument and source like other market writes.
        "/api/v1/market/quotes" => {
            req.action = CurveWrite;
            req.data = vec![D::Market];
        }
        "/api/v1/analytics"
        | "/api/v1/analytics/snapshots"
        | "/api/v1/analytics/snapshots/:snapshot_id/run/:calculation"
        | "/api/v1/analytics/snapshots/:snapshot_id"
        | "/api/v1/analytics/results/:result_id" => {
            if read {
                req.action = ResultRead;
            }
            // Complete supplied library contracts can contain every resource shape.
            // Until semantic per-operation scoping exists, require explicit broad
            // grants rather than relying on partial JSON-key scope discovery.
            req.data = vec![D::Reference, D::Market, D::Books, D::Results];
            // Every relevant resource already requires an explicit wildcard
            // grant. Large native reports need no partial key-based discovery;
            // the complete original entitlement is still rechecked on return.
            req.inspect_output_resources = false;
            req.resources.extend([
                Resource::new(R::Instrument, "*"),
                Resource::new(R::Curve, "*"),
                Resource::new(R::Source, "*"),
                Resource::new(R::Book, "*"),
            ]);
        }
        "/api/v1/analytics/curves" | "/api/v1/analytics/fixings" => {
            req.action = CurveWrite;
            req.data = vec![D::Market];
            req.resources
                .extend([Resource::new(R::Curve, "*"), Resource::new(R::Source, "*")]);
        }
        "/api/v1/configuration/schema" | "/api/v1/configuration/diff" => {
            req.action = Operate;
            req.data = vec![D::Configuration];
        }
        // Validation runs real fixtures over reference and market inputs; it writes nothing.
        "/api/v1/configuration/validate" => {
            req.action = Operate;
            req.data = vec![D::Configuration, D::Reference, D::Market];
        }
        "/api/v1/configuration/:id/history"
        | "/api/v1/configuration/:id/versions/:version"
        | "/api/v1/configuration/:id/diff" => {
            exact(R::Configuration, "id")?;
            req.action = Operate;
            req.data = vec![D::Configuration];
        }
        "/api/v1/configuration/:id/stage"
        | "/api/v1/configuration/:id/activate"
        | "/api/v1/configuration/:id/rollback" => {
            exact(R::Configuration, "id")?;
            req.action = ConfigurationWrite;
            req.data = vec![D::Configuration, D::Reference, D::Market];
        }
        "/api/v1/configuration/:id" => {
            exact(R::Configuration, "id")?;
            req.action = Operate;
            req.data = vec![D::Configuration, D::Reference, D::Market];
        }
        "/ws" | "/api/v1/ws" => {
            req.action = Stream;
            req.data.clear();
            req.body_required = false;
        }
        "/api/v1/books" => {
            req.action = BookRead;
            req.data = vec![D::Books];
            req.resources.push(Resource::new(R::Book, "*"));
        }
        "/api/v1/books/:book_id"
        | "/api/v1/books/:book_id/versions/:version"
        | "/api/v1/books/:book_id/history" => {
            exact(R::Book, "book_id")?;
            req.action = if read { BookRead } else { BookWrite };
            req.data = vec![D::Books];
            req.body_required = !read;
        }
        "/api/v1/portfolios" | "/api/v1/portfolios/batch" => {
            req.action = if read { BookRead } else { BookWrite };
            req.data = vec![D::Books];
            if read {
                req.resources.push(Resource::new(R::Book, "*"));
                req.resources.push(Resource::new(R::Instrument, "*"));
            }
        }
        "/api/v1/portfolios/:portfolio_id"
        | "/api/v1/portfolios/:portfolio_id/positions"
        | "/api/v1/portfolios/:portfolio_id/positions/:instrument_id" => {
            exact(R::Book, "portfolio_id")?;
            if params.contains_key("instrument_id") {
                exact(R::Instrument, "instrument_id")?;
            }
            req.action = if read { BookRead } else { BookWrite };
            req.data = vec![D::Books];
            req.body_required = !read && method != Method::DELETE;
        }
        "/api/v1/etf/inav"
        | "/api/v1/etf/inav/batch"
        | "/api/v1/etf/sec-yield"
        | "/api/v1/etf/basket"
        | "/api/v1/etf/basket/analyze"
        | "/api/v1/portfolio/analytics"
        | "/api/v1/portfolio/analytics/batch"
        | "/api/v1/portfolio/duration-contribution"
        | "/api/v1/portfolio/key-rate-duration"
        | "/api/v1/portfolio/risk-contributions"
        | "/api/v1/portfolio/buckets/sector"
        | "/api/v1/portfolio/buckets/rating"
        | "/api/v1/portfolio/buckets/maturity"
        | "/api/v1/portfolio/buckets/custom"
        | "/api/v1/portfolio/buckets"
        | "/api/v1/benchmark/compare"
        | "/api/v1/benchmark/active-weights"
        | "/api/v1/benchmark/tracking-error"
        | "/api/v1/benchmark/attribution"
        | "/api/v1/liquidity/metrics"
        | "/api/v1/liquidity/distribution"
        | "/api/v1/liquidity/analysis"
        | "/api/v1/credit/quality"
        | "/api/v1/credit/migration-risk"
        | "/api/v1/credit/analysis" => {
            req.resources.push(Resource::new(R::Source, "*"));
        }
        // New endpoints must explicitly state their access contract before they can run.
        _ => return Err(AccessError::Forbidden),
    }
    Ok(req)
}
/// Scope extraction visits at most 65536 JSON nodes and collects at most 1024 identities.
/// Original bytes are forwarded, preserving financial number/decimal representation.
pub(crate) fn resources(value: &serde_json::Value) -> Result<Vec<Resource>, AccessError> {
    resources_bounded(value, false)
}
fn resources_bounded(value: &serde_json::Value, book: bool) -> Result<Vec<Resource>, AccessError> {
    fn put(
        result: &mut Vec<Resource>,
        seen: &mut BTreeSet<(ResourceKind, String)>,
        resource: Resource,
        book: bool,
    ) -> Result<(), AccessError> {
        if book && !seen.insert((resource.kind, resource.id.clone())) {
            return Ok(());
        }
        if result.len() >= 1024 {
            return Err(AccessError::InvalidRequest);
        }
        result.push(resource);
        Ok(())
    }
    let maximum = if book { 1_048_576 } else { 65536 };
    let mut seen = BTreeSet::new();
    let mut frontier = vec![value];
    let mut count = 0usize;
    let mut result = Vec::new();
    while let Some(value) = frontier.pop() {
        count = count.saturating_add(1);
        if count > maximum {
            return Err(AccessError::InvalidRequest);
        }
        match value {
            serde_json::Value::Object(map) => {
                // A retained calculation definition may contain whole private
                // books. This also protects reads through reference receipts.
                if map.get("kind").and_then(|v| v.as_str()) == Some("calculation")
                    && map
                        .get("definition")
                        .and_then(|v| v.get("calculations"))
                        .is_some_and(|v| v.is_object())
                {
                    if !book && result.len() > 1020 {
                        return Err(AccessError::InvalidRequest);
                    }
                    for resource in [
                        Resource::new(ResourceKind::Instrument, "*"),
                        Resource::new(ResourceKind::Curve, "*"),
                        Resource::new(ResourceKind::Source, "*"),
                        Resource::new(ResourceKind::Book, "*"),
                    ] {
                        put(&mut result, &mut seen, resource, book)?;
                    }
                }
                // Canonical InputVersion uses kind/id for calibration dependencies.
                if let (Some(kind), Some(id)) = (
                    map.get("kind").and_then(|v| v.as_str()),
                    map.get("id").and_then(|v| v.as_str()),
                ) {
                    let kind = match kind {
                        "instrument" => Some(ResourceKind::Instrument),
                        "curve" => Some(ResourceKind::Curve),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        if (!book && result.len() >= 1024)
                            || !super::policy::valid_id(id)
                            || id == "*"
                        {
                            return Err(AccessError::InvalidRequest);
                        }
                        put(&mut result, &mut seen, Resource::new(kind, id), book)?;
                    }
                }
                for (key, value) in map {
                    let kind = match key.as_str() {
                        "instrument_id" | "security_id" => Some(ResourceKind::Instrument),
                        "portfolio_id" | "book_id" => Some(ResourceKind::Book),
                        "curve_id" => Some(ResourceKind::Curve),
                        "discount_curve_id"
                        | "benchmark_curve_id"
                        | "government_curve_id"
                        | "forward_curve_id" => Some(ResourceKind::Curve),
                        "etf_id" => Some(ResourceKind::Etf),
                        "source" => Some(ResourceKind::Source),
                        _ => None,
                    };
                    if let Some(kind) = match key.as_str() {
                        "required_curves" => Some(ResourceKind::Curve),
                        "instrument_ids" => Some(ResourceKind::Instrument),
                        _ => None,
                    } {
                        let ids = value.as_array().ok_or(AccessError::InvalidRequest)?;
                        for value in ids {
                            let id = value.as_str().ok_or(AccessError::InvalidRequest)?;
                            if (!book && result.len() >= 1024)
                                || !super::policy::valid_id(id)
                                || id == "*"
                            {
                                return Err(AccessError::InvalidRequest);
                            }
                            put(&mut result, &mut seen, Resource::new(kind, id), book)?;
                        }
                    }
                    if let (Some(kind), Some(id)) = (kind, value.as_str()) {
                        if (!book && result.len() >= 1024)
                            || !super::policy::valid_id(id)
                            || id == "*"
                        {
                            return Err(AccessError::InvalidRequest);
                        }
                        put(&mut result, &mut seen, Resource::new(kind, id), book)?;
                    }
                    frontier.push(value);
                }
            }
            serde_json::Value::Array(array) => frontier.extend(array.iter()),
            _ => {}
        }
        if frontier.len() > maximum {
            return Err(AccessError::InvalidRequest);
        }
    }
    Ok(result)
}
pub(crate) async fn enforce(
    State(access): State<Arc<AccessControl>>,
    mut request: Request,
    next: Next,
) -> Response {
    let book = request
        .extensions()
        .get::<MatchedPath>()
        .is_some_and(|path| book_transport(path.as_str()));
    match enforce_inner(&access, &mut request).await {
        Err(error) => response(error),
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Ok(Some((ctx, req, admission))) => {
            let websocket = request.uri().path() == "/ws" || request.uri().path() == "/api/v1/ws";
            request.headers_mut().remove(header::AUTHORIZATION);
            request.extensions_mut().insert(ctx.clone());
            // CPU handlers retain this same admission until their blocking work
            // actually exits, including when the HTTP caller disconnects.
            request.extensions_mut().insert(admission.clone());
            if websocket {
                request.extensions_mut().insert(WsAccess {
                    access: access.clone(),
                    context: ctx.clone(),
                    _admission: admission.clone(),
                });
            }
            // Ownership is transferred after extraction; no credential reaches handlers/log layers.
            let mut result = next.run(request).await;
            if !websocket && !req.data.is_empty() && !access.is_local_development() {
                if let Err(error) = access.authorize(&ctx, req.action, &req.data, &req.resources) {
                    return response(error);
                }
                if result.status().is_success()
                    && req.inspect_output_resources
                    && result
                        .headers()
                        .get(header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|v| v.starts_with("application/json"))
                {
                    let body = std::mem::replace(result.body_mut(), Body::empty());
                    let bytes = match tokio::time::timeout(
                        Duration::from_secs(3),
                        to_bytes(body, if book { 32 * 1024 * 1024 } else { MAX_RESPONSE }),
                    )
                    .await
                    {
                        Ok(Ok(bytes)) => bytes,
                        _ => return response(AccessError::Unavailable),
                    };
                    let scopes = match strict_json(&bytes).and_then(|v| {
                        if book {
                            resources_bounded(&v, true)
                        } else {
                            resources(&v)
                        }
                    }) {
                        Ok(scopes) => scopes,
                        Err(_) => return response(AccessError::Unavailable),
                    };
                    let mut data = req.data.clone();
                    if scopes
                        .iter()
                        .any(|r| r.kind == ResourceKind::Book && r.id == "*")
                    {
                        for scope in [
                            DataScope::Reference,
                            DataScope::Market,
                            DataScope::Books,
                            DataScope::Results,
                        ] {
                            if !data.contains(&scope) {
                                data.push(scope);
                            }
                        }
                    }
                    let mut required = req.resources.clone();
                    required.extend(scopes);
                    if let Err(error) = access.authorize(&ctx, req.action, &data, &required) {
                        return response(error);
                    }
                    *result.body_mut() = Body::from(bytes);
                }
            }
            result
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            tracing::info!(target: "convex_server::access_audit", policy_version = access.policy_version(), subject = ?ctx.subject_fingerprint(), action = ?req.action, status = result.status().as_u16(), "Access admitted");
            drop(admission);
            result
        }
    }
}
async fn enforce_inner(
    access: &Arc<AccessControl>,
    request: &mut Request,
) -> Result<Option<(AccessContext, Requirement, Arc<Admission>)>, AccessError> {
    if request.uri().to_string().len() > 4096 {
        return Err(AccessError::InvalidRequest);
    }
    let permit = access.admit_attempt()?;
    let origins = request
        .extensions()
        .get::<AllowedOrigins>()
        .cloned()
        .unwrap_or_default();
    check_origin(request.headers(), &origins)?;
    if preflight(request)? {
        return Ok(None);
    }
    let matched = request
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned());
    let public = matched.as_deref().is_some_and(|path| {
        [
            "/live",
            "/health",
            "/api/v1/health",
            "/ready",
            "/strategy-ready",
        ]
        .contains(&path)
    }) && (request.method() == Method::GET || request.method() == Method::HEAD);
    if public {
        // Public probes pass through the same quotas without manufacturing an authenticated identity.
        let ctx = AccessContext {
            identity: None,
            token: None,
            local: true,
            verified_at: std::time::Instant::now(),
        };
        let req = Requirement {
            action: Action::Operate,
            data: Vec::new(),
            resources: Vec::new(),
            body_required: false,
            inspect_output_resources: true,
        };
        return Ok(Some((
            ctx,
            req,
            Arc::new(Admission {
                access: access.clone(),
                subject: None,
                session: false,
                _permit: permit,
            }),
        )));
    }
    let token = bearer(request.headers())?;
    let ctx = tokio::time::timeout(Duration::from_secs(3), access.authenticate(token))
        .await
        .map_err(|_| AccessError::Unavailable)??;
    let path = matched.as_deref().ok_or(AccessError::Forbidden)?;
    let (mut parts, body) = std::mem::replace(request, Request::new(Body::empty())).into_parts();
    let params = RawPathParams::from_request_parts(&mut parts, &())
        .await
        .ok()
        .map(|p| {
            p.iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect()
        })
        .unwrap_or_default();
    *request = Request::from_parts(parts, body);
    let mut req = requirement(path, request.method(), &params)?;
    access.authorize(&ctx, req.action, &req.data, &req.resources)?;
    let websocket = ["/ws", "/api/v1/ws"].contains(&path);
    let admission = Arc::new(access.admit(&ctx, websocket, permit)?);
    if req.body_required && !access.is_local_development() {
        let body = std::mem::replace(request.body_mut(), Body::empty());
        let book = book_transport(path);
        let limit = if book {
            8 * 1024 * 1024
        } else if path.starts_with("/api/v1/configuration/") {
            MAX_CONFIGURATION_REQUEST
        } else {
            MAX_BODY
        };
        let bytes = tokio::time::timeout(Duration::from_secs(3), to_bytes(body, limit))
            .await
            .map_err(|_| AccessError::InvalidRequest)?
            .map_err(|_| AccessError::InvalidRequest)?;
        let value = strict_json(&bytes)?;
        let mut scopes = if book {
            resources_bounded(&value, true)?
        } else {
            resources(&value)?
        };
        if path == "/api/v1/curves/bootstrap" {
            scopes.retain(|s| s.kind != ResourceKind::Curve);
        }
        // An import file wraps the same transaction in its envelope's `command`.
        let command = match path {
            "/api/v1/reference/transactions" => Some(&value),
            "/api/v1/reference/import" => value.get("command"),
            _ => None,
        };
        if let Some(command) = command.filter(|c| {
            c.get("master_changes")
                .and_then(|v| v.as_array())
                .is_some_and(|a| !a.is_empty())
        }) {
            req.resources
                .push(Resource::new(ResourceKind::Instrument, "*"));
            if command
                .get("master_changes")
                .and_then(|v| v.as_array())
                .is_some_and(|items| {
                    items.iter().any(|i| {
                        i.pointer("/definition/kind").and_then(|v| v.as_str())
                            == Some("calculation")
                    })
                })
            {
                req.data
                    .extend([DataScope::Market, DataScope::Books, DataScope::Results]);
                req.resources.extend([
                    Resource::new(ResourceKind::Book, "*"),
                    Resource::new(ResourceKind::Curve, "*"),
                    Resource::new(ResourceKind::Source, "*"),
                ]);
            }
        }
        if scopes.iter().any(|s| s.kind == ResourceKind::Book)
            && !req.data.contains(&DataScope::Books)
        {
            req.data.push(DataScope::Books);
        }
        req.resources.extend(scopes);
        access.authorize(&ctx, req.action, &req.data, &req.resources)?;
        *request.body_mut() = Body::from(bytes);
    }
    Ok(Some((ctx, req, admission)))
}
/// Reject duplicate JSON members at every depth rather than letting different parsers disagree.
pub(crate) fn strict_json(bytes: &[u8]) -> Result<serde_json::Value, AccessError> {
    use serde::Deserialize;
    struct Unique(serde_json::Value);
    impl<'de> Deserialize<'de> for Unique {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Unique;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("bounded unambiguous JSON")
                }
                fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Unique, E> {
                    Ok(Unique(v.into()))
                }
                fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Unique, E> {
                    Ok(Unique(v.into()))
                }
                fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Unique, E> {
                    Ok(Unique(v.into()))
                }
                fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Unique, E> {
                    serde_json::Number::from_f64(v)
                        .map(|n| Unique(n.into()))
                        .ok_or_else(|| E::custom("nonfinite JSON"))
                }
                fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Unique, E> {
                    Ok(Unique(v.into()))
                }
                fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Unique, E> {
                    Ok(Unique(v.into()))
                }
                fn visit_unit<E: serde::de::Error>(self) -> Result<Unique, E> {
                    Ok(Unique(serde_json::Value::Null))
                }
                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut seq: A,
                ) -> Result<Unique, A::Error> {
                    let mut values = Vec::new();
                    while let Some(Unique(value)) = seq.next_element()? {
                        if values.len() >= 65536 {
                            return Err(serde::de::Error::custom("JSON capacity"));
                        }
                        values.push(value);
                    }
                    Ok(Unique(values.into()))
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut map: A,
                ) -> Result<Unique, A::Error> {
                    let mut values = serde_json::Map::new();
                    while let Some(key) = map.next_key::<String>()? {
                        if values.len() >= 65536 || values.contains_key(&key) {
                            return Err(serde::de::Error::custom("ambiguous JSON"));
                        }
                        let Unique(value) = map.next_value()?;
                        values.insert(key, value);
                    }
                    Ok(Unique(values.into()))
                }
            }
            deserializer.deserialize_any(Visitor)
        }
    }
    serde_json::from_slice::<Unique>(bytes)
        .map(|v| v.0)
        .map_err(|_| AccessError::InvalidRequest)
}
