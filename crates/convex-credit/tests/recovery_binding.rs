//! Recovery changes only one explicit model assumption in a complete contract.
#[path = "../../../tests/fixtures/packed.rs"]
mod packed;
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    contracts::FiniteF64,
    Date,
};
use convex_credit::{CalendarBinding, CdsCalendars, CdsTerms};
use serde_json::Value;

#[test]
fn recovery_binding_preserves_all_terms_and_actual_periods() {
    let corpus: Value = serde_json::from_str(packed::text(
        "tests/fixtures/library/recovery-hedge-admitted.json",
    ))
    .unwrap();
    let mut count = 0;
    for case in corpus["cases"].as_array().unwrap().iter().filter(|c| {
        c["expected_error"].is_null() && c["request"]["operation"]["product"]["kind"] == "cds"
    }) {
        let terms: CdsTerms =
            serde_json::from_value(case["request"]["operation"]["product"]["terms"].clone())
                .unwrap();
        let holidays: Vec<Date> = serde_json::from_value(
            case["request"]["inputs"]["calendars"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["pin"] == serde_json::to_value(&terms.premium_calendar).unwrap())
                .unwrap()["holidays"]
                .clone(),
        )
        .unwrap();
        let calendar = DynamicCalendar::from_dates(
            "actual supplied calendar",
            WeekendType::SaturdaySunday,
            holidays,
        );
        let calendars = CdsCalendars {
            premium: CalendarBinding {
                pin: &terms.premium_calendar,
                calendar: &calendar,
            },
            settlement: CalendarBinding {
                pin: &terms.settlement_calendar,
                calendar: &calendar,
            },
        };
        let contract = terms.build(&calendars).unwrap();
        for recovery in [0.0, 0.3999, 0.40005, 1.0] {
            let fraction = FiniteF64::new(recovery).unwrap();
            let scenario = contract.with_recovery_fraction(fraction).unwrap();
            let mut expected = terms.clone();
            expected.reference.recovery_fraction = fraction;
            assert_eq!(scenario.terms(), &expected);
            assert_eq!(scenario.premium_periods(), contract.premium_periods());
            assert_eq!(
                scenario.premium_periods(),
                expected.build(&calendars).unwrap().premium_periods()
            );
            assert_eq!(contract.terms(), &terms);
        }
        for recovery in [-0.0001, 1.0001] {
            assert!(contract
                .with_recovery_fraction(FiniteF64::new(recovery).unwrap())
                .is_err());
        }
        count += 1;
    }
    assert_eq!(count, 13);
}
