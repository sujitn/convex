use convex_ext_file::MemoryReferenceRepository;
use std::sync::Arc;
#[path = "../../convex-ports/tests/support/master_conformance.rs"]
mod conformance;

#[tokio::test]
async fn remap_preserves_identity_and_historical_resolution() {
    conformance::remap_preserves_identity_and_historical_resolution(Arc::new(
        MemoryReferenceRepository::default(),
    ))
    .await;
}
#[tokio::test]
async fn ambiguity_and_missing_terms_fail_closed() {
    conformance::ambiguity_and_missing_terms_fail_closed(Arc::new(
        MemoryReferenceRepository::default(),
    ))
    .await;
}
#[tokio::test]
async fn schedule_correction_preserves_snapshot_replay() {
    conformance::schedule_correction_preserves_snapshot_replay(Arc::new(
        MemoryReferenceRepository::default(),
    ))
    .await;
}
#[tokio::test]
async fn entity_resources_and_corrections_are_pinned() {
    conformance::entity_resources_and_corrections_are_pinned(Arc::new(
        MemoryReferenceRepository::default(),
    ))
    .await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn master_compare_and_set_race() {
    conformance::master_compare_and_set_race(Arc::new(MemoryReferenceRepository::default())).await;
}
