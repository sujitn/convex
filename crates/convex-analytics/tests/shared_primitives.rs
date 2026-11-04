//! Existing analytics names are aliases, without copied schedule/fixing formulas.

use convex_analytics::cashflows::{
    CouponSchedule, DateRules, Schedule, ScheduleConfig, ScheduleSpec, StubType,
};
use convex_core::{
    calendars::WeekendCalendar,
    types::{Date, Frequency},
};

#[test]
fn analytics_and_future_rates_credit_consumers_reach_identical_core_types() {
    let start = Date::parse("2024-01-01").unwrap();
    let end = Date::parse("2025-01-01").unwrap();
    let schedule: convex_core::cashflows::CouponSchedule = CouponSchedule::generate(
        &ScheduleSpec {
            start_date: start,
            end_date: end,
            frequency: Frequency::SemiAnnual,
            stub_type: StubType::None,
            end_of_month: true,
            first_regular_date: None,
            penultimate_date: None,
        },
        &DateRules::default(),
        &WeekendCalendar,
    )
    .unwrap();
    let bond: convex_bonds::cashflows::Schedule =
        Schedule::generate(ScheduleConfig::new(start, end, Frequency::SemiAnnual)).unwrap();
    assert_eq!(schedule.periods(), bond.coupon_periods());
    // Rates and credit legs can import the exact same core value; no product
    // crate, runtime trait, market repository or duplicated date model is needed.
    let rates_leg: &convex_core::cashflows::CouponSchedule = &schedule;
    let credit_premium_leg: &convex_core::cashflows::CouponSchedule = &schedule;
    assert_eq!(rates_leg, credit_premium_leg);
}
