//! Independent native original-payment and nominal-reference controls.
use convex_bonds::{
    instruments::FixedRateBond,
    traits::{Bond, BondAnalytics},
    types::CalendarId,
};
use convex_core::{
    calendars::BusinessDayConvention,
    daycounts::DayCountConvention,
    types::{Date, Frequency},
};
use rust_decimal::Decimal;
use serde_json::Value;

#[test]
fn actual_payment_tail_uses_the_next_original_nominal_reference_period() {
    let row: Value = serde_json::from_str(include_str!("fixtures/fixed-icma-tail.json")).unwrap();
    let date = |field: &str| Date::parse(row[field].as_str().unwrap()).unwrap();
    let decimal = |field: &str| row[field].as_str().unwrap().parse::<Decimal>().unwrap();
    let bond = FixedRateBond::builder()
        .cusip_unchecked("FIXED-TIME")
        .issue_date(date("issue_date"))
        .maturity(date("maturity"))
        .coupon_rate(decimal("coupon_rate"))
        .face_value(decimal("face"))
        .redemption_value(decimal("redemption"))
        .frequency(Frequency::SemiAnnual)
        .day_count(DayCountConvention::ActActIcma)
        .calendar(CalendarId::weekend_only())
        .business_day_convention(BusinessDayConvention::Following)
        .end_of_month(true)
        .build()
        .unwrap();
    let settlement = date("settlement");
    let cash = bond.try_cash_flows(settlement).unwrap();
    assert_eq!(cash.len(), 1);
    assert_eq!(cash[0].date, date("actual_remaining_payment_date"));
    assert_eq!(cash[0].amount, Decimal::new(1025, 1));
    let timed = bond
        .yield_cash_flows(settlement, Frequency::SemiAnnual)
        .unwrap();
    assert_eq!(timed.len(), 1);
    let expected_time = row["independent_discount_years"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let time_bound = row["time_absolute_bound"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!(
        (timed[0].0 - expected_time).abs() <= time_bound,
        "{timed:?}"
    );
    assert_eq!(timed[0].1, 102.5);
    let price = bond
        .dirty_price_from_yield(settlement, 0.05, Frequency::SemiAnnual)
        .unwrap();
    let expected_price = row["independent_dirty_price"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let price_bound = row["price_absolute_bound"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!(
        (price - expected_price).abs() <= price_bound,
        "{price} {expected_price}"
    );
    let risk: Value =
        serde_json::from_str(include_str!("fixtures/fixed-icma-tail-risk.json")).unwrap();
    for (field, actual) in [
        (
            "macaulay_duration",
            bond.macaulay_duration(settlement, 0.05, Frequency::SemiAnnual)
                .unwrap(),
        ),
        (
            "modified_duration",
            bond.modified_duration(settlement, 0.05, Frequency::SemiAnnual)
                .unwrap(),
        ),
        (
            "convexity",
            bond.convexity(settlement, 0.05, Frequency::SemiAnnual)
                .unwrap(),
        ),
        (
            "dv01_loss",
            bond.dv01(settlement, 0.05, price, Frequency::SemiAnnual)
                .unwrap(),
        ),
    ] {
        let reference = risk["risk"][field]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let bound = risk["risk_absolute_bound"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        assert!(
            (actual - reference).abs() <= bound,
            "{field} {actual} {reference}"
        );
    }
    assert_eq!(
        bond.try_accrued_interest(settlement).unwrap(),
        Decimal::ZERO
    );
    let inverse = bond
        .yield_to_maturity(
            settlement,
            Decimal::from_f64_retain(expected_price).unwrap(),
            Frequency::SemiAnnual,
        )
        .unwrap();
    assert!((inverse.yield_value - 0.05).abs() <= 1e-11);
    assert!(inverse.residual.abs() < 1e-10);
    assert!(
        (bond
            .dirty_price_from_yield(settlement, inverse.yield_value, Frequency::SemiAnnual)
            .unwrap()
            - expected_price)
            .abs()
            < 1e-10
    );
}

#[test]
fn nominal_reference_generation_retains_original_anchor_and_bounded_coverage() {
    use convex_core::cashflows::{
        generate_icma_reference_periods, ReferencePeriod, ScheduleSpec, StubType,
    };
    let row: Value = serde_json::from_str(include_str!("fixtures/fixed-icma-tail.json")).unwrap();
    let date = |field: &str| Date::parse(row[field].as_str().unwrap()).unwrap();
    let mut spec = ScheduleSpec {
        start_date: date("issue_date"),
        end_date: date("maturity"),
        frequency: Frequency::SemiAnnual,
        stub_type: StubType::None,
        end_of_month: true,
        first_regular_date: None,
        penultimate_date: None,
    };
    let expected: Vec<ReferencePeriod> =
        serde_json::from_value(row["original_nominal_mesh"].clone()).unwrap();
    assert_eq!(
        generate_icma_reference_periods(
            &spec,
            spec.start_date,
            date("actual_remaining_payment_date")
        )
        .unwrap(),
        expected
    );
    assert!(generate_icma_reference_periods(&spec, spec.end_date, spec.start_date).is_err());
    assert!(generate_icma_reference_periods(&spec, spec.start_date, spec.start_date).is_err());
    spec.frequency = Frequency::Zero;
    assert!(generate_icma_reference_periods(
        &spec,
        spec.start_date,
        date("actual_remaining_payment_date")
    )
    .is_err());
}

fn zero_time_fixed() -> (FixedRateBond, Value, Date) {
    let row: Value = serde_json::from_str(include_str!("fixtures/fixed-zero-time.json")).unwrap();
    let date = |field: &str| Date::parse(row[field].as_str().unwrap()).unwrap();
    let bond = FixedRateBond::builder()
        .cusip_unchecked("FIXED-ZERO-TIME")
        .issue_date(date("issue_date"))
        .maturity(date("maturity"))
        .coupon_rate(Decimal::new(5, 2))
        .face_value(Decimal::ONE_HUNDRED)
        .redemption_value(Decimal::ONE_HUNDRED)
        .frequency(Frequency::Annual)
        .day_count(DayCountConvention::Thirty360US)
        .calendar(CalendarId::weekend_only())
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .end_of_month(true)
        .build()
        .unwrap();
    let settlement = date("settlement");
    (bond, row, settlement)
}

#[test]
fn fixed_zero_discount_time_keeps_original_future_coupon_and_principal() {
    let (bond, row, settlement) = zero_time_fixed();
    let actual_cash = bond.try_cash_flows(settlement).unwrap();
    assert_eq!(actual_cash.len(), 1);
    assert_eq!(
        actual_cash[0].date,
        Date::parse(row["actual_remaining_payment_date"].as_str().unwrap()).unwrap()
    );
    assert_eq!(actual_cash[0].amount, Decimal::from(105));
    assert_eq!(
        bond.yield_cash_flows(settlement, Frequency::Annual)
            .unwrap()
            .discount_cash(),
        &[(0.0, 105.0)]
    );
    assert_eq!(
        bond.try_accrued_interest(settlement).unwrap(),
        Decimal::from(5)
    );
    for check in row["price_checks"].as_array().unwrap() {
        let rate = check["annual_yield"].as_str().unwrap().parse().unwrap();
        assert_eq!(
            bond.dirty_price_from_yield(settlement, rate, Frequency::Annual)
                .unwrap(),
            105.0
        );
        assert_eq!(
            bond.clean_price_from_yield(settlement, rate, Frequency::Annual)
                .unwrap(),
            100.0
        );
        assert_eq!(
            bond.macaulay_duration(settlement, rate, Frequency::Annual)
                .unwrap(),
            0.0
        );
        assert_eq!(
            bond.modified_duration(settlement, rate, Frequency::Annual)
                .unwrap(),
            0.0
        );
        assert_eq!(
            bond.convexity(settlement, rate, Frequency::Annual).unwrap(),
            0.0
        );
    }
}

#[test]
fn fixed_zero_time_inverse_has_no_identified_seed_yield() {
    let (bond, row, settlement) = zero_time_fixed();
    for check in row["inverse_checks"].as_array().unwrap() {
        let target = check["clean_per100"].as_str().unwrap().parse().unwrap();
        let error = bond
            .yield_to_maturity(settlement, target, Frequency::Annual)
            .unwrap_err();
        match check["expected_native_classification"].as_str().unwrap() {
            "YieldNonUnique" => assert!(
                matches!(error, convex_bonds::BondError::YieldNonUnique),
                "{error:?}"
            ),
            "YieldNoRoot" => assert!(
                matches!(error, convex_bonds::BondError::YieldNoRoot),
                "{error:?}"
            ),
            other => panic!("Undeclared root class {other}"),
        }
    }
}
