use convex_ext_file::{FileQuoteReplay, MAX_REPLAY_RECORD_BYTES};
use convex_ports::market_data::{QuoteEventSource, StreamError};

#[tokio::test]
async fn pull_replay_preserves_record_order_source_times_and_closed_state() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sequenced-quotes.jsonl");
    let mut source = FileQuoteReplay::open(&path).unwrap();
    let first = source.next_event().await.unwrap();
    assert_eq!(first.sequence, 1);
    assert_eq!((first.source_time_ms, first.receive_time_ms), (1000, 1000));
    for _ in 1..28 {
        source.next_event().await.unwrap();
    }
    assert_eq!(source.position(), 28);
    assert!(matches!(
        source.next_event().await,
        Err(StreamError::Closed)
    ));
    assert!(matches!(
        source.next_event().await,
        Err(StreamError::Closed)
    ));
}

#[test]
fn malformed_oversized_blank_and_unknown_schema_evidence_is_never_skipped() {
    let first = include_str!("fixtures/sequenced-quotes.jsonl")
        .lines()
        .next()
        .unwrap();
    let mut unknown = serde_json::from_str::<serde_json::Value>(first).unwrap();
    unknown["extra"] = serde_json::json!(true);
    for bad in [
        "{truncated".to_owned(),
        "\n".into(),
        "x".repeat(MAX_REPLAY_RECORD_BYTES + 1),
        serde_json::to_string(&unknown).unwrap(),
    ] {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), format!("{bad}\n{first}\n")).unwrap();
        let mut replay = FileQuoteReplay::open(file.path()).unwrap();
        assert!(matches!(replay.read_event(), Err(StreamError::Failure(_))));
        assert_eq!(replay.position(), 1);
        assert!(matches!(replay.read_event(), Err(StreamError::Closed)));
    }
}

#[test]
fn empty_missing_non_regular_and_oversized_files_have_explicit_outcomes() {
    let dir = tempfile::tempdir().unwrap();
    assert!(FileQuoteReplay::open(dir.path()).is_err());
    assert!(FileQuoteReplay::open(dir.path().join("missing")).is_err());
    let file = tempfile::NamedTempFile::new().unwrap();
    assert!(matches!(
        FileQuoteReplay::open(file.path()).unwrap().read_event(),
        Err(StreamError::Closed)
    ));
    file.as_file()
        .set_len(convex_ext_file::MAX_REPLAY_BYTES + 1)
        .unwrap();
    assert!(FileQuoteReplay::open(file.path()).is_err());
}
