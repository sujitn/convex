use std::sync::Arc;

use convex_ext_file::MemoryReferenceRepository;

#[path = "../../convex-ports/tests/support/reference_conformance.rs"]
mod conformance;

#[tokio::test]
async fn versions_history_and_mapping() {
    conformance::versions_history_and_mapping(Arc::new(MemoryReferenceRepository::default())).await;
}
#[tokio::test]
async fn conflict_is_atomic() {
    conformance::conflict_is_atomic(Arc::new(MemoryReferenceRepository::default())).await;
}
#[tokio::test]
async fn retry_preserves_original_result() {
    conformance::retry_preserves_original_result(Arc::new(MemoryReferenceRepository::default()))
        .await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expected_version_race() {
    conformance::expected_version_race(Arc::new(MemoryReferenceRepository::default())).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn identical_retry_race() {
    conformance::identical_retry_race(Arc::new(MemoryReferenceRepository::default())).await;
}
#[tokio::test]
async fn filters_and_ambiguity() {
    conformance::filters_and_ambiguity(Arc::new(MemoryReferenceRepository::default())).await;
}
#[tokio::test]
async fn exact_precision_and_failed_validation() {
    conformance::exact_precision_and_failed_validation(Arc::new(
        MemoryReferenceRepository::default(),
    ))
    .await;
}

#[test]
fn full_term_file_import_rejects_invented_defaults() {
    use convex_ext_file::load_bond_references;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bonds.json");
    let bond = conformance::bond("FILE");
    std::fs::write(&path, serde_json::to_vec(&vec![bond.clone()]).unwrap()).unwrap();
    assert_eq!(load_bond_references(&path).unwrap(), vec![bond]);
    let mut missing: serde_json::Value = serde_json::from_str(include_str!(
        "../../convex-ports/tests/fixtures/reference_bond.json"
    ))
    .unwrap();
    missing.as_object_mut().unwrap().remove("issue_date");
    std::fs::write(&path, serde_json::to_vec(&vec![missing]).unwrap()).unwrap();
    assert!(load_bond_references(&path).is_err());
    std::fs::write(&path, b"not JSON").unwrap();
    assert!(load_bond_references(&path).is_err());
    std::fs::remove_file(&path).unwrap();
    assert!(load_bond_references(&path).is_err());
}
