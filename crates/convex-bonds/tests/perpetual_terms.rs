//! Perpetual terms: version 1 documents unchanged, a perpetual only through
//! `build_perpetual`, its admission, and the dated cash it pays through a horizon.
use convex_bonds::contracts::{BondContractError, BondRedemption, BondTerms, PerpetualContract};
use convex_core::{cashflows::CashflowError, Date};
use rust_decimal_macros::dec;
use serde_json::{json, Value};

fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}

fn weekends() -> Box<dyn convex_core::calendars::Calendar> {
    convex_bonds::types::CalendarId::new("WEEKEND")
        .try_to_calendar()
        .unwrap()
}

fn perpetual(bond: usize) -> Value {
    let reference: Value = serde_json::from_str(include_str!(
        "../../convex-analytics/tests/fixtures/perpetual-quotes.json"
    ))
    .unwrap();
    reference["bonds"][bond]["terms"].clone()
}

fn build(terms: Value) -> Result<PerpetualContract, BondContractError> {
    serde_json::from_value::<BondTerms>(terms)
        .unwrap()
        .build_perpetual()
}

fn refused_field(terms: Value) -> String {
    match build(terms) {
        Err(BondContractError::Cashflow(CashflowError::InvalidTerms { field, .. })) => {
            field.to_string()
        }
        Err(BondContractError::Unsupported { feature, .. }) => feature.to_string(),
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn a_version_1_document_reads_and_writes_unchanged() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/bond-contracts.json")).unwrap();
    let mut documents = vec![];
    fn collect(value: &Value, documents: &mut Vec<Value>) {
        match value {
            Value::Object(map)
                if map.contains_key("schema_version") && map.contains_key("profiles") =>
            {
                documents.push(value.clone())
            }
            Value::Object(map) => map.values().for_each(|v| collect(v, documents)),
            Value::Array(items) => items.iter().for_each(|v| collect(v, documents)),
            _ => {}
        }
    }
    collect(&fixture, &mut documents);
    assert!(!documents.is_empty());
    for document in documents {
        let terms: BondTerms = serde_json::from_value(document.clone()).unwrap();
        assert_eq!(terms.redemption, BondRedemption::Dated);
        // It writes the keys it was read with, no `redemption`, and reads back the same.
        let written = serde_json::to_value(&terms).unwrap();
        let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        assert_eq!(keys(&written), keys(&document));
        assert_eq!(serde_json::from_value::<BondTerms>(written).unwrap(), terms);
    }
}

#[test]
fn a_perpetual_round_trips_and_builds_only_as_a_perpetual() {
    for bond in [0, 1] {
        let document = perpetual(bond);
        let terms: BondTerms = serde_json::from_value(document.clone()).unwrap();
        assert_eq!(serde_json::to_value(&terms).unwrap(), document);
        let contract = terms.clone().build_perpetual().unwrap();
        assert_eq!(contract.first_call(), date("2030-01-15"));
        assert_eq!(contract.terms(), &terms);
        // The first call is not a maturity: a perpetual never builds a dated contract.
        assert!(matches!(
            terms.build(),
            Err(BondContractError::Cashflow(CashflowError::InvalidTerms {
                field: "redemption",
                ..
            }))
        ));
    }
    // A dated bond is not a perpetual.
    let mut dated = perpetual(0);
    dated.as_object_mut().unwrap().remove("redemption");
    assert_eq!(refused_field(dated), "redemption");
    // Unknown fields are refused in the redemption kind as everywhere else.
    let mut unknown = perpetual(0);
    unknown["redemption"]["call"]["make_whole"] = json!(true);
    assert!(serde_json::from_value::<BondTerms>(unknown).is_err());
    let mut dated_with_fields = perpetual(0);
    dated_with_fields["redemption"] = json!({"kind": "dated", "first_call": "2030-01-15"});
    assert!(serde_json::from_value::<BondTerms>(dated_with_fields).is_err());
}

fn refused_after(bond: usize, edit: impl FnOnce(&mut Value)) -> String {
    let mut terms = perpetual(bond);
    edit(&mut terms);
    refused_field(terms)
}

#[test]
fn admission_refuses_what_a_perpetual_here_cannot_be() {
    let refused = |field: &str, edit: fn(&mut Value)| assert_eq!(refused_after(0, edit), field);
    refused("schema_version", |t| t["schema_version"] = json!(1));
    refused("perpetual day count", |t| t["day_count"] = json!("Act360"));
    refused("perpetual day count", |t| {
        t["day_count"] = json!("Thirty360EIsda")
    });
    refused("floating perpetual", |t| {
        t["coupon"] = json!({
            "kind": "overnight",
            "index_id": "USD.SOFR",
            "convention": {
                "lookback_business_days": 0,
                "lockout_business_days": 0,
                "observation_shift": false
            }
        })
    });
    refused("perpetual step", |t| {
        t["profiles"]["coupon"] = json!([
            {"start": "2025-01-15", "end": "2027-01-15", "value": "0.065"},
            {"start": "2027-01-15", "end": "2030-01-15", "value": "0.075"}
        ])
    });
    refused("perpetual step", |t| {
        t["profiles"]["notional"] = json!([
            {"start": "2025-01-15", "end": "2027-01-15", "value": "1000000"},
            {"start": "2027-01-15", "end": "2030-01-15", "value": "500000"}
        ])
    });
    refused("profiles", |t| {
        t["profiles"]["spread"][0]["value"] = json!("0.01")
    });
    refused("redemption", |t| t["redemption_multiplier"] = json!("1.01"));
    refused("redemption", |t| {
        t["exercise"] = json!([{
            "right": "call",
            "style": {"style": "european", "date": "2028-01-15"},
            "price_per_100": "100",
            "notice_offset": {"CalendarDays": -30},
            "settlement_lag": {"CalendarDays": 0},
            "settlement_adjustment": "Following",
            "accrued": "pay_accrued",
            "protection_end": null
        }])
    });
    refused("schedule", |t| {
        t["schedule"]["stub_type"] = json!("ShortLast")
    });
    refused("call.interval_months", |t| {
        t["redemption"]["call"]["interval_months"] = json!(4)
    });
    refused("exercise.offset", |t| {
        t["redemption"]["call"]["notice_offset"] = json!({"CalendarDays": 5})
    });
    let refused = |field: &str, edit: fn(&mut Value)| assert_eq!(refused_after(1, edit), field);
    // Before the first call, and off the coupon grid.
    refused("reset.first_reset", |t| {
        t["redemption"]["reset"]["first_reset"] = json!("2029-07-15")
    });
    refused("reset.first_reset", |t| {
        t["redemption"]["reset"]["first_reset"] = json!("2030-04-15")
    });
    refused("reset", |t| {
        t["redemption"]["reset"]["interval_months"] = json!(0)
    });
    refused("reset", |t| {
        t["redemption"]["reset"]["margin"] = json!("1.5")
    });
}

#[test]
fn calls_and_resets_recur_from_the_first_call() {
    let reset = build(perpetual(1)).unwrap();
    let calendar = weekends();
    assert_eq!(
        reset
            .call_dates(date("2029-12-31"), date("2031-01-15"))
            .unwrap(),
        ["2030-01-15", "2030-07-15", "2031-01-15"].map(date)
    );
    let resets = reset.resets(date("2040-01-15"), calendar.as_ref()).unwrap();
    assert_eq!(
        resets.iter().map(|r| r.reset_date).collect::<Vec<_>>(),
        ["2030-01-15", "2035-01-15"].map(date)
    );
    // Two business days before: Tuesday 15th to Friday 11th, Monday 15th to Thursday 11th.
    assert_eq!(
        resets.iter().map(|r| r.fixing_date).collect::<Vec<_>>(),
        ["2030-01-11", "2035-01-11"].map(date)
    );
    assert_eq!(
        reset.coupon_date_on_or_after(date("2031-02-20")).unwrap(),
        date("2031-07-15")
    );
    assert_eq!(reset.regular_coupon(dec!(0.0797)).unwrap(), dec!(39850));
}

#[test]
fn the_dated_composition_pays_each_reset_and_is_callable_before_its_end() {
    let reset = build(perpetual(1)).unwrap();
    let calendar = weekends();
    let calendars = convex_bonds::contracts::BondCalendars {
        payment: calendar.as_ref(),
        reset: calendar.as_ref(),
        observation: calendar.as_ref(),
        exercise: calendar.as_ref(),
    };
    let rates = [dec!(0.0362), dec!(0.0385)];
    let dated = reset
        .dated_through(date("2035-07-15"), date("2035-07-15"), &rates, &calendars)
        .unwrap();
    let coupon = &dated.terms().profiles.coupon;
    assert_eq!(
        coupon
            .iter()
            .map(|p| (p.start, p.end, p.value))
            .collect::<Vec<_>>(),
        [
            (date("2025-01-15"), date("2030-01-15"), dec!(0.07)),
            (date("2030-01-15"), date("2035-01-15"), dec!(0.0774)),
            (date("2035-01-15"), date("2035-07-15"), dec!(0.0797)),
        ]
    );
    let calls = match &dated.terms().exercise[..] {
        [call] => match &call.style {
            convex_bonds::contracts::ExerciseStyle::Bermudan { dates } => dates.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    assert_eq!(calls.first(), Some(&date("2030-01-15")));
    assert_eq!(calls.last(), Some(&date("2035-01-15")));
    assert_eq!(calls.len(), 11);
    // Calls stop at the last one quoted.
    let to_2031 = reset
        .dated_through(date("2035-07-15"), date("2031-01-15"), &rates, &calendars)
        .unwrap();
    match &to_2031.terms().exercise[0].style {
        convex_bonds::contracts::ExerciseStyle::Bermudan { dates } => assert_eq!(
            dates.as_slice(),
            ["2030-01-15", "2030-07-15", "2031-01-15"].map(date)
        ),
        other => panic!("{other:?}"),
    }
    // Not a regular coupon date, or not the resets before it.
    assert!(reset
        .dated_through(date("2035-08-15"), date("2035-08-15"), &rates, &calendars)
        .is_err());
    assert!(reset
        .dated_through(
            date("2035-07-15"),
            date("2035-07-15"),
            &rates[..1],
            &calendars
        )
        .is_err());
    assert!(reset
        .dated_through(date("2030-01-15"), date("2030-01-15"), &[], &calendars)
        .is_err());
}

/// The fixed perpetual moved to another coupon day: accrual from the same day in 2025, a
/// first call on it in 2030.
fn on_day(month_day: &str, end_of_month: bool) -> Value {
    let (start, end) = (format!("2025-{month_day}"), format!("2030-{month_day}"));
    let mut terms = perpetual(0);
    terms["issue_date"] = json!(start);
    terms["schedule"]["start_date"] = json!(start);
    terms["schedule"]["end_date"] = json!(end);
    terms["schedule"]["end_of_month"] = json!(end_of_month);
    for profile in ["coupon", "spread", "gearing", "notional"] {
        terms["profiles"][profile][0]["start"] = json!(start);
        terms["profiles"][profile][0]["end"] = json!(end);
    }
    terms
}

#[test]
fn coupon_dates_roll_by_whole_months_from_the_first_call() {
    // A 30 August call would clamp to 28 February, off the grid a schedule generated back
    // from a later coupon date has.
    assert_eq!(refused_field(on_day("08-30", false)), "schedule");
    assert_eq!(refused_field(on_day("06-28", true)), "schedule");
    // Month end, rolled to month end.
    let month_end = build(on_day("06-30", true)).unwrap();
    assert_eq!(
        month_end
            .call_dates(date("2030-06-01"), date("2031-12-31"))
            .unwrap(),
        ["2030-06-30", "2030-12-31", "2031-06-30", "2031-12-31"].map(date)
    );
    assert!(build(on_day("08-28", false)).is_ok());
}
