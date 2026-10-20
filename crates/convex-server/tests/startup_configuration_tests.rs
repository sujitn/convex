//! CVX-002: isolated processes prove actual startup settings and profile behavior.

#[path = "../../convex-ports/tests/support/reference_terms.rs"]
mod reference_terms;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use convex_core::ids::InstrumentId;
use convex_core::{Currency, Date};
use convex_engine::ports::reference_data::{BondReferenceData, BondType, IssuerType};
use futures_util::{SinkExt, StreamExt};
use rust_decimal_macros::dec;
use tokio_tungstenite::tungstenite::Message;

struct Probe {
    child: Child,
    directory: PathBuf,
}

enum ConfigFile<'a> {
    Absent,
    Selected(&'a str),
    Default(&'a str),
    Missing,
}

impl Probe {
    fn spawn(settings: &[(&str, &str)], file: ConfigFile<'_>) -> Self {
        // Atomically allocate a directory; concurrent probes can share a clock tick.
        let directory = tempfile::tempdir().unwrap().keep();
        let log = std::fs::File::create(directory.join("startup.log")).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_convex-server"));
        command
            .current_dir(&directory)
            .env_clear()
            .env("RUST_LOG", "info")
            .env("CONVEX_HOST", "127.0.0.1")
            .env("CONVEX_PORT", "0")
            .env("CONVEX_STORAGE_PATH", directory.join("private-marker.redb"))
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log));
        match file {
            ConfigFile::Absent => {}
            ConfigFile::Missing => {
                command.arg(directory.join("absent.toml"));
            }
            ConfigFile::Selected(content) => {
                let path = directory.join("settings.toml");
                std::fs::write(&path, content).unwrap();
                command.arg(path);
            }
            ConfigFile::Default(content) => {
                std::fs::create_dir(directory.join("config")).unwrap();
                std::fs::write(directory.join("config/convex.toml"), content).unwrap();
            }
        }
        for (name, value) in settings {
            command.env(name, value);
        }
        Self {
            child: command.spawn().unwrap(),
            directory,
        }
    }

    fn output(&self) -> String {
        std::fs::read_to_string(self.directory.join("startup.log")).unwrap()
    }

    fn address(&mut self) -> SocketAddr {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let output = self.output();
            for line in output.lines() {
                if let Some((_, address)) = line.split_once("Starting Convex server on ") {
                    return address.trim().parse().unwrap();
                }
            }
            assert!(
                self.child.try_wait().unwrap().is_none() && Instant::now() < deadline,
                "server did not start: {output}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn rejected(&mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                let output = self.output();
                assert!(!status.success(), "invalid startup succeeded: {output}");
                assert!(!output.contains("Starting Convex server on"), "{output}");
                assert!(
                    !self.directory.join("private-marker.redb").exists(),
                    "storage opened before validation: {output}"
                );
                return output;
            }
            assert!(
                Instant::now() < deadline,
                "invalid startup stayed running: {}",
                self.output()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap()
}

#[tokio::test]
async fn environment_only_startup_uses_actual_socket_and_storage() {
    let mut probe = Probe::spawn(
        &[
            ("CONVEX_PROFILE", "demo"),
            ("CONVEX_ACCESS_MODE", "local_development"),
            ("CONVEX_CORS_ORIGINS", r#"["https://example.test"]"#),
        ],
        ConfigFile::Absent,
    );
    let address = probe.address();
    assert_eq!(
        address.ip(),
        "127.0.0.1".parse::<std::net::IpAddr>().unwrap()
    );
    assert_ne!(address.port(), 0);
    assert!(probe.directory.join("private-marker.redb").exists());
    assert!(!probe.directory.join("data").exists());
    let response = client()
        .get(format!("http://{address}/health"))
        .header("Origin", "https://example.test")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["x-convex-profile"], "demo");
    assert_eq!(response.headers()["x-convex-data-quality"], "demo");
    let exposed = response.headers()["access-control-expose-headers"]
        .to_str()
        .unwrap();
    assert!(exposed.contains("x-convex-profile") && exposed.contains("x-convex-data-quality"));
    let output = probe.output();
    assert!(
        output.contains("Effective startup configuration"),
        "{output}"
    );
    assert!(!output.contains("private-marker"), "{output}");
    assert!(
        !output.contains(probe.directory.to_str().unwrap()),
        "{output}"
    );
}

#[tokio::test]
async fn optional_default_file_is_loaded_and_oversized_files_fail() {
    let mut probe = Probe::spawn(
        &[],
        ConfigFile::Default(
            "profile='demo'\nwebsocket_enabled=false\n[access]\nmode='local_development'",
        ),
    );
    let address = probe.address();
    assert_eq!(
        client()
            .get(format!("http://{address}/api/v1/ws/status"))
            .send()
            .await
            .unwrap()
            .status(),
        403 // Disabled/unclassified routes remain closed at the access boundary.
    );
    let oversized = format!("profile='demo'\n#{}", "x".repeat(65536));
    let mut invalid = Probe::spawn(&[], ConfigFile::Default(&oversized));
    assert!(invalid.rejected().contains("CONFIG_FILE_TOO_LARGE"));
}

#[test]
fn invalid_settings_fail_before_storage_or_listening() {
    for (name, value, code) in [
        ("CONVEX_PORT", "99999", "INVALID_CONFIG_VALUE"),
        ("CONVEX_HOST", "localhost", "INVALID_CONFIG_VALUE"),
        ("CONVEX_WEBSOCKET_ENABLED", "maybe", "INVALID_CONFIG_VALUE"),
        ("CONVEX_WEBSOCKET_PORT", "65000", "UNSUPPORTED_CONFIG"),
        ("CONVEX_QUOTES_FILE", "private-marker", "UNSUPPORTED_CONFIG"),
        ("CONVEX_CURVES_FILE", "private-marker", "UNSUPPORTED_CONFIG"),
        (
            "CONVEX_FIXINGS_FILE",
            "private-marker",
            "UNSUPPORTED_CONFIG",
        ),
        ("CONVEX_BONDS_FILE", "private-marker", "UNSUPPORTED_CONFIG"),
        (
            "CONVEX_PASSWORD",
            "secret-marker",
            "UNKNOWN_CONFIG_ENVIRONMENT",
        ),
    ] {
        // A valid ephemeral file prevents the old loader opening a default port.
        let mut probe = Probe::spawn(
            &[(name, value)],
            ConfigFile::Selected(
                "profile = 'demo'\nhost = '127.0.0.1'\nport = 0\n[access]\nmode = 'local_development'\n",
            ),
        );
        let output = probe.rejected();
        assert!(output.contains(code), "{name}: {output}");
        assert!(
            !output.contains("private-marker") && !output.contains("secret-marker"),
            "{output}"
        );
    }
}

#[test]
fn invalid_files_and_explicit_missing_file_are_rejected() {
    for content in [
        "profile = 'demo'\nunknown_password = 'secret-marker'",
        "profile = 'demo'\nport = 'secret-marker'",
        "profile = 'demo'\nhost = 'secret-marker'",
    ] {
        let mut probe = Probe::spawn(&[], ConfigFile::Selected(content));
        let output = probe.rejected();
        assert!(output.contains("CONFIG"), "{output}");
        assert!(!output.contains("secret-marker"), "{output}");
    }
    let mut probe = Probe::spawn(&[("CONVEX_PROFILE", "demo")], ConfigFile::Missing);
    assert!(probe.rejected().contains("CONFIG_FILE_IO"));
}

#[test]
fn production_default_and_override_refuse_placeholder_providers() {
    for (settings, file) in [
        (vec![], ConfigFile::Absent),
        (vec![("CONVEX_PROFILE", "production")], ConfigFile::Absent),
        (
            vec![("CONVEX_PROFILE", "production")],
            ConfigFile::Selected("profile = 'demo'"),
        ),
    ] {
        let mut probe = Probe::spawn(&settings, file);
        let output = probe.rejected();
        assert!(
            output.contains("UNSUPPORTED_CONFIG") && output.contains("profile"),
            "{output}"
        );
    }
}

#[tokio::test]
async fn environment_overrides_file_and_disables_all_websocket_routes() {
    let mut probe = Probe::spawn(
        &[
            ("CONVEX_PROFILE", "demo"),
            ("CONVEX_WEBSOCKET_ENABLED", "false"),
            ("CONVEX_ACCESS_MODE", "local_development"),
        ],
        ConfigFile::Selected("profile = 'production'\nhost = '192.0.2.1'\nport = 61234\nwebsocket_enabled = true\nstorage_path = './unused.redb'\n"),
    );
    let address = probe.address();
    assert_eq!(
        address.ip(),
        "127.0.0.1".parse::<std::net::IpAddr>().unwrap()
    );
    assert_ne!(address.port(), 61234);
    assert!(probe.directory.join("private-marker.redb").exists());
    assert!(!probe.directory.join("unused.redb").exists());
    for path in ["/ws", "/api/v1/ws", "/api/v1/ws/status"] {
        let response = client()
            .get(format!("http://{address}{path}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403, "{path}");
        assert_eq!(response.headers()["x-convex-data-quality"], "demo");
        let denied: serde_json::Value = response.json().await.unwrap();
        assert_eq!(denied["code"], "FORBIDDEN", "{path}");
    }
}

#[tokio::test]
async fn explicit_demo_labels_websocket_handshake_and_messages() {
    let mut probe = Probe::spawn(
        &[],
        ConfigFile::Selected("profile = 'demo'\n[access]\nmode = 'local_development'"),
    );
    let address = probe.address();
    let (mut socket, response) = tokio::time::timeout(
        Duration::from_secs(2),
        tokio_tungstenite::connect_async(format!("ws://{address}/ws")),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.headers()["x-convex-profile"], "demo");
    assert_eq!(response.headers()["x-convex-data-quality"], "demo");
    socket
        .send(Message::Text(r#"{"type":"ping","timestamp":123}"#.into()))
        .await
        .unwrap();
    let mut connected = false;
    let mut pong = false;
    for _ in 0..4 {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(text) = message {
            let json: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(json["profile"], "demo");
            assert_eq!(json["data_quality"], "demo");
            connected |= json["type"] == "connected";
            pong |= json["type"] == "pong";
            if connected && pong {
                break;
            }
        }
    }
    assert!(connected && pong);
    socket
        .send(Message::Text(r#"{"type":"subscribe_all_bonds"}"#.into()))
        .await
        .unwrap();
    let mut subscribed = false;
    for _ in 0..4 {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(text) = message {
            let json: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(json["data_quality"], "demo");
            if json["type"] == "subscribed" {
                subscribed = true;
                break;
            }
        }
    }
    assert!(subscribed);
    let command = convex_ports::reference_repository::ReferenceCommand {
        idempotency_key: convex_core::contracts::RequestId::new("startup-reference-resources")
            .unwrap(),
        changes: vec![],
        master_changes: reference_terms::resources(&example_bond()),
    };
    let seed = client()
        .post(format!("http://{address}/api/v1/reference/transactions"))
        .header("idempotency-key", "startup-reference-resources")
        .json(&command)
        .send()
        .await
        .unwrap();
    assert_eq!(seed.status(), 200);
    let response = client().post(format!("http://{address}/api/v1/quote"))
        .json(&serde_json::json!({"bond": example_bond(), "settlement_date": "2025-06-17", "market_price": "99.5"}))
        .send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["x-convex-data-quality"], "demo");
    let quote: serde_json::Value = response.json().await.unwrap();
    let mut published = false;
    for _ in 0..4 {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(text) = message {
            let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(json["profile"], "demo");
            assert_eq!(json["data_quality"], "demo");
            if json["type"] == "bond_quote" {
                let fields = json.as_object_mut().unwrap();
                fields.remove("profile");
                fields.remove("data_quality");
                fields.remove("type");
                assert_eq!(json, quote);
                published = true;
                break;
            }
        }
    }
    assert!(published);
    socket.close(None).await.unwrap();
}

fn example_bond() -> BondReferenceData {
    reference_terms::complete(BondReferenceData {
        contractual_terms: None,
        instrument_id: InstrumentId::new("CONFIG-DEMO-TEST"),
        isin: None,
        cusip: Some("912810TD0".into()),
        sedol: None,
        bbgid: None,
        description: "Fictional startup label fixture".into(),
        currency: Currency::USD,
        issue_date: Date::from_ymd(2020, 1, 15).unwrap(),
        maturity_date: Date::from_ymd(2030, 6, 15).unwrap(),
        coupon_rate: Some(dec!(0.05)),
        frequency: 2,
        day_count: "30/360".into(),
        face_value: dec!(100),
        bond_type: BondType::FixedBullet,
        issuer_type: IssuerType::CorporateIG,
        issuer_id: "TEST".into(),
        issuer_name: "Test issuer".into(),
        seniority: "Senior".into(),
        is_callable: false,
        call_schedule: vec![],
        is_putable: false,
        is_sinkable: false,
        floating_terms: None,
        inflation_index: None,
        inflation_base_index: None,
        has_deflation_floor: false,
        country_of_risk: "US".into(),
        sector: "Financials".into(),
        amount_outstanding: None,
        first_coupon_date: None,
        last_updated: 0,
        source: "test".into(),
    })
}
