#[path = "support/access_fixture.rs"]
pub mod fixture;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;
use convex_server::access::*;
use fixture::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn next(socket: &mut Socket) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                Message::Close(_) => panic!("unexpected close"),
                _ => {}
            }
        }
    })
    .await
    .unwrap()
}
async fn until(socket: &mut Socket, kind: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value = next(socket).await;
            if value["type"] == kind {
                return value;
            }
        }
    })
    .await
    .unwrap()
}
async fn closed(socket: &mut Socket) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                None | Some(Ok(Message::Close(_))) | Some(Err(_)) => break,
                Some(Ok(Message::Text(text))) => assert_ne!(
                    serde_json::from_str::<Value>(&text).unwrap()["type"],
                    "valuation",
                    "data delivered after revocation"
                ),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
}
async fn connect(
    address: std::net::SocketAddr,
    path: &str,
    token: Option<&str>,
) -> Result<Socket, tokio_tungstenite::tungstenite::Error> {
    let mut request = format!("ws://{address}{path}")
        .into_client_request()
        .unwrap();
    if let Some(token) = token {
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
    }
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
}
async fn serve(
    engine: Arc<convex_engine::PricingEngine>,
    access: Arc<AccessControl>,
) -> (
    std::net::SocketAddr,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = convex_server::routes::create_router_with_access(engine.clone(), true, access);
    let (stop, rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
                engine.shutdown().await.unwrap();
            })
            .await
            .unwrap();
    });
    (address, stop, task)
}

#[tokio::test]
async fn websocket_aliases_subscription_scopes_delivery_and_quota_release_use_current_authority() {
    let issuer = IssuerFixture::start().await;
    let mut doc = policy();
    doc.grants[0].quotas.websocket_sessions = 1;
    doc.grants[0].quotas.subscriptions = 1;
    let access = AccessControl::new(issuer.verifier.clone(), doc.clone()).unwrap();
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let (address, stop, task) = serve(engine.clone(), access.clone()).await;
    for path in ["/ws", "/api/v1/ws"] {
        match connect(address, path, None).await {
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                assert_eq!(response.status().as_u16(), 401)
            }
            _ => panic!("anonymous WS accepted"),
        }
    }
    let mut socket = connect(address, "/api/v1/ws", Some(HUMAN_TOKEN))
        .await
        .unwrap();
    assert_eq!(next(&mut socket).await["type"], "connected");
    socket
        .send(Message::Binary(
            serde_json::to_vec(&json!({"type":"subscribe_bonds","instrument_ids":["BOND-002"]}))
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(until(&mut socket, "error").await["code"], "FORBIDDEN");
    match connect(address, "/ws", Some(HUMAN_TOKEN)).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status().as_u16(), 429)
        }
        _ => panic!("session quota bypassed"),
    }
    socket
        .send(Message::Text(
            json!({"type":"subscribe_all_bonds"}).to_string().into(),
        ))
        .await
        .unwrap();
    let error = until(&mut socket, "error").await;
    assert_eq!(error["code"], "FORBIDDEN");
    socket
        .send(Message::Text(
            json!({"type":"subscribe_valuations","instrument_ids":["BOND-001","BOND-002"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(until(&mut socket, "error").await["code"], "FORBIDDEN");
    socket
        .send(Message::Text(
            json!({"type":"subscribe_valuations","instrument_ids":["BOND-001"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(until(&mut socket, "subscribed").await["count"], 1);
    socket
        .send(Message::Text(
            json!({"type":"subscribe_bonds","instrument_ids":["BOND-001"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(
        until(&mut socket, "error").await["code"],
        "ACCESS_QUOTA_EXCEEDED"
    );
    // Normal authorized sessions survive multiple remote-refresh periods.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    engine
        .submit_update(
            support::event(2, 1, "98.25"),
            support::time(1),
            support::time(1),
        )
        .await
        .unwrap();
    let update = until(&mut socket, "valuation").await;
    assert_eq!(update["generation"], 2);
    assert_eq!(update["result"]["status"], "Complete");
    doc.version = 2;
    doc.grants[0].resources.remove(&ResourceKind::Instrument);
    access.replace_policy(1, doc.clone()).unwrap();
    engine
        .submit_update(
            support::event(3, 2, "98.00"),
            support::time(2),
            support::time(2),
        )
        .await
        .unwrap();
    closed(&mut socket).await;
    doc.version = 3;
    doc.grants[0]
        .resources
        .insert(ResourceKind::Instrument, vec!["BOND-001".into()]);
    access.replace_policy(2, doc).unwrap();
    let mut new_socket = connect(address, "/ws", Some(HUMAN_TOKEN)).await.unwrap();
    assert_eq!(next(&mut new_socket).await["type"], "connected");
    new_socket.close(None).await.unwrap();
    drop(new_socket);
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    issuer.finish().await;
}

#[tokio::test]
async fn quiet_sessions_expire_and_remote_revocation_closes_without_more_data() {
    let issuer = IssuerFixture::start().await;
    let access = AccessControl::new(issuer.verifier.clone(), policy()).unwrap();
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let (address, stop, task) = serve(engine.clone(), access).await;
    let mut expiry = claims("alice");
    expiry["exp"] = json!(now() + 2);
    issuer.change(HUMAN_TOKEN, expiry);
    let mut socket = connect(address, "/ws", Some(HUMAN_TOKEN)).await.unwrap();
    assert_eq!(next(&mut socket).await["type"], "connected");
    closed(&mut socket).await;
    issuer.change(HUMAN_TOKEN, claims("alice"));
    let mut socket = connect(address, "/ws", Some(HUMAN_TOKEN)).await.unwrap();
    assert_eq!(next(&mut socket).await["type"], "connected");
    socket
        .send(Message::Text(
            json!({"type":"subscribe_valuations","instrument_ids":["BOND-001"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    until(&mut socket, "subscribed").await;
    issuer.change(HUMAN_TOKEN, json!({"active":false}));
    tokio::time::sleep(Duration::from_millis(1100)).await;
    engine
        .submit_update(
            support::event(2, 1, "98.25"),
            support::time(1),
            support::time(1),
        )
        .await
        .unwrap();
    closed(&mut socket).await;
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    issuer.finish().await;
}

#[tokio::test]
async fn cors_preflight_origin_and_startup_tls_boundaries_cannot_bypass_access() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use convex_server::access::identity::{AccessMode, AccessSettings, TlsBoundary};
    use convex_server::{Server, ServerConfig, ServerProfile};
    use tower::ServiceExt;
    let issuer = IssuerFixture::start().await;
    let access = AccessControl::new(issuer.verifier.clone(), policy()).unwrap();
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    let settings = AccessSettings {
        mode: AccessMode::Required,
        policy_file: Some(
            issuer
                .directory
                .path()
                .join("policy-path-secret-marker")
                .to_str()
                .unwrap()
                .into(),
        ),
        introspection: Some(issuer.settings.clone()),
        cors_origins: vec!["https://console.example.invalid".into()],
        tls_boundary: TlsBoundary::LocalOnly,
    };
    let config = ServerConfig {
        profile: ServerProfile::Demo,
        host: "127.0.0.1".into(),
        access: settings.clone(),
        ..Default::default()
    };
    config.validate_startup().unwrap();
    let router =
        Server::with_access_control(config.clone(), engine.clone(), access.clone()).router();
    assert_eq!(
        request(router.clone(), "GET", "/api/v1/bonds/BOND-001", None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let calls = issuer.state.calls.load(std::sync::atomic::Ordering::SeqCst);
    for (origin, method, headers) in [
        ("https://evil.example.invalid", "POST", "authorization"),
        ("https://console.example.invalid", "PATCH", "authorization"),
        (
            "https://console.example.invalid",
            "POST",
            "x-forwarded-user",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/portfolios")
                    .header("origin", origin)
                    .header("access-control-request-method", method)
                    .header("access-control-request-headers", headers)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(response
            .headers()
            .get("access-control-allow-origin")
            .is_none());
    }
    assert_eq!(
        issuer.state.calls.load(std::sync::atomic::Ordering::SeqCst),
        calls
    );
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/v1/portfolios")
                .header("origin", "https://console.example.invalid")
                .header("access-control-request-method", "POST")
                .header(
                    "access-control-request-headers",
                    "authorization,content-type",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_eq!(
        response.headers()["access-control-allow-origin"],
        "https://console.example.invalid"
    );
    assert!(response
        .headers()
        .get("access-control-allow-credentials")
        .is_none());
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/portfolios")
                .header("origin", "https://console.example.invalid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let mut remote = config.clone();
    remote.host = "0.0.0.0".into();
    assert!(remote.validate_startup().is_err());
    let mut production = config.clone();
    production.profile = ServerProfile::Production;
    assert!(production.validate_startup().is_err());
    let mut invalid = settings.clone();
    invalid.tls_boundary = TlsBoundary::Proxy;
    invalid.cors_origins = vec!["http://127.0.0.1".into()];
    assert!(invalid.validate().is_err());
    let mut invalid = settings.clone();
    invalid.cors_origins = vec!["*".into()];
    assert!(invalid.validate().is_err());
    let mut invalid = issuer.settings.clone();
    invalid.allow_http_loopback = false;
    assert!(invalid.validate().is_err());
    let mut invalid = issuer.settings.clone();
    invalid.endpoint = "http://192.0.2.1/introspect".into();
    assert!(invalid.validate().is_err());
    let mut invalid = issuer.settings.clone();
    invalid.endpoint = "https://user:secret@example.invalid/introspect?token=secret".into();
    assert!(invalid.validate().is_err());
    let diagnostics = format!(
        "{config:?} {}",
        serde_json::to_string(&config.effective_redacted()).unwrap()
    );
    assert!(!diagnostics.contains("secret-marker"));
    assert!(!diagnostics.contains(&issuer.settings.endpoint));
    // Even serve_until cannot use an insecure mismatched configuration/control.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    assert!(Server::new(config, engine)
        .serve_until(listener, async {})
        .await
        .is_err());
    issuer.finish().await;
}
