//! Requests for quote under required access, over a Live server: a source sends only for
//! the sources it is granted, a trader answers and reads only the books it is granted, a
//! closed request is refused to anyone else before its state is said, each answer records
//! its sender's audit fingerprint, and the `rfq` topic withholds what the list withholds.
#[allow(dead_code)] // This suite uses the issuer and policy subset.
#[path = "support/access_fixture.rs"]
mod fixture;
#[path = "support/live_server.rs"]
mod live_server;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use convex_server::access::identity::{AccessMode, AccessSettings, TlsBoundary};
use convex_server::access::*;
use live_server::{next_frame, send_frame, subscribe_rfqs};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

const ADMIN: &str = "desk-admin-token";
const VENUE: &str = "venue-token";
const TRADER_A: &str = "trader-a-token";
/// A reader of BOOK-A who may read the bond it holds, not the bond its requests are for.
const READER_B: &str = "reader-b-token";
/// A reader of BOOK-A who may read the bond its requests are for, not the bond it holds.
const READER_C: &str = "reader-c-token";
/// The bond BOOK-A holds; its requests are for BOND-001.
const HELD: &str = "BOND-W001";

/// Read `socket` until the server closes it.
async fn closed(socket: &mut live_server::Socket) {
    use futures_util::StreamExt;
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(message)) = socket.next().await {
            if matches!(message, tokio_tungstenite::tungstenite::Message::Close(_)) {
                return;
            }
        }
    })
    .await
    .expect("the session was not closed");
}

/// A reader of BOOK-A who may read only `instrument`.
fn reader(subject: &str, instrument: &str) -> Grant {
    let mut reader = grant(subject, vec![Role::Reader], &["BOOK-A"], "*");
    reader
        .resources
        .insert(ResourceKind::Instrument, vec![instrument.into()]);
    reader
}

fn grant(subject: &str, roles: Vec<Role>, books: &[&str], sources: &str) -> Grant {
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
            (
                ResourceKind::Book,
                books.iter().map(|book| (*book).to_owned()).collect(),
            ),
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
        (READER_B, "reader-b"),
        (READER_C, "reader-c"),
    ] {
        issuer.change(token, fixture::claims(subject));
    }
    let mut document = fixture::policy();
    document.grants = vec![
        grant(
            "desk-admin",
            vec![Role::Reader, Role::BookWriter],
            &["*"],
            "*",
        ),
        grant(
            "venue",
            vec![Role::RfqSource],
            &["BOOK-A", "BOOK-B"],
            "venue-sim",
        ),
        grant(
            "trader-a",
            vec![Role::Reader, Role::Trader],
            &["BOOK-A"],
            "*",
        ),
        reader("reader-b", HELD),
        reader("reader-c", "BOND-001"),
    ];
    let policy_file = issuer.directory.path().join("policy.json");
    std::fs::write(&policy_file, serde_json::to_vec(&document).unwrap()).unwrap();
    let (_dir, database, mut config, _) = live_server::wide_live_setup(2).await;
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
    for (book, held) in [("BOOK-A", HELD), ("BOOK-B", "BOND-001")] {
        let (status, reply) = send(
            &server,
            ADMIN,
            Method::PUT,
            &format!("/api/v1/books/{book}"),
            Some(json!({"currency": "USD", "positions": [
                {"instrument_id": held, "quantity": "1000000"},
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
        rfq("RFQ-Z", "venue-sim", "BOOK-Z"),
        json!(null),
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
            ("accepted", None),
            ("refused", Some("FORBIDDEN")),
            ("refused", Some("INVALID_INPUT")),
        ],
        "{reply}"
    );
    let key_a = reply["rfqs"][0]["rfq_key"].as_str().unwrap().to_owned();
    let key_b = reply["rfqs"][2]["rfq_key"].as_str().unwrap().to_owned();
    // A batch every request of which the caller may not send is refused whole.
    let (status, refusal) = send(
        &server,
        VENUE,
        Method::POST,
        "/api/v1/rfqs",
        Some(json!({"rfqs": [rfq("RFQ-Y", "venue-sim", "BOOK-Z")]})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refusal}");
    let path = |key: &str, action: &str| format!("/api/v1/rfqs/{key}/{action}");
    let quote = Some(json!({"clean_price": "99.5"}));

    // The topic: a trader follows only its books, and a reader who may not read a request's
    // bond, or a bond its book holds, gets in its place an error naming the book, as the
    // list withholds it.
    let mut trader_session = server.session(Some(TRADER_A)).await;
    let refused = subscribe_rfqs(&mut trader_session, &["BOOK-B"]).await;
    assert_eq!(refused.len(), 1);
    assert_eq!(
        (refused[0]["type"].as_str(), refused[0]["code"].as_str()),
        (Some("error"), Some("FORBIDDEN")),
        "{refused:?}"
    );
    let seed = subscribe_rfqs(&mut trader_session, &["BOOK-A"]).await;
    assert_eq!(seed.len(), 1, "{seed:?}");
    assert_eq!(
        (seed[0]["type"].as_str(), seed[0]["rfq_key"].as_str()),
        (Some("rfq"), Some(key_a.as_str()))
    );
    let mut readers = Vec::new();
    for token in [READER_B, READER_C] {
        let mut reader = server.session(Some(token)).await;
        let seed = subscribe_rfqs(&mut reader, &["BOOK-A"]).await;
        assert_eq!(seed.len(), 1, "{token}: {seed:?}");
        assert_eq!(
            (
                seed[0]["type"].as_str(),
                seed[0]["code"].as_str(),
                seed[0]["book_id"].as_str()
            ),
            (Some("rfq_withheld"), Some("FORBIDDEN"), Some("BOOK-A")),
            "{token}: {seed:?}"
        );
        let (status, list) = send(&server, token, Method::GET, "/api/v1/rfqs", None).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        // The list withholds the same request, and BOOK-B's.
        assert_eq!(
            (list["rfqs"].as_array().unwrap().len(), &list["withheld"]),
            (0, &json!(2)),
            "{token}"
        );
        readers.push(reader);
    }

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
    // The trader's session receives the quote; the reader's an error in its place, and
    // stays open.
    let quoted = next_frame(&mut trader_session).await;
    assert_eq!(
        (quoted["rfq_key"].as_str(), quoted["state"].as_str()),
        (Some(key_a.as_str()), Some("quoted")),
        "{quoted}"
    );
    for reader in &mut readers {
        let withheld = next_frame(reader).await;
        assert_eq!(
            (withheld["type"].as_str(), withheld["code"].as_str()),
            (Some("rfq_withheld"), Some("FORBIDDEN")),
            "{withheld}"
        );
        send_frame(reader, json!({"type": "ping", "timestamp": 1})).await;
        assert_eq!(next_frame(reader).await["type"], "pong");
    }

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
    // The receipt names the source's subject, the price the server, and the quote the
    // trader's subject.
    assert_eq!(users[1], &Value::Null, "the server's entry");
    let (source, trader) = (users[0].as_str().unwrap(), users[2].as_str().unwrap());
    assert!(
        source.starts_with("access:") && trader.starts_with("access:") && source != trader,
        "{source} {trader}"
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

    // A session whose grant no longer holds a book it follows is closed; the others stay.
    document.version += 1;
    document.grants[2] = grant(
        "trader-a",
        vec![Role::Reader, Role::Trader],
        &["BOOK-Z"],
        "*",
    );
    std::fs::write(&policy_file, serde_json::to_vec(&document).unwrap()).unwrap();
    closed(&mut trader_session).await;
    // The reader's withheld transitions of key A come first.
    send_frame(&mut readers[0], json!({"type": "ping", "timestamp": 2})).await;
    while next_frame(&mut readers[0]).await["type"] != "pong" {}
    server.stop().await;
    issuer.finish().await;
}
