//! Supplied analytics use the native wire contract and the existing access boundary.
#[path = "support/access_fixture.rs"]
pub mod fixture;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use axum::{body::Body, http::Request, http::StatusCode, Router};
use convex_server::access::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

async fn app(access: Arc<AccessControl>) -> (support::Setup, Router) {
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    let router = convex_server::routes::create_router_with_access(engine, false, access);
    (s, router)
}

fn price() -> Value {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/yield-pricing.json"
    ))
    .unwrap();
    cases["cases"][0]["request"].clone()
}

fn causal_book() -> Value {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/causal-pnl-v12-api-01.json"
    ))
    .unwrap();
    cases["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "causal.full.mixed")
        .unwrap()["request"]
        .clone()
}

fn market_only_book() -> Value {
    use convex::library::{CausalPnlCounterfactual, LibraryOperation, LibraryRequest};
    let mut request: LibraryRequest = serde_json::from_value(causal_book()).unwrap();
    let LibraryOperation::BookPnl { input } = &mut request.operation else {
        panic!("retained case must be a causal book");
    };
    let input = input.as_mut();
    input.steps.truncate(1);
    input.standalone.truncate(1);
    let mut bindings: Vec<_> = std::iter::once(&mut input.before)
        .chain(input.steps.iter_mut().map(|s| &mut s.after))
        .chain(input.standalone.iter_mut().filter_map(|s| match s {
            CausalPnlCounterfactual::Available { information } => Some(information.as_mut()),
            _ => None,
        }))
        .collect();
    // Retain complete original bundles used by this one original phase. This
    // changes no product terms, financial answer or bound in the retained pack.
    let used: std::collections::BTreeSet<_> = bindings
        .iter()
        .flat_map(|b| b.holdings.iter().copied())
        .collect();
    let remap: std::collections::BTreeMap<_, _> = used
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, new))
        .collect();
    input.holdings = used.iter().map(|i| input.holdings[*i].clone()).collect();
    for binding in &mut bindings {
        binding.holdings = binding.holdings.iter().map(|i| remap[i]).collect();
    }
    serde_json::to_value(request).unwrap()
}

fn nodes(value: &Value) -> usize {
    1 + match value {
        Value::Array(values) => values.iter().map(nodes).sum(),
        Value::Object(values) => values.values().map(nodes).sum(),
        _ => 0,
    }
}

async fn raw(router: Router, input: Vec<u8>) -> (StatusCode, Vec<u8>) {
    let result = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/analytics")
                .header("content-type", "application/json")
                .body(Body::from(input))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = result.status();
    let body = result
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, body)
}

#[tokio::test]
async fn supplied_prices_and_errors_are_byte_identical_to_native_dispatch() {
    let (_s, router) = app(AccessControl::local_development()).await;
    let mut unsupported_version = price();
    unsupported_version["schema_version"] = json!(999);
    let mut extra_term = price();
    extra_term["operation"]["ignored_term"] = json!(1);
    let mut invalid_cash = price();
    invalid_cash["operation"]["action"]["cash"] = json!([]);
    for input in [price(), unsupported_version, extra_term, invalid_cash] {
        let input = serde_json::to_string(&input).unwrap();
        let native = convex::library::execute_json(&input);
        let (status, body) = raw(router.clone(), input.into_bytes()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, native.as_bytes());
    }
    let duplicate = br#"{"schema_version":1,"schema_version":2}"#.to_vec();
    let native = convex::library::execute_json(std::str::from_utf8(&duplicate).unwrap());
    assert_eq!(raw(router.clone(), duplicate).await.1, native.as_bytes());
    assert_eq!(
        raw(router.clone(), vec![0xff]).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        raw(router, vec![b' '; 2 * 1024 * 1024 + 1]).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn large_native_reports_respect_the_http_response_limit() {
    let (_s, router) = app(AccessControl::local_development()).await;
    let (status, body) = raw(router, serde_json::to_vec(&causal_book()).unwrap()).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"code":"ANALYTICS_RESPONSE_TOO_LARGE"})
    );
}

#[tokio::test]
async fn supplied_analytics_require_explicit_calculate_data_and_wildcard_grants() {
    use fixture::*;
    let (_s, denied) = app(AccessControl::deny_all()).await;
    assert_eq!(
        raw(denied, serde_json::to_vec(&price()).unwrap()).await.0,
        StatusCode::UNAUTHORIZED
    );

    let issuer = IssuerFixture::start().await;
    let access = AccessControl::new(issuer.verifier.clone(), policy()).unwrap();
    let (_s, router) = app(access.clone()).await;
    assert_eq!(
        request(
            router.clone(),
            "POST",
            "/api/v1/analytics",
            Some(HUMAN_TOKEN),
            Some(price())
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut broad = policy();
    for grant in &mut broad.grants {
        for kind in [
            ResourceKind::Instrument,
            ResourceKind::Curve,
            ResourceKind::Source,
            ResourceKind::Book,
        ] {
            grant.resources.insert(kind, vec!["*".into()]);
        }
    }
    broad.grants[0].roles = vec![Role::Analyst];
    let mut revision = 1;
    let install = |mut document: PolicyDocument, revision: &mut u64| {
        let previous = *revision;
        *revision += 1;
        document.version = *revision;
        access.replace_policy(previous, document).unwrap();
    };
    install(broad.clone(), &mut revision);
    let (status, _, actual) = request(
        router.clone(),
        "POST",
        "/api/v1/analytics",
        Some(HUMAN_TOKEN),
        Some(price()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let native: Value =
        serde_json::from_str(&convex::library::execute_json(&price().to_string())).unwrap();
    assert_eq!(actual, native);
    assert_eq!(actual["ok"], "true");
    let book = market_only_book();
    let native: Value =
        serde_json::from_str(&convex::library::execute_json(&book.to_string())).unwrap();
    assert_eq!(native["ok"], "true");
    assert!(
        nodes(&native) > 65_536,
        "exercise a full native report beyond partial scope discovery"
    );
    let (status, _, actual) = request(
        router.clone(),
        "POST",
        "/api/v1/analytics",
        Some(HUMAN_TOKEN),
        Some(book),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(actual, native);
    assert_eq!(
        request(
            router.clone(),
            "POST",
            "/api/v1/analytics",
            Some(SERVICE_TOKEN),
            Some(price())
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    for kind in [
        ResourceKind::Instrument,
        ResourceKind::Curve,
        ResourceKind::Source,
        ResourceKind::Book,
    ] {
        let mut restricted = broad.clone();
        restricted.grants[0]
            .resources
            .insert(kind, vec!["ONE-NAMED-RESOURCE".into()]);
        install(restricted, &mut revision);
        assert_eq!(
            request(
                router.clone(),
                "POST",
                "/api/v1/analytics",
                Some(HUMAN_TOKEN),
                Some(price())
            )
            .await
            .0,
            StatusCode::FORBIDDEN,
            "missing wildcard {kind:?}"
        );
    }
    for scope in [
        DataScope::Reference,
        DataScope::Market,
        DataScope::Books,
        DataScope::Results,
    ] {
        let mut restricted = broad.clone();
        restricted.grants[0].data.retain(|s| *s != scope);
        install(restricted, &mut revision);
        assert_eq!(
            request(
                router.clone(),
                "POST",
                "/api/v1/analytics",
                Some(HUMAN_TOKEN),
                Some(price())
            )
            .await
            .0,
            StatusCode::FORBIDDEN,
            "missing data {scope:?}"
        );
    }
    let mut reader = broad.clone();
    reader.grants[0].roles = vec![Role::Reader];
    install(reader, &mut revision);
    assert_eq!(
        request(
            router.clone(),
            "POST",
            "/api/v1/analytics",
            Some(HUMAN_TOKEN),
            Some(price())
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    issuer.finish().await;
}
