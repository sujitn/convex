use axum::{
    extract::{Form, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use convex_server::access::{
    identity::{IntrospectionSettings, IntrospectionVerifier},
    *,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, RwLock,
    },
};
pub const HUMAN_TOKEN: &str = "human-fixture-secret-marker";
pub const SERVICE_TOKEN: &str = "service-fixture-secret-marker";
pub const ISSUER: &str = "https://identity.example.invalid/issuer";
pub const AUDIENCE: &str = "convex-api";
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
pub fn claims(subject: &str) -> Value {
    json!({"active":true,"sub":subject,"iss":ISSUER,"aud":AUDIENCE,"exp":now()+3600,"iat":now()-1,"nbf":now()-1,"scope":"convex.api","token_type":"Bearer"})
}
#[derive(Clone)]
pub struct IssuerState {
    pub tokens: Arc<RwLock<BTreeMap<String, Value>>>,
    pub unavailable: Arc<AtomicBool>,
    pub calls: Arc<AtomicUsize>,
    pub bad_authorization: Arc<AtomicUsize>,
    pub expected_basic: Arc<RwLock<String>>,
}
pub struct IssuerFixture {
    pub state: IssuerState,
    pub verifier: Arc<IntrospectionVerifier>,
    pub settings: IntrospectionSettings,
    pub directory: tempfile::TempDir,
    pub stop: Option<tokio::sync::oneshot::Sender<()>>,
    pub task: tokio::task::JoinHandle<()>,
}
async fn introspect(
    State(state): State<IssuerState>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    state.calls.fetch_add(1, Ordering::SeqCst);
    if headers.get("authorization").and_then(|h| h.to_str().ok())
        != Some(state.expected_basic.read().unwrap().as_str())
    {
        state.bad_authorization.fetch_add(1, Ordering::SeqCst);
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"private-remote-secret-marker"})),
        );
    }
    if state.unavailable.load(Ordering::SeqCst) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"private-remote-secret-marker"})),
        );
    }
    assert_eq!(
        form.get("token_type_hint").map(String::as_str),
        Some("access_token")
    );
    let response = state
        .tokens
        .read()
        .unwrap()
        .get(form.get("token").unwrap())
        .cloned()
        .unwrap_or(json!({"active":false}));
    (StatusCode::OK, Json(response))
}
impl IssuerFixture {
    pub async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let secret_file = directory.path().join("credential-private-path-marker");
        std::fs::write(&secret_file, "test-client-secret-marker").unwrap();
        let state = IssuerState {
            tokens: Arc::new(RwLock::new(BTreeMap::from([
                (HUMAN_TOKEN.into(), claims("alice")),
                (SERVICE_TOKEN.into(), claims("worker")),
            ]))),
            unavailable: Arc::new(AtomicBool::new(false)),
            calls: Arc::new(AtomicUsize::new(0)),
            bad_authorization: Arc::new(AtomicUsize::new(0)),
            expected_basic: Arc::new(RwLock::new(
                "Basic Y29udmV4LXRlc3Q6dGVzdC1jbGllbnQtc2VjcmV0LW1hcmtlcg==".into(),
            )),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new()
            .route("/introspect", post(introspect))
            .with_state(state.clone());
        let (stop, rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await
                .unwrap();
        });
        let settings = IntrospectionSettings {
            endpoint: format!("http://{address}/introspect"),
            issuer: ISSUER.into(),
            audience: AUDIENCE.into(),
            client_id: "convex-test".into(),
            client_secret_ref: format!("file://{}", secret_file.display()),
            allow_http_loopback: true,
        };
        let verifier = IntrospectionVerifier::new(settings.clone()).unwrap();
        Self {
            state,
            verifier,
            settings,
            directory,
            stop: Some(stop),
            task,
        }
    }
    pub async fn finish(mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        self.task.await.unwrap();
    }
    pub fn change(&self, token: &str, value: Value) {
        self.state
            .tokens
            .write()
            .unwrap()
            .insert(token.into(), value);
    }
}
pub fn policy() -> PolicyDocument {
    let resources = BTreeMap::from([
        (ResourceKind::Instrument, vec!["BOND-001".into()]),
        (ResourceKind::Curve, vec!["*".into()]),
        (ResourceKind::Source, vec!["*".into()]),
        (ResourceKind::Book, vec!["BOOK-A".into()]),
        (ResourceKind::Etf, vec!["ETF-A".into()]),
        (ResourceKind::Configuration, vec!["CONFIG-RESTORE".into()]),
    ]);
    let grant = Grant {
        subject: "alice".into(),
        kind: IdentityKind::Human,
        roles: vec![
            Role::Reader,
            Role::Analyst,
            Role::ReferenceWriter,
            Role::BookWriter,
            Role::Operator,
        ],
        data: vec![
            DataScope::Reference,
            DataScope::Market,
            DataScope::Results,
            DataScope::Books,
            DataScope::Configuration,
            DataScope::Operations,
        ],
        resources,
        token_scopes: vec!["convex.api".into()],
        valid_until: now() + 3600,
        quotas: Quotas::default(),
    };
    let mut service = grant.clone();
    service.subject = "worker".into();
    service.kind = IdentityKind::Service;
    service.roles = vec![Role::Reader];
    PolicyDocument {
        schema_version: 1,
        version: 1,
        issuer: ISSUER.into(),
        audience: AUDIENCE.into(),
        valid_until: now() + 7200,
        actor: "test-operator".into(),
        reason: "fictional access fixture".into(),
        grants: vec![grant, service],
    }
}
pub async fn request(
    router: axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    payload: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    let response = router
        .oneshot(
            req.body(
                payload
                    .map(|v| axum::body::Body::from(serde_json::to_vec(&v).unwrap()))
                    .unwrap_or_else(axum::body::Body::empty),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        headers,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        },
    )
}
