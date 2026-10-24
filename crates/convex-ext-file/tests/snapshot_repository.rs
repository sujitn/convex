#[path = "../../convex-ports/tests/support/snapshot_fixtures.rs"]
mod fixture;

use convex_ext_file::{FileSnapshotRepository, JsonFixingRepository};
use convex_ports::{fixing::FixingRepository, snapshot::SnapshotRepository};
use std::sync::Arc;

#[tokio::test]
async fn immutable_publication_retries_concurrent_writers_and_reopen_return_exact_content() {
    let directory = tempfile::tempdir().unwrap();
    let snapshot = Arc::new(fixture::snapshot());
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let repository = FileSnapshotRepository::new(directory.path()).unwrap();
        let snapshot = snapshot.clone();
        tasks.push(tokio::spawn(async move {
            repository.publish(&snapshot).await.unwrap();
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let reopened = FileSnapshotRepository::new(directory.path()).unwrap();
    assert_eq!(
        reopened.get(snapshot.id()).await.unwrap().unwrap(),
        *snapshot
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    let absent =
        convex_core::contracts::SnapshotId::new(format!("sha256:{}", "0".repeat(64))).unwrap();
    assert!(reopened.get(&absent).await.unwrap().is_none());
    let arbitrary = convex_core::contracts::SnapshotId::new("relative-path").unwrap();
    assert!(reopened.get(&arbitrary).await.is_err());
}

#[tokio::test]
async fn corruption_does_not_become_a_new_snapshot_or_get_overwritten_by_retry() {
    let directory = tempfile::tempdir().unwrap();
    let repository = FileSnapshotRepository::new(directory.path()).unwrap();
    let snapshot = fixture::snapshot();
    repository.publish(&snapshot).await.unwrap();
    let path = std::fs::read_dir(directory.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let original = std::fs::read(&path).unwrap();
    let corrupt = String::from_utf8(original.clone())
        .unwrap()
        .replace("99.25", "99.26");
    std::fs::write(&path, &corrupt).unwrap();
    assert!(repository.get(snapshot.id()).await.is_err());
    assert!(repository.publish(&snapshot).await.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), corrupt.as_bytes());
    // Only canonical persisted bytes are admitted; duplicate keys/alternate encoding cannot hide tampering.
    std::fs::write(&path, [original.as_slice(), b"\n"].concat()).unwrap();
    assert!(repository.get(snapshot.id()).await.is_err());
    std::fs::write(&path, &original).unwrap();
    assert_eq!(repository.get(snapshot.id()).await.unwrap(), Some(snapshot));
}

#[tokio::test]
async fn fixing_file_uses_actual_publication_and_recording_cutoffs_after_reopen() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let initial = fixture::fixing(1, "2026-10-02T09:00:00Z", "2026-10-02T09:01:00Z", "-0.001");
    let correction = fixture::fixing(2, "2026-10-03T09:00:00Z", "2026-10-03T09:01:00Z", "0.043");
    let write = |fixings| {
        std::fs::write(
            file.path(),
            serde_json::to_vec(&serde_json::json!({"schema_version":1,"fixings":fixings})).unwrap(),
        )
        .unwrap()
    };
    write(vec![initial.clone()]);
    let frozen = JsonFixingRepository::new(file.path()).unwrap();
    write(vec![correction.clone(), initial.clone()]);
    let reopened = JsonFixingRepository::new(file.path()).unwrap();
    assert_eq!(
        reopened
            .get_at(
                &fixture::fixing_key(),
                fixture::time("2026-10-02T15:00:00Z")
            )
            .await
            .unwrap(),
        Some(initial.clone())
    );
    assert_eq!(
        reopened
            .get_at(&fixture::fixing_key(), correction.recorded_at)
            .await
            .unwrap(),
        Some(correction)
    );
    assert_eq!(
        frozen
            .get_at(
                &fixture::fixing_key(),
                fixture::time("2026-10-04T00:00:00Z")
            )
            .await
            .unwrap(),
        Some(initial.clone())
    );
    write(vec![initial.clone(), initial]);
    assert!(JsonFixingRepository::new(file.path()).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_nonregular_and_oversized_files_are_refused() {
    let directory = tempfile::tempdir().unwrap();
    let file = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
    file.as_file().set_len(16 * 1024 * 1024 + 1).unwrap();
    assert!(JsonFixingRepository::new(file.path()).is_err());
    let link = directory.path().join("link");
    std::os::unix::fs::symlink(file.path(), &link).unwrap();
    assert!(JsonFixingRepository::new(&link).is_err());
    let root_link = directory.path().join("root-link");
    std::os::unix::fs::symlink(directory.path(), &root_link).unwrap();
    assert!(FileSnapshotRepository::new(&root_link).is_err());
    let store = FileSnapshotRepository::new(directory.path()).unwrap();
    let snapshot = fixture::snapshot();
    let path = directory.path().join(format!(
        "{}.json",
        snapshot.id().as_str().strip_prefix("sha256:").unwrap()
    ));
    std::os::unix::fs::symlink(file.path(), &path).unwrap();
    assert!(store.get(snapshot.id()).await.is_err());
    assert!(store.publish(&snapshot).await.is_err());
}
