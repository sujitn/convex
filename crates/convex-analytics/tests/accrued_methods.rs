//! Native accrual, full coupon context and predeclared independent cash values.
use convex_bonds::cashflows::AccruedInterestCalculator as Accrued;
use convex_core::calendars::{Calendar, WeekendCalendar};
use convex_core::{
    daycounts::DayCountConvention as Dc,
    types::{Date, Frequency},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::Value;
use std::str::FromStr;

fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}
#[test]
fn isda_accrual_uses_the_actual_calendar_year_fraction() {
    let actual = Accrued::standard(
        date("2025-04-15"),
        date("2025-01-15"),
        date("2025-07-15"),
        dec!(0.05),
        dec!(100),
        Dc::ActActIsda,
        Frequency::SemiAnnual,
    )
    .unwrap();
    // Independent exact amount:100*.05*90/365 =90/73.
    let expected = dec!(90) / dec!(73);
    assert!((actual - expected).abs() <= dec!(0.0000000000000000000000001));
}
#[test]
fn act365l_accrual_uses_the_original_coupon_end_and_frequency() {
    let actual = Accrued::standard(
        date("2024-06-01"),
        date("2024-03-01"),
        date("2024-09-01"),
        dec!(0.05),
        dec!(100),
        Dc::Act365Leap,
        Frequency::SemiAnnual,
    )
    .unwrap();
    // Nonannual coupon ends in2024:100*.05*92/366=230/183.
    let expected = dec!(230) / dec!(183);
    assert!((actual - expected).abs() <= dec!(0.0000000000000000000000001));
}
#[test]
fn accrued_decimal_overflow_is_an_error_instead_of_a_panic() {
    let actual = std::panic::catch_unwind(|| {
        Accrued::standard(
            date("2025-04-15"),
            date("2025-01-15"),
            date("2025-07-15"),
            dec!(2),
            Decimal::MAX,
            Dc::Thirty360US,
            Frequency::SemiAnnual,
        )
    });
    assert!(actual.is_ok(), "native accrued arithmetic panicked");
    assert!(actual.unwrap().is_err());
}

struct SuppliedCalendar;
impl Calendar for SuppliedCalendar {
    fn name(&self) -> &'static str {
        "CAL.ACCRUED.SUPPLIED@1"
    }
    fn is_business_day(&self, d: Date) -> bool {
        WeekendCalendar.is_business_day(d) && d != date("2025-07-08")
    }
}
fn decimal(row: &Value, key: &str) -> Decimal {
    Decimal::from_str(row[key].as_str().unwrap()).unwrap()
}
fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/accrued-methods.json")).unwrap()
}

#[test]
fn all_native_accrual_methods_match_independent_fraction_decimal110_values() {
    let rows = reference();
    assert_eq!(rows["cases"].as_array().unwrap().len(), 87);
    for row in rows["cases"].as_array().unwrap() {
        let settle = date(row["settlement"].as_str().unwrap());
        let start = date(row["period_start"].as_str().unwrap());
        let rate = decimal(row, "annual_rate");
        let face = decimal(row, "original_face");
        let dc: Dc = serde_json::from_value(row["day_count"].clone()).unwrap();
        let result = if row["kind"] == "year_fraction" {
            Accrued::using_year_fraction(settle, start, rate, face, dc)
        } else {
            let end = date(row["period_end"].as_str().unwrap());
            let freq = match row["frequency"].as_u64().unwrap() {
                0 => Frequency::Zero,
                1 => Frequency::Annual,
                2 => Frequency::SemiAnnual,
                4 => Frequency::Quarterly,
                12 => Frequency::Monthly,
                _ => panic!("undeclared frequency"),
            };
            match row["kind"].as_str().unwrap() {
                "standard" => Accrued::standard(settle, start, end, rate, face, dc, freq),
                "ex_dividend" => Accrued::ex_dividend(
                    settle,
                    start,
                    end,
                    rate,
                    face,
                    dc,
                    freq,
                    7,
                    &SuppliedCalendar,
                ),
                "irregular_period" => Accrued::irregular_period(
                    settle,
                    start,
                    end,
                    date(row["reference_start"].as_str().unwrap()),
                    date(row["reference_end"].as_str().unwrap()),
                    rate,
                    face,
                    dc,
                    freq,
                ),
                _ => panic!("undeclared method"),
            }
        };
        let actual = result.unwrap_or_else(|e| panic!("{}: {e}", row["id"]));
        assert!(
            (actual - decimal(row, "comparison_amount")).abs() <= decimal(row, "absolute_bound"),
            "{}: {actual}",
            row["id"]
        );
    }
}

#[test]
fn actual_supplied_holiday_changes_the_ex_coupon_cutoff() {
    use convex_core::calendars::checked;
    assert_eq!(
        checked::add_business_days(date("2025-07-15"), -7, &SuppliedCalendar).unwrap(),
        date("2025-07-03")
    );
    assert_eq!(
        checked::add_business_days(date("2025-07-15"), -7, &WeekendCalendar).unwrap(),
        date("2025-07-04")
    );
}

#[test]
fn full_coupon_period_preserves_context_for_partial_act365l_accrual() {
    use convex_core::cashflows::{CouponSchedule, DateRules, ScheduleSpec, StubType};
    let schedule = CouponSchedule::generate(
        &ScheduleSpec {
            start_date: date("2024-03-01"),
            end_date: date("2024-09-01"),
            frequency: Frequency::SemiAnnual,
            stub_type: StubType::None,
            end_of_month: false,
            first_regular_date: None,
            penultimate_date: None,
        },
        &DateRules::default(),
        &SuppliedCalendar,
    )
    .unwrap();
    assert_eq!(schedule.periods().len(), 1);
    let actual = schedule.periods()[0]
        .year_fraction(date("2024-03-01"), date("2024-06-01"), Dc::Act365Leap)
        .unwrap();
    assert!((actual - dec!(92) / dec!(366)).abs() <= dec!(0.0000000000000000000000000001));
}

#[test]
fn both_engine_traits_propagate_native_accrual_and_missing_context() {
    use convex_bonds::{
        pricing::{StandardYieldEngine, YieldEngine},
        types::{AccruedConvention, YieldCalculationRules},
    };
    let engine = StandardYieldEngine::new();
    let mut rules = YieldCalculationRules::us_corporate();
    for convention in [
        AccruedConvention::Standard,
        AccruedConvention::None,
        AccruedConvention::ExDividend,
        AccruedConvention::RecordDate,
        AccruedConvention::CumDividend,
    ] {
        rules.accrued_convention = convention;
        let native = YieldEngine::accrued_interest(
            &engine,
            date("2025-04-15"),
            date("2025-01-15"),
            date("2025-07-15"),
            dec!(0.05),
            dec!(100),
            &rules,
        );
        let analytics = convex_analytics::yields::YieldEngine::accrued_interest(
            &engine,
            date("2025-04-15"),
            date("2025-01-15"),
            date("2025-07-15"),
            dec!(0.05),
            dec!(100),
            &rules,
        );
        match convention {
            AccruedConvention::Standard => {
                assert_eq!(native.unwrap(), dec!(1.25));
                assert_eq!(analytics.unwrap(), dec!(1.25));
            }
            AccruedConvention::None => {
                assert_eq!(native.unwrap(), Decimal::ZERO);
                assert_eq!(analytics.unwrap(), Decimal::ZERO);
            }
            _ => {
                assert!(native.is_err());
                assert!(analytics.is_err());
            }
        }
    }
    rules.accrued_convention = AccruedConvention::None;
    assert!(YieldEngine::accrued_interest(
        &engine,
        date("2026-04-15"),
        date("2025-01-15"),
        date("2025-07-15"),
        dec!(0.05),
        dec!(100),
        &rules
    )
    .is_err());
}
