//! Sinking cash uses the original coupon mesh, including non-additive month ends.
use convex_bonds::{
    instruments::{FixedRateBond, SinkingFundBond, SinkingFundPayment, SinkingFundSchedule},
    traits::{Bond, BondAnalytics},
    types::CalendarId,
};
use convex_core::{
    calendars::BusinessDayConvention,
    daycounts::DayCountConvention,
    types::{Date, Frequency},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn date(year: i32, month: u32, day: u32) -> Date {
    Date::from_ymd(year, month, day).unwrap()
}

fn base(
    face: Decimal,
    day_count: DayCountConvention,
    issue: Date,
    maturity: Date,
) -> FixedRateBond {
    FixedRateBond::builder()
        .cusip_unchecked("SUPPLIED-SINKER-CLOCK")
        .coupon_rate(dec!(0.05))
        .issue_date(issue)
        .maturity(maturity)
        .face_value(face)
        .frequency(Frequency::SemiAnnual)
        .day_count(day_count)
        .calendar(CalendarId::weekend_only())
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .end_of_month(false)
        .build()
        .unwrap()
}

fn annual_sinker(face: Decimal) -> SinkingFundBond {
    let mut sinks = SinkingFundSchedule::new();
    for year in 2026..=2030 {
        sinks = sinks.with_payment(SinkingFundPayment::with_price(
            date(year, 12, 15),
            20.0,
            100.0,
        ));
    }
    SinkingFundBond::new(
        base(
            face,
            DayCountConvention::Thirty360US,
            date(2020, 12, 15),
            date(2030, 12, 15),
        ),
        sinks,
    )
}

#[test]
fn month_end_sinker_price_and_risk_use_remaining_coupon_period() {
    let bond = annual_sinker(dec!(100));
    let settlement = date(2025, 12, 31);
    let prepared = bond
        .yield_cash_flows(settlement, Frequency::SemiAnnual)
        .unwrap();
    let original = bond.try_cash_flows(settlement).unwrap();
    assert_eq!(prepared.len(), 15);
    for (flow, &(time, _)) in original.iter().zip(prepared.iter()) {
        let half_years = (flow.date.year() - 2026) * 2 + i32::from(flow.date.month() == 12);
        // 180 coupon days minus 16 elapsed days; direct settlement-to-June
        // subtraction gives 165 and is wrong for coupon-based street yields.
        let expected = (164.0 + 180.0 * f64::from(half_years)) / 360.0;
        assert!((time - expected).abs() < 1e-14);
    }
    // Independently executed QuantLib 1.40 AmortizingFixedRateBond, 4% yield,
    // 30/360 USA, NullCalendar/Unadjusted, same five 20%-at-par repayments.
    let frequency = Frequency::SemiAnnual;
    for (actual, expected) in [
        (
            bond.clean_price_from_yield(settlement, 0.04, frequency)
                .unwrap(),
            102.72470772087495,
        ),
        (
            bond.dirty_price_from_yield(settlement, 0.04, frequency)
                .unwrap(),
            102.94692994309716,
        ),
        (
            bond.macaulay_duration(settlement, 0.04, frequency).unwrap(),
            2.7551924948577637,
        ),
        (
            bond.modified_duration(settlement, 0.04, frequency).unwrap(),
            2.7011691126056507,
        ),
        (
            bond.convexity(settlement, 0.04, frequency).unwrap(),
            10.57476091718453,
        ),
        (
            bond.dv01(settlement, 0.04, 102.94692994309716, frequency)
                .unwrap(),
            0.027807706739987188,
        ),
    ] {
        assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
    }
    let inverse = bond
        .yield_to_maturity(settlement, dec!(102.72470772087495), frequency)
        .unwrap();
    assert!((inverse.yield_value - 0.04).abs() < 1e-9);
}

#[test]
fn intracoupon_principal_keeps_the_original_coupon_clock() {
    let bond = SinkingFundBond::new(
        base(
            dec!(100),
            DayCountConvention::Thirty360US,
            date(2025, 12, 15),
            date(2026, 12, 15),
        ),
        SinkingFundSchedule::new().with_payment(SinkingFundPayment::with_price(
            date(2026, 3, 15),
            50.0,
            100.0,
        )),
    );
    let settlement = date(2025, 12, 31);
    let flows = bond.try_cash_flows(settlement).unwrap();
    let cash = bond
        .yield_cash_flows(settlement, Frequency::SemiAnnual)
        .unwrap();
    assert_eq!(cash.len(), flows.len());
    assert_eq!(cash.len(), 4);
    for (flow, &(time, _)) in flows.iter().zip(cash.iter()) {
        let days = match flow.date.month() {
            3 => 74.0,
            6 => 164.0,
            12 => 344.0,
            _ => panic!("unexpected payment"),
        };
        assert!((time - days / 360.0).abs() < 1e-14);
    }
    assert_eq!(
        flows.iter().map(|flow| flow.amount).sum::<Decimal>(),
        dec!(103.125)
    );
}

#[test]
fn icma_sinker_retains_nominal_periods_and_same_date_components() {
    let bond = SinkingFundBond::new(
        base(
            dec!(100),
            DayCountConvention::ActActIcma,
            date(2025, 12, 15),
            date(2026, 12, 15),
        ),
        SinkingFundSchedule::new().with_payment(SinkingFundPayment::with_price(
            date(2026, 6, 15),
            50.0,
            100.0,
        )),
    );
    let settlement = date(2025, 12, 31);
    let flows = bond.try_cash_flows(settlement).unwrap();
    let prepared = bond
        .yield_cash_flows(settlement, Frequency::SemiAnnual)
        .unwrap();
    assert_eq!(prepared.len(), 4);
    let first_time = 166.0 / (2.0 * 182.0);
    for (flow, &(time, _)) in flows.iter().zip(prepared.iter()) {
        let expected = first_time + if flow.date.month() == 12 { 0.5 } else { 0.0 };
        assert!((time - expected).abs() < 1e-14);
    }
}

#[test]
fn quote_face_scaling_and_checked_failures_are_preserved() {
    let settlement = date(2025, 12, 31);
    let a = annual_sinker(dec!(100));
    let b = annual_sinker(dec!(1000));
    let first = a
        .yield_cash_flows(settlement, Frequency::SemiAnnual)
        .unwrap();
    let scaled = b
        .yield_cash_flows(settlement, Frequency::SemiAnnual)
        .unwrap();
    assert_eq!(first.discount_cash(), scaled.discount_cash());
    assert!(a.yield_cash_flows(settlement, Frequency::Zero).is_err());
    assert!(a
        .yield_cash_flows(date(2030, 12, 15), Frequency::SemiAnnual)
        .is_err());
    assert!(a
        .dirty_price_from_yield(settlement, -2.0, Frequency::SemiAnnual)
        .is_err());
}
