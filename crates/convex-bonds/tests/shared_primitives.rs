//! CVX-054 compatibility paths delegate to the shared schedule implementation.

use convex_bonds::cashflows::{Schedule, ScheduleConfig, StubType};
use convex_bonds::types::CalendarId;
use convex_core::{
    calendars::{BusinessDayConvention, WeekendCalendar},
    cashflows::{CouponSchedule, DateRules, ScheduleSpec},
    types::{Date, Frequency},
};

fn d(s: &str) -> Date {
    Date::parse(s).unwrap()
}

#[test]
fn long_front_and_back_schedules_and_reference_periods_are_shared() {
    for (start, end, stub) in [
        ("2024-03-15", "2025-06-15", StubType::LongFirst),
        ("2024-01-31", "2025-03-15", StubType::LongLast),
    ] {
        let config = ScheduleConfig::new(d(start), d(end), Frequency::SemiAnnual)
            .with_stub_type(stub)
            .with_business_day_convention(BusinessDayConvention::Unadjusted);
        let bond = Schedule::generate(config).unwrap();
        let core = CouponSchedule::generate(
            &ScheduleSpec {
                start_date: d(start),
                end_date: d(end),
                frequency: Frequency::SemiAnnual,
                stub_type: stub,
                end_of_month: true,
                first_regular_date: None,
                penultimate_date: None,
            },
            &DateRules::default(),
            &WeekendCalendar,
        )
        .unwrap();
        assert_eq!(bond.unadjusted_dates(), core.contractual_dates());
        assert_eq!(bond.coupon_periods(), core.periods());
    }
}

#[test]
fn penultimate_schedule_does_not_loop_and_supplied_calendar_is_supported() {
    let config = ScheduleConfig::new(d("2024-01-31"), d("2025-03-15"), Frequency::SemiAnnual)
        .with_penultimate_date(d("2025-01-31"));
    let bond = Schedule::generate_with_calendar(config, &WeekendCalendar).unwrap();
    assert_eq!(
        bond.unadjusted_dates(),
        ["2024-01-31", "2024-07-31", "2025-01-31", "2025-03-15"].map(d)
    );
    assert_eq!(
        bond.coupon_periods()[2].reference_periods()[0].end,
        d("2025-07-31")
    );
}

#[test]
fn unknown_calendar_fails_and_joint_calendar_uses_all_supplied_holidays() {
    let config = ScheduleConfig::new(d("2024-11-01"), d("2025-05-01"), Frequency::SemiAnnual)
        .with_calendar(CalendarId::new("SIFMA+TARGET2"));
    let bond = Schedule::generate(config).unwrap();
    assert_eq!(bond.dates()[1], d("2025-05-02"));
    assert!(Schedule::generate(
        ScheduleConfig::new(d("2024-11-01"), d("2025-05-01"), Frequency::SemiAnnual)
            .with_calendar(CalendarId::new("unknown"))
    )
    .is_err());
    assert!(CalendarId::new("SIFMA+").try_to_calendar().is_err());
}

#[test]
fn existing_default_allows_short_front_without_changing_shared_none_contract() {
    let config = ScheduleConfig::new(d("2024-03-15"), d("2025-06-15"), Frequency::SemiAnnual);
    assert_eq!(
        Schedule::generate(config).unwrap().unadjusted_dates(),
        ["2024-03-15", "2024-06-15", "2024-12-15", "2025-06-15"].map(d)
    );
}

#[test]
fn checked_bond_builders_reject_invalid_schedule_before_any_pricing() {
    use convex_bonds::FixedRateBond;
    let make = || {
        FixedRateBond::builder()
            .cusip_unchecked("TEST")
            .coupon_percent(5.0)
            .issue_date(d("2024-01-01"))
            .maturity(d("2025-01-01"))
    };
    assert!(make().calendar(CalendarId::new("unknown")).build().is_err());
    assert!(make().first_coupon_date(d("2025-06-01")).build().is_err());
    let bond = make().build().unwrap();
    assert_eq!(bond.try_schedule().unwrap().num_periods(), 2);
    let mut json = serde_json::to_value(&bond).unwrap();
    json["calendar"] = serde_json::json!("unknown");
    assert!(serde_json::from_value::<FixedRateBond>(json).is_err());
}
