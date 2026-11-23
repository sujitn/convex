//! Requests for quote under required access, over a Live server: a source sends only for
//! the sources it is granted, a trader answers and reads only the books it is granted, a
//! closed request is refused to anyone else before its state is said, and each answer
//! records its sender's audit fingerprint.
#[allow(dead_code)] // This suite uses the issuer and policy subset.
#[path = "support/access_fixture.rs"]
mod fixture;
#[path = "support/live_server.rs"]
mod live_server;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use convex_server::access::identity::{AccessMode, AccessSettings, TlsBoundary};
use convex_server::access::*;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

const ADMIN: &str = "desk-admin-token";
const VENUE: &str = "venue-token";
const TRADER_A: &str = "trader-a-token";

fn grant(subject: &str, roles: Vec<Role>, books: &str, sources: &str) -> Grant {
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
            (ResourceKind::Instrument, vec!["*".into()]),
            (ResourceKind::Book, vec![books.into()]),
            (ResourceKind::Source, vec![sources.into()]),
            (ResourceKind::Curve, vec!["*".into()]),
        ]),
        token_scopes: vec!["convex.api".into()],
        valid_until: fixture::now() + 3600,
        quotas: Quotas::default(),
    }
}

async fn send(
    server: &live_server::Running,
    token: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = server
        .client
        .request(method, format!("{}{path}", server.base))
        .bearer_auth(token)
        .header("idempotency-key", format!("key{}", path.replace('/', "-")));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

fn rfq(rfq_id: &str, source_id: &str, book_id: &str) -> Value {
    json!({
        "source_id": source_id, "rfq_id": rfq_id, "client_id": "CLIENT-17",
        "client_tier": "2", "instrument_id": "BOND-001", "side": "buy",
        "face": "1000000",
        "settlement_date": (chrono::Utc::now() + chrono::Duration::days(2)).date_naive().to_string(),
        "book_id": book_id,
        "deadline": (chrono::Utc::now() + chrono::Duration::seconds(60))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    })
}

#[tokio::test]
async fn sources_and_traders_reach_only_what_they_are_granted() {
    let issuer = fixture::IssuerFixture::start().await;
    for (token, subject) in [
        (ADMIN, "desk-admin"),
        (VENUE, "venue"),
        (TRADER_A, "trader-a"),
    ] {
        issuer.change(token, fixture::claims(subject));
    }
    let mut document = fixture::policy();
    document.grants = vec![
        grant("desk-admin", vec![Role::Reader, Role::BookWriter], "*", "*"),
        grant("venue", vec![Role::RfqSource], "*", "venue-sim"),
        grant("trader-a", vec![Role::Reader, Role::Trader], "BOOK-A", "*"),
    ];
    let policy_file = issuer.directory.path().join("policy.json");
    std::fs::write(&policy_file, serde_json::to_vec(&document).unwrap()).unwrap();
    let (_dir, database, mut config) = live_server::live_setup().await;
    live_server::activate_client_tiers(
        &database,
        json!([{"client_tier": "2", "width_factor": "1"}]),
    )
    .await;
    config.access = AccessSettings {
        mode: AccessMode::Required,
        policy_file: Some(policy_file.to_str().unwrap().into()),
        introspection: Some(issuer.settings.clone()),
        cors_origins: vec![],
        tls_boundary: TlsBoundary::LocalOnly,
    };
    let server = live_server::Running::start(config).await;
    for book in ["BOOK-A", "BOOK-B"] {
        let (status, reply) = send(
            &server,
            ADMIN,
            Method::PUT,
            &format!("/api/v1/books/{book}"),
            Some(json!({"currency": "USD", "positions": [
                {"instrument_id": "BOND-001", "quantity": "1000000"},
            ]})),
        )
        .await;
        assert!(status.is_success(), "{book}: {status} {reply}");
    }

    // The desk takes requests once its start's expiries are recorded.
    let batch = json!({"rfqs": [
        rfq("RFQ-A", "venue-sim", "BOOK-A"),
        rfq("RFQ-X", "venue-other", "BOOK-A"),
        rfq("RFQ-B", "venue-sim", "BOOK-B"),
    ]});
    let reply = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (status, reply) = send(
                &server,
                VENUE,
                Method::POST,
                "/api/v1/rfqs",
                Some(batch.clone()),
            )
            .await;
            if status != StatusCode::SERVICE_UNAVAILABLE {
                assert_eq!(status, StatusCode::OK, "{reply}");
                return reply;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let results: Vec<(&str, Option<&str>)> = reply["rfqs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|answer| (answer["result"].as_str().unwrap(), answer["code"].as_str()))
        .collect();
    assert_eq!(
        results,
        [
            ("accepted", None),
            ("refused", Some("FORBIDDEN")),
            ("accepted", None)
        ],
        "{reply}"
    );
    let key_a = reply["rfqs"][0]["rfq_key"].as_str().unwrap().to_owned();
    let key_b = reply["rfqs"][2]["rfq_key"].as_str().unwrap().to_owned();
    let path = |key: &str, action: &str| format!("/api/v1/rfqs/{key}/{action}");
    let quote = Some(json!({"clean_price": "99.5"}));

    // A trader answers only on its books, and a source sends no answer.
    let (status, _) = send(
        &server,
        TRADER_A,
        Method::POST,
        &path(&key_b, "quote"),
        quote.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        &server,
        VENUE,
        Method::POST,
        &path(&key_a, "quote"),
        quote.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, reply) = send(
        &server,
        TRADER_A,
        Method::POST,
        &path(&key_a, "quote"),
        quote,
    )
    .await;
    assert_eq!(
        (status, reply["state"].as_str()),
        (StatusCode::OK, Some("quoted"))
    );

    // The list leaves out what the trader cannot read, and counts it.
    let (status, list) = send(&server, TRADER_A, Method::GET, "/api/v1/rfqs", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["withheld"], 1, "{list}");
    let shown: Vec<&str> = list["rfqs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["rfq_key"].as_str().unwrap())
        .collect();
    assert_eq!(shown, [key_a.as_str()]);
    let (status, _) = send(
        &server,
        TRADER_A,
        Method::GET,
        &format!("/api/v1/rfqs/{key_b}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, entries) = send(
        &server,
        TRADER_A,
        Method::GET,
        &format!("/api/v1/rfqs/{key_a}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{entries}");
    let users: Vec<&Value> = entries["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| &entry["user"])
        .collect();
    assert_eq!(
        users[..2],
        [&Value::Null, &Value::Null],
        "the server's entries"
    );
    let subject = users[2].as_str().unwrap();
    assert!(
        subject.starts_with("access:") && !subject.contains("trader"),
        "{subject}"
    );

    // The outcome is the source's; a closed request is refused to anyone else before its
    // state is said.
    let done = Some(json!({"outcome": "done"}));
    let (status, _) = send(
        &server,
        TRADER_A,
        Method::POST,
        &path(&key_a, "outcome"),
        done.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, reply) = send(&server, VENUE, Method::POST, &path(&key_a, "outcome"), done).await;
    assert_eq!(
        (status, reply["state"].as_str()),
        (StatusCode::OK, Some("done"))
    );
    let (status, reply) = send(
        &server,
        VENUE,
        Method::POST,
        &path(&key_b, "outcome"),
        Some(json!({"outcome": "cancelled"})),
    )
    .await;
    assert_eq!(
        (status, reply["state"].as_str()),
        (StatusCode::OK, Some("cancelled"))
    );
    let (status, _) = send(&server, TRADER_A, Method::POST, &path(&key_b, "pass"), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "not RFQ_FINAL");
    let (status, reply) = send(&server, TRADER_A, Method::POST, &path(&key_a, "pass"), None).await;
    assert_eq!(
        (status, reply["code"].as_str()),
        (StatusCode::CONFLICT, Some("RFQ_FINAL"))
    );

    // A body over 128 KiB is refused whole.
    let padded = json!({"rfqs": [rfq(&"R".repeat(140 * 1024), "venue-sim", "BOOK-A")]});
    let (status, reply) = send(&server, VENUE, Method::POST, "/api/v1/rfqs", Some(padded)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{reply}");
    assert_eq!(reply["code"], "RFQ_BODY_TOO_LARGE");
    server.stop().await;
    issuer.finish().await;
}
