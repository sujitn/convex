//! Average life against `tests/scripts/average_life_reference.py`, which works each case
//! in exact rationals from the stated dates and amounts, and the refusals.

use std::str::FromStr;

use convex_bonds::contracts::BondContractError;
use convex_bonds::instruments::{
    FixedRateBond, SinkingFundBond, SinkingFundPayment, SinkingFundSchedule,
};
use convex_core::cashflows::CashflowError;
use convex_core::Date;
use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
    refused: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    face: String,
    maturity: String,
    settlement: String,
    sinks: Vec<Sink>,
    years: Option<String>,
}

#[derive(Deserialize)]
struct Sink {
    date: String,
    percent: String,
    price: String,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/average-life.json")).unwrap()
}

fn d(text: &str) -> Date {
    Date::parse(text).unwrap()
}

/// 6% semiannual 30/360 US corporate, issued 15 June 2019: the fixture's bond.
fn bullet(case: &Case) -> FixedRateBond {
    FixedRateBond::builder()
        .cusip_unchecked("AVERAGE-LIFE")
        .coupon_percent(6.0)
        .issue_date(d("2019-06-15"))
        .maturity(d(&case.maturity))
        .us_corporate()
        .face_value(Decimal::from_str(&case.face).unwrap())
        .build()
        .unwrap()
}

fn sinker(case: &Case) -> SinkingFundBond {
    let schedule = case
        .sinks
        .iter()
        .fold(SinkingFundSchedule::new(), |schedule, sink| {
            schedule.with_payment(SinkingFundPayment::with_price(
                d(&sink.date),
                sink.percent.parse().unwrap(),
                sink.price.parse().unwrap(),
            ))
        });
    SinkingFundBond::new(bullet(case), schedule)
}

fn average_life(case: &Case) -> Result<Decimal, BondContractError> {
    let settlement = d(&case.settlement);
    let life = if case.sinks.is_empty() {
        bullet(case)
            .contract_terms()?
            .build()?
            .average_life(settlement)?
    } else {
        sinker(case).average_life(settlement)?
    };
    assert_eq!(life.settlement(), settlement, "{}", case.name);
    Ok(life.years())
}

#[test]
fn each_average_life_is_the_references() {
    let fixture = fixture();
    assert_eq!(fixture.cases.len(), 9);
    for case in &fixture.cases {
        let expected = Decimal::from_str(case.years.as_deref().unwrap()).unwrap();
        assert_eq!(average_life(case).unwrap(), expected, "{}", case.name);
    }
}

#[test]
fn a_redemption_paying_nothing_still_retires_its_face() {
    let fixture = fixture();
    let case = fixture.cases.iter().find(|c| c.name == "bullet").unwrap();
    let mut terms = bullet(case).contract_terms().unwrap();
    terms.redemption_multiplier = Decimal::ZERO;
    let life = terms
        .build()
        .unwrap()
        .average_life(d(&case.settlement))
        .unwrap();
    assert_eq!(life.years(), average_life(case).unwrap());
}

#[test]
fn no_principal_outstanding_is_refused() {
    let fixture = fixture();
    assert_eq!(fixture.refused.len(), 3);
    for case in &fixture.refused {
        assert!(case.years.is_none());
        match average_life(case) {
            Err(BondContractError::Cashflow(CashflowError::InvalidTerms { field, .. })) => {
                assert_eq!(field, "settlement", "{}", case.name);
            }
            other => panic!("{}: {other:?}", case.name),
        }
    }
}

#[test]
fn a_settlement_after_accrual_starts_but_before_issue_is_refused() {
    let fixture = fixture();
    let case = fixture.cases.iter().find(|c| c.name == "bullet").unwrap();
    let mut terms = bullet(case).contract_terms().unwrap();
    // Accrual starts on 15 June 2019; the bond is issued two weeks later.
    terms.issue_date = d("2019-07-01");
    match terms.build().unwrap().average_life(d("2019-06-20")) {
        Err(BondContractError::Cashflow(CashflowError::InvalidTerms { field, .. })) => {
            assert_eq!(field, "settlement");
        }
        other => panic!("{other:?}"),
    }
}
