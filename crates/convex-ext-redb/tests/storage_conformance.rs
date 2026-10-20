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
async fn curve_snapshots_are_immutable_and_ordered() {
    let (_directory, s) = storage();
    conformance::curve_snapshots_are_immutable_and_ordered(s.curves).await;
}
#[tokio::test]
async fn curve_config_projection_queries_and_deletion_are_consistent() {
    use convex_core::ids::CurveId;
    use convex_core::Currency;
    use convex_ports::storage::CurveConfig;
    let (_directory, s) = storage();
    let first = CurveConfig {
        curve_id: CurveId::new("Z"),
        name: "fictional curve".into(),
        currency: Currency::USD,
        interpolation: "Linear".into(),
        day_count: "Act365Fixed".into(),
        tenors: vec!["1Y".into()],
        build_schedule: "manual".into(),
        last_updated: 0,
    };
    let mut second = first.clone();
    second.curve_id = CurveId::new("A");
    s.curves.save_config(&first).await.unwrap();
    s.curves.save_config(&second).await.unwrap();
    assert_eq!(
        s.curves.list_configs().await.unwrap(),
        [second, first.clone()]
    );
    assert!(s.curves.delete_config(&first.curve_id).await.unwrap());
    assert!(s
        .curves
        .get_config(&first.curve_id)
        .await
        .unwrap()
        .is_none());
    assert!(!s.curves.delete_config(&first.curve_id).await.unwrap());
}
