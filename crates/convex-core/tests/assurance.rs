//! Checked external date arithmetic must fail with an error.

use convex_core::Date;
use proptest::prelude::*;

#[test]
fn month_arithmetic_handles_negative_years_and_integer_limits() {
    let date = Date::from_ymd(0, 1, 31).unwrap();
    assert_eq!(
        date.add_months(-1).unwrap(),
        Date::from_ymd(-1, 12, 31).unwrap()
    );
    let date = Date::from_ymd(2026, 10, 4).unwrap();
    for months in [i32::MIN, i32::MAX] {
        assert!(date.add_months(months).is_err());
        assert!(date.add_years(months).is_err());
    }
}

#[test]
fn supplied_day_offsets_return_errors_at_calendar_boundaries() {
    let maximum = Date::from(chrono::NaiveDate::MAX);
    let minimum = Date::from(chrono::NaiveDate::MIN);
    assert!(maximum.checked_add_days(1).is_err());
    assert!(minimum.checked_add_days(-1).is_err());
    assert!(maximum.checked_add_days(i64::MAX).is_err());
    assert_eq!(maximum.checked_add_days(0).unwrap(), maximum);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, rng_seed: proptest::test_runner::RngSeed::Fixed(0x039), .. ProptestConfig::default() })]
    #[test]
    fn arbitrary_date_offsets_never_panic(months in any::<i32>(), years in any::<i32>(), days in any::<i64>()) {
        let date = Date::from_ymd(2026, 10, 4).unwrap();
        // The test process fails on a panic; both out-of-range and valid offsets
        // remain observable Results, without substituting a date on error.
        for result in [date.add_months(months), date.add_years(years), date.checked_add_days(days)] {
            if let Ok(value) = result { prop_assert_eq!(Date::parse(&value.to_string()).unwrap(), value); }
        }
    }
}
