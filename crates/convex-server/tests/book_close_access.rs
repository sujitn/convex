//! Desk closes under required access, over a Live server: a close needs the trader's
//! `BookClose` on the book and each instrument it holds, records the trader's audit
//! fingerprint, and a close, P&L since it or the book's summary is read under status's
//! read rule.
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
const TRADER: &str = "trader-token";
/// A trader of BOOK-A who may not read the bond it holds.
const TRADER_W: &str = "trader-w-token";
const READER: &str = "reader-token";

fn grant(subject: &str, roles: Vec<Role>, books: &[&str], instrument: &str) -> Grant {
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
            (
                ResourceKind::Book,
                books.iter().map(|book| (*book).to_owned()).collect(),
            ),
            (ResourceKind::Source, vec!["*".into()]),
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
    key: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = server
        .client
        .request(method, format!("{}{path}", server.base))
        .bearer_auth(token)
        .header("idempotency-key", key);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn close(
    server: &live_server::Running,
    token: &str,
    book: &str,
    key: &str,
) -> (StatusCode, Value) {
    let path = format!("/api/v1/books/{book}/close");
    send(server, token, Method::POST, &path, key, None).await
}

#[tokio::test]
async fn a_close_needs_the_traders_grant_on_the_book_and_its_bonds() {
    let issuer = fixture::IssuerFixture::start().await;
    for (token, subject) in [
        (ADMIN, "desk-admin"),
        (TRADER, "trader"),
        (TRADER_W, "trader-w"),
        (READER, "reader"),
    ] {
        issuer.change(token, fixture::claims(subject));
    }
    let mut document = fixture::policy();
    document.grants = vec![
        grant(
            "desk-admin",
            vec![Role::Reader, Role::BookWriter, Role::ReferenceWriter],
            &["*"],
            "*",
        ),
        grant("trader", vec![Role::Reader, Role::Trader], &["BOOK-A"], "*"),
        grant(
            "trader-w",
            vec![Role::Reader, Role::Trader],
            &["BOOK-A"],
            "BOND-W001",
        ),
        grant("reader", vec![Role::Reader], &["BOOK-A"], "*"),
    ];
    let policy_file = issuer.directory.path().join("policy.json");
    std::fs::write(&policy_file, serde_json::to_vec(&document).unwrap()).unwrap();
    let (_dir, _, mut config, keys) = live_server::wide_live_setup(2).await;
    config.access = AccessSettings {
        mode: AccessMode::Required,
        policy_file: Some(policy_file.to_str().unwrap().into()),
        introspection: Some(issuer.settings.clone()),
        cors_origins: vec![],
        tls_boundary: TlsBoundary::LocalOnly,
    };
    let server = live_server::Running::start(config).await;
    let events = live_server::images(&keys, 1, "99.25");
    let (status, reply) = send(
        &server,
        ADMIN,
        Method::POST,
        "/api/v1/market/quotes",
        "quotes-1",
        Some(json!({"events": events})),
    )
    .await;
    assert!(status.is_success(), "{reply}");
    for book in ["BOOK-A", "BOOK-B"] {
        let (status, reply) = send(
            &server,
            ADMIN,
            Method::PUT,
            &format!("/api/v1/books/{book}"),
            &format!("put-{book}"),
            Some(json!({"currency": "USD", "positions": [
                {"instrument_id": "BOND-001", "quantity": "1000000"},
            ]})),
        )
        .await;
        assert!(status.is_success(), "{book}: {status} {reply}");
    }

    // A reader may not close; a trader only its book, and only what it may read.
    for (token, book, key) in [
        (READER, "BOOK-A", "close-reader"),
        (TRADER, "BOOK-B", "close-other-book"),
        (TRADER_W, "BOOK-A", "close-unread-bond"),
    ] {
        let (status, reply) = close(&server, token, book, key).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{token} {book}: {reply}");
    }
    // The runtime values the bond once it takes the posted image.
    let taken = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (status, reply) = close(&server, TRADER, "BOOK-A", "close-trader").await;
            if status == StatusCode::CREATED {
                return reply;
            }
            assert_eq!(reply["code"], "BOOK_CLOSE_UNVALUED", "{status} {reply}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let subject = taken["taken_by"]["subject"].as_str().unwrap();
    assert!(subject.starts_with("access:"), "{taken}");
    assert_eq!(taken["taken_by"]["kind"], "desk");

    for (token, path, expected) in [
        (READER, "/api/v1/books/BOOK-A/close", StatusCode::OK),
        (READER, "/api/v1/books/BOOK-A/pnl", StatusCode::OK),
        (
            TRADER_W,
            "/api/v1/books/BOOK-A/close",
            StatusCode::FORBIDDEN,
        ),
        (TRADER_W, "/api/v1/books/BOOK-A/pnl", StatusCode::FORBIDDEN),
        (READER, "/api/v1/books/BOOK-B/pnl", StatusCode::FORBIDDEN),
    ] {
        let (status, reply) = send(&server, token, Method::GET, path, "read", None).await;
        assert_eq!(status, expected, "{token} {path}: {reply}");
    }

    // A summary is read as P&L is: the reader gets BOOK-A's, which its close watches; the
    // trader who may not read the bond gets why in its place; no one follows a book it may
    // not read.
    let mut reader = server.session(Some(READER)).await;
    let seed = live_server::subscribe_books(&mut reader, &["BOOK-A"]).await;
    let summary = match seed.first() {
        Some(frame) => frame.clone(),
        None => live_server::next_frame(&mut reader).await,
    };
    assert_eq!(summary["type"], "book_summary", "{summary}");
    assert_eq!(summary["book_id"], "BOOK-A", "{summary}");
    let mut unread = server.session(Some(TRADER_W)).await;
    let seed = live_server::subscribe_books(&mut unread, &["BOOK-A"]).await;
    assert_eq!(seed.len(), 1, "{seed:?}");
    assert_eq!(seed[0]["code"], "FORBIDDEN", "{seed:?}");
    let refused = live_server::subscribe_books(&mut reader, &["BOOK-B"]).await;
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0]["code"], "FORBIDDEN", "{refused:?}");
    server.stop().await;
}
