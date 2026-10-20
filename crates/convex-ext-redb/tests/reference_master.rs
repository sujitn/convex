use convex_ext_redb::RedbReferenceRepository;
use convex_ports::reference_repository::ReferenceRepository;
use std::sync::Arc;
#[path = "../../convex-ports/tests/support/master_conformance.rs"]
mod conformance;

fn repository() -> (tempfile::TempDir, Arc<RedbReferenceRepository>) {
    let dir = tempfile::tempdir().unwrap();
    let repo = Arc::new(
        RedbReferenceRepository::new(Arc::new(
            redb::Database::create(dir.path().join("master.redb")).unwrap(),
        ))
        .unwrap(),
    );
    (dir, repo)
}
#[tokio::test]
async fn remap_preserves_identity_and_historical_resolution() {
    let (_dir, repo) = repository();
    conformance::remap_preserves_identity_and_historical_resolution(repo).await;
}
#[tokio::test]
async fn ambiguity_and_missing_terms_fail_closed() {
    let (_dir, repo) = repository();
    conformance::ambiguity_and_missing_terms_fail_closed(repo).await;
}
#[tokio::test]
async fn schedule_correction_preserves_snapshot_replay() {
    let (_dir, repo) = repository();
    conformance::schedule_correction_preserves_snapshot_replay(repo).await;
}
#[tokio::test]
async fn entity_resources_and_corrections_are_pinned() {
    let (_dir, repo) = repository();
    conformance::entity_resources_and_corrections_are_pinned(repo).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn master_compare_and_set_race() {
    let (_dir, repo) = repository();
    conformance::master_compare_and_set_race(repo).await;
}

#[tokio::test]
async fn reopen_preserves_master_history_and_exact_receipt() {
    let (dir, repo) = repository();
    let receipt =
        conformance::seed(&(repo.clone() as Arc<dyn ReferenceRepository>), "REOPEN").await;
    drop(repo);
    let reopened = RedbReferenceRepository::new(Arc::new(
        redb::Database::open(dir.path().join("master.redb")).unwrap(),
    ))
    .unwrap();
    assert_eq!(
        reopened.get_commit(&receipt.idempotency_key).await.unwrap(),
        Some(receipt.clone())
    );
    for record in receipt.master_records {
        assert_eq!(
            reopened
                .get_master_version(&record.definition.key(), record.version)
                .await
                .unwrap(),
            Some(record)
        );
    }
    let lookup = conformance::lookup("2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z");
    assert_eq!(
        reopened
            .for_valuation(&convex_core::ids::InstrumentId::new("REOPEN"), lookup)
            .await
            .unwrap()
            .unwrap()
            .record,
        receipt.records[0]
    );
}

#[tokio::test]
async fn older_serialized_versions_remain_exact_and_require_explicit_migration_for_valuation() {
    use convex_ports::reference_repository::*;
    use redb::{Durability, TableDefinition};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("older.redb");
    let db = Arc::new(redb::Database::create(&path).unwrap());
    let mut bond_value: serde_json::Value = serde_json::from_str(include_str!(
        "../../convex-ports/tests/fixtures/reference_bond.json"
    ))
    .unwrap();
    bond_value
        .as_object_mut()
        .unwrap()
        .remove("contractual_terms");
    let record_value = serde_json::json!({"schema_version":"2.0","instrument_id":"BOND-001","version":1,"recorded_at":"2025-01-01T00:00:00Z","bond":bond_value});
    let command_value = serde_json::json!({"idempotency_key":"old-key","changes":[{"kind":"create","bond":bond_value}]});
    let receipt_value = serde_json::json!({"idempotency_key":"old-key","recorded_at":"2025-01-01T00:00:00Z","records":[record_value]});
    let mut write = db.begin_write().unwrap();
    write.set_durability(Durability::Immediate).unwrap();
    {
        let record = serde_json::to_vec(&record_value).unwrap();
        write
            .open_table(TableDefinition::<&str, &[u8]>::new("reference_current_v2"))
            .unwrap()
            .insert("BOND-001", record.as_slice())
            .unwrap();
        write
            .open_table(TableDefinition::<(&str, u64), &[u8]>::new(
                "reference_history_v2",
            ))
            .unwrap()
            .insert(("BOND-001", 1), record.as_slice())
            .unwrap();
        let entry = serde_json::to_vec(
            &serde_json::json!({"command":command_value,"result":receipt_value}),
        )
        .unwrap();
        write
            .open_table(TableDefinition::<&str, &[u8]>::new("reference_commits_v2"))
            .unwrap()
            .insert("old-key", entry.as_slice())
            .unwrap();
    }
    write.commit().unwrap();
    let repository = RedbReferenceRepository::new(db).unwrap();
    let id = convex_core::ids::InstrumentId::new("BOND-001");
    let old = repository
        .get_version(&id, conformance::version(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(serde_json::to_value(&old).unwrap(), record_value);
    let lookup = conformance::lookup("2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z");
    assert!(matches!(
        repository.for_valuation(&id, lookup).await,
        Err(ReferenceError::InvalidInput(_))
    ));
    let old_command: ReferenceCommand = serde_json::from_value(command_value).unwrap();
    let original: ReferenceCommit = serde_json::from_value(receipt_value).unwrap();
    assert_eq!(
        repository
            .commit(&old_command, lookup.known_at)
            .await
            .unwrap(),
        original
    );
    let current = conformance::bond("BOND-001");
    let mut resources = conformance::command("unused", vec![], vec![]);
    // Matching resources are supplied explicitly; no terms are inferred from the older record.
    resources.master_changes = reference_terms::resources(&current);
    resources.idempotency_key =
        convex_core::contracts::RequestId::new("explicit-migration").unwrap();
    resources.changes = vec![ReferenceChange::Update {
        bond: Box::new(current),
        expected_version: conformance::version(1),
    }];
    repository
        .commit(&resources, lookup.known_at)
        .await
        .unwrap();
    assert!(repository
        .for_valuation(&id, lookup)
        .await
        .unwrap()
        .is_some());
    assert_eq!(
        serde_json::to_value(
            repository
                .get_version(&id, conformance::version(1))
                .await
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        record_value
    );
}

use conformance::reference_terms;
