//! Book limits over the real router (P14b): limit sets with compare-and-set and idempotent
//! retries, two subjects for every change under required access (D85), the bound on
//! limited books (D87), the generic reference routes' refusal, and status on request at
//! and just past each bound and band, with each reason the server reaches without results.
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
