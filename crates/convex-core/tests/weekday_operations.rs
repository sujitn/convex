//! Independent Gregorian references and representable-calendar boundaries.
use convex_core::Date;
use serde_json::Value;
fn d(text: &str) -> Date {
    Date::parse(text).unwrap()
}

#[test]
fn weekend_operations_match_all_original_independent_references() {
    let pack: Value =
        serde_json::from_str(include_str!("fixtures/weekday-operations.json")).unwrap();
    let mut actual = Vec::new();
    for row in pack["offset_cases"].as_array().unwrap() {
        let answer = d(row["start"].as_str().unwrap())
            .add_business_days(row["offset"].as_i64().unwrap() as i32)
            .unwrap();
        assert_eq!(answer, d(row["expected_date"].as_str().unwrap()));
        actual.push(answer.to_string());
    }
    for row in pack["signed_count_cases"].as_array().unwrap() {
        let answer = d(row["start"].as_str().unwrap())
            .business_days_between(&d(row["end"].as_str().unwrap()))
            .unwrap();
        assert_eq!(answer, row["expected_count"].as_i64().unwrap());
        actual.push(answer.to_string());
    }
    for row in pack["weekday_cases"].as_array().unwrap() {
        let date = d(row["start"].as_str().unwrap());
        let next = date.next_weekday().unwrap();
        let previous = date.prev_weekday().unwrap();
        assert_eq!(next, d(row["next_weekday"].as_str().unwrap()));
        assert_eq!(previous, d(row["previous_weekday"].as_str().unwrap()));
        actual.extend([next.to_string(), previous.to_string()]);
    }
    assert_eq!(actual.len(), 341);
    println!(
        "{}",
        serde_json::json!({"kind": "checked_weekday_operations", "answers": actual})
    );
}

#[test]
fn extreme_offsets_and_exhausted_dates_return_errors_without_unbounded_search() {
    let ordinary = d("2025-01-06");
    assert!(ordinary.add_business_days(i32::MIN).is_err());
    assert!(ordinary.add_business_days(i32::MAX).is_err());
    let low = Date::from(chrono::NaiveDate::MIN);
    let high = Date::from(chrono::NaiveDate::MAX);
    assert!(low.add_business_days(-1).is_err());
    assert!(high.add_business_days(1).is_err());
    assert_eq!(low.add_business_days(0).unwrap(), low);
    assert_eq!(high.add_business_days(0).unwrap(), high);
    assert_eq!(high.business_days_between(&high).unwrap(), 0);
    assert!(low.business_days_between(&high).unwrap() > 0);
    assert_eq!(
        low.business_days_between(&high).unwrap(),
        -high.business_days_between(&low).unwrap()
    );
}

#[test]
fn complete_gregorian_cycles_remain_supported_without_an_invented_short_horizon() {
    let first = d("2000-01-03");
    let later = d("2400-01-03");
    assert_eq!(first.days_between(&later), 146097);
    assert_eq!(first.business_days_between(&later).unwrap(), 104355);
    assert_eq!(first.add_business_days(104355).unwrap(), later);
    assert_eq!(later.add_business_days(-104355).unwrap(), first);
}
