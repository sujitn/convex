//! CVX-054 AC1–AC3: exact supplied contracts and bounded analytic controls.

use convex_core::{
    calendars::{
        checked, BusinessDayConvention, Calendar, JointCalendar, SIFMACalendar, Target2Calendar,
        UKCalendar, WeekendCalendar,
    },
    cashflows::*,
    contracts::{IndexId, SourceId, UtcTimestamp, Version},
    daycounts::{DayCount, DayCountConvention, Thirty360EIsda},
    fixings::*,
    types::Frequency,
    Date,
};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;

fn d(s: &str) -> Date {
    Date::parse(s).unwrap()
}
fn t(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse(s).unwrap()
}
fn spec(start: &str, end: &str, frequency: Frequency, stub_type: StubType) -> ScheduleSpec {
    ScheduleSpec {
        start_date: d(start),
        end_date: d(end),
        frequency,
        stub_type,
        end_of_month: true,
        first_regular_date: None,
        penultimate_date: None,
    }
}
fn schedule(start: &str, end: &str) -> CouponSchedule {
    CouponSchedule::generate(
        &spec(start, end, Frequency::SemiAnnual, StubType::None),
        &DateRules::default(),
        &WeekendCalendar,
    )
    .unwrap()
}
fn profile(kind: ProfileKind, entries: &[(&str, &str, Decimal)]) -> DatedProfile {
    DatedProfile::new(
        kind,
        entries
            .iter()
            .map(|&(start, end, value)| DatedValue {
                start: d(start),
                end: d(end),
                value,
            })
            .collect(),
    )
    .unwrap()
}
fn profiles(start: &str, end: &str, coupon: Decimal) -> CouponProfiles {
    CouponProfiles::new(
        profile(ProfileKind::Coupon, &[(start, end, coupon)]),
        profile(ProfileKind::Spread, &[(start, end, dec!(0))]),
        profile(ProfileKind::Gearing, &[(start, end, dec!(0))]),
        profile(ProfileKind::Notional, &[(start, end, dec!(100))]),
    )
    .unwrap()
}
fn calc(day_count: DayCountConvention) -> CouponCalculation {
    CouponCalculation {
        index_rate: dec!(0),
        day_count,
        principal: CouponPrincipal::AccrualProfile,
        rounding: CashRounding::Unrounded,
        negative_coupons: NegativeCouponPolicy::Reject,
    }
}
fn close(actual: Decimal, expected: Decimal) {
    assert!(
        (actual - expected).abs() <= dec!(0.000000000000000000000001),
        "{actual} != {expected}"
    );
}

#[derive(Deserialize)]
struct DateCase {
    id: String,
    spec: ScheduleSpec,
    expected_dates: Vec<Date>,
}
#[derive(Deserialize)]
struct Fixtures {
    date_cases: Vec<DateCase>,
}
fn fixtures() -> Fixtures {
    serde_json::from_str(include_str!("fixtures/cashflow-primitives.json")).unwrap()
}

#[test]
fn ac1_pinned_short_long_front_back_and_month_rolls() {
    for case in fixtures().date_cases {
        let schedule =
            CouponSchedule::generate(&case.spec, &DateRules::default(), &WeekendCalendar).unwrap();
        assert_eq!(
            schedule.contractual_dates(),
            case.expected_dates,
            "{}",
            case.id
        );
        assert_eq!(
            schedule.periods().len() + 1,
            schedule.contractual_dates().len()
        );
    }
}

#[test]
fn ac1_long_stubs_keep_each_icma_reference_denominator_and_accrual_split() {
    let cases = fixtures().date_cases;
    let first =
        CouponSchedule::generate(&cases[1].spec, &DateRules::default(), &WeekendCalendar).unwrap();
    let period = &first.periods()[0];
    assert_eq!(
        period.reference_periods(),
        &[
            ReferencePeriod {
                start: d("2023-12-15"),
                end: d("2024-06-15")
            },
            ReferencePeriod {
                start: d("2024-06-15"),
                end: d("2024-12-15")
            }
        ]
    );
    let dc = DayCountConvention::ActActIcma;
    close(
        period
            .year_fraction(d("2024-03-15"), d("2024-12-15"), dc)
            .unwrap(),
        dec!(92) / dec!(366) + dec!(0.5),
    );
    close(
        period
            .year_fraction(d("2024-03-15"), d("2024-07-15"), dc)
            .unwrap(),
        dec!(92) / dec!(366) + dec!(30) / dec!(366),
    );
    let last =
        CouponSchedule::generate(&cases[3].spec, &DateRules::default(), &WeekendCalendar).unwrap();
    let period = &last.periods()[1];
    assert_eq!(
        period.reference_periods(),
        &[
            ReferencePeriod {
                start: d("2024-07-31"),
                end: d("2025-01-31")
            },
            ReferencePeriod {
                start: d("2025-01-31"),
                end: d("2025-07-31")
            }
        ]
    );
    close(
        period
            .year_fraction(d("2024-07-31"), d("2025-03-15"), dc)
            .unwrap(),
        dec!(0.5) + dec!(43) / dec!(362),
    );
    let p = profiles("2024-01-31", "2025-03-15", dec!(0.06));
    close(
        p.coupon_amount(period, calc(dc)).unwrap(),
        dec!(6) * (dec!(0.5) + dec!(43) / dec!(362)),
    );
    close(
        p.accrued(
            period,
            d("2025-02-15"),
            calc(dc),
            ExCouponAccrual::SubtractCoupon,
        )
        .unwrap(),
        dec!(6) * (dec!(0.5) + dec!(15) / dec!(362)),
    );
}

#[test]
fn ac1_explicit_front_and_back_dates_preserve_reference_periods() {
    let case = &fixtures().date_cases[4];
    let s = CouponSchedule::generate(&case.spec, &DateRules::default(), &WeekendCalendar).unwrap();
    let first = &s.periods()[0];
    assert_eq!(
        first.reference_periods()[0],
        ReferencePeriod {
            start: d("2023-09-30"),
            end: d("2024-03-31")
        }
    );
    close(
        first
            .year_fraction(
                d("2023-12-01"),
                d("2024-03-31"),
                DayCountConvention::ActActIcma,
            )
            .unwrap(),
        dec!(121) / dec!(366),
    );
    let last = &s.periods()[3];
    close(
        last.year_fraction(
            d("2025-03-31"),
            d("2025-07-15"),
            DayCountConvention::ActActIcma,
        )
        .unwrap(),
        dec!(106) / dec!(366),
    );
}

#[test]
fn ac1_joint_holidays_do_not_rewrite_contractual_accrual_or_observation_dates() {
    let joint = JointCalendar::new(vec![
        Box::new(SIFMACalendar::new()),
        Box::new(Target2Calendar::new()),
    ]);
    let rules = DateRules {
        payment_adjustment: BusinessDayConvention::Following,
        payment_delay: DateOffset::BusinessDays(1),
        fixing_offset: Some(DateOffset::BusinessDays(-2)),
        observation_offset: Some(DateOffset::BusinessDays(-1)),
        ex_coupon: Some(ExCouponRule {
            offset: DateOffset::BusinessDays(-2),
            anchor: ExDateAnchor::PaymentDate,
            boundary: ExCouponBoundary::OnDate,
        }),
        ..DateRules::default()
    };
    let s = CouponSchedule::generate(
        &spec(
            "2024-10-01",
            "2025-05-01",
            Frequency::SemiAnnual,
            StubType::ShortFirst,
        ),
        &rules,
        &joint,
    )
    .unwrap();
    let p = &s.periods()[0];
    assert_eq!(p.contractual_dates(), (d("2024-10-01"), d("2024-11-01")));
    assert_eq!(p.accrual_dates(), p.contractual_dates());
    assert_eq!(s.periods()[1].payment_date(), d("2025-05-02")); // May 1 TARGET holiday
    assert_eq!(p.fixing_date(), Some(d("2024-09-27")));
    assert_eq!(
        p.observation_dates(),
        Some((d("2024-09-30"), d("2024-10-31")))
    );
    assert_eq!(
        p.ex_coupon(),
        Some((d("2024-10-31"), ExCouponBoundary::OnDate))
    );
    assert_eq!(
        DateOffset::BusinessDays(2)
            .apply(d("2025-01-17"), &joint)
            .unwrap(),
        d("2025-01-22")
    ); // US MLK
    assert_eq!(
        DateOffset::BusinessDays(-2)
            .apply(d("2025-05-02"), &joint)
            .unwrap(),
        d("2025-04-29")
    );
}

#[test]
fn ac1_adjusted_accrual_is_a_separate_contract_choice() {
    let rules = DateRules {
        accrual_adjustment: BusinessDayConvention::Following,
        payment_adjustment: BusinessDayConvention::Following,
        payment_delay: DateOffset::BusinessDays(2),
        ..DateRules::default()
    };
    let s = CouponSchedule::generate(
        &spec(
            "2024-06-30",
            "2025-06-30",
            Frequency::SemiAnnual,
            StubType::None,
        ),
        &rules,
        &WeekendCalendar,
    )
    .unwrap();
    assert_eq!(
        s.periods()[0].contractual_dates(),
        (d("2024-06-30"), d("2024-12-31"))
    );
    assert_eq!(
        s.periods()[0].accrual_dates(),
        (d("2024-07-01"), d("2024-12-31"))
    );
    assert_eq!(s.periods()[0].payment_date(), d("2025-01-02"));
}

#[test]
fn ac1_gilt_record_date_boundary_and_ex_coupon_rebate_are_explicit() {
    let rules = DateRules {
        payment_adjustment: BusinessDayConvention::Following,
        ex_coupon: Some(ExCouponRule {
            offset: DateOffset::BusinessDays(-7),
            anchor: ExDateAnchor::ContractualEnd,
            boundary: ExCouponBoundary::AfterDate,
        }),
        ..DateRules::default()
    };
    let s = CouponSchedule::generate(
        &spec(
            "2024-07-31",
            "2025-07-31",
            Frequency::SemiAnnual,
            StubType::None,
        ),
        &rules,
        &UKCalendar::new(),
    )
    .unwrap();
    let p = &s.periods()[1];
    assert_eq!(
        p.ex_coupon(),
        Some((d("2025-07-22"), ExCouponBoundary::AfterDate))
    );
    assert!(p.is_entitled(d("2025-07-22"), PaymentDateEntitlement::Exclude));
    assert!(!p.is_entitled(d("2025-07-23"), PaymentDateEntitlement::Exclude));
    let profiles = profiles("2024-07-31", "2025-07-31", dec!(0.06));
    let c = calc(DayCountConvention::ActActIcma);
    close(
        profiles
            .accrued(p, d("2025-07-22"), c, ExCouponAccrual::SubtractCoupon)
            .unwrap(),
        dec!(3) * dec!(172) / dec!(181),
    );
    close(
        profiles
            .accrued(p, d("2025-07-23"), c, ExCouponAccrual::SubtractCoupon)
            .unwrap(),
        -dec!(3) * dec!(8) / dec!(181),
    );
    assert_eq!(
        profiles
            .accrued(p, d("2025-07-23"), c, ExCouponAccrual::Zero)
            .unwrap(),
        dec!(0)
    );
    let principal = principal_cashflows(
        profiles.notional(),
        &rules,
        &UKCalendar::new(),
        dec!(1),
        CashRounding::Unrounded,
    )
    .unwrap();
    assert!(principal[0].is_entitled(d("2025-07-23"), PaymentDateEntitlement::Exclude));
    let inclusive = DateRules {
        ex_coupon: Some(ExCouponRule {
            boundary: ExCouponBoundary::OnDate,
            ..rules.ex_coupon.unwrap()
        }),
        ..rules
    };
    let s = CouponSchedule::generate(
        &spec(
            "2024-07-31",
            "2025-07-31",
            Frequency::SemiAnnual,
            StubType::None,
        ),
        &inclusive,
        &UKCalendar::new(),
    )
    .unwrap();
    assert!(!s.periods()[1].is_entitled(d("2025-07-22"), PaymentDateEntitlement::Exclude));
}

#[test]
fn ac1_payment_date_state_and_fully_matured_accrual() {
    let s = schedule("2024-01-01", "2025-01-01");
    let p = &s.periods()[0];
    assert!(p.is_entitled(d("2024-07-01"), PaymentDateEntitlement::Include));
    assert!(!p.is_entitled(d("2024-07-01"), PaymentDateEntitlement::Exclude));
    let pr = profiles("2024-01-01", "2025-01-01", dec!(0.06));
    for period in s.periods() {
        assert_eq!(
            pr.accrued(
                period,
                period.payment_date(),
                calc(DayCountConvention::ActActIcma),
                ExCouponAccrual::SubtractCoupon
            )
            .unwrap(),
            dec!(0)
        );
        assert_eq!(
            pr.accrued(
                period,
                d("2025-01-02"),
                calc(DayCountConvention::ActActIcma),
                ExCouponAccrual::SubtractCoupon
            )
            .unwrap(),
            dec!(0)
        );
    }
    let cash = principal_cashflows(
        pr.notional(),
        &DateRules::default(),
        &WeekendCalendar,
        dec!(1),
        CashRounding::NearestEven(2),
    )
    .unwrap();
    assert!(cash[0].is_entitled(d("2025-01-01"), PaymentDateEntitlement::Include));
    assert!(!cash[0].is_entitled(d("2025-01-01"), PaymentDateEntitlement::Exclude));
    assert!(!cash[0].is_entitled(d("2025-01-02"), PaymentDateEntitlement::Include));
}

#[test]
fn ac1_payment_delay_and_notice_offsets_keep_effective_dates() {
    let rules = DateRules {
        payment_delay: DateOffset::BusinessDays(2),
        ..DateRules::default()
    };
    let s = CouponSchedule::generate(
        &spec(
            "2024-01-01",
            "2025-01-01",
            Frequency::SemiAnnual,
            StubType::None,
        ),
        &rules,
        &WeekendCalendar,
    )
    .unwrap();
    let profiles = profiles("2024-01-01", "2025-01-01", dec!(0.06));
    assert_eq!(s.periods()[0].payment_date(), d("2024-07-03"));
    assert_eq!(
        profiles
            .accrued(
                &s.periods()[0],
                d("2024-07-02"),
                calc(DayCountConvention::ActActIcma),
                ExCouponAccrual::SubtractCoupon
            )
            .unwrap(),
        dec!(3)
    );
    assert_eq!(
        DateOffset::BusinessDays(-2)
            .apply(d("2025-07-31"), &WeekendCalendar)
            .unwrap(),
        d("2025-07-29")
    ); // option notice
}

#[test]
fn ac1_checked_30e_isda_only_february_termination_is_exceptional() {
    let last31 = Thirty360EIsda::new(Some(d("2025-03-31")));
    assert_eq!(
        last31
            .checked_year_fraction(d("2025-02-28"), d("2025-03-31"))
            .unwrap(),
        dec!(30) / dec!(360)
    );
    // Existing compatibility kernel's numerical fixtures are deliberately unchanged.
    assert_eq!(
        last31.year_fraction(d("2025-02-28"), d("2025-03-31")),
        dec!(31) / dec!(360)
    );
    let feb = Thirty360EIsda::new(Some(d("2025-02-28")));
    assert_eq!(
        feb.checked_year_fraction(d("2024-08-31"), d("2025-02-28"))
            .unwrap(),
        dec!(178) / dec!(360)
    );
    let regular = Thirty360EIsda::new(Some(d("2025-08-31")));
    assert_eq!(
        regular
            .checked_year_fraction(d("2024-08-31"), d("2025-02-28"))
            .unwrap(),
        dec!(180) / dec!(360)
    );
    let s = schedule("2024-08-31", "2025-08-31");
    let p = &s.periods()[0];
    assert_eq!(
        p.year_fraction(
            p.accrual_dates().0,
            p.accrual_dates().1,
            DayCountConvention::Thirty360EIsda
        )
        .unwrap(),
        dec!(0.5)
    );
}

#[test]
fn ac1_invalid_and_inconsistent_schedule_terms_fail_without_sorting() {
    let mut s = spec(
        "2024-03-15",
        "2025-06-15",
        Frequency::SemiAnnual,
        StubType::None,
    );
    assert!(CouponSchedule::generate(&s, &DateRules::default(), &WeekendCalendar).is_err());
    for invalid in ["2024-03-15", "2024-03-01", "2025-06-15", "2026-01-01"] {
        s.first_regular_date = Some(d(invalid));
        assert!(CouponSchedule::generate(&s, &DateRules::default(), &WeekendCalendar).is_err());
    }
    s.first_regular_date = Some(d("2024-09-15"));
    s.penultimate_date = Some(d("2024-06-15"));
    assert!(CouponSchedule::generate(&s, &DateRules::default(), &WeekendCalendar).is_err());
    s = spec(
        "2024-01-01",
        "2024-02-01",
        Frequency::SemiAnnual,
        StubType::LongLast,
    );
    assert!(CouponSchedule::generate(&s, &DateRules::default(), &WeekendCalendar).is_err());
    s = spec("2024-01-01", "2025-01-01", Frequency::Zero, StubType::None);
    let zero = CouponSchedule::generate(&s, &DateRules::default(), &WeekendCalendar).unwrap();
    assert_eq!(
        zero.contractual_dates(),
        &[d("2024-01-01"), d("2025-01-01")]
    );
    assert!(zero.periods()[0]
        .year_fraction(
            d("2024-01-01"),
            d("2025-01-01"),
            DayCountConvention::ActActIcma
        )
        .is_err());
    assert!(CouponSchedule::generate(
        &spec(
            "2024-01-01",
            "2025-01-01",
            Frequency::SemiAnnual,
            StubType::None
        ),
        &DateRules {
            payment_delay: DateOffset::CalendarDays(-1),
            ..DateRules::default()
        },
        &WeekendCalendar
    )
    .is_err());
}

struct ClosedCalendar;
impl Calendar for ClosedCalendar {
    fn name(&self) -> &'static str {
        "Closed"
    }
    fn is_business_day(&self, _: Date) -> bool {
        false
    }
}

#[derive(Debug, Default)]
struct CountedClosedCalendar(std::sync::atomic::AtomicUsize);
impl Calendar for CountedClosedCalendar {
    fn name(&self) -> &'static str {
        "CountedClosed"
    }
    fn is_business_day(&self, _: Date) -> bool {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        false
    }
}

#[test]
fn ac1_closed_calendars_and_extreme_offsets_are_bounded_errors() {
    for forward in [true, false] {
        let calendar = CountedClosedCalendar::default();
        let result = if forward {
            checked::business_day_on_or_after(d("2025-01-01"), &calendar)
        } else {
            checked::business_day_on_or_before(d("2025-01-01"), &calendar)
        };
        assert!(result.is_err());
        assert_eq!(
            calendar.0.load(std::sync::atomic::Ordering::Relaxed),
            checked::MAX_CALENDAR_SEARCH_DAYS
        );
    }
    assert!(DateOffset::BusinessDays(1)
        .apply(d("2025-01-01"), &ClosedCalendar)
        .is_err());
    assert!(ClosedCalendar
        .adjust(d("2025-01-01"), BusinessDayConvention::Following)
        .is_err());
    assert!(ClosedCalendar
        .adjust(d("2025-01-01"), BusinessDayConvention::Preceding)
        .is_err());
    for n in [i32::MIN, i32::MAX, -367, 367] {
        assert!(DateOffset::CalendarDays(n)
            .apply(d("2025-01-01"), &WeekendCalendar)
            .is_err());
        assert!(checked::add_business_days(d("2025-01-01"), n, &WeekendCalendar).is_err());
    }
    assert!(DateOffset::CalendarDays(1)
        .apply(d("9999-12-31"), &WeekendCalendar)
        .is_err());
    assert!(CouponSchedule::generate(
        &spec(
            "1800-01-01",
            "2025-01-01",
            Frequency::Monthly,
            StubType::None
        ),
        &DateRules::default(),
        &WeekendCalendar
    )
    .is_err());
}

#[test]
fn ac3_profiles_split_independently_without_averaged_terms() {
    let profiles = CouponProfiles::new(
        profile(
            ProfileKind::Coupon,
            &[
                ("2024-01-01", "2024-04-01", dec!(0.04)),
                ("2024-04-01", "2024-07-01", dec!(0.06)),
            ],
        ),
        profile(
            ProfileKind::Spread,
            &[
                ("2024-01-01", "2024-03-01", dec!(0.001)),
                ("2024-03-01", "2024-07-01", dec!(0.002)),
            ],
        ),
        profile(
            ProfileKind::Gearing,
            &[
                ("2024-01-01", "2024-05-01", dec!(1)),
                ("2024-05-01", "2024-07-01", dec!(2)),
            ],
        ),
        profile(
            ProfileKind::Notional,
            &[
                ("2024-01-01", "2024-02-15", dec!(100)),
                ("2024-02-15", "2024-07-01", dec!(80)),
            ],
        ),
    )
    .unwrap();
    let s = schedule("2024-01-01", "2024-07-01");
    let c = CouponCalculation {
        index_rate: dec!(0.03),
        ..calc(DayCountConvention::Act360)
    };
    // Five independently derived segments: 45,15,31,30,61 actual days.
    close(
        profiles.coupon_amount(&s.periods()[0], c).unwrap(),
        dec!(1399.42) / dec!(360),
    );
    close(
        profiles
            .accrued(
                &s.periods()[0],
                d("2024-05-01"),
                c,
                ExCouponAccrual::SubtractCoupon,
            )
            .unwrap(),
        dec!(804.06) / dec!(360),
    );
}

#[test]
fn ac3_principal_before_after_coupon_boundary_and_redemption_are_explicit() {
    let profiles = CouponProfiles::new(
        profile(
            ProfileKind::Coupon,
            &[("2024-01-01", "2025-01-01", dec!(0.06))],
        ),
        profile(
            ProfileKind::Spread,
            &[("2024-01-01", "2025-01-01", dec!(0))],
        ),
        profile(
            ProfileKind::Gearing,
            &[("2024-01-01", "2025-01-01", dec!(0))],
        ),
        profile(
            ProfileKind::Notional,
            &[
                ("2024-01-01", "2024-07-01", dec!(100)),
                ("2024-07-01", "2025-01-01", dec!(60)),
            ],
        ),
    )
    .unwrap();
    assert_eq!(
        profiles
            .notional()
            .value_at(d("2024-07-01"), BoundarySide::Before)
            .unwrap(),
        dec!(100)
    );
    assert_eq!(
        profiles
            .notional()
            .value_at(d("2024-07-01"), BoundarySide::After)
            .unwrap(),
        dec!(60)
    );
    let s = schedule("2024-01-01", "2025-01-01");
    let c = calc(DayCountConvention::ActActIcma);
    assert_eq!(
        profiles
            .coupon_amount(
                &s.periods()[0],
                CouponCalculation {
                    principal: CouponPrincipal::BeforeCouponBoundary,
                    ..c
                }
            )
            .unwrap(),
        dec!(3)
    );
    assert_eq!(
        profiles
            .coupon_amount(
                &s.periods()[0],
                CouponCalculation {
                    principal: CouponPrincipal::AfterCouponBoundary,
                    ..c
                }
            )
            .unwrap(),
        dec!(1.8)
    );
    assert_eq!(
        profiles.coupon_amount(&s.periods()[1], c).unwrap(),
        dec!(1.8)
    );
    assert_eq!(
        profiles
            .coupon_amount(
                &s.periods()[1],
                CouponCalculation {
                    principal: CouponPrincipal::AfterCouponBoundary,
                    ..c
                }
            )
            .unwrap(),
        dec!(0)
    );
    let cash = principal_cashflows(
        profiles.notional(),
        &DateRules::default(),
        &WeekendCalendar,
        dec!(1.02),
        CashRounding::NearestEven(2),
    )
    .unwrap();
    assert_eq!(
        cash.iter().map(|c| c.amount).collect::<Vec<_>>(),
        vec![dec!(40), dec!(61.20)]
    );
    assert_eq!(
        cash[0].principal,
        PrincipalChange {
            before: dec!(100),
            after: dec!(60),
            repayment: dec!(40)
        }
    );
    assert_eq!(cash[1].principal.after, dec!(0));
    assert!(cash[1].is_redemption);
}

#[test]
fn ac3_invalid_unsorted_overlapping_gapped_and_inconsistent_profiles_fail() {
    let valid = DatedValue {
        start: d("2024-01-01"),
        end: d("2024-07-01"),
        value: dec!(1),
    };
    for entries in [
        vec![],
        vec![valid, valid],
        vec![DatedValue {
            end: d("2024-01-01"),
            ..valid
        }],
        vec![
            valid,
            DatedValue {
                start: d("2024-06-01"),
                end: d("2025-01-01"),
                ..valid
            },
        ],
        vec![
            valid,
            DatedValue {
                start: d("2024-08-01"),
                end: d("2025-01-01"),
                ..valid
            },
        ],
        vec![
            DatedValue {
                start: d("2024-07-01"),
                end: d("2025-01-01"),
                ..valid
            },
            valid,
        ],
    ] {
        assert!(DatedProfile::new(ProfileKind::Coupon, entries).is_err());
    }
    assert!(DatedProfile::new(
        ProfileKind::Notional,
        vec![DatedValue {
            value: dec!(-1),
            ..valid
        }]
    )
    .is_err());
    assert!(DatedProfile::new(ProfileKind::Coupon, vec![valid; MAX_SCHEDULE_PERIODS + 1]).is_err());
    assert!(CouponProfiles::new(
        profile(
            ProfileKind::Coupon,
            &[("2024-01-01", "2025-01-01", dec!(0.06))]
        ),
        profile(
            ProfileKind::Spread,
            &[("2024-01-01", "2024-07-01", dec!(0))]
        ),
        profile(
            ProfileKind::Gearing,
            &[("2024-01-01", "2025-01-01", dec!(0))]
        ),
        profile(
            ProfileKind::Notional,
            &[("2024-01-01", "2025-01-01", dec!(100))]
        )
    )
    .is_err());
    let p = profile(
        ProfileKind::Coupon,
        &[("2024-01-01", "2025-01-01", dec!(0.01))],
    );
    assert!(p.principal_change(d("2024-07-01")).is_err());
    assert!(p.value_at(d("2023-12-31"), BoundarySide::After).is_err());
    assert!(p.value_at(d("2025-01-01"), BoundarySide::After).is_err());
    assert!(p.value_at(d("2024-01-01"), BoundarySide::Before).is_err());
}

#[test]
fn ac3_negative_coupons_advance_cash_and_rounding_are_contract_choices() {
    let s = schedule("2024-01-01", "2025-01-01");
    let p = profiles("2024-01-01", "2025-01-01", dec!(-0.01));
    assert!(p
        .coupon_amount(&s.periods()[0], calc(DayCountConvention::ActActIcma))
        .is_err());
    assert_eq!(
        p.coupon_amount(
            &s.periods()[0],
            CouponCalculation {
                negative_coupons: NegativeCouponPolicy::Allow,
                ..calc(DayCountConvention::ActActIcma)
            }
        )
        .unwrap(),
        dec!(-0.5)
    );
    assert_eq!(
        CashRounding::NearestEven(2).apply(dec!(1.005)).unwrap(),
        dec!(1.00)
    );
    assert_eq!(
        CashRounding::MidpointAwayFromZero(2)
            .apply(dec!(-1.005))
            .unwrap(),
        dec!(-1.01)
    );
    assert_eq!(
        CashRounding::TowardZero(2).apply(dec!(-1.009)).unwrap(),
        dec!(-1.00)
    );
    assert!(CashRounding::NearestEven(29).apply(dec!(1)).is_err());
    let n = profile(
        ProfileKind::Notional,
        &[
            ("2024-01-01", "2024-07-01", dec!(80)),
            ("2024-07-01", "2025-01-01", dec!(100)),
        ],
    );
    let cash = principal_cashflows(
        &n,
        &DateRules::default(),
        &WeekendCalendar,
        dec!(1),
        CashRounding::NearestEven(2),
    )
    .unwrap();
    assert_eq!(cash[0].amount, dec!(-20));
    assert_eq!(cash[1].amount, dec!(100));
    assert!(principal_cashflows(
        &n,
        &DateRules::default(),
        &WeekendCalendar,
        dec!(-1),
        CashRounding::Unrounded
    )
    .is_err());
}

fn index() -> IndexId {
    IndexId::new("USD.SOFR").unwrap()
}
fn requirement(date: &str, publication: &str) -> FixingRequirement {
    FixingRequirement {
        index_id: index(),
        observation_date: d(date),
        expected_publication: t(publication),
    }
}
fn observation(
    date: &str,
    rate: Decimal,
    published: &str,
    known: &str,
    revision: u64,
) -> FixingObservation {
    FixingObservation {
        index_id: index(),
        observation_date: d(date),
        rate,
        published_at: t(published),
        known_at: t(known),
        revision: Version::new(revision).unwrap(),
        status: FixingStatus::Final,
        source_id: SourceId::new("fixture").unwrap(),
    }
}
fn context(date: &str, cutoff: &str) -> FixingContext {
    FixingContext {
        valuation_date: d(date),
        known_at: t(cutoff),
        unpublished: UnpublishedFixingPolicy::Forecast,
        final_only: true,
    }
}

#[test]
fn ac2_publication_receipt_nanosecond_and_revision_cutoffs_are_independent() {
    let req = requirement("2025-01-02", "2025-01-03T08:00:00Z");
    let first = observation(
        "2025-01-02",
        dec!(0.04),
        "2025-01-03T08:00:00Z",
        "2025-01-03T08:00:00.000000001Z",
        1,
    );
    let revision = observation(
        "2025-01-02",
        dec!(0.041),
        "2025-01-03T14:30:00Z",
        "2025-01-03T14:30:00.000000001Z",
        2,
    );
    let history = FixingSeries::new(vec![revision.clone(), first.clone()]).unwrap();
    assert_eq!(
        history
            .resolve(
                std::slice::from_ref(&req),
                context("2025-01-03", "2025-01-03T08:00:00Z")
            )
            .unwrap()
            .missing(),
        std::slice::from_ref(&req)
    );
    let old = history
        .resolve(
            std::slice::from_ref(&req),
            context("2025-01-03", "2025-01-03T14:30:00Z"),
        )
        .unwrap()
        .project(|_| panic!("known rate cannot forecast"))
        .unwrap();
    assert_eq!(old[0].origin, RateOrigin::Published(first));
    let new = history
        .resolve(
            &[req],
            context("2025-01-03", "2025-01-03T14:30:00.000000001Z"),
        )
        .unwrap()
        .project(|_| panic!("known rate cannot forecast"))
        .unwrap();
    assert_eq!(new[0].origin, RateOrigin::Published(revision));
    assert_eq!(old[0].rate, dec!(0.04)); // later resolution cannot mutate a prior result
}

#[test]
fn ac2_missing_published_list_fails_before_any_projection_callback() {
    let reqs = [
        requirement("2025-01-02", "2025-01-03T08:00:00Z"),
        requirement("2025-01-03", "2025-01-06T08:00:00Z"),
        requirement("2025-01-07", "2025-01-08T08:00:00Z"),
    ];
    let r = FixingSeries::default()
        .resolve(&reqs, context("2025-01-06", "2025-01-06T09:00:00Z"))
        .unwrap();
    assert_eq!(r.missing(), &reqs[..2]);
    let mut calls = 0;
    assert!(
        matches!(r.project(|_| {calls+=1;Ok(dec!(0.05))}),Err(CashflowError::MissingFixings(missing)) if missing==reqs[..2])
    );
    assert_eq!(calls, 0);
}

#[test]
fn ac2_not_yet_published_is_distinct_from_future_and_policy_is_explicit() {
    let reqs = [
        requirement("2025-01-03", "2025-01-06T08:00:00Z"),
        requirement("2025-01-06", "2025-01-07T08:00:00Z"),
    ];
    let c = context("2025-01-03", "2025-01-03T16:00:00Z");
    let result = FixingSeries::default()
        .resolve(&reqs, c)
        .unwrap()
        .project(|_| Ok(dec!(-0.005)))
        .unwrap();
    assert_eq!(
        result[0].origin,
        RateOrigin::Projected(ProjectionReason::AwaitingPublication)
    );
    assert_eq!(
        result[1].origin,
        RateOrigin::Projected(ProjectionReason::FutureObservation)
    );
    assert_eq!(
        FixingSeries::default()
            .resolve(
                &reqs,
                FixingContext {
                    unpublished: UnpublishedFixingPolicy::RequirePublished,
                    ..c
                }
            )
            .unwrap()
            .missing(),
        &reqs[..1]
    );
}

#[test]
fn ac2_future_observation_history_does_not_leak_into_a_restated_context() {
    let history = FixingSeries::new(vec![observation(
        "2025-01-06",
        dec!(0.09),
        "2025-01-07T08:00:00Z",
        "2025-01-07T08:00:00Z",
        1,
    )])
    .unwrap();
    let rates = history
        .resolve(
            &[requirement("2025-01-06", "2025-01-07T08:00:00Z")],
            context("2025-01-03", "2025-01-10T08:00:00Z"),
        )
        .unwrap()
        .project(|_| Ok(dec!(0.03)))
        .unwrap();
    assert_eq!(rates[0].rate, dec!(0.03));
    assert_eq!(
        rates[0].origin,
        RateOrigin::Projected(ProjectionReason::FutureObservation)
    );
}

#[test]
fn ac2_invalid_revisions_sources_status_and_projection_values_fail() {
    let first = observation(
        "2025-01-02",
        dec!(0.04),
        "2025-01-03T08:00:00Z",
        "2025-01-03T08:00:00Z",
        1,
    );
    assert!(FixingSeries::new(vec![first.clone(), first.clone()]).is_err());
    assert!(FixingSeries::new(vec![FixingObservation {
        known_at: t("2025-01-03T07:59:59Z"),
        ..first.clone()
    }])
    .is_err());
    assert!(FixingSeries::new(vec![FixingObservation {
        rate: dec!(11),
        ..first.clone()
    }])
    .is_err());
    for second in [
        FixingObservation {
            revision: Version::new(2).unwrap(),
            source_id: SourceId::new("other").unwrap(),
            ..first.clone()
        },
        FixingObservation {
            revision: Version::new(2).unwrap(),
            status: FixingStatus::Preliminary,
            ..first.clone()
        },
        FixingObservation {
            revision: Version::new(2).unwrap(),
            published_at: t("2025-01-03T07:00:00Z"),
            ..first.clone()
        },
    ] {
        assert!(FixingSeries::new(vec![first.clone(), second]).is_err());
    }
    let req = requirement("2025-01-06", "2025-01-07T08:00:00Z");
    assert!(FixingSeries::default()
        .resolve(
            &[req.clone(), req.clone()],
            context("2025-01-03", "2025-01-03T16:00:00Z")
        )
        .is_err());
    let resolved = FixingSeries::default()
        .resolve(&[req], context("2025-01-03", "2025-01-03T16:00:00Z"))
        .unwrap();
    assert!(resolved
        .project(|_| Err(CashflowError::InvalidTerms {
            field: "curve",
            reason: "unavailable".into()
        }))
        .is_err());
    assert!(resolved.project(|_| Ok(dec!(11))).is_err());
    let preliminary = FixingSeries::new(vec![FixingObservation {
        status: FixingStatus::Preliminary,
        ..first
    }])
    .unwrap();
    let req = requirement("2025-01-02", "2025-01-03T08:00:00Z");
    assert_eq!(
        preliminary
            .resolve(
                std::slice::from_ref(&req),
                context("2025-01-03", "2025-01-03T16:00:00Z")
            )
            .unwrap()
            .missing()
            .len(),
        1
    );
    assert!(preliminary
        .resolve(
            &[req],
            FixingContext {
                final_only: false,
                ..context("2025-01-03", "2025-01-03T16:00:00Z")
            }
        )
        .unwrap()
        .missing()
        .is_empty());
}

fn overnight(start: &str, end: &str, convention: OvernightConvention) -> OvernightSchedule {
    OvernightSchedule::generate(
        d(start),
        d(end),
        DayCountConvention::Act360,
        &WeekendCalendar,
        convention,
    )
    .unwrap()
}
fn flat() -> OvernightConvention {
    OvernightConvention {
        lookback_business_days: 0,
        observation_shift: false,
        lockout_business_days: 0,
    }
}
fn publication(date: Date) -> Result<UtcTimestamp, CashflowError> {
    let next = DateOffset::BusinessDays(1).apply(date, &WeekendCalendar)?;
    UtcTimestamp::parse(&format!("{next}T08:00:00Z"))
        .map_err(|error| CashflowError::Date(error.to_string()))
}

#[test]
fn ac2_fed_published_index_example_matches_with_exact_weekend_weights() {
    let schedule = overnight("2018-04-02", "2018-04-09", flat());
    assert_eq!(
        schedule
            .observations()
            .iter()
            .map(|o| o.weight_start.days_between(&o.weight_end))
            .collect::<Vec<_>>(),
        vec![1, 1, 1, 1, 3]
    );
    let mut observations = Vec::new();
    for (date, rate) in [
        "2018-04-02",
        "2018-04-03",
        "2018-04-04",
        "2018-04-05",
        "2018-04-06",
    ]
    .into_iter()
    .zip([
        dec!(0.0180),
        dec!(0.0183),
        dec!(0.0174),
        dec!(0.0175),
        dec!(0.0175),
    ]) {
        let timestamp = publication(d(date)).unwrap();
        observations.push(FixingObservation {
            index_id: index(),
            observation_date: d(date),
            rate,
            published_at: timestamp,
            known_at: timestamp,
            revision: Version::new(1).unwrap(),
            status: FixingStatus::Final,
            source_id: SourceId::new("NYFED-methodology-example").unwrap(),
        });
    }
    let result = schedule
        .compound(
            &index(),
            &FixingSeries::new(observations).unwrap(),
            context("2018-04-09", "2018-04-09T09:00:00Z"),
            publication,
            |_, _| panic!("all supplied history"),
        )
        .unwrap();
    assert_eq!(
        CashRounding::NearestEven(8)
            .apply(result.period_factor)
            .unwrap(),
        dec!(1.00034365)
    );
    assert_eq!(result.period_year_fraction, dec!(7) / dec!(360));
}

#[test]
fn ac2_lookback_shift_lockout_and_initial_weekend_have_distinct_weights() {
    let lookback = overnight(
        "2025-01-06",
        "2025-01-13",
        OvernightConvention {
            lookback_business_days: 2,
            ..flat()
        },
    );
    let shifted = overnight(
        "2025-01-06",
        "2025-01-13",
        OvernightConvention {
            lookback_business_days: 2,
            observation_shift: true,
            ..flat()
        },
    );
    assert_eq!(lookback.observations()[1].observation_date, d("2025-01-03"));
    assert_eq!(
        lookback.observations()[1]
            .weight_start
            .days_between(&lookback.observations()[1].weight_end),
        1
    );
    assert_eq!(shifted.observations()[1].observation_date, d("2025-01-03"));
    assert_eq!(
        shifted.observations()[1]
            .weight_start
            .days_between(&shifted.observations()[1].weight_end),
        3
    );
    let frozen = overnight(
        "2025-01-06",
        "2025-01-13",
        OvernightConvention {
            lockout_business_days: 2,
            ..flat()
        },
    );
    assert_eq!(
        frozen
            .observations()
            .iter()
            .map(|o| o.observation_date)
            .collect::<Vec<_>>(),
        [
            "2025-01-06",
            "2025-01-07",
            "2025-01-08",
            "2025-01-08",
            "2025-01-08"
        ]
        .map(d)
    );
    assert_eq!(frozen.requirements(&index(), publication).unwrap().len(), 3);
    let mut forecasts = 0;
    let result = frozen
        .compound(
            &index(),
            &FixingSeries::default(),
            context("2025-01-03", "2025-01-03T16:00:00Z"),
            publication,
            |_, _| {
                forecasts += 1;
                Ok(dec!(0.04))
            },
        )
        .unwrap();
    assert_eq!(forecasts, 3);
    assert_eq!(result.observations[4].dates.weight_end, d("2025-01-13"));
    let weekend = overnight("2025-01-04", "2025-01-06", flat());
    assert_eq!(weekend.observations()[0].observation_date, d("2025-01-03"));
    assert_eq!(weekend.observations()[0].weight_start, d("2025-01-04"));
    assert_eq!(weekend.observations()[0].weight_end, d("2025-01-06"));
    let result = weekend
        .compound(
            &index(),
            &FixingSeries::default(),
            context("2025-01-03", "2025-01-03T16:00:00Z"),
            publication,
            |_, _| Ok(dec!(0.04)),
        )
        .unwrap();
    close(
        result.period_factor,
        dec!(1) + dec!(0.04) * dec!(2) / dec!(360),
    );
}

#[test]
fn ac2_holiday_carry_and_missing_history_prevent_curve_fallback() {
    let s = OvernightSchedule::generate(
        d("2025-07-03"),
        d("2025-07-07"),
        DayCountConvention::Act360,
        &SIFMACalendar::new(),
        flat(),
    )
    .unwrap();
    assert_eq!(s.observations().len(), 1);
    assert_eq!(
        s.observations()[0]
            .weight_start
            .days_between(&s.observations()[0].weight_end),
        4
    );
    let s = overnight("2025-01-02", "2025-01-08", flat());
    let mut calls = 0;
    let result = s.compound(
        &index(),
        &FixingSeries::default(),
        context("2025-01-06", "2025-01-06T09:00:00Z"),
        publication,
        |_, _| {
            calls += 1;
            Ok(dec!(0.04))
        },
    );
    assert!(matches!(result,Err(CashflowError::MissingFixings(m)) if m.len()==2));
    assert_eq!(calls, 0);
}

#[test]
fn ac2_known_fixings_are_stable_under_forecast_bumps() {
    let s = overnight("2025-01-02", "2025-01-08", flat());
    let h = FixingSeries::new(vec![
        observation(
            "2025-01-02",
            dec!(0.04),
            "2025-01-03T08:00:00Z",
            "2025-01-03T08:00:00Z",
            1,
        ),
        observation(
            "2025-01-03",
            dec!(0.041),
            "2025-01-06T08:00:00Z",
            "2025-01-06T08:00:00Z",
            1,
        ),
    ])
    .unwrap();
    let c = context("2025-01-06", "2025-01-06T09:00:00Z");
    let a = s
        .compound(&index(), &h, c, publication, |_, _| Ok(dec!(0.05)))
        .unwrap();
    let b = s
        .compound(&index(), &h, c, publication, |_, _| Ok(dec!(0.06)))
        .unwrap();
    assert_eq!(&a.observations[..2], &b.observations[..2]);
    assert!(b.period_factor > a.period_factor);
    assert_eq!(
        a.observations[2].origin,
        RateOrigin::Projected(ProjectionReason::AwaitingPublication)
    );
    assert_eq!(
        a.observations[3].origin,
        RateOrigin::Projected(ProjectionReason::FutureObservation)
    );
}

#[test]
fn ac2_invalid_overnight_terms_and_nonpositive_factors_fail() {
    for convention in [
        OvernightConvention {
            lookback_business_days: 367,
            ..flat()
        },
        OvernightConvention {
            lockout_business_days: 5,
            ..flat()
        },
    ] {
        assert!(OvernightSchedule::generate(
            d("2025-01-06"),
            d("2025-01-13"),
            DayCountConvention::Act360,
            &WeekendCalendar,
            convention
        )
        .is_err());
    }
    assert!(OvernightSchedule::generate(
        d("2025-01-04"),
        d("2025-01-13"),
        DayCountConvention::Act360,
        &WeekendCalendar,
        OvernightConvention {
            observation_shift: true,
            ..flat()
        }
    )
    .is_err());
    assert!(OvernightSchedule::generate(
        d("2025-01-06"),
        d("2025-01-13"),
        DayCountConvention::ActActIcma,
        &WeekendCalendar,
        flat()
    )
    .is_err());
    assert!(OvernightSchedule::generate(
        d("2000-01-01"),
        d("2025-01-01"),
        DayCountConvention::Act360,
        &WeekendCalendar,
        flat()
    )
    .is_err());
}

proptest! {
    #![proptest_config(ProptestConfig {cases:256,rng_seed:proptest::test_runner::RngSeed::Fixed(0x054),..ProptestConfig::default()})]
    #[test]
    fn ac1_anchored_grid_partition_and_icma_regular_fraction(year in 2000_i32..2050, month in 1_u32..13, day in 1_u32..29, count in 1_i32..30) {
        let start=Date::from_ymd(year,month,day).unwrap();
        let end=start.add_months(count*6).unwrap();
        let s=CouponSchedule::generate(&ScheduleSpec {start_date:start,end_date:end,frequency:Frequency::SemiAnnual,stub_type:StubType::None,end_of_month:false,first_regular_date:None,penultimate_date:None},&DateRules::default(),&WeekendCalendar).unwrap();
        prop_assert_eq!(s.periods().len(),count as usize);
        for p in s.periods() {
            prop_assert_eq!(p.year_fraction(p.accrual_dates().0,p.accrual_dates().1,DayCountConvention::ActActIcma).unwrap(),dec!(0.5));
        }
        prop_assert_eq!(s.periods().iter().map(|p|p.accrual_dates().0.days_between(&p.accrual_dates().1)).sum::<i64>(),start.days_between(&end));
    }
}

#[test]
fn ac1_settlement_requires_an_explicit_nonnegative_lag_and_calendar() {
    let joint = JointCalendar::new(vec![
        Box::new(SIFMACalendar::new()),
        Box::new(Target2Calendar::new()),
    ]);
    assert_eq!(
        settlement_date(
            d("2025-01-17"),
            DateOffset::BusinessDays(2),
            BusinessDayConvention::Following,
            &joint
        )
        .unwrap(),
        d("2025-01-22")
    );
    assert_eq!(
        settlement_date(
            d("2025-05-01"),
            DateOffset::CalendarDays(0),
            BusinessDayConvention::Following,
            &joint
        )
        .unwrap(),
        d("2025-05-02")
    );
    assert!(settlement_date(
        d("2025-01-17"),
        DateOffset::BusinessDays(-1),
        BusinessDayConvention::Following,
        &joint
    )
    .is_err());
    assert!(settlement_date(
        d("2025-01-19"),
        DateOffset::CalendarDays(0),
        BusinessDayConvention::Preceding,
        &WeekendCalendar
    )
    .is_err());
}

#[test]
fn ac2_fixing_input_and_request_sizes_are_bounded_before_resolution() {
    let value = observation(
        "2025-01-02",
        dec!(0.04),
        "2025-01-03T08:00:00Z",
        "2025-01-03T08:00:00Z",
        1,
    );
    assert!(matches!(
        FixingSeries::new(vec![value; MAX_FIXING_OBSERVATIONS + 1]),
        Err(CashflowError::BoundExceeded("fixing history"))
    ));
    let requirement = requirement("2025-01-02", "2025-01-03T08:00:00Z");
    assert!(matches!(
        FixingSeries::default().resolve(
            &vec![requirement; MAX_FIXING_REQUIREMENTS + 1],
            context("2025-01-03", "2025-01-03T09:00:00Z")
        ),
        Err(CashflowError::BoundExceeded("fixing requirements"))
    ));
}

struct SparseCalendar;
impl Calendar for SparseCalendar {
    fn name(&self) -> &'static str {
        "Supplied sparse calendar"
    }
    fn is_business_day(&self, date: Date) -> bool {
        date == d("2025-01-02") || date == d("2025-04-03")
    }
}

#[test]
fn ac2_nonpositive_and_overflowing_compound_products_are_errors() {
    let sparse = OvernightSchedule::generate(
        d("2025-01-02"),
        d("2025-04-03"),
        DayCountConvention::Act360,
        &SparseCalendar,
        flat(),
    )
    .unwrap();
    let negative = sparse.compound(
        &index(),
        &FixingSeries::default(),
        context("2025-01-01", "2025-01-01T09:00:00Z"),
        publication,
        |_, _| Ok(dec!(-10)),
    );
    assert!(matches!(
        negative,
        Err(CashflowError::InvalidTerms {
            field: "overnight_factor",
            ..
        })
    ));
    let long = overnight("2025-01-01", "2035-01-01", flat());
    assert!(matches!(
        long.compound(
            &index(),
            &FixingSeries::default(),
            context("2024-12-31", "2024-12-31T09:00:00Z"),
            publication,
            |_, _| Ok(dec!(10))
        ),
        Err(CashflowError::Arithmetic("overnight product"))
    ));
}

#[test]
fn ac3_typed_wire_inputs_cannot_hide_unknown_or_invalid_terms() {
    let s = spec(
        "2024-01-01",
        "2025-01-01",
        Frequency::SemiAnnual,
        StubType::None,
    );
    let mut json = serde_json::to_value(&s).unwrap();
    json["end_date"] = serde_json::json!("2023-01-01");
    let invalid: ScheduleSpec = serde_json::from_value(json.clone()).unwrap();
    assert!(CouponSchedule::generate(&invalid, &DateRules::default(), &WeekendCalendar).is_err());
    json["ignored_calendar"] = serde_json::json!("UK");
    assert!(serde_json::from_value::<ScheduleSpec>(json).is_err());
    let entries:Vec<DatedValue>=serde_json::from_str(r#"[{"start":"2024-01-01","end":"2024-07-01","value":"0.05"},{"start":"2024-06-01","end":"2025-01-01","value":"0.06"}]"#).unwrap();
    assert!(DatedProfile::new(ProfileKind::Coupon, entries).is_err());
}
