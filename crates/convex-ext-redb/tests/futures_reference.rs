#[path = "../../convex-ports/tests/support/futures_conformance.rs"]
mod conformance;
use convex_ext_redb::RedbReferenceRepository;
use convex_ports::reference_repository::ReferenceRepository;
use std::sync::Arc;

fn open(path: &std::path::Path) -> RedbReferenceRepository {
    RedbReferenceRepository::new(Arc::new(redb::Database::create(path).unwrap())).unwrap()
}

#[tokio::test]
async fn persistent_futures_history_and_mapping_replay_use_exact_revisions() {
    let dir = tempfile::tempdir().unwrap();
    conformance::exact_history_and_known_at_replay(&open(&dir.path().join("futures.redb"))).await;
}

#[tokio::test]
async fn missing_exact_futures_and_security_pins_commit_no_prefix() {
    let dir = tempfile::tempdir().unwrap();
    conformance::missing_pins_and_nonatomic_corrections_are_rejected(&open(
        &dir.path().join("futures.redb"),
    ))
    .await;
}

#[tokio::test]
async fn separate_process_import_reopens_exact_contract_basket_and_receipt() {
    if let Some(path) = std::env::var_os("CVX022_TEST_DATABASE") {
        open(std::path::Path::new(&path))
            .commit(&conformance::command(), conformance::first_time())
            .await
            .unwrap();
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reopened.redb");
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "separate_process_import_reopens_exact_contract_basket_and_receipt",
            "--nocapture",
        ])
        .env("CVX022_TEST_DATABASE", &path)
        .status()
        .unwrap();
    assert!(status.success());
    let repo = open(&path);
    let original = repo
        .get_commit(&conformance::command().idempotency_key)
        .await
        .unwrap()
        .unwrap();
    for record in original.master_records {
        assert_eq!(
            repo.get_master_version(&record.definition.key(), record.version)
                .await
                .unwrap(),
            Some(record)
        );
    }
    conformance::exact_history_and_known_at_replay(&repo).await;
}
