#![cfg(unix)]
#[path = "support/access_fixture.rs"]
pub mod fixture;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;
use convex_server::{
    access::identity::{AccessMode, AccessSettings, TlsBoundary},
    ServerConfig, ServerProfile,
};
use fixture::*;
use std::{
    process::{Command, Stdio},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::test]
async fn binary_requires_identity_composition_reloads_grants_and_preserves_runtime_during_outage() {
    let issuer = IssuerFixture::start().await;
    let s = support::setup().await;
    let directory = s.dir;
    let database = directory.path().join("inputs.redb");
    let mut plan = s.plan.clone();
    plan.mode = convex_engine::RuntimeMode::Replay;
    plan.seed_events.clear();
    plan.build_id = convex_core::contracts::ModelId::new(convex_server::SERVER_BUILD_ID).unwrap();
    let plan_file = directory.path().join("runtime.json");
    let quotes = directory.path().join("quotes.jsonl");
    let policy_file = directory.path().join("access.json");
    let config_file = directory.path().join("server.toml");
    std::fs::write(&plan_file, serde_json::to_vec(&plan).unwrap()).unwrap();
    std::fs::write(
        &quotes,
        serde_json::to_string(&support::event(1, 0, "99.25")).unwrap() + "\n",
    )
    .unwrap();
    let mut policy = policy();
    std::fs::write(&policy_file, serde_json::to_vec(&policy).unwrap()).unwrap();
    let config = ServerConfig {
        profile: ServerProfile::Replay,
        host: "127.0.0.1".into(),
        port: 0,
        storage_path: database.to_str().unwrap().into(),
        runtime_plan: Some(plan_file.to_str().unwrap().into()),
        quotes_file: Some(quotes.to_str().unwrap().into()),
        access: AccessSettings {
            mode: AccessMode::Required,
            policy_file: Some(policy_file.to_str().unwrap().into()),
            introspection: Some(issuer.settings.clone()),
            cors_origins: vec![],
            tls_boundary: TlsBoundary::LocalOnly,
        },
        ..Default::default()
    };
    std::fs::write(&config_file, toml::to_string(&config).unwrap()).unwrap();
    drop(s.storage);
    drop(s.store);
    drop(s.snapshots);
    drop(s.results);
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_convex-server"));
    for (name, _) in
        std::env::vars_os().filter(|(name, _)| name.to_string_lossy().starts_with("CONVEX_"))
    {
        command.env_remove(name);
    }
    command
        .arg(&config_file)
        .current_dir(directory.path())
        .env("RUST_LOG", "info")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut startup = String::new();
    let address = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let line = lines
                .next_line()
                .await
                .unwrap()
                .expect("server exited before binding");
            startup.push_str(&line);
            startup.push('\n');
            if let Some((_, address)) = line.split_once("Starting Convex server on ") {
                break address.trim().to_owned();
            }
        }
    })
    .await
    .unwrap();
    let reader = tokio::spawn(async move {
        let mut logs = String::new();
        while let Some(line) = lines.next_line().await.unwrap() {
            logs.push_str(&line);
            logs.push('\n');
        }
        logs
    });
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("http://{address}/api/v1/runtime"))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        401
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if client
                .get(format!("http://{address}/ready"))
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let original: serde_json::Value = client
        .get(format!("http://{address}/api/v1/runtime"))
        .bearer_auth(HUMAN_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(original["ready"], true);
    assert_eq!(
        client
            .get(format!("http://{address}/api/v1/bonds/BOND-001"))
            .bearer_auth(HUMAN_TOKEN)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        200
    );
    policy.version = 2;
    policy.grants[0]
        .resources
        .remove(&convex_server::access::ResourceKind::Instrument);
    // Operator replacement is atomic; transient incomplete writes must never retain old authority.
    let next = directory.path().join("access.next.json");
    std::fs::write(&next, serde_json::to_vec(&policy).unwrap()).unwrap();
    std::fs::rename(&next, &policy_file).unwrap();
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if client
                .get(format!("http://{address}/api/v1/bonds/BOND-001"))
                .bearer_auth(HUMAN_TOKEN)
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
                == 403
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    std::fs::remove_file(&policy_file).unwrap();
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if client
                .get(format!("http://{address}/ready"))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
                == 503
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        client
            .get(format!("http://{address}/live"))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        200
    );
    assert_eq!(
        client
            .get(format!("http://{address}/api/v1/runtime"))
            .bearer_auth(HUMAN_TOKEN)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        503
    );
    policy.version = 3;
    policy.grants[0].resources.insert(
        convex_server::access::ResourceKind::Instrument,
        vec!["BOND-001".into()],
    );
    std::fs::write(&next, serde_json::to_vec(&policy).unwrap()).unwrap();
    std::fs::rename(&next, &policy_file).unwrap();
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if client
                .get(format!("http://{address}/ready"))
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let restored: serde_json::Value = client
        .get(format!("http://{address}/api/v1/runtime"))
        .bearer_auth(HUMAN_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(original, restored);
    let strategy: serde_json::Value = client
        .get(format!("http://{address}/strategy-ready"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(strategy["ready"], false);
    assert!(Command::new("kill")
        .args(["-TERM", &child.id().unwrap().to_string()])
        .status()
        .unwrap()
        .success());
    assert!(tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
    startup.push_str(&reader.await.unwrap());
    assert!(startup.contains("POLICY_ACTIVATED"));
    assert!(startup.contains("POLICY_UNAVAILABLE"));
    for marker in [
        HUMAN_TOKEN,
        SERVICE_TOKEN,
        "test-client-secret-marker",
        "credential-private-path-marker",
        "private-remote-secret-marker",
    ] {
        assert!(!startup.contains(marker), "process log leaked {marker}");
    }
    // Invalid credentials fail before a new database is opened.
    let mut invalid = config;
    invalid.storage_path = directory
        .path()
        .join("must-not-open.redb")
        .to_str()
        .unwrap()
        .into();
    let path = std::path::PathBuf::from(&invalid.storage_path);
    std::fs::write(&config_file, toml::to_string(&invalid).unwrap()).unwrap();
    issuer
        .state
        .unavailable
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_convex-server"));
    for (name, _) in std::env::vars_os().filter(|(n, _)| n.to_string_lossy().starts_with("CONVEX_"))
    {
        command.env_remove(name);
    }
    command.arg(&config_file).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .unwrap()
        .unwrap();
    assert!(!output.status.success());
    assert!(!path.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ACCESS_UNAVAILABLE"));
    issuer.finish().await;
}
