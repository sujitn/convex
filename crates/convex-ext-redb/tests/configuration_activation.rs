#[path = "../../convex-ports/tests/support/configuration_fixtures.rs"]
mod fixture;
use convex_core::contracts::*;
use convex_ports::{configuration::*, storage::Pagination, TraitError};
use fixture::*;
use std::sync::Arc;

#[test]
fn partial_unmarked_or_newer_activation_schema_is_rejected_without_recreating_tables() {
    use redb::{ReadableDatabase, TableDefinition};
    const HISTORY: TableDefinition<(&str, u64), &[u8]> =
        TableDefinition::new("configuration_activations_v1");
    const SCHEMA: TableDefinition<&str, u64> = TableDefinition::new("convex_schema");
    for state in ["partial", "unmarked", "newer"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.redb");
        drop(convex_ext_redb::create_redb_storage(&path).unwrap());
        let db = Arc::new(redb::Database::open(&path).unwrap());
        let write = db.begin_write().unwrap();
        match state {
            "partial" => {
                assert!(write.delete_table(HISTORY).unwrap());
            }
            "unmarked" => {
                write
                    .open_table(SCHEMA)
                    .unwrap()
                    .remove("configuration_activation")
                    .unwrap();
            }
            "newer" => {
                write
                    .open_table(SCHEMA)
                    .unwrap()
                    .insert("configuration_activation", 2)
                    .unwrap();
            }
            _ => unreachable!(),
        }
        write.commit().unwrap();
        assert!(matches!(
            convex_ext_redb::RedbConfigurationRepository::new(db.clone()),
            Err(TraitError::Unsupported(_))
        ));
        if state == "partial" {
            assert!(matches!(
                db.begin_read().unwrap().open_table(HISTORY),
                Err(redb::TableError::TableDoesNotExist(_))
            ));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cvx010_ac1_concurrent_cas_has_exactly_one_winner_and_coherent_content() {
    let dir = tempfile::tempdir().unwrap();
    let storage = convex_ext_redb::create_redb_storage(dir.path().join("config.redb")).unwrap();
    let repo = storage.configuration;
    let initial = staged(1);
    repo.stage(&initial).await.unwrap();
    assert!(repo
        .active(&activate(1, 0, "initial").id)
        .await
        .unwrap()
        .is_none());
    repo.activate(&activate(1, 0, "initial")).await.unwrap();
    repo.stage(&staged(2)).await.unwrap();
    repo.stage(&staged(3)).await.unwrap();
    assert_eq!(
        repo.active(&activate(1, 0, "initial").id)
            .await
            .unwrap()
            .unwrap()
            .staged,
        initial
    );
    let barrier = Arc::new(tokio::sync::Barrier::new(9));
    let mut tasks = vec![];
    for index in 0..8 {
        let repo = repo.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            repo.activate(&activate(2 + index % 2, 1, &format!("race-{index}")))
                .await
        }));
    }
    barrier.wait().await;
    let mut winners = vec![];
    for task in tasks {
        match task.await.unwrap() {
            Ok(record) => winners.push(record),
            Err(TraitError::Conflict(_)) => {}
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
    assert_eq!(winners.len(), 1);
    let active = repo.active(&winners[0].command.id).await.unwrap().unwrap();
    active.validate().unwrap();
    assert_eq!(active.activation, winners[0]);
    assert_eq!(
        active.staged.document.pricing.version,
        winners[0].command.configuration_version.get()
    );
    assert_eq!(
        repo.history(&active.activation.command.id, &Pagination::new(0, 100))
            .await
            .unwrap()
            .total,
        2
    );
}

#[tokio::test]
async fn immutable_staging_retries_rollback_and_reopen_preserve_the_original_audit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.redb");
    let storage = convex_ext_redb::create_redb_storage(&path).unwrap();
    let repo = storage.configuration.clone();
    let first = staged(1);
    repo.stage(&first).await.unwrap();
    repo.stage(&first).await.unwrap();
    let mut changed = first.clone();
    changed.reason = "Changed metadata".into();
    assert!(matches!(
        repo.stage(&changed).await,
        Err(TraitError::Conflict(_))
    ));
    let initial = repo.activate(&activate(1, 0, "initial")).await.unwrap();
    repo.stage(&staged(2)).await.unwrap();
    let second = repo.activate(&activate(2, 1, "second")).await.unwrap();
    assert_eq!(repo.activate(&initial.command).await.unwrap(), initial);
    assert_eq!(
        repo.active(&initial.command.id)
            .await
            .unwrap()
            .unwrap()
            .activation,
        second
    );
    let mut rollback = activate(1, 2, "rollback");
    rollback.action = ActivationKind::Rollback {
        to_revision: initial.revision,
    };
    let restored = repo.activate(&rollback).await.unwrap();
    assert_eq!(restored.revision.get(), 3);
    assert_eq!(restored.previous_version, Some(snapshot::version(2)));
    assert_eq!(
        storage
            .configs
            .get("CONFIG-RESTORE")
            .await
            .unwrap()
            .unwrap()
            .version,
        2
    );
    let mut reused = rollback.clone();
    reused.reason = "Different request".into();
    assert!(matches!(
        repo.activate(&reused).await,
        Err(TraitError::Conflict(_))
    ));
    drop(repo);
    drop(storage);
    let storage = convex_ext_redb::create_redb_storage(&path).unwrap();
    let active = storage
        .configuration
        .active(&rollback.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.activation, restored);
    assert_eq!(active.staged, first);
    let history = storage
        .configuration
        .history(&rollback.id, &Pagination::new(1, 1))
        .await
        .unwrap();
    assert_eq!(history.total, 3);
    assert_eq!(history.items, [second]);
    assert!(matches!(
        storage
            .configuration
            .history(&rollback.id, &Pagination::new(0, 1001))
            .await,
        Err(TraitError::InvalidInput(_))
    ));
}

#[tokio::test]
async fn initial_hot_restart_changes_and_false_rollback_targets_fail_without_pointer_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let storage = convex_ext_redb::create_redb_storage(dir.path().join("config.redb")).unwrap();
    let repo = storage.configuration;
    repo.stage(&staged(1)).await.unwrap();
    let mut command = activate(1, 0, "initial");
    command.mode = ReloadClass::Hot;
    assert!(matches!(
        repo.activate(&command).await,
        Err(TraitError::Unsupported(_))
    ));
    command.mode = ReloadClass::RestartRequired;
    let first = repo.activate(&command).await.unwrap();
    let mut next = staged(2);
    next.document.process.max_concurrent_calcs = 5;
    repo.stage(&next).await.unwrap();
    assert!(matches!(
        repo.activate(&activate(2, 1, "hot")).await,
        Err(TraitError::Unsupported(_))
    ));
    let mut false_target = activate(2, 1, "wrong-rollback");
    false_target.mode = ReloadClass::RestartRequired;
    false_target.action = ActivationKind::Rollback {
        to_revision: Version::new(1).unwrap(),
    };
    assert!(matches!(
        repo.activate(&false_target).await,
        Err(TraitError::Conflict(_))
    ));
    assert_eq!(
        repo.active(&first.command.id)
            .await
            .unwrap()
            .unwrap()
            .activation,
        first
    );
}

#[tokio::test]
async fn operator_supplied_fixture_evidence_is_retained_and_replay_checked_keeps_its_bytes() {
    // Replay-checked records omit the field, so records written before it keep their bytes.
    let checked = serde_json::to_value(staged(1)).unwrap();
    assert!(checked.get("fixture_evidence").is_none());
    let earlier: StagedConfiguration = serde_json::from_value(checked).unwrap();
    assert_eq!(earlier.fixture_evidence, FixtureEvidence::ReplayChecked);
    let dir = tempfile::tempdir().unwrap();
    let storage = convex_ext_redb::create_redb_storage(dir.path().join("config.redb")).unwrap();
    let repo = storage.configuration.clone();
    let mut supplied = staged(1);
    supplied.fixture_evidence = FixtureEvidence::OperatorSupplied;
    assert_eq!(
        serde_json::to_value(&supplied).unwrap()["fixture_evidence"],
        "operator_supplied"
    );
    repo.stage(&supplied).await.unwrap();
    let mut command = activate(1, 0, "supplied");
    command.fixture_evidence = FixtureEvidence::OperatorSupplied;
    let record = repo.activate(&command).await.unwrap();
    let active = repo.active(&command.id).await.unwrap().unwrap();
    assert_eq!(active.staged, supplied);
    assert_eq!(active.activation, record);
    assert_eq!(
        record.command.fixture_evidence,
        FixtureEvidence::OperatorSupplied
    );
}
