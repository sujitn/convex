use convex_engine::market_ingestion::*;
use convex_ext_file::FileQuoteReplay;
use convex_ports::market_data::*;
use rust_decimal::Decimal;

fn events() -> Vec<QuoteEvent> {
    include_str!("../../convex-ext-file/tests/fixtures/sequenced-quotes.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn recorded_replay_covers_gaps_clears_duplicates_crossing_sessions_and_status() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../convex-ext-file/tests/fixtures/sequenced-quotes.jsonl");
    let expected: Vec<IngestionResult> = serde_json::from_str(include_str!(
        "../../convex-ext-file/tests/fixtures/sequenced-quotes.expected.json"
    ))
    .unwrap();
    let run = || {
        let mut source = FileQuoteReplay::open(&path).unwrap();
        let mut assembler = MarketDataAssembler::default();
        let mut trace = Vec::new();
        loop {
            match source.read_event() {
                Ok(event) => {
                    let now = event.receive_time_ms;
                    trace.push(assembler.ingest(event, now));
                }
                Err(StreamError::Closed) => break,
                Err(error) => panic!("{error}"),
            }
        }
        assert_eq!(assembler.len(), 2);
        (trace, assembler)
    };
    let (actual, assembler) = run();
    assert_eq!(actual, expected);
    assert_eq!(run().0, actual);
    let e = events();
    let first = assembler.snapshot(&e[0].key, 9000).unwrap();
    let other = assembler.snapshot(&e[27].key, 9000).unwrap();
    assert_eq!(
        first.fields[&QuoteField::BidPrice].value.value,
        Decimal::from(96)
    );
    assert_eq!(
        other.fields[&QuoteField::BidPrice].value.value,
        Decimal::from(95)
    );
    assert!(assembler.has_other_source(&e[0].key));
}

#[test]
fn partial_updates_preserve_values_times_and_exact_precision_and_clear_one_field() {
    let e = events();
    let mut assembler = MarketDataAssembler::default();
    for event in &e[..3] {
        assembler.ingest(event.clone(), event.receive_time_ms);
    }
    let snapshot = assembler.snapshot(&e[0].key, 1200).unwrap();
    assert_eq!(
        snapshot.fields[&QuoteField::BidPrice].value.value,
        Decimal::new(995, 1)
    );
    assert_eq!(snapshot.fields[&QuoteField::BidPrice].receive_time_ms, 1100);
    assert_eq!(snapshot.fields[&QuoteField::MidPrice].receive_time_ms, 1000);
    assert_eq!(
        snapshot.fields[&QuoteField::BidYield]
            .value
            .value
            .to_string(),
        "-0.0012345678901234567890123456"
    );
    assert!(!snapshot.fields.contains_key(&QuoteField::AskPrice));
    let mut unchanged = e[1].clone();
    unchanged.sequence = 4;
    unchanged.receive_time_ms = 1300;
    unchanged.source_time_ms = 1300;
    unchanged.event = QuoteEventKind::Delta {
        fields: vec![QuoteChange {
            field: QuoteField::MidPrice,
            update: FieldUpdate::Unchanged,
        }],
    };
    assert_eq!(
        assembler.ingest(unchanged, 1300).outcome,
        IngestionOutcome::Applied
    );
    assert_eq!(
        assembler.snapshot(&e[0].key, 1300).unwrap().fields,
        snapshot.fields
    );
}

#[test]
fn gaps_and_invalid_quotes_never_replace_retained_values_or_accept_deltas() {
    let e = events();
    let mut assembler = MarketDataAssembler::default();
    for event in &e[..5] {
        assembler.ingest(event.clone(), event.receive_time_ms);
    }
    let retained = assembler.snapshot(&e[0].key, 1200).unwrap().fields;
    for event in &e[5..8] {
        assert_eq!(
            assembler.ingest(event.clone(), event.receive_time_ms).state,
            DataState::Gap
        );
        assert_eq!(
            assembler
                .snapshot(&e[0].key, event.receive_time_ms)
                .unwrap()
                .fields,
            retained
        );
    }
    assert_eq!(assembler.ingest(e[8].clone(), 1600).state, DataState::Live);
    assert_eq!(
        assembler.ingest(e[9].clone(), 1700).state,
        DataState::Invalid
    );
    assert_eq!(
        assembler.snapshot(&e[0].key, 1700).unwrap().fields[&QuoteField::BidPrice]
            .value
            .value,
        Decimal::from(99)
    );
    assert_eq!(assembler.ingest(e[10].clone(), 1800).state, DataState::Gap);
    assert_eq!(assembler.ingest(e[11].clone(), 1900).state, DataState::Live);
}

#[test]
fn transport_loss_conflicting_duplicate_and_entitlement_need_validated_image() {
    let e = events();
    let mut assembler = MarketDataAssembler::default();
    assembler.ingest(e[0].clone(), 1000);
    assembler.transport_gap();
    assert_eq!(
        assembler.snapshot(&e[0].key, 1000).unwrap().state,
        DataState::Gap
    );
    assert_eq!(
        assembler.ingest(e[1].clone(), 1100).outcome,
        IngestionOutcome::RecoveryRequired
    );
    let mut image = e[0].clone();
    image.sequence = 2;
    image.source_time_ms = 1100;
    image.receive_time_ms = 1100;
    assert_eq!(assembler.ingest(image.clone(), 1100).state, DataState::Live);
    image.evidence_reference = Some("fixture:conflicting-retry".into());
    assert_eq!(
        assembler.ingest(image.clone(), 1100).outcome,
        IngestionOutcome::Invalid
    );
    assert_eq!(
        assembler.snapshot(&image.key, 1100).unwrap().state,
        DataState::Invalid
    );
    image.sequence = 3;
    assert_eq!(assembler.ingest(image.clone(), 1100).state, DataState::Live);
    let mut revoked = image.clone();
    revoked.entitled = false;
    revoked.event = QuoteEventKind::Heartbeat;
    assert_eq!(
        assembler.ingest(revoked, 1100).state,
        DataState::Unauthorized
    );
    assert_eq!(assembler.ingest(image, 1100).state, DataState::Live);
    assembler.close();
    assert_eq!(
        assembler.snapshot(&e[0].key, 1100).unwrap().state,
        DataState::Closed
    );
}

#[test]
fn overload_is_observable_deterministic_and_retained_memory_is_bounded() {
    let policy = IngestionPolicy {
        max_keys: 1,
        ..IngestionPolicy::default()
    };
    let mut assembler = MarketDataAssembler::new(policy).unwrap();
    let first = events()[0].clone();
    assembler.ingest(first.clone(), 1000);
    for i in 0..10_000 {
        let mut next = first.clone();
        next.key.instrument_id = format!("UNSUBSCRIBED-{i}").into();
        let result = assembler.ingest(next, 1000);
        assert_eq!(
            (result.outcome, result.state),
            (IngestionOutcome::CapacityExceeded, DataState::Gap)
        );
        assert_eq!(assembler.len(), 1);
    }
    assert_eq!(
        assembler.snapshot(&first.key, 1000).unwrap().state,
        DataState::Gap
    );
    assert_eq!(assembler.ingest(first.clone(), 1000).state, DataState::Live);
    assembler.remove(&first.key);
    assert!(assembler.is_empty());
    let next = events()[1].clone();
    assert_eq!(assembler.ingest(next, 1100).state, DataState::Gap);
}

#[test]
fn stale_old_fields_and_heartbeats_cannot_refresh_freshness() {
    let e = events();
    let mut assembler = MarketDataAssembler::new(IngestionPolicy {
        stale_after_ms: 100,
        ..IngestionPolicy::default()
    })
    .unwrap();
    assembler.ingest(e[0].clone(), 1000);
    assert_eq!(assembler.ingest(e[1].clone(), 1100).state, DataState::Live);
    assert_eq!(assembler.fresh_for_ms(&e[0].key, 1100), Some(0));
    let mut heartbeat = e[1].clone();
    heartbeat.event = QuoteEventKind::Heartbeat;
    heartbeat.receive_time_ms = 1101;
    heartbeat.source_time_ms = 1101;
    assert_eq!(assembler.ingest(heartbeat, 1101).state, DataState::Stale);
    assert_eq!(
        assembler
            .snapshot(&e[0].key, 1101)
            .unwrap()
            .watermark
            .receive_time_ms,
        1100
    );
}

#[test]
fn schema_unit_time_and_sequence_limits_fail_closed() {
    let first = events()[0].clone();
    let mutations: Vec<fn(&mut QuoteEvent)> = vec![
        |e| e.schema_version = 2,
        |e| e.source_time_ms = -1,
        |e| e.source_time_ms = e.receive_time_ms + 101,
        |e| e.receive_time_ms = 1001,
        |e| e.sequence = 0,
        |e| e.key.source = " ".into(),
        |e| e.entitlement_tags = vec!["tag".into(); 17],
        |e| {
            if let QuoteEventKind::Image { fields } = &mut e.event {
                fields.push(fields[0].clone());
            }
        },
        |e| {
            if let QuoteEventKind::Image { fields } = &mut e.event {
                if let FieldUpdate::Set(v) = &mut fields[0].update {
                    v.unit = QuoteUnit::YieldDecimal;
                }
            }
        },
    ];
    for change in mutations {
        let mut assembler = MarketDataAssembler::default();
        let mut invalid = first.clone();
        change(&mut invalid);
        assert_eq!(
            assembler.ingest(invalid, 1000).outcome,
            IngestionOutcome::Invalid
        );
        assert!(assembler.is_empty());
    }
    let mut assembler = MarketDataAssembler::default();
    let mut image = first.clone();
    image.sequence = u64::MAX;
    assert_eq!(assembler.ingest(image.clone(), 1000).state, DataState::Live);
    let mut rolled = first;
    rolled.event = QuoteEventKind::Delta { fields: vec![] };
    assert_eq!(
        assembler.ingest(rolled, 1000).outcome,
        IngestionOutcome::OutOfOrder
    );
    assert_eq!(
        assembler
            .snapshot(&image.key, 1000)
            .unwrap()
            .watermark
            .sequence,
        u64::MAX
    );
}
