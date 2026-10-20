use convex_core::{ids::*, Date};
use convex_ext_file::{CsvIndexFixingSource, CsvQuoteSource, JsonCurveInputSource};
use convex_ports::market_data::*;
use rust_decimal::Decimal;

const HEADER: &str = "instrument_id,bid_price,ask_price,mid_price,last_price,timestamp,source\n";

#[tokio::test]
async fn quote_snapshots_are_exact_and_atomic_and_reload_removes_absent_keys() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        file.path(),
        format!(
            "{HEADER}A,99.12345678901234567890123456,101,100,,1000,file\nB,98,100,99,,1000,file\n"
        ),
    )
    .unwrap();
    let source = CsvQuoteSource::new(file.path()).unwrap();
    let before = source.get_quote(&"A".into()).await.unwrap().unwrap();
    assert_eq!(
        before.bid_price.unwrap().to_string(),
        "99.12345678901234567890123456"
    );
    assert_eq!(before.timestamp, 1000);
    for invalid in [
        format!("{HEADER}A,97,99,98,,1100,file\nB,NaN,100,99,,1100,file\n"),
        format!("{HEADER}A,102,101,100,,1100,file\n"),
        format!("{HEADER}A,99,101,100,,1100,file\nA,98,100,99,,1100,file\n"),
        "instrument_id,bid_price,ask_price\nA,99,101\n".into(),
    ] {
        std::fs::write(file.path(), invalid).unwrap();
        assert!(source.reload().is_err());
        assert_eq!(
            source
                .get_quote(&"A".into())
                .await
                .unwrap()
                .unwrap()
                .bid_price,
            before.bid_price
        );
        assert!(source.get_quote(&"B".into()).await.unwrap().is_some());
    }
    std::fs::write(file.path(), format!("{HEADER}A,98,100,99,,2000,file\n")).unwrap();
    source.reload().unwrap();
    assert!(source.get_quote(&"B".into()).await.unwrap().is_none());
    assert_eq!(
        source
            .get_quote(&"A".into())
            .await
            .unwrap()
            .unwrap()
            .timestamp,
        2000
    );
    std::fs::remove_file(file.path()).unwrap();
    assert!(source.reload().is_err());
    assert_eq!(
        source
            .get_quote(&"A".into())
            .await
            .unwrap()
            .unwrap()
            .timestamp,
        2000
    );
}

#[tokio::test]
async fn curve_inputs_require_explicit_terms_time_and_exact_rates_and_never_drop_bad_rows() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let valid = r#"[{"curve_id":"USD.TEST","inputs":[{"tenor":"2Y","rate":"-0.0012345678901234567890123456","instrument_type":"Swap","timestamp":1000,"source":"fixture"}]}]"#;
    std::fs::write(file.path(), valid).unwrap();
    let source = JsonCurveInputSource::new(file.path()).unwrap();
    let before = source.get_curve_inputs(&"USD.TEST".into()).await.unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(
        before[0].rate.to_string(),
        "-0.0012345678901234567890123456"
    );
    assert_eq!(before[0].timestamp, 1000);
    for invalid in [
        valid.replace("2Y", "nonsense"),
        valid.replace("2Y", "0Y"),
        valid.replace("2Y", "💥"),
        valid.replace("\"-0.0012345678901234567890123456\"", "0.04"),
        valid.replace("\"instrument_type\":\"Swap\",", ""),
    ] {
        std::fs::write(file.path(), invalid).unwrap();
        assert!(source.reload().is_err());
        assert_eq!(
            source.get_curve_inputs(&"USD.TEST".into()).await.unwrap()[0].rate,
            before[0].rate
        );
    }
    std::fs::write(file.path(), "[]").unwrap();
    source.reload().unwrap();
    assert!(source
        .get_curve_inputs(&"USD.TEST".into())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn actual_fixing_records_preserve_date_order_negative_rates_and_fail_atomically() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let header = "index,date,rate,timestamp,source\n";
    std::fs::write(file.path(), format!("{header}SOFR,2026-10-02,0.04,2000,file\nSOFR,2026-10-01,-0.001,1000,file\nEURIBOR-3M,2026-10-01,0.03,1000,file\n")).unwrap();
    let source = CsvIndexFixingSource::new(file.path()).unwrap();
    let from = Date::parse("2026-10-01").unwrap();
    let to = Date::parse("2026-10-02").unwrap();
    let fixings = source
        .get_fixings(&FloatingRateIndex::Sofr, from, to)
        .await
        .unwrap();
    assert_eq!(fixings.len(), 2);
    assert_eq!(
        (fixings[0].date, fixings[0].rate, fixings[0].timestamp),
        (from, Decimal::new(-1, 3), 1000)
    );
    assert!(source
        .get_fixing(&FloatingRateIndex::Euribor(Tenor::Months(3)), from)
        .await
        .unwrap()
        .is_some());
    assert!(source
        .get_fixings(&FloatingRateIndex::Sofr, to, from)
        .await
        .is_err());
    for invalid in [
        format!("{header}SOFR,2026-10-01,0.1,3000,file\nSOFR,bad,0.04,3000,file\n"),
        format!("{header}SOFR,2026-10-01,0.1,3000,file\nSOFR,2026-10-01,0.2,3000,file\n"),
    ] {
        std::fs::write(file.path(), invalid).unwrap();
        assert!(source.reload().is_err());
        assert_eq!(
            source
                .get_fixing(&FloatingRateIndex::Sofr, from)
                .await
                .unwrap()
                .unwrap()
                .rate,
            fixings[0].rate
        );
    }
}

#[test]
fn missing_and_oversized_snapshot_files_fail_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    assert!(CsvQuoteSource::new(dir.path().join("missing")).is_err());
    assert!(JsonCurveInputSource::new(dir.path().join("missing")).is_err());
    assert!(CsvIndexFixingSource::new(dir.path().join("missing")).is_err());
    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file().set_len(16 * 1024 * 1024 + 1).unwrap();
    assert!(CsvQuoteSource::new(file.path()).is_err());
    assert!(JsonCurveInputSource::new(file.path()).is_err());
    assert!(CsvIndexFixingSource::new(file.path()).is_err());
}
