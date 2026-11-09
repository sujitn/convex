//! Book limits over the real router (P14b): limit sets with compare-and-set and idempotent
//! retries, two subjects for every change under required access (D85), the bound on
//! limited books (D87), the generic reference routes' refusal, and status on request at
//! and just past each bound and band, with each reason the server reaches without results.
//! The pre-trade check (P14d): D89's row rule, each refusal, and D91's access.
#[allow(dead_code)] // This suite uses the issuer and request subset.
#[path = "support/access_fixture.rs"]
mod fixture;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use convex_engine::PricingEngineBuilder;
use convex_server::access::*;
use fixture::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use tower::ServiceExt;

const BOND: &str = "BOND-001";

/// One request; returns status, ETag (if any) and the JSON body (Null if empty).
async fn call(
    router: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    key: Option<&str>,
    if_match: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Option<String>, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    if let Some(if_match) = if_match {
        request = request.header("if-match", if_match);
    }
    let body = body.map_or_else(Body::empty, |v| Body::from(serde_json::to_vec(&v).unwrap()));
    let response = router
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let etag = response
        .headers()
        .get("etag")
        .map(|v| v.to_str().unwrap().to_owned());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        etag,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A router over the runtime fixture's reference repository (`BOND-001`), with no runtime.
async fn router(access: Arc<AccessControl>) -> (support::Setup, Router) {
    let setup = support::setup().await;
    let engine = PricingEngineBuilder::new()
        .with_storage(setup.storage.clone())
        .build()
        .unwrap();
    let router = convex_server::routes::create_router_with_access(Arc::new(engine), false, access);
    (setup, router)
}

async fn put_book(router: &Router, token: Option<&str>, id: &str, body: Value) {
    let (status, _, response) = call(
        router,
        "PUT",
        &format!("/api/v1/books/{id}"),
        token,
        Some(&format!("book-{id}")),
        None,
        Some(body),
    )
    .await;
    assert!(status.is_success(), "{id}: {status} {response}");
}

fn position(quantity: &str) -> Value {
    json!({"instrument_id": BOND, "quantity": quantity})
}

/// A face limit on the fixture bond.
fn face(id: &str, lower: Option<&str>, upper: Option<&str>, fraction: Option<&str>) -> Value {
    let mut limit = json!({
        "limit_id": id,
        "measure": {"kind": "face", "instrument": {"one": BOND}},
    });
    for (field, value) in [
        ("lower", lower),
        ("upper", upper),
        ("warning_fraction", fraction),
    ] {
        if let Some(value) = value {
            limit[field] = json!(value);
        }
    }
    limit
}

fn limit(id: &str, measure: Value, upper: &str) -> Value {
    json!({"limit_id": id, "measure": measure, "upper": upper})
}

fn set(limits: Vec<Value>) -> Value {
    json!({ "limits": limits })
}

/// Each limit's evaluation in a status response, by id.
fn by_id(status: &Value) -> BTreeMap<String, Value> {
    status["limits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| (item["limit_id"].as_str().unwrap().to_owned(), item.clone()))
        .collect()
}

async fn status(router: &Router, token: Option<&str>, book: &str) -> Value {
    let (code, _, body) = call(
        router,
        "GET",
        &format!("/api/v1/books/{book}/limits/status"),
        token,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    body
}

/// Commit a limit set in one step (local development).
async fn commit_limits(router: &Router, book: &str, key: &str, body: Value) -> Value {
    let (code, _, response) = call(
        router,
        "PUT",
        &format!("/api/v1/books/{book}/limits"),
        None,
        Some(key),
        None,
        Some(body),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{response}");
    response
}

#[tokio::test]
async fn status_is_within_at_each_bound_and_band_edge_and_not_just_past_it() {
    let (_setup, router) = router(AccessControl::local_development()).await;
    put_book(
        &router,
        None,
        "LONG",
        json!({"currency": "USD", "positions": [position("1000000")]}),
    )
    .await;
    put_book(
        &router,
        None,
        "SHORT",
        json!({"currency": "USD", "positions": [position("-1000000")]}),
    )
    .await;
    commit_limits(
        &router,
        "LONG",
        "limits-long",
        set(vec![
            face("upper-at", None, Some("1000000"), None),
            face("upper-past", None, Some("999999.99"), None),
            face("lower-at", Some("1000000"), None, None),
            face("lower-past", Some("1000000.01"), None, None),
            face("band-edge", None, Some("2000000"), Some("0.5")),
            face("band-past", None, Some("2000000"), Some("0.4999999")),
            face("band-at-bound", None, Some("1000000"), Some("0.5")),
            json!({"limit_id": "each", "measure": {"kind": "face", "instrument": "each"}, "upper": "1000000"}),
        ]),
    )
    .await;
    commit_limits(
        &router,
        "SHORT",
        "limits-short",
        set(vec![
            face("lower-at", Some("-1000000"), None, None),
            face("lower-past", Some("-999999.99"), None, None),
            face("band-edge", Some("-2000000"), None, Some("0.5")),
            face("band-past", Some("-2000000"), None, Some("0.4999999")),
            face("upper-past", None, Some("-1000000.01"), None),
        ]),
    )
    .await;

    let long = status(&router, None, "LONG").await;
    assert_eq!(long["book_version"], 1, "{long}");
    assert_eq!(long["limit_set_version"], 1, "{long}");
    assert_eq!(
        long["snapshot_ids"],
        json!([]),
        "a face limit reads no result: {long}"
    );
    let long = by_id(&long);
    for (id, expected) in [
        ("upper-at", "within"),
        ("upper-past", "breach"),
        ("lower-at", "within"),
        ("lower-past", "breach"),
        ("band-edge", "within"),
        ("band-past", "warning"),
        ("band-at-bound", "warning"),
        ("each", "within"),
    ] {
        assert_eq!(long[id]["status"], expected, "{id}: {}", long[id]);
        assert_eq!(long[id]["value"], "1000000", "{id}: {}", long[id]);
    }
    assert_eq!(
        long["band-edge"]["utilisation"], "0.5",
        "{}",
        long["band-edge"]
    );
    assert_eq!(long["band-edge"]["upper"], "2000000");
    assert_eq!(long["band-edge"]["warning_fraction"], "0.5");
    assert_eq!(long["each"]["member"], BOND, "{}", long["each"]);

    let short = by_id(&status(&router, None, "SHORT").await);
    for (id, expected) in [
        ("lower-at", "within"),
        ("lower-past", "breach"),
        ("band-edge", "within"),
        ("band-past", "warning"),
        ("upper-past", "breach"),
    ] {
        assert_eq!(short[id]["status"], expected, "{id}: {}", short[id]);
        assert_eq!(short[id]["value"], "-1000000", "{id}: {}", short[id]);
    }
}

#[tokio::test]
async fn each_missing_input_is_unknown_with_its_reason_and_face_still_evaluates() {
    let (_setup, router) = router(AccessControl::local_development()).await;
    let market_value = json!({"kind": "market_value"});
    put_book(
        &router,
        None,
        "NO-RESULTS",
        json!({"currency": "USD", "positions": [position("1000000")]}),
    )
    .await;
    put_book(
        &router,
        None,
        "NO-CURRENCY",
        json!({"positions": [position("1000000")]}),
    )
    .await;
    put_book(
        &router,
        None,
        "UNITS",
        json!({"currency": "USD", "positions": [
            {"instrument_id": BOND, "quantity": "10", "unit": "units"}
        ]}),
    )
    .await;
    put_book(
        &router,
        None,
        "EMPTY",
        json!({"currency": "USD", "positions": []}),
    )
    .await;
    for book in ["NO-RESULTS", "NO-CURRENCY", "UNITS"] {
        commit_limits(
            &router,
            book,
            &format!("limits-{book}"),
            set(vec![
                limit("value", market_value.clone(), "1"),
                face("face", None, Some("2000000"), None),
            ]),
        )
        .await;
    }
    commit_limits(
        &router,
        "EMPTY",
        "limits-EMPTY",
        set(vec![
            limit("value", market_value.clone(), "1"),
            limit(
                "share",
                json!({"kind": "group_share", "by": "issuer", "group": {"one": "ISSUER-001"}}),
                "0.5",
            ),
            limit("duration", json!({"kind": "modified_duration"}), "10"),
        ]),
    )
    .await;

    // No runtime here: no position has a result. A face limit needs none.
    let missing = by_id(&status(&router, None, "NO-RESULTS").await);
    assert_eq!(missing["value"]["status"], "unknown");
    assert_eq!(
        missing["value"]["reason"],
        json!({"code": "missing_result", "instruments": {"count": 1, "first": [BOND]}})
    );
    assert!(
        missing["value"].get("value").is_none(),
        "{}",
        missing["value"]
    );
    assert_eq!(missing["face"]["status"], "within", "{}", missing["face"]);

    // Book-level reasons come first, in D84's order.
    let currency = by_id(&status(&router, None, "NO-CURRENCY").await);
    assert_eq!(
        currency["value"]["reason"],
        json!({"code": "book_currency_required"})
    );
    assert_eq!(currency["face"]["status"], "within");
    let units = by_id(&status(&router, None, "UNITS").await);
    let in_units =
        json!({"code": "unsupported_quantity_unit", "instruments": {"count": 1, "first": [BOND]}});
    assert_eq!(units["value"]["reason"], in_units);
    assert_eq!(
        units["face"]["reason"], in_units,
        "a face limit takes face: {}",
        units["face"]
    );

    // A book with nothing in it has zero net and gross value.
    let empty = by_id(&status(&router, None, "EMPTY").await);
    assert_eq!(empty["value"]["status"], "within", "{}", empty["value"]);
    assert_eq!(empty["value"]["value"], "0");
    assert_eq!(
        empty["share"]["reason"],
        json!({"code": "zero_gross_value"})
    );
    assert_eq!(
        empty["duration"]["reason"],
        json!({"code": "zero_net_value"})
    );
}

#[tokio::test]
async fn a_limit_set_is_compare_and_set_and_a_retry_never_applies_twice() {
    let (_setup, router) = router(AccessControl::local_development()).await;
    let path = "/api/v1/books/BOOK-A/limits";
    let first = set(vec![face("face", None, Some("2000000"), None)]);
    // No book, no limits.
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        None,
        Some("k0"),
        None,
        Some(first.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND, "{body}");
    put_book(
        &router,
        None,
        "BOOK-A",
        json!({"currency": "USD", "positions": [position("1000000")]}),
    )
    .await;
    let (code, _, body) = call(&router, "GET", path, None, None, None, None).await;
    assert_eq!(code, StatusCode::NOT_FOUND, "{body}");

    // Local development commits in one step, recorded as such.
    let (code, etag, created) = call(
        &router,
        "PUT",
        path,
        None,
        Some("k1"),
        None,
        Some(first.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{created}");
    assert_eq!(etag.as_deref(), Some("\"1\""));
    assert_eq!(created["proposed_by"], "local-development");
    assert_eq!(created["approved_by"], "local-development");
    assert!(created.get("proposal_version").is_none(), "{created}");
    assert_eq!(created["evidence_reference"], "limits-api:k1");
    let (code, _, body) = call(
        &router,
        "GET",
        &format!("{path}/proposal"),
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(
        code,
        StatusCode::NOT_FOUND,
        "no proposal in one step: {body}"
    );

    // An exact retry returns the same version; the same key with another body conflicts.
    let (code, _, retried) = call(
        &router,
        "PUT",
        path,
        None,
        Some("k1"),
        None,
        Some(first.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{retried}");
    assert_eq!(retried, created);
    let second = set(vec![face("face", None, Some("3000000"), None)]);
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        None,
        Some("k1"),
        None,
        Some(second.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");

    // A set exists: If-Match must name it.
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        None,
        Some("k2"),
        None,
        Some(second.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        None,
        Some("k2"),
        Some("2"),
        Some(second.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    let (code, etag, replaced) = call(
        &router,
        "PUT",
        path,
        None,
        Some("k2"),
        Some("\"1\""),
        Some(second.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{replaced}");
    assert_eq!(etag.as_deref(), Some("\"2\""));
    assert_eq!(replaced["limits"][0]["upper"], "3000000");
    // A retry after the set moved on still returns its own version.
    let (code, _, retried) = call(&router, "PUT", path, None, Some("k1"), None, Some(first)).await;
    assert_eq!(code, StatusCode::CREATED, "{retried}");
    assert_eq!(retried["version"], 1);
    // The same key with another If-Match is another request.
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        None,
        Some("k2"),
        Some("\"2\""),
        Some(second.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    // The generic commit route does not return a limit commit.
    let (code, _, body) = call(
        &router,
        "GET",
        "/api/v1/reference/commits/k1",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "LIMITS_ROUTE_REQUIRED", "{body}");

    let (code, _, old) = call(
        &router,
        "GET",
        &format!("{path}/versions/1"),
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{old}");
    assert_eq!(old["limits"][0]["upper"], "2000000");
    let (code, _, history) = call(
        &router,
        "GET",
        &format!("{path}/history?limit=1"),
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{history}");
    assert_eq!(history["current_version"], 2);
    assert_eq!(history["next_from"], 2);
    assert_eq!(history["versions"][0]["limit_count"], 1);
    let (code, etag, current) = call(&router, "GET", path, None, None, None, None).await;
    assert_eq!((code, etag.as_deref()), (StatusCode::OK, Some("\"2\"")));
    assert_eq!(current, replaced);
}

#[tokio::test]
async fn invalid_sets_and_generic_reference_writes_of_limits_are_refused() {
    let (_setup, router) = router(AccessControl::local_development()).await;
    put_book(
        &router,
        None,
        "BOOK-A",
        json!({"currency": "USD", "positions": [position("1000000")]}),
    )
    .await;
    let path = "/api/v1/books/BOOK-A/limits";
    let many: Vec<Value> = (0..65)
        .map(|i| face(&format!("l{i}"), None, Some("1"), None))
        .collect();
    let mut unknown_field = face("a", None, Some("1"), None);
    unknown_field["note"] = json!("x");
    let refused = [
        set(many),
        set(vec![
            face("a", None, Some("1"), None),
            face("a", None, Some("2"), None),
        ]),
        set(vec![face("a", Some("2"), Some("1"), None)]),
        set(vec![face("a", None, None, None)]),
        set(vec![face("a", None, Some("-1"), Some("0.5"))]),
        set(vec![face("a", None, Some("1"), Some("1"))]),
        set(vec![unknown_field]),
        set(vec![limit("a", json!({"kind": "var"}), "1")]),
        set(vec![limit(
            "a",
            json!({"kind": "key_rate_dv01", "tenor": "spot"}),
            "1",
        )]),
        set(vec![limit(
            "a",
            json!({"kind": "group_share", "by": "issuer", "group": {"one": " X"}}),
            "1",
        )]),
        json!({"limits": [], "extra": 1}),
    ];
    for (index, body) in refused.into_iter().enumerate() {
        let key = format!("bad-{index}");
        let (code, _, response) =
            call(&router, "PUT", path, None, Some(&key), None, Some(body)).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{index}: {response}");
    }
    let (code, _, response) = call(&router, "GET", path, None, None, None, None).await;
    assert_eq!(code, StatusCode::NOT_FOUND, "nothing committed: {response}");

    // The generic reference routes cannot write a limit set or proposal, and do not list them.
    let definition = json!({
        "book_id": "BOOK-A",
        "limits": [],
        "proposed_by": "access:x",
        "approved_by": "access:x",
        "effective": {"from": "2026-01-01T00:00:00Z", "until": null},
        "provenance": {"source": "elsewhere", "evidence_reference": "elsewhere"},
    });
    for kind in ["limit_set", "limit_proposal"] {
        let mut definition = definition.clone();
        if kind == "limit_proposal" {
            let object = definition.as_object_mut().unwrap();
            object.remove("approved_by");
        }
        let command = json!({
            "idempotency_key": format!("generic-{kind}"),
            "changes": [],
            "master_changes": [{"definition": {"kind": kind, "definition": definition}}],
        });
        let (code, _, response) = call(
            &router,
            "POST",
            "/api/v1/reference/transactions",
            None,
            Some(&format!("generic-{kind}")),
            None,
            Some(command),
        )
        .await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{kind}: {response}");
        assert_eq!(response["code"], "LIMITS_ROUTE_REQUIRED", "{response}");
        let (code, _, response) = call(
            &router,
            "GET",
            &format!("/api/v1/reference/masters/{kind}"),
            None,
            None,
            None,
            None,
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{kind}: {response}");
    }
}

#[tokio::test]
async fn at_most_64_books_hold_a_non_empty_limit_set() {
    let (_setup, router) = router(AccessControl::local_development()).await;
    let limited = set(vec![face("face", None, Some("2000000"), None)]);
    for index in 0..64 {
        let book = format!("BOOK-{index:02}");
        put_book(
            &router,
            None,
            &book,
            json!({"currency": "USD", "positions": [position("1")]}),
        )
        .await;
        commit_limits(&router, &book, &format!("limits-{index}"), limited.clone()).await;
    }
    put_book(
        &router,
        None,
        "BOOK-64",
        json!({"currency": "USD", "positions": [position("1")]}),
    )
    .await;
    let path = "/api/v1/books/BOOK-64/limits";
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        None,
        Some("limits-64"),
        None,
        Some(limited.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "LIMITED_BOOKS_EXCEEDED", "{body}");
    // An empty set limits nothing, so it takes no place; clearing a set frees one.
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        None,
        Some("empty-64"),
        None,
        Some(set(vec![])),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
    let (code, _, body) = call(
        &router,
        "PUT",
        "/api/v1/books/BOOK-00/limits",
        None,
        Some("clear-00"),
        Some("1"),
        Some(set(vec![])),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        None,
        Some("limits-64b"),
        Some("1"),
        Some(limited),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
}

// =============================================================================
// REQUIRED ACCESS: TWO SUBJECTS (D85)
// =============================================================================

const RISK_A: &str = "risk-a-token";
const RISK_B: &str = "risk-b-token";
const OUTSIDER: &str = "outsider-token";

fn grant(subject: &str, roles: Vec<Role>, instrument: &str) -> Grant {
    Grant {
        subject: subject.into(),
        kind: IdentityKind::Human,
        roles,
        data: vec![
            DataScope::Reference,
            DataScope::Market,
            DataScope::Results,
            DataScope::Books,
        ],
        resources: BTreeMap::from([
            (ResourceKind::Instrument, vec![instrument.into()]),
            (ResourceKind::Book, vec!["BOOK-A".into()]),
            (ResourceKind::Source, vec!["*".into()]),
            (ResourceKind::Curve, vec!["*".into()]),
        ]),
        token_scopes: vec!["convex.api".into()],
        valid_until: now() + 3600,
        quotas: Quotas::default(),
    }
}

async fn two_subjects() -> (IssuerFixture, support::Setup, Router) {
    let issuer = IssuerFixture::start().await;
    for (token, subject) in [
        (RISK_A, "risk-a"),
        (RISK_B, "risk-b"),
        (OUTSIDER, "outsider"),
    ] {
        issuer.change(token, claims(subject));
    }
    let mut document = policy();
    document.grants = vec![
        grant(
            "alice",
            vec![Role::Reader, Role::ReferenceWriter, Role::BookWriter],
            "*",
        ),
        grant("risk-a", vec![Role::Reader, Role::RiskManager], BOND),
        grant("risk-b", vec![Role::Reader, Role::RiskManager], BOND),
        grant(
            "outsider",
            vec![Role::Reader, Role::RiskManager],
            "BOND-999",
        ),
    ];
    let access = AccessControl::new(issuer.verifier.clone(), document).unwrap();
    let (setup, router) = router(access).await;
    put_book(
        &router,
        Some(HUMAN_TOKEN),
        "BOOK-A",
        json!({"currency": "USD", "positions": [position("1000000")]}),
    )
    .await;
    (issuer, setup, router)
}

#[tokio::test]
async fn a_limit_change_needs_a_risk_manager_and_a_second_subject() {
    let (_issuer, _setup, router) = two_subjects().await;
    let path = "/api/v1/books/BOOK-A/limits";
    let approve = "/api/v1/books/BOOK-A/limits/proposal/approve";
    let first = set(vec![face("face", None, Some("2000000"), None)]);

    // A book writer may not write limits.
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        Some(HUMAN_TOKEN),
        Some("p0"),
        None,
        Some(first.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");

    // A risk manager proposes; nothing is committed yet.
    let (code, etag, proposal) = call(
        &router,
        "PUT",
        path,
        Some(RISK_A),
        Some("p1"),
        None,
        Some(first.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{proposal}");
    assert_eq!(etag.as_deref(), Some("\"1\""));
    assert_eq!(proposal["open"], true);
    let proposer = proposal["proposed_by"].as_str().unwrap().to_owned();
    assert!(proposer.starts_with("access:"), "{proposal}");
    let (code, _, body) = call(&router, "GET", path, Some(RISK_A), None, None, None).await;
    assert_eq!(code, StatusCode::NOT_FOUND, "{body}");

    // The proposer cannot approve its own proposal; nor can anyone approve a stale version.
    let (code, _, body) = call(
        &router,
        "POST",
        approve,
        Some(RISK_A),
        Some("a1"),
        Some("1"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "LIMIT_SELF_APPROVAL", "{body}");
    let (code, _, body) = call(
        &router,
        "POST",
        approve,
        Some(RISK_B),
        Some("a2"),
        Some("2"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    let (code, _, body) = call(
        &router,
        "POST",
        approve,
        Some(HUMAN_TOKEN),
        Some("a3"),
        Some("1"),
        None,
    )
    .await;
    assert_eq!(
        code,
        StatusCode::FORBIDDEN,
        "a book writer cannot approve: {body}"
    );

    // A second risk manager approves: the set and the closed proposal commit together.
    let (code, etag, approved) = call(
        &router,
        "POST",
        approve,
        Some(RISK_B),
        Some("a4"),
        Some("1"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{approved}");
    assert_eq!(etag.as_deref(), Some("\"1\""));
    assert_eq!(approved["proposed_by"], proposer.as_str());
    let approver = approved["approved_by"].as_str().unwrap().to_owned();
    assert!(
        approver.starts_with("access:") && approver != proposer,
        "{approved}"
    );
    assert_eq!(approved["proposal_version"], 1);
    assert_eq!(approved["limits"], first["limits"]);
    let (code, _, retried) = call(
        &router,
        "POST",
        approve,
        Some(RISK_B),
        Some("a4"),
        Some("1"),
        None,
    )
    .await;
    assert_eq!((code, &retried), (StatusCode::CREATED, &approved));
    let (code, _, body) = call(
        &router,
        "POST",
        approve,
        Some(RISK_B),
        Some("a5"),
        Some("1"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "the proposal moved on: {body}");
    assert_eq!(body["code"], "VERSION_CONFLICT", "{body}");
    let (code, _, body) = call(
        &router,
        "POST",
        approve,
        Some(RISK_B),
        Some("a5"),
        Some("2"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "LIMIT_PROPOSAL_CLOSED", "{body}");
    let (code, etag, closed) = call(
        &router,
        "GET",
        &format!("{path}/proposal"),
        Some(RISK_A),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(
        (code, etag.as_deref()),
        (StatusCode::OK, Some("\"2\"")),
        "{closed}"
    );
    assert_eq!(closed["open"], false);
    assert_eq!(
        closed["closed"],
        json!({"outcome": "approved", "by": approver, "limit_set_version": 1})
    );

    // The next proposal names the set version it replaces; a retry is idempotent.
    let second = set(vec![face("face", None, Some("3000000"), None)]);
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        Some(RISK_A),
        Some("p2"),
        None,
        Some(second.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    let (code, _, next) = call(
        &router,
        "PUT",
        path,
        Some(RISK_A),
        Some("p3"),
        Some("1"),
        Some(second.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{next}");
    assert_eq!(
        (next["version"].clone(), next["base_version"].clone()),
        (json!(3), json!(1))
    );
    let (code, _, retried) = call(
        &router,
        "PUT",
        path,
        Some(RISK_A),
        Some("p3"),
        Some("1"),
        Some(second),
    )
    .await;
    assert_eq!((code, &retried), (StatusCode::ACCEPTED, &next));
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        Some(RISK_A),
        Some("p3"),
        Some("1"),
        Some(first),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");

    // Another subject may withdraw it, which commits nothing.
    let withdraw = "/api/v1/books/BOOK-A/limits/proposal/withdraw";
    let (code, _, withdrawn) = call(
        &router,
        "POST",
        withdraw,
        Some(RISK_B),
        Some("w1"),
        Some("3"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{withdrawn}");
    assert_eq!(withdrawn["closed"]["outcome"], "withdrawn");
    assert_eq!(withdrawn["closed"]["by"], approver.as_str());
    let (code, _, body) = call(
        &router,
        "POST",
        approve,
        Some(RISK_B),
        Some("a6"),
        Some("4"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    let (code, etag, current) = call(&router, "GET", path, Some(RISK_A), None, None, None).await;
    assert_eq!(
        (code, etag.as_deref()),
        (StatusCode::OK, Some("\"1\"")),
        "{current}"
    );
    assert_eq!(current["limits"][0]["upper"], "2000000");
}

#[tokio::test]
async fn reading_limits_needs_every_instrument_the_book_holds() {
    let (_issuer, _setup, router) = two_subjects().await;
    let path = "/api/v1/books/BOOK-A/limits";
    let body = set(vec![face("face", None, Some("2000000"), None)]);
    let (code, _, _) = call(
        &router,
        "PUT",
        path,
        Some(RISK_A),
        Some("p1"),
        None,
        Some(body),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED);
    let (code, _, _) = call(
        &router,
        "POST",
        "/api/v1/books/BOOK-A/limits/proposal/approve",
        Some(RISK_B),
        Some("a1"),
        Some("1"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::CREATED);
    for suffix in ["", "/versions/1", "/proposal", "/status"] {
        let path = format!("{path}{suffix}");
        let (code, _, body) =
            call(&router, "GET", &path, Some(SERVICE_TOKEN), None, None, None).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{path}: no grant: {body}");
        let (code, _, body) = call(&router, "GET", &path, Some(OUTSIDER), None, None, None).await;
        assert_eq!(
            code,
            StatusCode::FORBIDDEN,
            "{path}: not entitled to {BOND}: {body}"
        );
        let (code, _, body) = call(&router, "GET", &path, Some(RISK_A), None, None, None).await;
        assert_eq!(code, StatusCode::OK, "{path}: {body}");
    }
    // History summaries name no instrument, so they need the book only.
    let history = format!("{path}/history");
    let (code, _, body) = call(
        &router,
        "GET",
        &history,
        Some(SERVICE_TOKEN),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "no grant: {body}");
    let (code, _, body) = call(&router, "GET", &history, Some(OUTSIDER), None, None, None).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let mut keys: Vec<&str> = body["versions"][0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "approved_by",
            "limit_count",
            "proposal_version",
            "proposed_by",
            "recorded_at",
            "version"
        ],
        "{body}"
    );
    let status = status(&router, Some(RISK_A), "BOOK-A").await;
    assert_eq!(by_id(&status)["face"]["status"], "within", "{status}");

    // Nor may an entitled outsider write a limit naming an instrument it cannot see.
    let other = set(vec![face("face", None, Some("1"), None)]);
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        Some(OUTSIDER),
        Some("o1"),
        Some("1"),
        Some(other),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn replacing_an_open_proposal_needs_each_instrument_it_names() {
    let (_issuer, _setup, router) = two_subjects().await;
    let path = "/api/v1/books/BOOK-A/limits";
    let (code, _, proposal) = call(
        &router,
        "PUT",
        path,
        Some(RISK_A),
        Some("p1"),
        None,
        Some(set(vec![face("face", None, Some("2000000"), None)])),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{proposal}");

    // The outsider may write limits on its own instrument, but replacing the open proposal
    // would withdraw a limit on one it cannot see.
    let mut other = face("other", None, Some("1"), None);
    other["measure"]["instrument"] = json!({"one": "BOND-999"});
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        Some(OUTSIDER),
        Some("o1"),
        None,
        Some(set(vec![other.clone()])),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
    let (code, etag, open) = call(
        &router,
        "GET",
        &format!("{path}/proposal"),
        Some(RISK_A),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(
        (code, etag.as_deref()),
        (StatusCode::OK, Some("\"1\"")),
        "{open}"
    );
    assert_eq!((&open, &open["open"]), (&proposal, &json!(true)));

    // A subject holding the open proposal's instruments replaces it.
    let (code, _, next) = call(
        &router,
        "PUT",
        path,
        Some(RISK_B),
        Some("b1"),
        None,
        Some(set(vec![face("face", None, Some("3000000"), None)])),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{next}");
    assert_eq!(next["version"], 2, "{next}");

    // Once the proposal is closed, nothing open is replaced and its instruments are not needed.
    let (code, _, body) = call(
        &router,
        "POST",
        "/api/v1/books/BOOK-A/limits/proposal/withdraw",
        Some(RISK_A),
        Some("w1"),
        Some("2"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        Some(OUTSIDER),
        Some("o2"),
        None,
        Some(set(vec![other])),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{body}");
}

#[tokio::test]
async fn dropping_a_limit_needs_write_rights_on_its_instrument() {
    let (_issuer, _setup, router) = two_subjects().await;
    let path = "/api/v1/books/BOOK-A/limits";
    let approve = "/api/v1/books/BOOK-A/limits/proposal/approve";
    let (code, _, body) = call(
        &router,
        "PUT",
        path,
        Some(RISK_A),
        Some("p1"),
        None,
        Some(set(vec![face("face", None, Some("2000000"), None)])),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{body}");
    let (code, _, body) = call(
        &router,
        "POST",
        approve,
        Some(RISK_B),
        Some("a1"),
        Some("1"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");

    // The outsider cannot propose a set that drops the limit on BOND-001, whether it
    // clears the set or replaces the limit with one on its own instrument.
    let mut other = face("other", None, Some("1"), None);
    other["measure"]["instrument"] = json!({"one": "BOND-999"});
    for (key, limits) in [("o1", vec![]), ("o2", vec![other])] {
        let (code, _, body) = call(
            &router,
            "PUT",
            path,
            Some(OUTSIDER),
            Some(key),
            Some("1"),
            Some(set(limits)),
        )
        .await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{key}: {body}");
    }

    // Nor can it approve another subject's proposal to drop it.
    let (code, _, proposal) = call(
        &router,
        "PUT",
        path,
        Some(RISK_A),
        Some("p2"),
        Some("1"),
        Some(set(vec![])),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{proposal}");
    let version = proposal["version"].to_string();
    let (code, _, body) = call(
        &router,
        "POST",
        approve,
        Some(OUTSIDER),
        Some("a2"),
        Some(&version),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
    let (code, etag, current) = call(&router, "GET", path, Some(RISK_A), None, None, None).await;
    assert_eq!(
        (code, etag.as_deref()),
        (StatusCode::OK, Some("\"1\"")),
        "{current}"
    );
    assert_eq!(current["limits"][0]["limit_id"], "face");

    // A subject holding BOND-001 approves it.
    let (code, _, cleared) = call(
        &router,
        "POST",
        approve,
        Some(RISK_B),
        Some("a3"),
        Some(&version),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["limits"], json!([]));
}

#[tokio::test]
async fn a_reference_writer_cannot_write_limits_through_the_generic_routes() {
    let (_issuer, _setup, router) = two_subjects().await;
    let command = json!({
        "idempotency_key": "generic-limit-set",
        "changes": [],
        "master_changes": [{"definition": {"kind": "limit_set", "definition": {
            "book_id": "BOOK-A",
            "limits": [],
            "proposed_by": "access:x",
            "approved_by": "access:x",
            "effective": {"from": "2026-01-01T00:00:00Z", "until": null},
            "provenance": {"source": "elsewhere", "evidence_reference": "elsewhere"},
        }}}],
    });
    let (code, _, body) = call(
        &router,
        "POST",
        "/api/v1/reference/transactions",
        Some(HUMAN_TOKEN),
        Some("generic-limit-set"),
        None,
        Some(command),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "LIMITS_ROUTE_REQUIRED", "{body}");
}

// =============================================================================
// THE MONITOR UNDER REQUIRED ACCESS (P14c)
// =============================================================================

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn next_frame(socket: &mut Socket) -> Value {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    if value["type"] != "heartbeat" {
                        return value;
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended: {other:?}"),
            }
        }
    })
    .await
    .expect("a frame")
}

async fn socket(address: std::net::SocketAddr, token: &str) -> Socket {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = format!("ws://{address}/ws").into_client_request().unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(next_frame(&mut socket).await["type"], "connected");
    socket
}

async fn subscribe_limits(socket: &mut Socket, books: &[&str]) -> Value {
    use futures_util::SinkExt;
    let message = json!({"type": "subscribe_limits", "book_ids": books});
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            message.to_string().into(),
        ))
        .await
        .unwrap();
    next_frame(socket).await
}

/// A `limit_status` frame reaches a subscriber only with what the status route would show
/// it: `BookRead` on the book and on each instrument it holds or a limit names. A book
/// outside the caller's grant cannot be subscribed to, and a change is recorded before it
/// is published.
#[tokio::test]
async fn a_limit_status_frame_needs_what_the_status_route_needs() {
    use convex_server::access::identity::{AccessMode, AccessSettings, TlsBoundary};
    use convex_server::{Server, ServerConfig, ServerProfile};
    let issuer = IssuerFixture::start().await;
    for (token, subject) in [
        (RISK_A, "risk-a"),
        (RISK_B, "risk-b"),
        (OUTSIDER, "outsider"),
    ] {
        issuer.change(token, claims(subject));
    }
    let mut document = policy();
    document.grants = vec![
        grant(
            "alice",
            vec![Role::Reader, Role::ReferenceWriter, Role::BookWriter],
            "*",
        ),
        grant("risk-a", vec![Role::Reader, Role::RiskManager], BOND),
        grant("risk-b", vec![Role::Reader, Role::RiskManager], BOND),
        grant(
            "outsider",
            vec![Role::Reader, Role::RiskManager],
            "BOND-999",
        ),
    ];
    // The server rereads its policy file each second; it holds the same document.
    let policy_file = issuer.directory.path().join("policy.json");
    std::fs::write(&policy_file, serde_json::to_vec(&document).unwrap()).unwrap();
    let access = AccessControl::new(issuer.verifier.clone(), document).unwrap();
    let setup = support::setup().await;
    let audit = setup.storage.audit.clone();
    let engine = Arc::new(
        PricingEngineBuilder::new()
            .with_storage(setup.storage.clone())
            .build()
            .unwrap(),
    );
    let config = ServerConfig {
        profile: ServerProfile::Demo,
        host: "127.0.0.1".into(),
        access: AccessSettings {
            mode: AccessMode::Required,
            policy_file: Some(policy_file.to_str().unwrap().into()),
            introspection: Some(issuer.settings.clone()),
            cors_origins: vec![],
            tls_boundary: TlsBoundary::LocalOnly,
        },
        ..Default::default()
    };
    let server =
        Server::with_access_control(config, engine, access).with_limit_monitor(audit.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let serving = tokio::spawn(async move {
        server
            .serve_until(listener, async {
                let _ = stopped.await;
            })
            .await
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let base = format!("http://{address}");
    let send = |method: reqwest::Method,
                path: &str,
                token: &str,
                key: &str,
                version: Option<&str>,
                body: Option<Value>| {
        let mut request = client
            .request(method, format!("{base}{path}"))
            .bearer_auth(token)
            .header("idempotency-key", key);
        if let Some(version) = version {
            request = request.header("if-match", version);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        async move {
            let response = request.send().await.unwrap();
            let status = response.status().as_u16();
            (
                status,
                response.json::<Value>().await.unwrap_or(Value::Null),
            )
        }
    };

    let (code, body) = send(
        reqwest::Method::PUT,
        "/api/v1/books/BOOK-A",
        HUMAN_TOKEN,
        "book-a",
        None,
        Some(json!({"currency": "USD", "positions": [position("1000000")]})),
    )
    .await;
    assert_eq!(code, 201, "{body}");

    // Each may subscribe to a book in its grant, and none to one outside it.
    let mut holder = socket(address, RISK_A).await;
    let mut outsider = socket(address, OUTSIDER).await;
    let refused = subscribe_limits(&mut outsider, &["BOOK-B"]).await;
    assert_eq!(
        (&refused["type"], &refused["code"]),
        (&json!("error"), &json!("FORBIDDEN")),
        "{refused}"
    );
    for session in [&mut holder, &mut outsider] {
        let ack = subscribe_limits(session, &["BOOK-A"]).await;
        assert_eq!(ack["type"], "subscribed", "{ack}");
    }

    // Two subjects commit a breached face limit; the monitor records it, then publishes.
    let (code, proposal) = send(
        reqwest::Method::PUT,
        "/api/v1/books/BOOK-A/limits",
        RISK_A,
        "p1",
        None,
        Some(set(vec![face("face", None, Some("500000"), None)])),
    )
    .await;
    assert_eq!(code, 202, "{proposal}");
    let (code, body) = send(
        reqwest::Method::POST,
        "/api/v1/books/BOOK-A/limits/proposal/approve",
        RISK_B,
        "a1",
        Some(&proposal["version"].to_string()),
        None,
    )
    .await;
    assert_eq!(code, 201, "{body}");

    let change = next_frame(&mut holder).await;
    assert_eq!(change["type"], "limit_status", "{change}");
    assert_eq!(
        (&change["limit_id"], &change["previous"], &change["status"]),
        (&json!("face"), &json!("unevaluated"), &json!("breach")),
        "{change}"
    );
    let recorded = audit
        .query(
            &convex_engine::ports::storage::AuditFilter::default(),
            &convex_engine::ports::storage::Pagination::new(0, 10),
        )
        .await
        .unwrap();
    assert_eq!(recorded.items.len(), 1, "{:?}", recorded.items);
    assert_eq!(change["record_id"], recorded.items[0].id);
    assert_eq!(recorded.items[0].entity_id, "BOOK-A");

    // The outsider holds the book but not BOND-001, which the book holds and the limit
    // names: it is told, and shown nothing of the change.
    let denied = next_frame(&mut outsider).await;
    assert_eq!(
        (&denied["type"], &denied["code"]),
        (&json!("error"), &json!("FORBIDDEN")),
        "{denied}"
    );
    assert!(!denied.to_string().contains("breach"), "{denied}");

    drop((holder, outsider));
    stop.send(()).unwrap();
    serving.await.unwrap().unwrap();
}

// =============================================================================
// THE MONITOR'S AUDIT FAILURES AND SHUTDOWN (P14c)
// =============================================================================

use convex_engine::ports::storage::{AuditEntry, AuditFilter, AuditStore, Page, Pagination};
use convex_engine::ports::TraitError;
use std::sync::atomic::{AtomicUsize, Ordering};

/// An audit store over the fixture's that delays each write, refuses some, or refuses all
/// for capacity, as a test needs. A batch is one write, as it is one transaction in redb;
/// it counts every write attempted.
struct ScriptedAudit {
    inner: Arc<dyn AuditStore>,
    delay: std::time::Duration,
    transient: AtomicUsize,
    full: bool,
    attempts: AtomicUsize,
}

/// How a test scripts the store.
#[derive(Default)]
struct Script {
    delay: std::time::Duration,
    transient: usize,
    full: bool,
}

impl ScriptedAudit {
    fn over(inner: Arc<dyn AuditStore>, script: Script) -> Self {
        Self {
            inner,
            delay: script.delay,
            transient: AtomicUsize::new(script.transient),
            full: script.full,
            attempts: AtomicUsize::new(0),
        }
    }

    fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }

    /// One write attempted: its delay, then the scripted refusal, if any.
    async fn scripted(&self) -> Result<(), TraitError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        if self.full {
            return Err(TraitError::CapacityExceeded);
        }
        if self
            .transient
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(TraitError::DatabaseError("injected".into()));
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl AuditStore for ScriptedAudit {
    async fn append(&self, entry: &AuditEntry) -> Result<(), TraitError> {
        self.scripted().await?;
        self.inner.append(entry).await
    }

    async fn append_all(&self, entries: &[AuditEntry]) -> Result<(), TraitError> {
        self.scripted().await?;
        self.inner.append_all(entries).await
    }

    async fn entry_count(&self) -> Result<u64, TraitError> {
        self.inner.entry_count().await
    }

    async fn query(
        &self,
        filter: &AuditFilter,
        pagination: &Pagination,
    ) -> Result<Page<AuditEntry>, TraitError> {
        self.inner.query(filter, pagination).await
    }

    async fn last_id(&self) -> Result<Option<u64>, TraitError> {
        self.inner.last_id().await
    }

    async fn entity_page(
        &self,
        entity_type: &str,
        entity_id: &str,
        after: u64,
        limit: usize,
    ) -> Result<convex_engine::ports::storage::EntityPage, TraitError> {
        self.inner
            .entity_page(entity_type, entity_id, after, limit)
            .await
    }
}

/// A local-development server over the fixture repository, no runtime, whose monitor
/// records through `audit` into the fixture's audit store; `books` books (`BOOK-00`, ...)
/// each hold the fixture bond and `count` face limits.
struct Monitored {
    audit: Arc<ScriptedAudit>,
    base: String,
    stop: tokio::sync::oneshot::Sender<()>,
    serving: tokio::task::JoinHandle<Result<(), convex_server::StartupError>>,
    setup: support::Setup,
}

async fn monitored(script: Script, books: usize, count: usize) -> Monitored {
    use convex_server::access::identity::{AccessMode, AccessSettings};
    use convex_server::{Server, ServerConfig, ServerProfile};
    let setup = support::setup().await;
    let audit = Arc::new(ScriptedAudit::over(setup.storage.audit.clone(), script));
    let engine = Arc::new(
        PricingEngineBuilder::new()
            .with_storage(setup.storage.clone())
            .build()
            .unwrap(),
    );
    let config = ServerConfig {
        profile: ServerProfile::Demo,
        host: "127.0.0.1".into(),
        access: AccessSettings {
            mode: AccessMode::LocalDevelopment,
            ..Default::default()
        },
        ..Default::default()
    };
    let server = Server::with_access_control(config, engine, AccessControl::local_development())
        .with_limit_monitor(audit.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let serving = tokio::spawn(async move {
        server
            .serve_until(listener, async {
                let _ = stopped.await;
            })
            .await
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let writes = (0..books).flat_map(|b| {
        [
            (
                format!("/api/v1/books/BOOK-{b:02}"),
                format!("book-{b:02}"),
                json!({"currency": "USD", "positions": [position("1000000")]}),
            ),
            (
                format!("/api/v1/books/BOOK-{b:02}/limits"),
                format!("limits-{b:02}"),
                set((0..count)
                    .map(|i| face(&format!("face-{i:02}"), None, Some("2000000"), None))
                    .collect()),
            ),
        ]
    });
    for (path, key, body) in writes {
        let response = client
            .put(format!("{base}{path}"))
            .header("idempotency-key", key)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "{path}: {}",
            response.status()
        );
    }
    Monitored {
        audit,
        base,
        stop,
        serving,
        setup,
    }
}

async fn until(what: &str, condition: impl Fn() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !condition() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("never {what}"));
}

/// Shutdown waits for the book's commit in progress, not for the rest of the pass: one
/// batch a book (D92), and 16 books at 400 ms each would hold it 6.4 s.
#[tokio::test]
async fn shutdown_does_not_wait_for_the_monitors_pass() {
    let server = monitored(
        Script {
            delay: std::time::Duration::from_millis(400),
            ..Default::default()
        },
        16,
        4,
    )
    .await;
    let audit = server.audit.clone();
    until("a commit started", || audit.attempts() >= 1).await;
    let asked = std::time::Instant::now();
    server.stop.send(()).unwrap();
    server.serving.await.unwrap().unwrap();
    let waited = asked.elapsed();
    assert!(
        waited < std::time::Duration::from_secs(3),
        "shutdown waited {waited:?} for {} commits",
        audit.attempts()
    );
    assert!(audit.attempts() < 16, "{}", audit.attempts());
}

/// A full store stays full: the book's changes, one batch refused whole, are tried at its
/// next evaluation, not every second.
#[tokio::test]
async fn a_full_audit_store_is_not_retried_on_a_timer() {
    let server = monitored(
        Script {
            full: true,
            ..Default::default()
        },
        1,
        2,
    )
    .await;
    let audit = server.audit.clone();
    until("an append was attempted", || audit.attempts() >= 1).await;
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    assert_eq!(audit.attempts(), 1);
    server.stop.send(()).unwrap();
    server.serving.await.unwrap().unwrap();
}

/// Any other failure is tried again within a second, with nothing else to trigger it.
#[tokio::test]
async fn a_failed_append_is_retried_on_the_timer() {
    let server = monitored(
        Script {
            transient: 1,
            ..Default::default()
        },
        1,
        1,
    )
    .await;
    let audit = server.audit.clone();
    let inner = server.setup.storage.audit.clone();
    let recorded = || async {
        inner
            .query(&AuditFilter::default(), &Pagination::new(0, 10))
            .await
            .unwrap()
            .total
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while recorded().await < 1 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the change is recorded on the retry");
    assert_eq!(audit.attempts(), 2);
    server.stop.send(()).unwrap();
    server.serving.await.unwrap().unwrap();
}

// =============================================================================
// THE PRE-TRADE CHECK (P14d)
// =============================================================================

async fn check(
    router: &Router,
    token: Option<&str>,
    book: &str,
    changes: Value,
) -> (StatusCode, Value) {
    let (code, _, body) = call(
        router,
        "POST",
        &format!("/api/v1/books/{book}/limits/check"),
        token,
        None,
        None,
        Some(json!({ "changes": changes })),
    )
    .await;
    (code, body)
}

/// Store copies of the fixture bond under `ids`, and map ISIN `CHECK-ISIN` to the first.
async fn store_copies(setup: &support::Setup, ids: &[String]) {
    use convex_core::contracts::RequestId;
    use convex_engine::ports::reference_repository::{
        ReferenceChange, ReferenceCommand, MAX_REFERENCE_CHANGES,
    };
    let at = support::fixture::time("2025-01-01T00:00:00Z");
    for (chunk, ids) in ids.chunks(MAX_REFERENCE_CHANGES).enumerate() {
        let changes = ids
            .iter()
            .map(|id| {
                let mut bond = support::fixture::bond();
                bond.instrument_id = convex_core::ids::InstrumentId::new(id.as_str());
                bond.isin = Some(format!("fixture-isin-{id}"));
                bond.cusip = Some(format!("fixture-cusip-{id}"));
                ReferenceChange::Create {
                    bond: Box::new(bond),
                }
            })
            .collect();
        let command = ReferenceCommand {
            idempotency_key: RequestId::new(format!("copies-{chunk}")).unwrap(),
            changes,
            master_changes: vec![],
        };
        setup.storage.bonds.commit(&command, at).await.unwrap();
    }
    let mapping = serde_json::from_value(json!({
        "expected_version": null,
        "definition": {"kind": "identifier", "definition": {
            "mapping_id": "isin-check",
            "identifier": {"kind": "isin", "value": "CHECK-ISIN"},
            "target": {"kind": "instrument", "instrument_id": ids[0]},
            "effective": {"from": "2000-01-01T00:00:00Z", "until": null},
            "provenance": {"source": "fictional-fixture", "evidence_reference": "test"}
        }}
    }))
    .unwrap();
    let command = ReferenceCommand {
        idempotency_key: RequestId::new("mapping-check").unwrap(),
        changes: vec![],
        master_changes: vec![mapping],
    };
    setup.storage.bonds.commit(&command, at).await.unwrap();
}

/// The current version a route reports in its `ETag`.
async fn version(router: &Router, token: Option<&str>, path: &str) -> String {
    let (code, etag, body) = call(router, "GET", path, token, None, None, None).await;
    assert_eq!(code, StatusCode::OK, "{path}: {body}");
    etag.unwrap()
}

#[tokio::test]
async fn a_check_adds_each_row_to_the_held_position_and_refuses_what_a_commit_would() {
    let (setup, router) = router(AccessControl::local_development()).await;
    store_copies(&setup, &["BOND-C01".to_owned()]).await;
    put_book(
        &router,
        None,
        "BOOK-A",
        json!({"currency": "USD", "positions": [
            {"instrument_id": BOND, "quantity": "1000000", "rating": "A"}
        ]}),
    )
    .await;
    let one = json!([{"instrument_id": BOND, "quantity": "1"}]);
    let (code, body) = check(&router, None, "BOOK-A", one.clone()).await;
    assert_eq!(code, StatusCode::NOT_FOUND, "no limit set: {body}");
    let (code, body) = check(&router, None, "BOOK-Z", one).await;
    assert_eq!(code, StatusCode::NOT_FOUND, "no book: {body}");
    commit_limits(
        &router,
        "BOOK-A",
        "limits-a",
        set(vec![
            face("face", None, Some("2000000"), None),
            limit(
                "each",
                json!({"kind": "face", "instrument": "each"}),
                "1500000",
            ),
            limit("net", json!({"kind": "market_value"}), "1000000000"),
        ]),
    )
    .await;
    let book_etag = version(&router, None, "/api/v1/books/BOOK-A").await;
    let set_etag = version(&router, None, "/api/v1/books/BOOK-A/limits").await;

    // No rows: the current status.
    let current = status(&router, None, "BOOK-A").await;
    let (code, body) = check(&router, None, "BOOK-A", json!([])).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["limits"], current["limits"], "{body}");
    assert_eq!(body["book_version"], 1, "{body}");
    assert_eq!(body["limit_set_version"], 1, "{body}");

    // A row adds to the held position; restating its rating is allowed.
    let (code, body) = check(
        &router,
        None,
        "BOOK-A",
        json!([{"instrument_id": BOND, "quantity": "1500000", "rating": "A"}]),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let limits = by_id(&body);
    assert_eq!(limits["face"]["status"], "breach", "{}", limits["face"]);
    assert_eq!(limits["face"]["value"], "2500000");
    assert_eq!(limits["each"]["status"], "breach", "{}", limits["each"]);
    assert_eq!(limits["each"]["member"], BOND);

    // Selling the whole position drops it: nothing is missing, so net value is known.
    let (code, body) = check(
        &router,
        None,
        "BOOK-A",
        json!([{"instrument_id": BOND, "quantity": "-1000000.00"}]),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let limits = by_id(&body);
    assert_eq!(limits["face"]["value"], "0", "{}", limits["face"]);
    assert_eq!(limits["net"]["status"], "within", "{}", limits["net"]);
    assert_eq!(limits["net"]["value"], "0", "{}", limits["net"]);
    assert_eq!(current["limits"][2]["status"], "unknown", "{current}");

    // A new instrument, named by its ISIN, follows the held position. It has no result
    // here (no runtime), so net value is unknown; face limits still evaluate.
    let (code, body) = check(
        &router,
        None,
        "BOOK-A",
        json!([{"identifier": {"kind": "isin", "value": "CHECK-ISIN"}, "quantity": "300000"}]),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let limits = by_id(&body);
    assert_eq!(
        limits["net"]["reason"],
        json!({"code": "missing_result", "instruments": {"count": 2, "first": [BOND, "BOND-C01"]}}),
        "{}",
        limits["net"]
    );
    assert_eq!(limits["face"]["value"], "1000000", "{}", limits["face"]);
    assert_eq!(limits["each"]["member"], BOND, "{}", limits["each"]);

    // Each refusal names the row; nothing is evaluated.
    for (rows, index, code, reason) in [
        (
            json!([{"instrument_id": BOND, "quantity": "1"}, {"instrument_id": BOND, "quantity": "2"}]),
            1,
            "DUPLICATE_INSTRUMENT",
            None,
        ),
        (
            json!([{"instrument_id": "NO-SUCH", "quantity": "1"}]),
            0,
            "UNKNOWN_INSTRUMENT",
            None,
        ),
        (
            json!([{"identifier": {"kind": "isin", "value": "NO-SUCH"}, "quantity": "1"}]),
            0,
            "UNKNOWN_IDENTIFIER",
            None,
        ),
        (
            json!([{"instrument_id": BOND, "quantity": "1", "unit": "units"}]),
            0,
            "INVALID_ROW",
            Some("a change keeps the held position's unit"),
        ),
        (
            json!([{"instrument_id": BOND, "quantity": "1", "rating": "BBB"}]),
            0,
            "INVALID_ROW",
            Some("a change keeps the held position's rating"),
        ),
        (
            json!([{"instrument_id": BOND, "quantity": "1", "sector": "Utilities"}]),
            0,
            "INVALID_ROW",
            Some("a change keeps the held position's sector"),
        ),
        (
            json!([{"instrument_id": BOND, "quantity": "1", "reference_version": 1}]),
            0,
            "INVALID_ROW",
            Some("a change keeps the held position's reference version"),
        ),
        (
            json!([{"instrument_id": BOND, "quantity": "79228162514264337593543950335"}]),
            0,
            "INVALID_ROW",
            Some("the changed quantity exceeds exact representation"),
        ),
    ] {
        let (status, body) = check(&router, None, "BOOK-A", rows.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{rows}: {body}");
        assert_eq!(body["code"], "BOOK_ROWS_REJECTED", "{rows}: {body}");
        assert_eq!(body["rows"][0]["index"], index, "{rows}: {body}");
        assert_eq!(body["rows"][0]["code"], code, "{rows}: {body}");
        if let Some(reason) = reason {
            assert_eq!(body["rows"][0]["reason"], reason, "{rows}: {body}");
        }
    }
    let rows: Vec<Value> = (0..257)
        .map(|_| json!({"instrument_id": BOND, "quantity": "1"}))
        .collect();
    let (code, body) = check(&router, None, "BOOK-A", json!(rows)).await;
    assert_eq!(code, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(body["code"], "CHECK_TOO_LARGE", "{body}");
    let (code, _, body) = call(
        &router,
        "POST",
        "/api/v1/books/BOOK-A/limits/check",
        None,
        None,
        None,
        Some(json!({"changes": [], "dry_run": true})),
    )
    .await;
    assert!(code.is_client_error(), "{code}: {body}");

    // A check stores nothing.
    assert_eq!(
        version(&router, None, "/api/v1/books/BOOK-A").await,
        book_etag
    );
    assert_eq!(
        version(&router, None, "/api/v1/books/BOOK-A/limits").await,
        set_etag
    );
}

/// A check whose applied book would exceed `MAX_BOOK_POSITIONS` is refused as committing
/// that book would be; one that closes a position to make room is not.
#[tokio::test]
async fn a_check_past_the_position_bound_is_refused_as_its_commit_would_be() {
    let (setup, router) = router(AccessControl::local_development()).await;
    let ids: Vec<String> = (0..1_001).map(|i| format!("BOND-C{i:04}")).collect();
    store_copies(&setup, &ids).await;
    let positions: Vec<Value> = ids[..1_000]
        .iter()
        .map(|id| json!({"instrument_id": id, "quantity": "1000"}))
        .collect();
    put_book(
        &router,
        None,
        "FULL",
        json!({"currency": "USD", "positions": positions}),
    )
    .await;
    commit_limits(
        &router,
        "FULL",
        "limits-full",
        set(vec![limit(
            "each",
            json!({"kind": "face", "instrument": "each"}),
            "1000000",
        )]),
    )
    .await;
    let new = json!({"instrument_id": ids[1_000], "quantity": "1000"});
    let (code, body) = check(&router, None, "FULL", json!([new.clone()])).await;
    assert_eq!(code, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(body["code"], "BOOK_TOO_LARGE", "{body}");
    let close = json!({"instrument_id": ids[0], "quantity": "-1000"});
    let (code, body) = check(&router, None, "FULL", json!([close, new])).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(by_id(&body)["each"]["status"], "within", "{body}");
}

/// D91: a check needs `BookRead` on the book, each instrument it holds, each one a limit
/// names, and each one a row names, by id or through an identifier.
#[tokio::test]
async fn a_check_needs_the_book_and_every_instrument_it_reads() {
    let (_issuer, setup, router) = two_subjects().await;
    store_copies(&setup, &["BOND-C01".to_owned()]).await;
    let path = "/api/v1/books/BOOK-A/limits";
    let body = set(vec![face("face", None, Some("2000000"), None)]);
    let (code, _, response) = call(
        &router,
        "PUT",
        path,
        Some(RISK_A),
        Some("p1"),
        None,
        Some(body),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{response}");
    let (code, _, response) = call(
        &router,
        "POST",
        "/api/v1/books/BOOK-A/limits/proposal/approve",
        Some(RISK_B),
        Some("a1"),
        Some("1"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{response}");
    let book_etag = version(&router, Some(HUMAN_TOKEN), "/api/v1/books/BOOK-A").await;
    let set_etag = version(&router, Some(RISK_A), path).await;

    let held = json!([{"instrument_id": BOND, "quantity": "500000"}]);
    let by_id_row = json!([{"instrument_id": "BOND-C01", "quantity": "1"}]);
    let by_isin = json!([{"identifier": {"kind": "isin", "value": "CHECK-ISIN"}, "quantity": "1"}]);
    let (code, body) = check(&router, Some(RISK_A), "BOOK-A", held.clone()).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(by_id(&body)["face"]["value"], "1500000", "{body}");
    for (token, rows, why) in [
        (SERVICE_TOKEN, json!([]), "no grant"),
        (
            OUTSIDER,
            json!([]),
            "the book holds an instrument it cannot read",
        ),
        (
            RISK_A,
            by_id_row.clone(),
            "a row names an instrument it cannot read",
        ),
        (
            RISK_A,
            by_isin.clone(),
            "an identifier reaches an instrument it cannot read",
        ),
    ] {
        let (code, body) = check(&router, Some(token), "BOOK-A", rows).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{why}: {body}");
    }
    for rows in [by_id_row, by_isin] {
        let (code, body) = check(&router, Some(HUMAN_TOKEN), "BOOK-A", rows.clone()).await;
        assert_eq!(code, StatusCode::OK, "{rows}: {body}");
    }

    // A check stores nothing.
    assert_eq!(
        version(&router, Some(HUMAN_TOKEN), "/api/v1/books/BOOK-A").await,
        book_etag
    );
    assert_eq!(version(&router, Some(RISK_A), path).await, set_etag);
}

// =============================================================================
// THE RECORDS ROUTE (P15b)
// =============================================================================

/// The monitor's records of `book`, as stored, oldest first.
async fn stored(audit: &Arc<dyn AuditStore>, book: &str) -> Vec<Value> {
    let filter = AuditFilter {
        entity_id: Some(book.into()),
        ..AuditFilter::default()
    };
    let page = audit
        .query(&filter, &Pagination::new(0, 100))
        .await
        .unwrap();
    page.items
        .iter()
        .map(|entry| serde_json::from_str(entry.details.as_deref().unwrap()).unwrap())
        .collect()
}

/// The route returns the monitor's records of one book, each unchanged from what it
/// stored and its frame carried, oldest first; a reader following by cursor sees each
/// new record once.
#[tokio::test]
async fn the_records_route_follows_the_monitors_records_by_cursor() {
    let server = monitored(Script::default(), 2, 3).await;
    let inner = server.setup.storage.audit.clone();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while stored(&inner, "BOOK-00").await.len() < 3 || stored(&inner, "BOOK-01").await.len() < 3
        {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("each first status is recorded");
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let base = server.base.clone();
    let get = |query: String| {
        let request = client.get(format!("{base}/api/v1/books/BOOK-00/limits/records{query}"));
        async move {
            let response = request.send().await.unwrap();
            let code = response.status().as_u16();
            (code, response.json::<Value>().await.unwrap())
        }
    };

    let (code, first) = get("?limit=2".into()).await;
    assert_eq!(code, 200, "{first}");
    assert_eq!(
        (&first["book_id"], &first["withheld"], &first["more"]),
        (&json!("BOOK-00"), &json!(0), &json!(true)),
        "{first}"
    );
    let (code, second) = get(format!("?after={}&limit=2", first["next_after"])).await;
    assert_eq!(code, 200, "{second}");
    assert_eq!(second["more"], false, "{second}");
    let mut read: Vec<Value> = first["records"].as_array().unwrap().clone();
    read.extend(second["records"].as_array().unwrap().iter().cloned());
    assert_eq!(read, stored(&inner, "BOOK-00").await);
    assert_eq!(second["next_after"], read[2]["record_id"]);
    assert!(read
        .iter()
        .all(|r| r["book_id"] == "BOOK-00" && r["status"] == "within"));

    // One limit breaches: one new record, and the cursor reads only it.
    let cursor = second["next_after"].clone();
    let current = client
        .get(format!("{base}/api/v1/books/BOOK-00/limits"))
        .send()
        .await
        .unwrap();
    let etag = current.headers()["etag"].to_str().unwrap().to_owned();
    let response = client
        .put(format!("{base}/api/v1/books/BOOK-00/limits"))
        .header("idempotency-key", "limits-00-breach")
        .header("if-match", etag)
        .json(&set(vec![
            face("face-00", None, Some("2000000"), None),
            face("face-01", None, Some("500000"), None),
            face("face-02", None, Some("2000000"), None),
        ]))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{}", response.status());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while stored(&inner, "BOOK-00").await.len() < 4 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the breach is recorded");
    let (code, next) = get(format!("?after={cursor}")).await;
    assert_eq!(code, 200, "{next}");
    let records = next["records"].as_array().unwrap();
    assert_eq!(records.len(), 1, "{next}");
    assert_eq!(
        (
            &records[0]["limit_id"],
            &records[0]["previous"],
            &records[0]["status"]
        ),
        (&json!("face-01"), &json!("within"), &json!("breach"))
    );
    assert_eq!(records[0], stored(&inner, "BOOK-00").await[3]);
    assert_eq!(
        (&next["next_after"], &next["more"]),
        (&records[0]["record_id"], &json!(false))
    );
    let (code, nothing) = get(format!("?after={}", next["next_after"])).await;
    assert_eq!(code, 200, "{nothing}");
    assert_eq!(
        (
            &nothing["records"],
            &nothing["next_after"],
            &nothing["more"]
        ),
        (&json!([]), &next["next_after"], &json!(false))
    );

    server.stop.send(()).unwrap();
    server.serving.await.unwrap().unwrap();
}

/// A record as the monitor stores it, for `book`, with `fields` added.
fn record(id: u64, book: &str, measure: Value, fields: Value) -> AuditEntry {
    let mut details = json!({
        "record_id": id,
        "book_id": book,
        "book_version": 1,
        "limit_set_version": 1,
        "evaluated_at": "2026-11-07T12:00:00Z",
        "monitor_started_at": "2026-11-07T12:00:00Z",
        "previous": "unevaluated",
        "measure": measure,
        "limit_id": format!("limit-{id}"),
        "status": "unknown",
        "snapshot_ids": [],
    });
    details
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    AuditEntry {
        id,
        timestamp: 0,
        event_type: "limit_status".into(),
        entity_type: "book".into(),
        entity_id: book.into(),
        user: None,
        action: "unknown".into(),
        details: Some(details.to_string()),
    }
}

async fn records(
    router: &Router,
    token: Option<&str>,
    book: &str,
    query: &str,
) -> (StatusCode, Value) {
    let (code, _, body) = call(
        router,
        "GET",
        &format!("/api/v1/books/{book}/limits/records{query}"),
        token,
        None,
        None,
        None,
    )
    .await;
    (code, body)
}

fn ids(page: &Value) -> Vec<u64> {
    page["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["record_id"].as_u64().unwrap())
        .collect()
}

/// Reading records needs status's rule on the book now, and each instrument a record
/// names: a record naming one the caller cannot read, though the book no longer holds
/// it, is withheld and counted, and the cursor passes it.
#[tokio::test]
async fn a_record_naming_an_instrument_the_caller_cannot_read_is_withheld() {
    let (_issuer, setup, router) = two_subjects().await;
    let router = router.layer(axum::Extension(setup.storage.audit.clone()));
    let face_on = |id: &str| json!({"kind": "face", "instrument": {"one": id}});
    let each_face = json!({"kind": "face", "instrument": "each"});
    let market_value = json!({"kind": "market_value"});
    let mut other_event = record(6, "BOOK-A", market_value.clone(), json!({}));
    other_event.event_type = "note".into();
    setup
        .storage
        .audit
        .append_all(&[
            record(1, "BOOK-A", face_on(BOND), json!({"status": "breach", "value": "1000000"})),
            record(
                2,
                "BOOK-A",
                market_value.clone(),
                json!({"reason": {"code": "missing_result", "instruments": {"count": 1, "first": ["BOND-002"]}}}),
            ),
            record(3, "BOOK-A", each_face, json!({"status": "breach", "member": "BOND-003"})),
            record(
                4,
                "BOOK-A",
                json!({"kind": "dv01"}),
                json!({"reason": {"code": "quote_refused", "instrument": BOND, "detail": "stale"}}),
            ),
            record(5, "BOOK-B", face_on(BOND), json!({})),
            other_event,
        ])
        .await
        .unwrap();

    // The book has no limit set now; its records are still read.
    let (code, page) = records(&router, Some(RISK_A), "BOOK-A", "?limit=2").await;
    assert_eq!(code, StatusCode::OK, "{page}");
    assert_eq!(ids(&page), [1]);
    assert_eq!(
        (&page["withheld"], &page["next_after"], &page["more"]),
        (&json!(1), &json!(2), &json!(true))
    );
    assert!(!page.to_string().contains("BOND-002"), "{page}");
    let (code, page) = records(&router, Some(RISK_A), "BOOK-A", "?after=2&limit=2").await;
    assert_eq!(code, StatusCode::OK, "{page}");
    assert_eq!(ids(&page), [4]);
    assert_eq!(
        (&page["withheld"], &page["next_after"], &page["more"]),
        (&json!(1), &json!(4), &json!(true))
    );
    // An entry that is not a status change is passed, and neither shown nor counted.
    let (code, page) = records(&router, Some(RISK_A), "BOOK-A", "?after=4").await;
    assert_eq!(code, StatusCode::OK, "{page}");
    assert_eq!(
        (
            ids(&page),
            &page["withheld"],
            &page["next_after"],
            &page["more"]
        ),
        (vec![], &json!(0), &json!(6), &json!(false))
    );

    // A subject holding every instrument sees each record of the book.
    let (code, page) = records(&router, Some(HUMAN_TOKEN), "BOOK-A", "").await;
    assert_eq!(code, StatusCode::OK, "{page}");
    assert_eq!(
        (ids(&page), &page["withheld"]),
        (vec![1, 2, 3, 4], &json!(0))
    );

    // A record whose measure this server cannot read (one a newer server wrote) names
    // instruments it cannot check: it is withheld from everyone, and the cursor passes it.
    setup
        .storage
        .audit
        .append(&record(
            7,
            "BOOK-A",
            json!({"kind": "value_at_risk"}),
            json!({}),
        ))
        .await
        .unwrap();
    let (code, page) = records(&router, Some(HUMAN_TOKEN), "BOOK-A", "?after=4").await;
    assert_eq!(code, StatusCode::OK, "{page}");
    assert_eq!(
        (ids(&page), &page["withheld"], &page["next_after"]),
        (vec![], &json!(1), &json!(7))
    );

    for (token, book, why) in [
        (
            OUTSIDER,
            "BOOK-A",
            "the book holds an instrument it cannot read",
        ),
        (SERVICE_TOKEN, "BOOK-A", "no grant"),
        (RISK_A, "BOOK-B", "a book outside its grant"),
    ] {
        let (code, body) = records(&router, Some(token), book, "").await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{why}: {body}");
        assert!(!body.to_string().contains("breach"), "{why}: {body}");
    }
    for query in ["?limit=0", "?limit=1001", "?after=-1", "?cursor=1"] {
        let (code, body) = records(&router, Some(RISK_A), "BOOK-A", query).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{query}: {body}");
    }
}

/// A page stops before a record that would take it past 60,000 JSON values, so the
/// access layer's response scan (at most 65,536) reads it whole; the cursor reads the rest.
#[tokio::test]
async fn a_page_of_large_records_stops_where_the_response_scan_can_read_it() {
    let (_issuer, setup, router) = two_subjects().await;
    let router = router.layer(axum::Extension(setup.storage.audit.clone()));
    let snapshots: Vec<String> = (0..1000).map(|i| format!("snap-{i:04}")).collect();
    let large: Vec<AuditEntry> = (1..=100)
        .map(|id| {
            record(
                id,
                "BOOK-A",
                json!({"kind": "market_value"}),
                json!({"snapshot_ids": snapshots}),
            )
        })
        .collect();
    for chunk in large.chunks(64) {
        setup.storage.audit.append_all(chunk).await.unwrap();
    }
    let mut cursor = 0;
    let mut read = Vec::new();
    let mut pages = 0;
    loop {
        let query = format!("?after={cursor}&limit=1000");
        let (code, page) = records(&router, Some(HUMAN_TOKEN), "BOOK-A", &query).await;
        assert_eq!(code, StatusCode::OK, "page {pages}: {page}");
        pages += 1;
        read.extend(ids(&page));
        cursor = page["next_after"].as_u64().unwrap();
        if page["more"] == false {
            break;
        }
    }
    assert_eq!(read, (1..=100).collect::<Vec<u64>>());
    assert_eq!(pages, 2, "about 59 records of 1,013 values a page");
}

/// Without a monitor the route has no store to read; a book that does not exist is not
/// found.
#[tokio::test]
async fn the_records_route_needs_a_monitor_and_the_book() {
    let (setup, router) = router(AccessControl::local_development()).await;
    put_book(
        &router,
        None,
        "BOOK-A",
        json!({"currency": "USD", "positions": [position("1000000")]}),
    )
    .await;
    let (code, body) = records(&router, None, "BOOK-A", "").await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(
        body.to_string().contains("does not monitor limits"),
        "{body}"
    );
    let router = router.layer(axum::Extension(setup.storage.audit.clone()));
    let (code, body) = records(&router, None, "BOOK-A", "").await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(
        (&body["records"], &body["next_after"], &body["more"]),
        (&json!([]), &json!(0), &json!(false))
    );
    let (code, body) = records(&router, None, "BOOK-Z", "").await;
    assert_eq!(code, StatusCode::NOT_FOUND, "{body}");
}

// =============================================================================
// ACKNOWLEDGEMENTS (P15c)
// =============================================================================

const ACKNOWLEDGEMENTS: &str = "/api/v1/books/BOOK-A/limits/acknowledgements";

/// A time `seconds` from now, as the routes write one.
fn in_seconds(seconds: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::seconds(seconds))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn acknowledgement(limit_id: &str, seconds: i64, note: &str) -> Value {
    json!({"limit_id": limit_id, "expires_at": in_seconds(seconds), "note": note})
}

async fn acknowledge(
    router: &Router,
    token: Option<&str>,
    book: &str,
    key: &str,
    body: &Value,
) -> (StatusCode, Option<String>, Value) {
    call(
        router,
        "POST",
        &format!("/api/v1/books/{book}/limits/acknowledgements"),
        token,
        Some(key),
        None,
        Some(body.clone()),
    )
    .await
}

async fn withdraw(
    router: &Router,
    token: Option<&str>,
    book: &str,
    key: &str,
    limit_id: &str,
) -> (StatusCode, Value) {
    let (code, _, body) = call(
        router,
        "POST",
        &format!("/api/v1/books/{book}/limits/acknowledgements/withdraw"),
        token,
        Some(key),
        None,
        Some(json!({ "limit_id": limit_id })),
    )
    .await;
    (code, body)
}

async fn read(router: &Router, token: Option<&str>, path: &str) -> (StatusCode, Value) {
    let (code, _, body) = call(router, "GET", path, token, None, None, None).await;
    (code, body)
}

/// The limit ids of an acknowledgements view's entries.
fn acknowledged(view: &Value) -> Vec<&str> {
    view["acknowledgements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["limit_id"].as_str().unwrap())
        .collect()
}

/// Commit `body` as BOOK-A's limits under required access: risk-a proposes, risk-b
/// approves.
async fn approved(router: &Router, key: &str, base: Option<&str>, body: Value) {
    let (code, _, response) = call(
        router,
        "PUT",
        "/api/v1/books/BOOK-A/limits",
        Some(RISK_A),
        Some(&format!("{key}-propose")),
        base,
        Some(body),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{response}");
    let version = response["version"].to_string();
    let (code, _, response) = call(
        router,
        "POST",
        "/api/v1/books/BOOK-A/limits/proposal/approve",
        Some(RISK_B),
        Some(&format!("{key}-approve")),
        Some(&version),
        None,
    )
    .await;
    assert!(code.is_success(), "{response}");
}

/// P15c (D95): a risk manager with status's read rule acknowledges a limit in breach or
/// unknown now; each refusal stores nothing, and a retry never acknowledges twice.
#[tokio::test]
async fn an_acknowledgement_needs_a_risk_manager_and_a_limit_in_breach_or_unknown() {
    let (_issuer, _setup, router) = two_subjects().await;
    approved(
        &router,
        "first",
        None,
        set(vec![
            face("breach", None, Some("500000"), None),
            face("within", None, Some("2000000"), None),
            limit("value", json!({"kind": "market_value"}), "1"),
        ]),
    )
    .await;
    let body = acknowledgement("breach", 3600, "seen; the desk sells half today");

    // A book writer is not a risk manager, and a risk manager who cannot read the book's
    // instrument cannot see its status.
    for (token, key) in [(HUMAN_TOKEN, "k-writer"), (OUTSIDER, "k-outsider")] {
        let (code, _, response) = acknowledge(&router, Some(token), "BOOK-A", key, &body).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{token}: {response}");
    }
    let refusals = [
        (
            acknowledgement("within", 3600, "x"),
            StatusCode::CONFLICT,
            "LIMIT_NOT_ACKNOWLEDGEABLE",
        ),
        (
            acknowledgement("breach", 7 * 24 * 3600 + 60, "x"),
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
        ),
        (
            acknowledgement("breach", -60, "x"),
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
        ),
        (
            acknowledgement("breach", 3600, " "),
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
        ),
        (
            acknowledgement("none", 3600, "x"),
            StatusCode::NOT_FOUND,
            "LIMIT_NOT_FOUND",
        ),
        // Without a monitor there is no record to check a record id against.
        (
            json!({"limit_id": "breach", "expires_at": in_seconds(3600), "note": "x", "record_id": 1}),
            StatusCode::SERVICE_UNAVAILABLE,
            "",
        ),
        (
            json!({"limit_id": "breach", "expires_at": in_seconds(3600), "note": "x", "status": "breach"}),
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
        ),
    ];
    for (index, (body, status, code)) in refusals.into_iter().enumerate() {
        let (got, _, response) = acknowledge(
            &router,
            Some(RISK_A),
            "BOOK-A",
            &format!("bad-{index}"),
            &body,
        )
        .await;
        assert_eq!(got, status, "{index}: {response}");
        if !code.is_empty() {
            assert_eq!(response["code"], code, "{index}: {response}");
        }
    }
    let (code, empty) = read(&router, Some(RISK_A), ACKNOWLEDGEMENTS).await;
    assert_eq!(code, StatusCode::OK, "{empty}");
    assert_eq!(
        empty["acknowledgements"],
        json!([]),
        "nothing stored: {empty}"
    );
    assert!(empty.get("version").is_none(), "{empty}");

    // A breach, and an unknown limit, are acknowledged; a retry returns the same version.
    let (code, etag, made) = acknowledge(&router, Some(RISK_A), "BOOK-A", "k-1", &body).await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    assert_eq!(etag.as_deref(), Some("\"1\""));
    assert_eq!(
        made["change"],
        json!({"action": "acknowledged", "limit_id": "breach"})
    );
    assert_eq!(acknowledged(&made), ["breach"]);
    let entry = made["acknowledgements"][0].clone();
    let risk_a = entry["acknowledged_by"].as_str().unwrap().to_owned();
    assert!(risk_a.starts_with("access:"), "{made}");
    assert_eq!(
        (
            &entry["note"],
            &entry["expires_at"],
            &entry["limit_set_version"]
        ),
        (&body["note"], &body["expires_at"], &json!(1))
    );
    assert_eq!(entry["limit"]["upper"], "500000");
    let (code, _, retried) = acknowledge(&router, Some(RISK_A), "BOOK-A", "k-1", &body).await;
    assert_eq!((code, &retried), (StatusCode::CREATED, &made));
    // A retry needs what acknowledging needs, so a retained version is no way around it.
    for token in [HUMAN_TOKEN, OUTSIDER] {
        let (code, _, response) = acknowledge(&router, Some(token), "BOOK-A", "k-1", &body).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{token}: {response}");
    }
    let mut other = body.clone();
    other["note"] = json!("another note");
    let (code, _, response) = acknowledge(&router, Some(RISK_A), "BOOK-A", "k-1", &other).await;
    assert_eq!(code, StatusCode::CONFLICT, "{response}");
    let unknown = acknowledgement("value", 600, "no results yet");
    let (code, _, made) = acknowledge(&router, Some(RISK_B), "BOOK-A", "k-2", &unknown).await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    assert_eq!(acknowledged(&made), ["breach", "value"]);

    // Status carries each acknowledgement on its limit; a limit within carries none.
    let limits = by_id(&status(&router, Some(RISK_B), "BOOK-A").await);
    assert_eq!(limits["breach"]["status"], "breach");
    assert_eq!(
        limits["breach"]["acknowledgement"],
        json!({"acknowledged_by": risk_a, "acknowledged_at": entry["acknowledged_at"],
               "expires_at": body["expires_at"], "limit_set_version": 1, "note": body["note"]})
    );
    assert_eq!(limits["value"]["status"], "unknown");
    assert_eq!(limits["value"]["acknowledgement"]["note"], "no results yet");
    assert!(limits["within"].get("acknowledgement").is_none());

    // Reads: a reader sees them; one who cannot read the book's instrument does not.
    let (code, current) = read(&router, Some(HUMAN_TOKEN), ACKNOWLEDGEMENTS).await;
    assert_eq!(code, StatusCode::OK, "{current}");
    assert_eq!(acknowledged(&current), ["breach", "value"]);
    assert_eq!(current["version"], 2);
    let history = format!("{ACKNOWLEDGEMENTS}/history");
    let (code, versions) = read(&router, Some(HUMAN_TOKEN), &history).await;
    assert_eq!(code, StatusCode::OK, "{versions}");
    assert_eq!(
        versions["versions"][0],
        json!({"version": 1, "recorded_at": versions["versions"][0]["recorded_at"],
               "action": "acknowledged", "limit_id": "breach", "by": risk_a,
               "acknowledgement_count": 1})
    );
    assert_eq!(versions["current_version"], 2);
    let first = format!("{ACKNOWLEDGEMENTS}/versions/1");
    let (code, version) = read(&router, Some(HUMAN_TOKEN), &first).await;
    assert_eq!(code, StatusCode::OK, "{version}");
    assert_eq!(acknowledged(&version), ["breach"]);
    for path in [ACKNOWLEDGEMENTS, history.as_str(), first.as_str()] {
        let (code, response) = read(&router, Some(OUTSIDER), path).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{path}: {response}");
    }
    let (code, response) = withdraw(&router, Some(OUTSIDER), "BOOK-A", "w-0", "breach").await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{response}");
    let (code, response) = withdraw(&router, Some(HUMAN_TOKEN), "BOOK-A", "w-0", "breach").await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{response}");
}

/// P15c (D94): each acknowledgement replaces only its own limit's entry, so two risk
/// managers acknowledging different limits at once, on four threads, both keep theirs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_subjects_acknowledging_different_limits_at_once_both_keep_theirs() {
    let (_issuer, _setup, router) = two_subjects().await;
    approved(
        &router,
        "first",
        None,
        set((0..8)
            .map(|i| face(&format!("face-{i}"), None, Some("500000"), None))
            .collect()),
    )
    .await;
    let start = Arc::new(tokio::sync::Barrier::new(8));
    let tasks: Vec<_> = (0..8)
        .map(|i| {
            let router = router.clone();
            let start = start.clone();
            let body = acknowledgement(&format!("face-{i}"), 3600, &format!("seen {i}"));
            tokio::spawn(async move {
                let token = if i % 2 == 0 { RISK_A } else { RISK_B };
                start.wait().await;
                acknowledge(&router, Some(token), "BOOK-A", &format!("k-{i}"), &body).await
            })
        })
        .collect();
    let mut made = Vec::new();
    for task in tasks {
        made.push(task.await.unwrap());
    }
    for (code, _, body) in &made {
        assert_eq!(*code, StatusCode::CREATED, "{body}");
    }
    let (code, current) = read(&router, Some(RISK_A), ACKNOWLEDGEMENTS).await;
    assert_eq!(code, StatusCode::OK, "{current}");
    assert_eq!(current["version"], 8);
    assert_eq!(
        acknowledged(&current),
        (0..8).map(|i| format!("face-{i}")).collect::<Vec<_>>()
    );
    let subjects: std::collections::BTreeSet<&str> = current["acknowledgements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["acknowledged_by"].as_str().unwrap())
        .collect();
    assert_eq!(subjects.len(), 2, "{current}");
}

/// P15c (D95): an acknowledgement ends when it is withdrawn, when it expires, or when a
/// set version changes its limit; one whose limit the set keeps stays. Entries no longer
/// in force leave the record at its next version, and the history keeps them.
#[tokio::test]
async fn withdrawal_expiry_and_a_changed_limit_end_an_acknowledgement() {
    let (_setup, router) = router(AccessControl::local_development()).await;
    put_book(
        &router,
        None,
        "BOOK-A",
        json!({"currency": "USD", "positions": [position("1000000")]}),
    )
    .await;
    let limits = |changed: &str| {
        set(vec![
            face("kept", None, Some("500000"), None),
            face("short", None, Some("500000"), None),
            face("changed", None, Some(changed), None),
            face("withdrawn", None, Some("500000"), None),
        ])
    };
    commit_limits(&router, "BOOK-A", "limits-1", limits("500000")).await;
    for (key, limit_id, seconds) in [
        ("k-kept", "kept", 3600),
        ("k-short", "short", 10),
        ("k-changed", "changed", 3600),
        ("k-withdrawn", "withdrawn", 3600),
    ] {
        let body = acknowledgement(limit_id, seconds, "seen");
        let (code, _, made) = acknowledge(&router, None, "BOOK-A", key, &body).await;
        assert_eq!(code, StatusCode::CREATED, "{limit_id}: {made}");
    }

    // Withdrawn: it leaves the next version, which names who withdrew it; a second
    // withdrawal finds none, and a retry returns the same version.
    let (code, withdrawn) = withdraw(&router, None, "BOOK-A", "w-1", "withdrawn").await;
    assert_eq!(code, StatusCode::OK, "{withdrawn}");
    assert_eq!(
        withdrawn["change"],
        json!({"action": "withdrawn", "limit_id": "withdrawn", "by": "local-development"})
    );
    assert_eq!(acknowledged(&withdrawn), ["kept", "short", "changed"]);
    let (code, retried) = withdraw(&router, None, "BOOK-A", "w-1", "withdrawn").await;
    assert_eq!((code, &retried), (StatusCode::OK, &withdrawn));
    let (code, response) = withdraw(&router, None, "BOOK-A", "w-2", "withdrawn").await;
    assert_eq!(code, StatusCode::NOT_FOUND, "{response}");
    assert_eq!(response["code"], "LIMIT_ACKNOWLEDGEMENT_NOT_FOUND");

    // A set version that changes one limit ends its acknowledgement only.
    let (code, etag, _) = call(
        &router,
        "GET",
        "/api/v1/books/BOOK-A/limits",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, _, response) = call(
        &router,
        "PUT",
        "/api/v1/books/BOOK-A/limits",
        None,
        Some("limits-2"),
        etag.as_deref(),
        Some(limits("600000")),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{response}");
    let current = by_id(&status(&router, None, "BOOK-A").await);
    assert!(current["kept"]["acknowledgement"].is_object());
    assert!(current["changed"].get("acknowledgement").is_none());
    assert_eq!(current["changed"]["status"], "breach");
    assert!(current["withdrawn"].get("acknowledgement").is_none());
    let (code, view) = read(&router, None, ACKNOWLEDGEMENTS).await;
    assert_eq!(code, StatusCode::OK, "{view}");
    assert_eq!(acknowledged(&view), ["kept", "short"]);

    // Expired: status and the current view drop it at once. Expiry is judged on the wall
    // clock, so the wait is too: under WSL2 it steps back by over 2 s, every 30 s when memory
    // is tight. A step during the wait lengthens it; the margin covers one just after.
    let expires_at: chrono::DateTime<chrono::Utc> = view["acknowledgements"][1]["expires_at"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    while chrono::Utc::now() < expires_at + chrono::Duration::seconds(3) {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let current = by_id(&status(&router, None, "BOOK-A").await);
    assert!(
        current["short"].get("acknowledgement").is_none(),
        "{}",
        current["short"]
    );
    let (code, view) = read(&router, None, ACKNOWLEDGEMENTS).await;
    assert_eq!(code, StatusCode::OK, "{view}");
    assert_eq!(acknowledged(&view), ["kept"]);
    assert_eq!(view["version"], 5);

    // The stored version still holds what ended, until the next version drops it.
    let (code, stored) = read(&router, None, &format!("{ACKNOWLEDGEMENTS}/versions/5")).await;
    assert_eq!(code, StatusCode::OK, "{stored}");
    assert_eq!(acknowledged(&stored), ["kept", "short", "changed"]);
    let body = acknowledgement("changed", 3600, "seen again, at the new bound");
    let (code, _, made) = acknowledge(&router, None, "BOOK-A", "k-again", &body).await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    assert_eq!(acknowledged(&made), ["kept", "changed"]);
    assert_eq!(made["acknowledgements"][1]["limit_set_version"], 2);
    let (code, history) = read(&router, None, &format!("{ACKNOWLEDGEMENTS}/history")).await;
    assert_eq!(code, StatusCode::OK, "{history}");
    let actions: Vec<(&str, &str)> = history["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v["action"].as_str().unwrap(),
                v["limit_id"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        actions,
        [
            ("acknowledged", "kept"),
            ("acknowledged", "short"),
            ("acknowledged", "changed"),
            ("acknowledged", "withdrawn"),
            ("withdrawn", "withdrawn"),
            ("acknowledged", "changed"),
        ]
    );
}

/// P15c: an exact retry returns the version it wrote even after a set version has dropped
/// the limit. The key still refuses a different body, and a new key finds no limit.
#[tokio::test]
async fn an_exact_retry_returns_its_version_after_the_set_drops_the_limit() {
    let (_setup, router) = router(AccessControl::local_development()).await;
    put_book(
        &router,
        None,
        "BOOK-A",
        json!({"currency": "USD", "positions": [position("1000000")]}),
    )
    .await;
    let stays = face("stays", None, Some("500000"), None);
    let limits = set(vec![
        stays.clone(),
        face("dropped", None, Some("500000"), None),
        face("withdrawn", None, Some("500000"), None),
    ]);
    commit_limits(&router, "BOOK-A", "limits-1", limits).await;
    let body = acknowledgement("dropped", 3600, "seen");
    let (code, _, made) = acknowledge(&router, None, "BOOK-A", "k-1", &body).await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    let ended = acknowledgement("withdrawn", 3600, "seen");
    let (code, _, response) = acknowledge(&router, None, "BOOK-A", "k-2", &ended).await;
    assert_eq!(code, StatusCode::CREATED, "{response}");
    let (code, withdrawn) = withdraw(&router, None, "BOOK-A", "w-1", "withdrawn").await;
    assert_eq!(code, StatusCode::OK, "{withdrawn}");

    let (code, etag, _) = call(
        &router,
        "GET",
        "/api/v1/books/BOOK-A/limits",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, _, response) = call(
        &router,
        "PUT",
        "/api/v1/books/BOOK-A/limits",
        None,
        Some("limits-2"),
        etag.as_deref(),
        Some(set(vec![stays])),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{response}");

    let (code, _, retried) = acknowledge(&router, None, "BOOK-A", "k-1", &body).await;
    assert_eq!((code, &retried), (StatusCode::CREATED, &made));
    let (code, retried) = withdraw(&router, None, "BOOK-A", "w-1", "withdrawn").await;
    assert_eq!((code, &retried), (StatusCode::OK, &withdrawn));

    let mut other = body.clone();
    other["note"] = json!("another note");
    let (code, _, response) = acknowledge(&router, None, "BOOK-A", "k-1", &other).await;
    assert_eq!(code, StatusCode::CONFLICT, "{response}");
    let (code, _, response) = acknowledge(&router, None, "BOOK-A", "k-new", &body).await;
    assert_eq!(code, StatusCode::NOT_FOUND, "{response}");
    assert_eq!(response["code"], "LIMIT_NOT_FOUND");
}

/// P15c (D94): the generic reference routes neither write nor list acknowledgements, nor
/// show a receipt that names them.
#[tokio::test]
async fn generic_reference_routes_refuse_acknowledgements() {
    let (_setup, router) = router(AccessControl::local_development()).await;
    put_book(
        &router,
        None,
        "BOOK-A",
        json!({"currency": "USD", "positions": [position("1000000")]}),
    )
    .await;
    commit_limits(
        &router,
        "BOOK-A",
        "limits-1",
        set(vec![face("breach", None, Some("500000"), None)]),
    )
    .await;
    let definition = json!({
        "book_id": "BOOK-A",
        "acknowledgements": [],
        "change": {"action": "withdrawn", "limit_id": "breach", "by": "access:x"},
        "effective": {"from": "2026-01-01T00:00:00Z", "until": null},
        "provenance": {"source": "elsewhere", "evidence_reference": "elsewhere"},
    });
    let command = json!({
        "idempotency_key": "generic-acks",
        "changes": [],
        "master_changes": [{"definition": {"kind": "limit_acknowledgements", "definition": definition}}],
    });
    let (code, _, response) = call(
        &router,
        "POST",
        "/api/v1/reference/transactions",
        None,
        Some("generic-acks"),
        None,
        Some(command),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{response}");
    assert_eq!(response["code"], "LIMITS_ROUTE_REQUIRED", "{response}");
    let body = acknowledgement("breach", 3600, "seen");
    let (code, _, made) = acknowledge(&router, None, "BOOK-A", "k-1", &body).await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    let (code, response) = read(
        &router,
        None,
        "/api/v1/reference/masters/limit_acknowledgements",
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{response}");
    let (code, response) = read(&router, None, "/api/v1/reference/commits/k-1").await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{response}");
    assert_eq!(response["code"], "LIMITS_ROUTE_REQUIRED", "{response}");
}

/// P15c (D97): a recorded change of a limit in breach or unknown carries the
/// acknowledgement in force, and an acknowledgement records nothing of its own. A record
/// id must name a status record of this book's limit.
#[tokio::test]
async fn a_recorded_change_carries_the_acknowledgement_in_force() {
    let server = monitored(Script::default(), 2, 2).await;
    let inner = server.setup.storage.audit.clone();
    let wait = |book: &'static str, count: usize| {
        let inner = inner.clone();
        async move {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while stored(&inner, book).await.len() < count {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("{book} never had {count} records"));
            stored(&inner, book).await
        }
    };
    wait("BOOK-00", 2).await;
    let other_book = wait("BOOK-01", 2).await[0]["record_id"].as_u64().unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let base = server.base.clone();
    let put = |path: &'static str, key: &'static str, body: Value| {
        let request = client
            .put(format!("{base}{path}"))
            .header("idempotency-key", key)
            .header("if-match", "\"1\"")
            .json(&body);
        async move {
            let response = request.send().await.unwrap();
            assert!(
                response.status().is_success(),
                "{path}: {}",
                response.status()
            );
        }
    };
    let post = |body: Value, key: &'static str| {
        let request = client
            .post(format!(
                "{base}/api/v1/books/BOOK-00/limits/acknowledgements"
            ))
            .header("idempotency-key", key)
            .json(&body);
        async move {
            let response = request.send().await.unwrap();
            let code = response.status().as_u16();
            (code, response.json::<Value>().await.unwrap())
        }
    };

    // face-01 breaches: one record, without an acknowledgement.
    put(
        "/api/v1/books/BOOK-00/limits",
        "limits-00-breach",
        set(vec![
            face("face-00", None, Some("2000000"), None),
            face("face-01", None, Some("500000"), None),
        ]),
    )
    .await;
    let records = wait("BOOK-00", 3).await;
    let breach = records[2].clone();
    assert_eq!(
        (&breach["limit_id"], &breach["status"]),
        (&json!("face-01"), &json!("breach"))
    );
    assert!(breach.get("acknowledgement").is_none(), "{breach}");
    let within = records[0]["record_id"].as_u64().unwrap();
    assert_eq!(records[0]["limit_id"], "face-00");

    // A record id names a status record of this book's limit.
    for (record_id, key) in [
        (other_book, "k-other-book"),
        (within, "k-other-limit"),
        (999_999, "k-none"),
    ] {
        let body = json!({"limit_id": "face-01", "expires_at": in_seconds(3600),
                          "note": "x", "record_id": record_id});
        let (code, response) = post(body, key).await;
        assert_eq!(code, 422, "{record_id}: {response}");
        assert_eq!(response["code"], "LIMIT_RECORD_MISMATCH", "{response}");
    }
    let record_id = breach["record_id"].as_u64().unwrap();
    let body = json!({"limit_id": "face-01", "expires_at": in_seconds(3600),
                      "note": "seen; reducing", "record_id": record_id});
    let (code, made) = post(body.clone(), "k-1").await;
    assert_eq!(code, 201, "{made}");
    assert_eq!(made["acknowledgements"][0]["record_id"], record_id);

    // The acknowledgement records nothing: the monitor rereads it and finds no change.
    let entries = inner.entry_count().await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(inner.entry_count().await.unwrap(), entries);

    // A position in units makes both limits unknown: face-01's record carries the
    // acknowledgement, and face-00's, never acknowledged, carries none.
    put(
        "/api/v1/books/BOOK-00",
        "book-00-units",
        json!({"currency": "USD", "positions": [
            {"instrument_id": BOND, "quantity": "10", "unit": "units"}
        ]}),
    )
    .await;
    let records = wait("BOOK-00", 5).await;
    let latest = |limit: &str| {
        records[3..]
            .iter()
            .find(|r| r["limit_id"] == limit)
            .unwrap_or_else(|| panic!("{limit}: {records:?}"))
            .clone()
    };
    let unknown = latest("face-01");
    assert_eq!(
        (&unknown["previous"], &unknown["status"]),
        (&json!("breach"), &json!("unknown"))
    );
    let carried = &unknown["acknowledgement"];
    assert_eq!(
        (
            &carried["note"],
            &carried["record_id"],
            &carried["limit_set_version"],
            &carried["expires_at"]
        ),
        (
            &json!("seen; reducing"),
            &json!(record_id),
            &json!(2),
            &body["expires_at"]
        ),
        "{unknown}"
    );
    let other = latest("face-00");
    assert_eq!(other["status"], "unknown");
    assert!(other.get("acknowledgement").is_none(), "{other}");

    server.stop.send(()).unwrap();
    server.serving.await.unwrap().unwrap();
}

// =============================================================================
// TEMPORARY EXCESS (P15d)
// =============================================================================

/// `limit` with an upper-bound excess expiring at `expires_at`.
fn excess(mut limit: Value, upper: &str, expires_at: &str) -> Value {
    limit["excess"] = json!({"upper": upper, "expires_at": expires_at, "reason": "mandate change"});
    limit
}

/// Wait until the wall clock is `margin` past `expires_at`: expiry is judged on it, and
/// under WSL2 it steps back by over 2 s (see the acknowledgement expiry test).
async fn wait_past(expires_at: &str, margin: i64) {
    let expires_at: chrono::DateTime<chrono::Utc> = expires_at.parse().unwrap();
    while chrono::Utc::now() < expires_at + chrono::Duration::seconds(margin) {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// PUT `body` as BOOK-A's limits over `base`.
async fn put_limits(
    router: &Router,
    token: Option<&str>,
    key: &str,
    base: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let (code, _, response) = call(
        router,
        "PUT",
        "/api/v1/books/BOOK-A/limits",
        token,
        Some(key),
        base,
        Some(body),
    )
    .await;
    (code, response)
}

/// P15d (D96): an excess goes through the two-subject proposal. The proposal refuses one
/// that does not widen, has expired or runs past thirty days; approval refuses one that has
/// expired by then, and leaves the set as it was.
#[tokio::test]
async fn an_excess_is_proposed_and_approved_within_its_window() {
    let (_issuer, _setup, router) = two_subjects().await;
    let base = || face("net", None, Some("500000"), None);
    approved(&router, "first", None, set(vec![base()])).await;

    for (key, proposed) in [
        ("narrower", excess(base(), "400000", &in_seconds(3600))),
        ("expired", excess(base(), "2000000", &in_seconds(-60))),
        (
            "too-long",
            excess(base(), "2000000", &in_seconds(30 * 24 * 3600 + 60)),
        ),
    ] {
        let (code, response) =
            put_limits(&router, Some(RISK_A), key, Some("1"), set(vec![proposed])).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{key}: {response}");
    }

    let thirty = in_seconds(29 * 24 * 3600);
    let long = set(vec![excess(base(), "2000000", &thirty)]);
    let (code, response) = put_limits(&router, Some(RISK_A), "long", Some("1"), long).await;
    assert_eq!(code, StatusCode::ACCEPTED, "{response}");

    // One person cannot approve their own excess.
    let version = response["version"].to_string();
    let (code, _, refused) = call(
        &router,
        "POST",
        "/api/v1/books/BOOK-A/limits/proposal/approve",
        Some(RISK_A),
        Some("self"),
        Some(&version),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(refused["code"], "LIMIT_SELF_APPROVAL");

    // Expired between the proposal and its approval: refused, and the set is unchanged.
    let soon = in_seconds(5);
    let body = set(vec![excess(base(), "2000000", &soon)]);
    let (code, proposal) =
        put_limits(&router, Some(RISK_A), "short", Some("1"), body.clone()).await;
    assert_eq!(code, StatusCode::ACCEPTED, "{proposal}");
    wait_past(&soon, 3).await;
    let version = proposal["version"].to_string();
    let (code, _, refused) = call(
        &router,
        "POST",
        "/api/v1/books/BOOK-A/limits/proposal/approve",
        Some(RISK_B),
        Some("late"),
        Some(&version),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "LIMIT_EXCESS_EXPIRED");
    let (code, current) = read(&router, Some(RISK_A), "/api/v1/books/BOOK-A/limits").await;
    assert_eq!(code, StatusCode::OK, "{current}");
    assert_eq!(current["version"], 1);

    // An exact retry of the proposal returns what it wrote, though the excess has expired.
    let (code, retried) = put_limits(&router, Some(RISK_A), "short", Some("1"), body).await;
    assert_eq!((code, &retried), (StatusCode::ACCEPTED, &proposal));

    // Approved in time: the set holds it, and status judges against it.
    let later = in_seconds(3600);
    approved(
        &router,
        "granted",
        Some("1"),
        set(vec![excess(base(), "2000000", &later)]),
    )
    .await;
    let current = by_id(&status(&router, Some(RISK_A), "BOOK-A").await);
    assert_eq!(current["net"]["status"], "within", "{}", current["net"]);
    assert_eq!(current["net"]["upper"], "2000000");
    assert_eq!(current["net"]["excess"]["base_upper"], "500000");
    assert_eq!(current["net"]["excess"]["expires_at"], json!(later));
    assert_eq!(current["net"]["excess"]["reason"], "mandate change");
}

/// P15d (D96): on request, an excess applies exactly while the response's `evaluated_at` is
/// before its expiry, whichever way the clock moves. Granting one ends the limit's
/// acknowledgement; an expired one carried forward unchanged is inert and refuses nothing,
/// and changing it to one already expired is refused.
#[tokio::test]
async fn an_excess_applies_until_its_expiry_and_an_expired_one_can_be_carried_forward() {
    let (_setup, router) = router(AccessControl::local_development()).await;
    put_book(
        &router,
        None,
        "BOOK-A",
        json!({"currency": "USD", "positions": [position("1000000")]}),
    )
    .await;
    let breached = || face("granted", None, Some("500000"), None);
    let other = |upper: &str| face("other", None, Some(upper), None);
    commit_limits(
        &router,
        "BOOK-A",
        "first",
        set(vec![breached(), other("500000")]),
    )
    .await;
    for (key, limit_id) in [("ack-granted", "granted"), ("ack-other", "other")] {
        let body = acknowledgement(limit_id, 3600, "seen");
        let (code, _, made) = acknowledge(&router, None, "BOOK-A", key, &body).await;
        assert_eq!(code, StatusCode::CREATED, "{limit_id}: {made}");
    }

    // Granting the excess changes the limit's definition, so it ends its acknowledgement.
    let expires_at = in_seconds(5);
    let granted = excess(breached(), "2000000", &expires_at);
    let (code, response) = put_limits(
        &router,
        None,
        "grant",
        Some("1"),
        set(vec![granted.clone(), other("500000")]),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{response}");
    let current = by_id(&status(&router, None, "BOOK-A").await);
    assert!(current["granted"].get("acknowledgement").is_none());
    assert!(current["other"]["acknowledgement"].is_object());

    // Every response across the expiry: the excess and the time it was read at agree.
    let expiry: chrono::DateTime<chrono::Utc> = expires_at.parse().unwrap();
    let (mut applied, mut expired) = (0, 0);
    while expired < 5 {
        let view = status(&router, None, "BOOK-A").await;
        let evaluated_at: chrono::DateTime<chrono::Utc> =
            view["evaluated_at"].as_str().unwrap().parse().unwrap();
        let item = by_id(&view).remove("granted").unwrap();
        if evaluated_at < expiry {
            applied += 1;
            assert_eq!(item["status"], "within", "{item}");
            assert_eq!(item["upper"], "2000000", "{item}");
            assert_eq!(item["excess"]["base_upper"], "500000", "{item}");
        } else {
            expired += 1;
            assert_eq!(item["status"], "breach", "{item}");
            assert_eq!(item["upper"], "500000", "{item}");
            assert!(item.get("excess").is_none(), "{item}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(applied > 0, "no response read the excess before its expiry");

    // The expired excess stays in the set, inert. A new acknowledgement of the breach is
    // kept when a later version carries the excess forward and changes another limit.
    wait_past(&expires_at, 3).await;
    let (code, held) = read(&router, None, "/api/v1/books/BOOK-A/limits").await;
    assert_eq!(code, StatusCode::OK, "{held}");
    assert_eq!(held["limits"][0]["excess"]["expires_at"], json!(expires_at));
    let body = acknowledgement("granted", 3600, "seen at the limit's own bound");
    let (code, _, made) = acknowledge(&router, None, "BOOK-A", "ack-again", &body).await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    let (code, response) = put_limits(
        &router,
        None,
        "carry",
        Some("2"),
        set(vec![granted, other("600000")]),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{response}");
    let current = by_id(&status(&router, None, "BOOK-A").await);
    assert_eq!(current["granted"]["status"], "breach");
    assert!(current["granted"]["acknowledgement"].is_object());
    assert!(current["other"].get("acknowledgement").is_none());

    // Changing it to another expired excess is a new one, and is refused.
    let (code, response) = put_limits(
        &router,
        None,
        "changed",
        Some("3"),
        set(vec![
            excess(breached(), "3000000", &expires_at),
            other("600000"),
        ]),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{response}");
}
