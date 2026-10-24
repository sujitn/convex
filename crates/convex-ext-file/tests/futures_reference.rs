#[path = "../../convex-ports/tests/support/futures_conformance.rs"]
mod conformance;
use convex_ext_file::{import_reference_file, load_reference_import, MemoryReferenceRepository};
use convex_ports::reference_repository::ReferenceRepository;

#[tokio::test]
async fn actual_futures_file_import_and_known_at_replay_preserve_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("actual-futures.json");
    std::fs::write(&path, conformance::INITIAL).unwrap();
    let repo = MemoryReferenceRepository::default();
    let receipt = import_reference_file(&repo, &path, conformance::first_time())
        .await
        .unwrap();
    assert_eq!(receipt.records.len(), 10);
    assert_eq!(receipt.master_records.len(), 8);
    assert_eq!(
        load_reference_import(&path).unwrap().command,
        conformance::command()
    );
    assert_eq!(
        import_reference_file(&repo, &path, conformance::t("2026-10-07T10:00:00Z"))
            .await
            .unwrap(),
        receipt
    );
    conformance::exact_history_and_known_at_replay(&repo).await;
}

#[tokio::test]
async fn missing_exact_futures_and_security_pins_commit_no_prefix() {
    conformance::missing_pins_and_nonatomic_corrections_are_rejected(
        &MemoryReferenceRepository::default(),
    )
    .await;
}

#[tokio::test]
async fn strict_import_reports_wrong_family_row_and_rejects_atomic_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.json");
    let mut raw: serde_json::Value = serde_json::from_str(conformance::INITIAL).unwrap();
    raw["command"]["master_changes"][3]["definition"]["definition"]["contract"]["settlement"]
        ["accrual"] = serde_json::json!({});
    std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    let repo = MemoryReferenceRepository::default();
    let rejected = import_reference_file(&repo, &path, conformance::first_time())
        .await
        .unwrap_err();
    assert_eq!(rejected.diagnostics[0].section, "file");
    assert!(rejected.diagnostics[0].line.is_some());
    assert!(repo
        .get_master(&conformance::future_key())
        .await
        .unwrap()
        .is_none());
    assert!(repo
        .get_commit(&conformance::command().idempotency_key)
        .await
        .unwrap()
        .is_none());
}
