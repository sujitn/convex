use std::sync::Arc;

use convex_ext_redb::RedbReferenceRepository;
use convex_ports::reference_repository::{ReferenceError, ReferenceRepository};

#[path = "../../convex-ports/tests/support/reference_conformance.rs"]
mod conformance;

fn repository() -> (tempfile::TempDir, Arc<RedbReferenceRepository>) {
    let directory = tempfile::tempdir().unwrap();
    let db = Arc::new(redb::Database::create(directory.path().join("reference.redb")).unwrap());
    let repository = Arc::new(RedbReferenceRepository::new(db).unwrap());
    (directory, repository)
}

#[tokio::test]
async fn versions_history_and_mapping() {
    let (_directory, repository) = repository();
    conformance::versions_history_and_mapping(repository).await;
}
#[tokio::test]
async fn conflict_is_atomic() {
    let (_directory, repository) = repository();
    conformance::conflict_is_atomic(repository).await;
}
#[tokio::test]
async fn retry_preserves_original_result() {
    let (_directory, repository) = repository();
    conformance::retry_preserves_original_result(repository).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expected_version_race() {
    let (_directory, repository) = repository();
    conformance::expected_version_race(repository).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn identical_retry_race() {
    let (_directory, repository) = repository();
    conformance::identical_retry_race(repository).await;
}
#[tokio::test]
async fn filters_and_ambiguity() {
    let (_directory, repository) = repository();
    conformance::filters_and_ambiguity(repository).await;
}
#[tokio::test]
async fn exact_precision_and_failed_validation() {
    let (_directory, repository) = repository();
    conformance::exact_precision_and_failed_validation(repository).await;
}

#[tokio::test]
async fn reopen_recovers_history_and_lost_response_by_key() {
    use conformance::*;
    let (directory, repository) = repository();
    let first = command("lost-reply", vec![create(bond("DURABLE"))]);
    let mut first = first;
    first.master_changes = reference_terms::resources(&bond("DURABLE"));
    let receipt = repository.commit(&first, time()).await.unwrap();
    let mut revised = bond("DURABLE");
    revised.description = "revision".into();
    repository
        .commit(
            &command("revision", vec![update(revised.clone(), 1)]),
            time(),
        )
        .await
        .unwrap();
    drop(repository);
    let reopened = RedbReferenceRepository::new(Arc::new(
        redb::Database::open(directory.path().join("reference.redb")).unwrap(),
    ))
    .unwrap();
    assert_eq!(
        reopened.get_commit(&first.idempotency_key).await.unwrap(),
        Some(receipt.clone())
    );
    assert_eq!(reopened.commit(&first, time()).await.unwrap(), receipt);
    assert_eq!(
        reopened
            .get_version(&revised.instrument_id, version(1))
            .await
            .unwrap(),
        Some(receipt.records[0].clone())
    );
    assert_eq!(
        reopened
            .get(&revised.instrument_id)
            .await
            .unwrap()
            .unwrap()
            .version,
        version(2)
    );
}

#[test]
fn unversioned_persisted_bonds_require_explicit_import() {
    let directory = tempfile::tempdir().unwrap();
    let db = Arc::new(redb::Database::create(directory.path().join("old.redb")).unwrap());
    let write = db.begin_write().unwrap();
    {
        let mut table = write
            .open_table(redb::TableDefinition::<&str, &[u8]>::new("bonds"))
            .unwrap();
        table.insert("existing", b"{}".as_slice()).unwrap();
    }
    write.commit().unwrap();
    assert!(matches!(
        RedbReferenceRepository::new(db),
        Err(ReferenceError::Unsupported(_))
    ));
}
