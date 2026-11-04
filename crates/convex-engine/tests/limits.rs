#![forbid(unsafe_code)]
//! P14a (D83, D84): each limit measure against a hand-computed book, each status at its
//! bounds, and each reason a limit is unknown.

use convex_core::contracts::{ExactDecimal, InstrumentId};
use convex_core::Currency;
use convex_engine::limits::{evaluate_limits, LimitBook, LimitPosition, LimitTerms};
use convex_engine::portfolio_analytics::{Portfolio, PortfolioAnalyzer, Position};
use convex_engine::ports::limits::{
    InstrumentList, LimitDefinition, LimitEvaluation, LimitGroup, LimitStatus, UnknownReason,
};
use convex_engine::ports::output::BondQuoteOutput;
use convex_engine::ports::reference_master::QuantityUnit;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

fn d(text: &str) -> Decimal {
    Decimal::from_str(text).unwrap()
}

fn id(text: &str) -> InstrumentId {
    InstrumentId::new(text).unwrap()
}

/// A complete runtime quote: dirty price `clean + 1`, and per-100-face sensitivities.
fn quote(
    instrument: &str,
    clean: &str,
    dv01: &str,
    cs01: &str,
    duration: &str,
    krd_5y: &str,
) -> BondQuoteOutput {
    serde_json::from_value(json!({"instrument_id":instrument,"currency":"USD","settlement_date":"2026-11-04",
        "clean_price_mid":clean,"accrued_interest":"1","modified_duration":duration,"dv01":dv01,"cs01":cs01,
        "key_rate_durations":[["2Y","0.5"],["5Y",krd_5y]],"timestamp":1793750400000i64,
        "pricing_spec":"limits-test","source":"runtime","is_stale":false,"quality":100}))
    .unwrap()
}

fn terms(issuer: &'static str) -> Option<LimitTerms<'static>> {
    Some(LimitTerms {
        issuer,
        country: "US",
        sector: "Government",
    })
}

fn position<'a>(
    instrument: &'a InstrumentId,
    quantity: &str,
    quote: Option<&'a BondQuoteOutput>,
    terms: Option<LimitTerms<'a>>,
) -> LimitPosition<'a> {
    LimitPosition {
        instrument_id: instrument,
        quantity: d(quantity),
        unit: QuantityUnit::Face,
        rating: Some("AA"),
        quote,
        terms,
    }
}

fn limits(values: Value) -> Vec<LimitDefinition> {
    serde_json::from_value(values).unwrap()
}

fn evaluate(book: &LimitBook<'_>, values: Value) -> Vec<LimitEvaluation> {
    evaluate_limits(book, &limits(values)).unwrap()
}

fn value(evaluation: &LimitEvaluation) -> Decimal {
    evaluation.value.expect("a known value").decimal()
}

fn reason(evaluation: &LimitEvaluation) -> &UnknownReason {
    assert_eq!(evaluation.status, LimitStatus::Unknown, "{evaluation:?}");
    evaluation
        .reason
        .as_ref()
        .expect("an unknown status has a reason")
}

fn list(ids: &[&InstrumentId]) -> InstrumentList {
    InstrumentList::collect(ids.iter().copied()).unwrap()
}

/// A long position in A and a short one in B:
/// - A: 1,000,000 face at dirty 100, value 1,000,000; DV01 0.05, CS01 0.04, duration 5,
///   5Y key-rate duration 4, issuer ISS-A.
/// - B: −500,000 face at dirty 102, value −510,000; DV01 0.08, CS01 0.06, duration 6,
///   5Y key-rate duration 3, issuer ISS-B.
struct LongShort {
    a: InstrumentId,
    b: InstrumentId,
    qa: BondQuoteOutput,
    qb: BondQuoteOutput,
}

impl LongShort {
    fn new() -> Self {
        Self {
            a: id("A"),
            b: id("B"),
            qa: quote("A", "99", "0.05", "0.04", "5", "4"),
            qb: quote("B", "101", "0.08", "0.06", "6", "3"),
        }
    }

    fn positions(&self) -> Vec<LimitPosition<'_>> {
        vec![
            position(&self.a, "1000000", Some(&self.qa), terms("ISS-A")),
            position(&self.b, "-500000", Some(&self.qb), terms("ISS-B")),
        ]
    }
}

fn usd<'a>(positions: &'a [LimitPosition<'a>]) -> LimitBook<'a> {
    LimitBook {
        currency: Some(Currency::USD),
        positions,
    }
}

#[test]
fn each_measure_matches_a_hand_computed_long_short_book() {
    let book = LongShort::new();
    let positions = book.positions();
    let wide = |kind: Value| json!({"limit_id":"x","measure":kind,"lower":"-100000000","upper":"100000000"});
    let measures = [
        (json!({"kind":"market_value"}), "490000"),
        (json!({"kind":"gross_market_value"}), "1510000"),
        // 0.05 × 1,000,000 / 100 + 0.08 × −500,000 / 100.
        (json!({"kind":"dv01"}), "100"),
        // 0.04 × 1,000,000 / 100 + 0.06 × −500,000 / 100.
        (json!({"kind":"cs01"}), "100"),
        // (1,000,000 × 4 − 510,000 × 3) / 10,000.
        (json!({"kind":"key_rate_dv01","tenor":"5Y"}), "247"),
        // (1,000,000 × 5 − 510,000 × 6) / 490,000.
        (
            json!({"kind":"modified_duration"}),
            "3.9591836734693877551020408163",
        ),
        (
            json!({"kind":"group_market_value","by":"issuer","group":{"one":"ISS-B"}}),
            "-510000",
        ),
        (
            json!({"kind":"group_market_value","by":"issuer","group":{"one":"ISS-C"}}),
            "0",
        ),
        (
            json!({"kind":"group_share","by":"issuer","group":{"one":"ISS-A"}}),
            "0.6622516556291390728476821192",
        ),
        (
            json!({"kind":"group_share","by":"rating","group":{"one":"AA"}}),
            "0.3245033112582781456953642384",
        ),
        (json!({"kind":"face","instrument":{"one":"B"}}), "-500000"),
        (json!({"kind":"face","instrument":{"one":"C"}}), "0"),
    ];
    for (kind, expected) in measures {
        let evaluation = &evaluate(&usd(&positions), json!([wide(kind.clone())]))[0];
        assert_eq!(
            evaluation.status,
            LimitStatus::Within,
            "{kind}: {evaluation:?}"
        );
        assert_eq!(value(evaluation), d(expected), "{kind}");
        assert!(evaluation.member.is_none() && evaluation.reason.is_none());
    }
}

/// The limits read the numbers analytics by book return for the same positions.
#[test]
fn market_value_dv01_and_duration_are_the_analyzers() {
    let quotes: Vec<_> = (0..5)
        .map(|i| {
            let n = i.to_string();
            let mut q = quote(
                &format!("P{i}"),
                &format!("9{n}.125"),
                &format!("0.0{}", i + 3),
                "0.02",
                &format!("{}.5", i + 2),
                "1.5",
            );
            // The analyzer also requires these.
            q.convexity = Some(d("30"));
            q.ytm_mid = Some(d("0.05"));
            q.z_spread_mid = Some(d("100"));
            q
        })
        .collect();
    let ids: Vec<_> = (0..5).map(|i| id(&format!("P{i}"))).collect();
    let quantities = ["250000", "1500000", "730000", "12000", "990000"];
    let positions: Vec<_> = ids
        .iter()
        .zip(&quotes)
        .zip(quantities)
        .map(|((i, q), quantity)| position(i, quantity, Some(q), terms("ISS")))
        .collect();
    let portfolio = Portfolio {
        portfolio_id: convex_core::ids::PortfolioId::new("book"),
        name: "book".into(),
        currency: Currency::USD,
        positions: ids
            .iter()
            .zip(quantities)
            .map(|(i, quantity)| Position {
                instrument_id: convex_core::ids::InstrumentId::new(i.as_str()),
                notional: d(quantity),
                sector: None,
                rating: None,
            })
            .collect(),
    };
    let analytics = PortfolioAnalyzer::new()
        .calculate(&portfolio, &quotes)
        .unwrap();
    let evaluations = evaluate(
        &usd(&positions),
        json!([
            {"limit_id":"mv","measure":{"kind":"market_value"},"upper":"1000000000"},
            {"limit_id":"dv01","measure":{"kind":"dv01"},"upper":"1000000000"},
            {"limit_id":"md","measure":{"kind":"modified_duration"},"upper":"100"}
        ]),
    );
    assert_eq!(value(&evaluations[0]), analytics.market_value);
    assert_eq!(value(&evaluations[1]), analytics.dv01);
    assert_eq!(value(&evaluations[2]), analytics.duration);
}

#[test]
fn a_value_at_a_bound_is_within_it_and_one_just_past_is_not() {
    let book = LongShort::new();
    let positions = book.positions();
    let book = usd(&positions);
    let status = |limit: Value| evaluate(&book, json!([limit]))[0].status;
    let dv01 = |bounds: Value| {
        let mut limit = json!({"limit_id":"dv01","measure":{"kind":"dv01"}});
        limit
            .as_object_mut()
            .unwrap()
            .extend(bounds.as_object().unwrap().clone());
        limit
    };
    // DV01 is exactly 100.
    assert_eq!(status(dv01(json!({"upper":"100"}))), LimitStatus::Within);
    assert_eq!(status(dv01(json!({"upper":"99.99"}))), LimitStatus::Breach);
    assert_eq!(status(dv01(json!({"lower":"100"}))), LimitStatus::Within);
    assert_eq!(status(dv01(json!({"lower":"100.01"}))), LimitStatus::Breach);
    // The band of 0.5 × 200 starts at 100: at it is within, past it a warning.
    assert_eq!(
        status(dv01(json!({"upper":"200","warning_fraction":"0.5"}))),
        LimitStatus::Within
    );
    assert_eq!(
        status(dv01(json!({"upper":"199.98","warning_fraction":"0.5"}))),
        LimitStatus::Warning
    );
    // A lower band: ISS-B's −510,000 against −1,020,000 × 0.5, then −1,019,998 × 0.5.
    let issuer_b = |lower: &str| {
        json!({"limit_id":"b","measure":{"kind":"group_market_value","by":"issuer","group":{"one":"ISS-B"}},
            "lower":lower,"upper":"1","warning_fraction":"0.5"})
    };
    assert_eq!(status(issuer_b("-1020000")), LimitStatus::Within);
    assert_eq!(status(issuer_b("-1019998")), LimitStatus::Warning);
    assert_eq!(status(issuer_b("-510000")), LimitStatus::Warning);
    assert_eq!(status(issuer_b("-509999.99")), LimitStatus::Breach);

    // A ratio is compared exactly, not as its rounded report: the duration lies strictly
    // between two adjacent Decimals, so exactly one bound at the reported value holds.
    let reported = value(
        &evaluate(
            &book,
            json!([{"limit_id":"md","measure":{"kind":"modified_duration"},"upper":"10"}]),
        )[0],
    );
    let upper = status(
        json!({"limit_id":"md","measure":{"kind":"modified_duration"},"upper":reported.to_string()}),
    );
    let lower = status(
        json!({"limit_id":"md","measure":{"kind":"modified_duration"},"lower":reported.to_string()}),
    );
    assert_ne!(upper, lower);
    assert!([upper, lower].contains(&LimitStatus::Breach));
}

#[test]
fn utilisation_is_the_value_over_the_bound_that_faces_away_from_zero() {
    let book = LongShort::new();
    let positions = book.positions();
    let evaluations = evaluate(
        &usd(&positions),
        json!([
            {"limit_id":"up","measure":{"kind":"dv01"},"upper":"300"},
            {"limit_id":"both","measure":{"kind":"group_market_value","by":"issuer","group":{"one":"ISS-B"}},"lower":"-600000","upper":"2000000"},
            {"limit_id":"away","measure":{"kind":"dv01"},"lower":"50"}
        ]),
    );
    let utilisation = |e: &LimitEvaluation| e.utilisation.map(ExactDecimal::decimal);
    assert_eq!(utilisation(&evaluations[0]), Some(d("0.33333333")));
    // −510,000 / −600,000 beats −510,000 / 2,000,000.
    assert_eq!(utilisation(&evaluations[1]), Some(d("0.85")));
    // A lower bound above zero does not face away from it.
    assert_eq!(utilisation(&evaluations[2]), None);
    assert_eq!(evaluations[2].status, LimitStatus::Within);
}

#[test]
fn a_limit_on_each_member_reports_the_most_extreme() {
    let (a, b, c) = (id("A"), id("B"), id("C"));
    let qa = quote("A", "99", "0.05", "0.04", "5", "4");
    let qb = quote("B", "99", "0.05", "0.04", "5", "4");
    let qc = quote("C", "99", "0.05", "0.04", "5", "4");
    let positions = [
        position(&a, "100", Some(&qa), terms("ISS-A")),
        position(&b, "-300", Some(&qb), terms("ISS-B")),
        position(&c, "250", Some(&qc), terms("ISS-A")),
    ];
    let evaluations = evaluate(
        &usd(&positions),
        json!([
            {"limit_id":"face","measure":{"kind":"face","instrument":"each"},"lower":"-200","upper":"200"},
            {"limit_id":"face-up","measure":{"kind":"face","instrument":"each"},"upper":"240","warning_fraction":"0.5"},
            {"limit_id":"issuer","measure":{"kind":"group_market_value","by":"issuer","group":"each"},"upper":"300"}
        ]),
    );
    // −300 breaches the lower bound; 250 breaches only the upper, less far.
    assert_eq!(evaluations[0].status, LimitStatus::Breach);
    assert_eq!(evaluations[0].member.as_deref(), Some("B"));
    assert_eq!(value(&evaluations[0]), d("-300"));
    assert_eq!(evaluations[1].member.as_deref(), Some("C"));
    assert_eq!(evaluations[1].status, LimitStatus::Breach);
    // ISS-A holds 350 of value, ISS-B −300.
    assert_eq!(evaluations[2].status, LimitStatus::Breach);
    assert_eq!(evaluations[2].member.as_deref(), Some("ISS-A"));
    assert_eq!(value(&evaluations[2]), d("350"));
}

#[test]
fn every_missing_input_makes_its_limits_unknown_and_face_limits_still_evaluate() {
    let book = LongShort::new();
    let face = json!({"limit_id":"face","measure":{"kind":"face","instrument":{"one":"A"}},"upper":"2000000"});
    let mv = json!({"limit_id":"mv","measure":{"kind":"market_value"},"upper":"1"});

    // No reporting currency.
    let positions = book.positions();
    let none = LimitBook {
        currency: None,
        positions: &positions,
    };
    let evaluations = evaluate(&none, json!([mv, face]));
    assert_eq!(
        reason(&evaluations[0]),
        &UnknownReason::BookCurrencyRequired {}
    );
    assert_eq!(evaluations[1].status, LimitStatus::Within);

    // A position in units: market measures, and a face limit on it, are unknown.
    let mut units = book.positions();
    units[1].unit = QuantityUnit::Units;
    let evaluations = evaluate(
        &usd(&units),
        json!([mv, face,
            {"limit_id":"face-b","measure":{"kind":"face","instrument":{"one":"B"}},"upper":"1"},
            {"limit_id":"each","measure":{"kind":"face","instrument":"each"},"upper":"2000000"},
            {"limit_id":"each-breach","measure":{"kind":"face","instrument":"each"},"upper":"10"}]),
    );
    let units_b = UnknownReason::UnsupportedQuantityUnit {
        instruments: list(&[&book.b]),
    };
    assert_eq!(reason(&evaluations[0]), &units_b);
    assert_eq!(evaluations[1].status, LimitStatus::Within);
    assert_eq!(reason(&evaluations[2]), &units_b);
    assert_eq!(reason(&evaluations[3]), &units_b);
    // A known breach outranks the member that cannot be seen.
    assert_eq!(evaluations[4].status, LimitStatus::Breach);
    assert_eq!(evaluations[4].member.as_deref(), Some("A"));

    // No accepted result for B.
    let mut missing = book.positions();
    missing[1].quote = None;
    let evaluations = evaluate(&usd(&missing), json!([mv, face]));
    assert_eq!(
        reason(&evaluations[0]),
        &UnknownReason::MissingResult {
            instruments: list(&[&book.b])
        }
    );
    assert_eq!(evaluations[1].status, LimitStatus::Within);

    // A result in another currency, then a stale one.
    let mut eur = book.qb.clone();
    eur.currency = Currency::EUR;
    let mut foreign = book.positions();
    foreign[1].quote = Some(&eur);
    assert_eq!(
        reason(&evaluate(&usd(&foreign), json!([mv]))[0]),
        &UnknownReason::ForeignCurrency {
            instruments: list(&[&book.b])
        }
    );
    let mut stale = book.qb.clone();
    stale.is_stale = true;
    let mut refused = book.positions();
    refused[1].quote = Some(&stale);
    match reason(&evaluate(&usd(&refused), json!([mv]))[0]) {
        UnknownReason::QuoteRefused { instrument, detail } => {
            assert_eq!(instrument, &book.b);
            assert!(detail.contains("non-stale"), "{detail}");
        }
        other => panic!("{other:?}"),
    }

    // A measure one result lacks is unknown; the others are known.
    let mut no_cs01 = book.qb.clone();
    no_cs01.cs01 = None;
    let mut partial = book.positions();
    partial[1].quote = Some(&no_cs01);
    let evaluations = evaluate(
        &usd(&partial),
        json!([
            {"limit_id":"cs01","measure":{"kind":"cs01"},"upper":"1000"},
            {"limit_id":"dv01","measure":{"kind":"dv01"},"upper":"1000"},
            {"limit_id":"krd","measure":{"kind":"key_rate_dv01","tenor":"7Y"},"upper":"1000"}
        ]),
    );
    assert_eq!(
        reason(&evaluations[0]),
        &UnknownReason::MeasureNotComputed {
            measure: "cs01_mid".into(),
            instruments: list(&[&book.b])
        }
    );
    assert_eq!(evaluations[1].status, LimitStatus::Within);
    assert_eq!(
        reason(&evaluations[2]),
        &UnknownReason::MeasureNotComputed {
            measure: "krd_7y_mid".into(),
            instruments: list(&[&book.a, &book.b])
        }
    );

    // A blank issuer: every issuer limit is unknown, a rating limit is known.
    let mut blank = book.positions();
    blank[0].terms = Some(LimitTerms {
        issuer: " ",
        country: "US",
        sector: "Government",
    });
    let evaluations = evaluate(
        &usd(&blank),
        json!([
            {"limit_id":"one","measure":{"kind":"group_share","by":"issuer","group":{"one":"ISS-B"}},"upper":"1"},
            {"limit_id":"each","measure":{"kind":"group_market_value","by":"issuer","group":"each"},"upper":"1"},
            {"limit_id":"rating","measure":{"kind":"group_share","by":"rating","group":"each"},"upper":"1"}
        ]),
    );
    let issuer = UnknownReason::MissingClassification {
        by: LimitGroup::Issuer,
        instruments: list(&[&book.a]),
    };
    assert_eq!(reason(&evaluations[0]), &issuer);
    assert_eq!(reason(&evaluations[1]), &issuer);
    assert_eq!(evaluations[2].status, LimitStatus::Within);
}

#[test]
fn zero_denominators_and_an_empty_book_are_explicit() {
    // Long and short the same value: net value is zero.
    let (a, b) = (id("A"), id("B"));
    let qa = quote("A", "99", "0.05", "0.04", "5", "4");
    let qb = quote("B", "99", "0.05", "0.04", "6", "4");
    let flat = [
        position(&a, "1000", Some(&qa), terms("ISS-A")),
        position(&b, "-1000", Some(&qb), terms("ISS-B")),
    ];
    let duration = json!({"limit_id":"md","measure":{"kind":"modified_duration"},"upper":"10"});
    assert_eq!(
        reason(&evaluate(&usd(&flat), json!([duration]))[0]),
        &UnknownReason::ZeroNetValue {}
    );

    let empty = usd(&[]);
    let evaluations = evaluate(
        &empty,
        json!([
            {"limit_id":"mv","measure":{"kind":"market_value"},"upper":"10"},
            duration,
            {"limit_id":"share","measure":{"kind":"group_share","by":"issuer","group":{"one":"X"}},"upper":"1"},
            {"limit_id":"each","measure":{"kind":"group_market_value","by":"issuer","group":"each"},"upper":"1"},
            {"limit_id":"face","measure":{"kind":"face","instrument":"each"},"upper":"1"}
        ]),
    );
    assert_eq!(evaluations[0].status, LimitStatus::Within);
    assert_eq!(value(&evaluations[0]), Decimal::ZERO);
    assert_eq!(reason(&evaluations[1]), &UnknownReason::ZeroNetValue {});
    assert_eq!(reason(&evaluations[2]), &UnknownReason::ZeroGrossValue {});
    for vacuous in &evaluations[3..] {
        assert_eq!(vacuous.status, LimitStatus::Within);
        assert!(vacuous.value.is_none() && vacuous.member.is_none());
    }
}

#[test]
fn a_reason_names_a_bounded_prefix_and_the_full_count() {
    let ids: Vec<_> = (0..20).map(|i| id(&format!("M{i:02}"))).collect();
    let positions: Vec<_> = ids.iter().map(|i| position(i, "1", None, None)).collect();
    let evaluation = &evaluate(
        &usd(&positions),
        json!([{"limit_id":"mv","measure":{"kind":"market_value"},"upper":"1"}]),
    )[0];
    let UnknownReason::MissingResult { instruments } = reason(evaluation) else {
        panic!("{evaluation:?}");
    };
    assert_eq!(instruments.count, 20);
    assert_eq!(instruments.first, ids[..16]);
}

#[test]
fn input_outside_the_contract_is_refused() {
    let book = LongShort::new();
    let mut twice = book.positions();
    twice[1].instrument_id = &book.a;
    let dv01 = json!([{"limit_id":"dv01","measure":{"kind":"dv01"},"upper":"1"}]);
    assert!(evaluate_limits(&usd(&twice), &limits(dv01.clone())).is_err());

    let ids: Vec<_> = (0..1001).map(|i| id(&format!("N{i}"))).collect();
    let many: Vec<_> = ids.iter().map(|i| position(i, "1", None, None)).collect();
    assert!(evaluate_limits(&usd(&many), &limits(dv01.clone())).is_err());
    assert!(evaluate_limits(&usd(&many[..1000]), &limits(dv01)).is_ok());

    let invalid = limits(json!([{"limit_id":"dv01","measure":{"kind":"dv01"}}]));
    let positions = book.positions();
    assert!(evaluate_limits(&usd(&positions), &invalid).is_err());
}
