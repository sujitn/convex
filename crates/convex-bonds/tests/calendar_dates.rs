//! Native checked calendar dates and exact supplied holiday-set controls.
use convex_bonds::{
    instruments::FloatingRateNote,
    traits::FloatingCouponBond,
    types::{CalendarId, RateIndex},
};
use convex_core::{
    calendars::{checked, UKCalendar, WeekendCalendar},
    daycounts::DayCountConvention,
    types::{Currency, Date, Frequency},
};

fn d(text: &str) -> Date {
    Date::parse(text).unwrap()
}

#[test]
fn native_floating_fixing_dates_use_the_supplied_uk_calendar() {
    let pack: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/floating-calendar-time.json")).unwrap();
    for row in pack["cases"].as_array().unwrap() {
        let reset = row["reset_date"].as_str().unwrap();
        let lag = row["reset_lag"].as_i64().unwrap() as i32;
        let expected = row["expected_fixing_date"].as_str().unwrap();
        let instrument = FloatingRateNote::builder()
            .cusip_unchecked("CALENDAR-CONTROL")
            .issue_date(d("2025-01-02"))
            .maturity(d("2027-01-02"))
            .index(RateIndex::Sonia)
            .currency(Currency::GBP)
            .frequency(Frequency::Quarterly)
            .day_count(DayCountConvention::Act365Fixed)
            .calendar(CalendarId::uk())
            .reset_lag(lag)
            .build()
            .unwrap();
        assert_eq!(instrument.fixing_date(d(reset)).unwrap(), d(expected));
    }
}

#[test]
fn checked_holiday_search_rejects_dates_outside_actual_bitmap_coverage() {
    let calendar = UKCalendar::new();
    for input in ["1969-12-31", "2101-01-01"] {
        assert!(
            checked::business_day_on_or_after(d(input), &calendar).is_err(),
            "{input}"
        );
    }
}

#[test]
fn checked_offsets_reject_when_they_need_unsupported_holiday_coverage() {
    let calendar = UKCalendar::new();
    assert!(checked::add_business_days(d("2100-12-31"), 1, &calendar).is_err());
    assert!(checked::add_business_days(d("1970-01-01"), -1, &calendar).is_err());
}

#[test]
fn weekend_only_rule_does_not_depend_on_a_finite_holiday_bitmap() {
    assert_eq!(
        checked::business_day_on_or_after(d("2101-01-01"), &WeekendCalendar).unwrap(),
        d("2101-01-03")
    );
}

#[test]
fn supplied_holidays_survive_outside_the_builtin_bitmap_range() {
    use convex_core::calendars::{Calendar, DynamicCalendar, WeekendType};
    let pack: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/supplied-wide-holidays.json")).unwrap();
    let holidays: Vec<_> = pack["input_holidays"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| d(value.as_str().unwrap()))
        .collect();
    let original =
        DynamicCalendar::from_dates("SuppliedWide", WeekendType::SaturdaySunday, holidays);
    assert_eq!(
        original.holiday_count(),
        pack["expected_holiday_count"].as_u64().unwrap() as usize
    );
    for holiday in ["1969-12-31", "2101-01-03"] {
        assert!(original.is_holiday_date(d(holiday)), "{holiday}");
        assert!(!original.is_business_day(d(holiday)), "{holiday}");
    }
    let data = original.to_calendar_data();
    assert_eq!(
        data.holidays.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["1969-12-31", "2101-01-03"]
    );
    let restored = DynamicCalendar::from_calendar_data(data).unwrap();
    let mut merged = DynamicCalendar::new("MergedWide", WeekendType::SaturdaySunday);
    merged.merge(&restored);
    assert_eq!(merged.holiday_count(), 2);
    assert!(!merged.is_business_day(d("2101-01-03")));
}

#[test]
fn builtin_calendar_coverage_and_joint_components_are_checked() {
    use convex_core::calendars::{
        Calendar, JapanCalendar, JointCalendar, SIFMACalendar, Target2Calendar,
        USGovernmentCalendar,
    };
    let calendars: Vec<Box<dyn Calendar>> = vec![
        Box::new(UKCalendar::new()),
        Box::new(JapanCalendar::new()),
        Box::new(SIFMACalendar::new()),
        Box::new(Target2Calendar::new()),
        Box::new(USGovernmentCalendar::new()),
    ];
    for calendar in calendars {
        for input in ["1969-12-31", "2101-01-01"] {
            assert!(
                calendar.try_is_business_day(d(input)).is_err(),
                "{} {input}",
                calendar.name()
            );
        }
        for input in ["1970-01-01", "2100-12-31"] {
            assert!(
                calendar.try_is_business_day(d(input)).is_ok(),
                "{} {input}",
                calendar.name()
            );
        }
    }
    let joint = JointCalendar::new(vec![Box::new(WeekendCalendar), Box::new(UKCalendar::new())]);
    assert!(joint.try_is_business_day(d("2101-01-01")).is_err());
    assert!(JointCalendar::new(vec![])
        .try_is_business_day(d("2025-01-02"))
        .is_err());
    let excessive = JointCalendar::new(
        (0..17)
            .map(|_| Box::new(WeekendCalendar) as Box<dyn Calendar>)
            .collect(),
    );
    assert!(excessive.try_is_business_day(d("2025-01-02")).is_err());
}

#[test]
fn date_only_operations_and_supplied_weekend_rules_keep_their_full_domain() {
    use convex_core::calendars::{BusinessDayConvention, Calendar, DynamicCalendar, WeekendType};
    let outside = d("2101-01-03");
    assert!(WeekendCalendar.try_is_business_day(outside).unwrap());
    assert_eq!(
        checked::add_business_days(outside, 0, &UKCalendar::new()).unwrap(),
        outside
    );
    assert_eq!(
        UKCalendar::new()
            .adjust(outside, BusinessDayConvention::Unadjusted)
            .unwrap(),
        outside
    );
    assert!(UKCalendar::new()
        .adjust(outside, BusinessDayConvention::Following)
        .is_err());
    assert!(checked::add_business_days(outside, i32::MIN, &WeekendCalendar).is_err());
    assert!(checked::add_business_days(outside, i32::MAX, &WeekendCalendar).is_err());
    let mut supplied = DynamicCalendar::from_dates(
        "Exact",
        WeekendType::SaturdaySunday,
        [d("0001-01-01"), outside, d("9999-12-31")],
    );
    assert_eq!(supplied.holiday_count(), 3);
    for holiday in [d("0001-01-01"), outside, d("9999-12-31")] {
        assert!(!supplied.try_is_business_day(holiday).unwrap());
        assert!(supplied.is_holiday_date(holiday));
    }
    supplied.remove_holiday(outside);
    assert!(supplied.try_is_business_day(outside).unwrap());
    supplied.add_holiday(outside);
    supplied.add_holiday(outside);
    assert_eq!(supplied.holiday_count(), 3);
    let roundtrip = DynamicCalendar::from_json(&supplied.to_json().unwrap()).unwrap();
    assert_eq!(
        roundtrip.to_calendar_data().holidays,
        supplied.to_calendar_data().holidays
    );
    assert!(!roundtrip.try_is_business_day(outside).unwrap());
}

#[test]
fn checked_queries_propagate_calendar_coverage_before_calling_business_rules() {
    use convex_core::{calendars::Calendar, ConvexError, ConvexResult};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Unavailable(AtomicUsize);
    impl Calendar for Unavailable {
        fn name(&self) -> &'static str {
            "Unavailable source"
        }
        fn validate_date(&self, _date: Date) -> ConvexResult<()> {
            Err(ConvexError::CalendarError {
                reason: "original calendar data unavailable".into(),
            })
        }
        fn is_business_day(&self, _date: Date) -> bool {
            self.0.fetch_add(1, Ordering::SeqCst);
            true
        }
    }
    let unavailable = Unavailable(AtomicUsize::new(0));
    assert!(checked::business_day_on_or_after(d("2025-01-02"), &unavailable).is_err());
    assert!(checked::business_day_on_or_before(d("2025-01-02"), &unavailable).is_err());
    assert!(checked::add_business_days(d("2025-01-02"), 1, &unavailable).is_err());
    assert_eq!(unavailable.0.load(Ordering::SeqCst), 0);
}
