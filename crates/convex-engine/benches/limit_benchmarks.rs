//! P14a: one book's limit evaluation at the bounds, which sets P14c's budget per book.
//!
//! - `limits/evaluate`: 1,000 positions (`MAX_BOOK_POSITIONS`), long and short, against 64
//!   limits (`MAX_BOOK_LIMITS`) over every measure: face, net and gross value, DV01, CS01,
//!   key-rate DV01 at ten tenors, modified duration, and group value and share by issuer,
//!   country, sector and rating, on one member and on each.
//! - `limits/market_value`: the same book against one net-value limit, the fixed cost of
//!   admitting its market values.

use convex_core::contracts::{InstrumentId, UtcTimestamp};
use convex_core::Currency;
use convex_engine::limits::{evaluate_limits, LimitBook, LimitPosition, LimitTerms};
use convex_engine::ports::limits::{LimitDefinition, MAX_BOOK_LIMITS};
use convex_engine::ports::output::BondQuoteOutput;
use convex_engine::ports::reference_master::{QuantityUnit, MAX_BOOK_POSITIONS};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rust_decimal::Decimal;
use serde_json::{json, Value};

const TENORS: [&str; 10] = [
    "1Y", "2Y", "3Y", "5Y", "7Y", "10Y", "15Y", "20Y", "25Y", "30Y",
];
const COUNTRIES: [&str; 10] = ["US", "GB", "DE", "FR", "IT", "ES", "NL", "JP", "CA", "AU"];
const SECTORS: [&str; 10] = [
    "Government",
    "Agency",
    "Banks",
    "Insurance",
    "Utilities",
    "Energy",
    "Industrials",
    "Telecom",
    "Technology",
    "Consumer",
];
const RATINGS: [&str; 5] = ["AAA", "AA", "A", "BBB", "BB"];

fn quote(i: usize) -> BondQuoteOutput {
    let key_rates: Vec<Value> = TENORS
        .iter()
        .enumerate()
        .map(|(t, tenor)| json!([tenor, format!("0.{}{}", (i + t) % 9 + 1, t)]))
        .collect();
    serde_json::from_value(json!({"instrument_id":format!("BOND-{i:04}"),"currency":"USD",
        "settlement_date":"2026-11-04","clean_price_mid":format!("9{}.{:03}", i % 10, i % 997),
        "accrued_interest":format!("0.{:04}", i % 9973),"modified_duration":format!("{}.{:02}", i % 25 + 1, i % 97),
        "dv01":format!("0.0{:03}", i % 900 + 10),"cs01":format!("0.0{:03}", i % 800 + 5),
        "key_rate_durations":key_rates,"timestamp":1793750400000i64,"pricing_spec":"limits-bench",
        "source":"runtime","is_stale":false,"quality":100}))
    .unwrap()
}

fn limits() -> Vec<LimitDefinition> {
    let mut limits = vec![json!({"kind":"face","instrument":"each"})];
    for i in 0..7 {
        limits.push(json!({"kind":"face","instrument":{"one":format!("BOND-{:04}", i * 101)}}));
    }
    for kind in ["market_value", "gross_market_value"] {
        limits.extend([json!({ "kind": kind }), json!({ "kind": kind })]);
    }
    for kind in ["dv01", "cs01"] {
        limits.extend((0..4).map(|_| json!({ "kind": kind })));
    }
    limits.extend(
        TENORS
            .iter()
            .map(|t| json!({"kind":"key_rate_dv01","tenor":t})),
    );
    limits.extend([
        json!({"kind":"modified_duration"}),
        json!({"kind":"modified_duration"}),
    ]);
    for kind in ["group_market_value", "group_share"] {
        for by in ["issuer", "country", "sector", "rating"] {
            limits.push(json!({"kind":kind,"by":by,"group":"each"}));
        }
        for i in 0..3 {
            limits.push(
                json!({"kind":kind,"by":"issuer","group":{"one":format!("ISSUER-{:02}", i * 7)}}),
            );
            limits.push(json!({"kind":kind,"by":"country","group":{"one":COUNTRIES[i]}}));
            limits.push(json!({"kind":kind,"by":"sector","group":{"one":SECTORS[i]}}));
            limits.push(json!({"kind":kind,"by":"rating","group":{"one":RATINGS[i]}}));
        }
    }
    assert_eq!(limits.len(), MAX_BOOK_LIMITS);
    limits
        .into_iter()
        .enumerate()
        .map(|(i, measure)| {
            serde_json::from_value(json!({"limit_id":format!("limit-{i:02}"),"measure":measure,
                "lower":"-100000000000","upper":"100000000000","warning_fraction":"0.8"}))
            .unwrap()
        })
        .collect()
}

fn bench_limits(c: &mut Criterion) {
    let ids: Vec<InstrumentId> = (0..MAX_BOOK_POSITIONS)
        .map(|i| InstrumentId::new(format!("BOND-{i:04}")).unwrap())
        .collect();
    let quotes: Vec<BondQuoteOutput> = (0..MAX_BOOK_POSITIONS).map(quote).collect();
    let issuers: Vec<String> = (0..50).map(|i| format!("ISSUER-{i:02}")).collect();
    let positions: Vec<LimitPosition<'_>> = ids
        .iter()
        .zip(&quotes)
        .enumerate()
        .map(|(i, (id, quote))| LimitPosition {
            instrument_id: id,
            // A third of the book is short.
            quantity: Decimal::from((i as i64 % 50 + 1) * 10_000 * if i % 3 == 0 { -1 } else { 1 }),
            unit: QuantityUnit::Face,
            rating: Some(RATINGS[i % RATINGS.len()]),
            quote: Some(quote),
            terms: Some(LimitTerms {
                issuer: &issuers[i % issuers.len()],
                country: COUNTRIES[i % COUNTRIES.len()],
                sector: SECTORS[i % SECTORS.len()],
            }),
        })
        .collect();
    let book = LimitBook {
        currency: Some(Currency::USD),
        positions: &positions,
    };
    let all = limits();
    let at = UtcTimestamp::parse("2026-11-09T12:00:00Z").unwrap();
    let evaluated = evaluate_limits(&book, &all, at).unwrap();
    assert!(
        evaluated.iter().all(|e| e.reason.is_none()),
        "{evaluated:?}"
    );
    let market_value: Vec<_> = all
        .iter()
        .filter(|l| l.limit_id.as_str() == "limit-08")
        .cloned()
        .collect();

    // P15d: an excess in force on every limit, so each is judged against the excess bounds
    // and reports it.
    let excess: Vec<LimitDefinition> = all
        .iter()
        .cloned()
        .map(|mut limit| {
            limit.excess = Some(
                serde_json::from_value(json!({"lower":"-200000000000","upper":"200000000000",
                    "expires_at":"2026-11-10T12:00:00Z","reason":"a temporary mandate change"}))
                .unwrap(),
            );
            limit
        })
        .collect();
    let widened = evaluate_limits(&book, &excess, at).unwrap();
    assert!(widened.iter().all(|e| e.excess.is_some()), "{widened:?}");

    let mut group = c.benchmark_group("limits");
    group.bench_function("evaluate", |b| {
        b.iter(|| evaluate_limits(black_box(&book), black_box(&all), black_box(at)).unwrap())
    });
    group.bench_function("evaluate_excess", |b| {
        b.iter(|| evaluate_limits(black_box(&book), black_box(&excess), black_box(at)).unwrap())
    });
    group.bench_function("market_value", |b| {
        b.iter(|| {
            evaluate_limits(black_box(&book), black_box(&market_value), black_box(at)).unwrap()
        })
    });
    group.finish();
}

criterion_group!(benches, bench_limits);
criterion_main!(benches);
