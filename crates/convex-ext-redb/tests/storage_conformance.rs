use convex_ext_redb::create_redb_storage;
use convex_ports::storage::StorageAdapter;

#[path = "../../convex-ports/tests/support/storage_conformance.rs"]
mod conformance;

fn storage() -> (tempfile::TempDir, StorageAdapter) {
    let directory = tempfile::tempdir().unwrap();
    let storage = create_redb_storage(directory.path().join("conformance.redb")).unwrap();
    (directory, storage)
}

#[tokio::test]
async fn exact_configuration_history() {
    let (_directory, s) = storage();
    conformance::exact_configuration_history(s.configs).await;
}
#[tokio::test]
async fn configuration_conflicts_and_validation() {
    let (_directory, s) = storage();
    conformance::configuration_conflicts_and_validation(s.configs).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_configuration_writers() {
    let (_directory, s) = storage();
    conformance::concurrent_configuration_writers(s.configs).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_configuration_retries() {
    let (_directory, s) = storage();
    conformance::concurrent_configuration_retries(s.configs).await;
}
#[tokio::test]
async fn configuration_decimal_precision() {
    let (_directory, s) = storage();
    conformance::configuration_decimal_precision(s.configs).await;
}
#[tokio::test]
async fn audit_filter_count_and_pagination() {
    let (_directory, s) = storage();
    conformance::audit_filter_count_and_pagination(s.audit).await;
}
#[tokio::test]
async fn audit_is_append_only() {
    let (_directory, s) = storage();
    conformance::audit_is_append_only(s.audit).await;
}
#[tokio::test]
async fn curve_recipes_resolve_at_known_at() {
    let (_directory, s) = storage();
    conformance::curve_recipes_resolve_at_known_at(s.curves).await;
}
#[tokio::test]
async fn curve_revisions_keep_recording_order() {
    let (_directory, s) = storage();
    conformance::curve_revisions_keep_recording_order(s.curves).await;
}
