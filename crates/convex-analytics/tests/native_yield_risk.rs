//! Independent complete-cash risk, explicit time context and failure propagation.
use convex_analytics::risk::{
    analytical_convexity, macaulay_duration, modified_duration, modified_from_macaulay,
    BondRiskCalculator, Duration,
};
use convex_bonds::{
    pricing::{
        dated_cash::cash_yield_risk,
        discount_time::{DatedYieldCash, DiscountTimeBasis, PreparedDatedCash},
    },
    types::CalendarId,
    FixedRateBond,
};
use convex_core::{
    calendars::BusinessDayConvention,
    contracts::ExactDecimal,
    daycounts::DayCountConvention,
    types::{Compounding, Date, Frequency},
};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use std::str::FromStr;

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/native-yield-risk.json")).unwrap()
}
fn number(row: &Value, key: &str) -> f64 {
    row[key].as_str().unwrap().parse().unwrap()
}
fn array(row: &Value, key: &str) -> Vec<f64> {
    row[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().parse().unwrap())
        .collect()
}
fn comp(row: &Value) -> Compounding {
    serde_json::from_value(row["compounding"].clone()).unwrap()
}
fn close(actual: f64, expected: f64, bound: f64) {
    assert!(
        (actual - expected).abs() <= bound,
        "{actual} vs {expected}, bound {bound}"
    );
}

#[test]
fn every_independent_compounding_case_uses_the_complete_cash_derivatives() {
    let pack = reference();
    let mut actual = vec![];
    for row in pack["cases"].as_array().unwrap() {
        let times = array(row, "times");
        let amounts = array(row, "amounts");
        let y = number(row, "annual_yield");
        let basis = comp(row);
        let cash: Vec<_> = times.iter().copied().zip(amounts.iter().copied()).collect();
        let native = cash_yield_risk(&cash, y, basis).unwrap();
        for (key, value) in [
            ("present_value", native.present_value),
            ("first_derivative", native.first_derivative),
            ("second_derivative", native.second_derivative),
        ] {
            close(value, number(row, key), number(&pack["bounds"], "price"));
        }
        let calc = BondRiskCalculator::from_cash_flows(
            times.clone(),
            amounts.clone(),
            y,
            basis,
            number(row, "present_value"),
            100.0,
        )
        .unwrap();
        let all = calc.all_metrics().unwrap();
        for (key, value) in [
            ("macaulay_duration", all.macaulay_duration.as_f64()),
            ("modified_duration", all.modified_duration.as_f64()),
            ("convexity", all.convexity.as_f64()),
        ] {
            close(value, number(row, key), number(&pack["bounds"], "risk"));
        }
        close(
            all.dv01_per_100.as_f64(),
            number(row, "dv01_per_100"),
            number(&pack["bounds"], "dv01"),
        );
        assert_eq!(all.macaulay_duration, calc.macaulay_duration().unwrap());
        assert_eq!(all.modified_duration, calc.modified_duration().unwrap());
        assert_eq!(all.convexity, calc.convexity().unwrap());
        assert_eq!(all.dv01_per_100, calc.dv01_per_100().unwrap());
        assert_eq!(all.dv01, calc.dv01().unwrap());
        close(
            macaulay_duration(&times, &amounts, y, basis)
                .unwrap()
                .as_f64(),
            number(row, "macaulay_duration"),
            1e-11,
        );
        close(
            modified_duration(&times, &amounts, y, basis)
                .unwrap()
                .as_f64(),
            number(row, "modified_duration"),
            1e-11,
        );
        close(
            analytical_convexity(&times, &amounts, y, basis)
                .unwrap()
                .as_f64(),
            number(row, "convexity"),
            1e-11,
        );
        actual.push(json!({"id":row["id"],"native":native,"macaulay_duration":all.macaulay_duration.years().to_string(),
            "modified_duration":all.modified_duration.years().to_string(),"convexity":all.convexity.value().to_string(),
            "dv01_per_100":all.dv01_per_100.value().to_string()}));
    }
    assert_eq!(actual.len(), 63);
    println!(
        "{}",
        json!({"kind":"complete_native_yield_risk_results","cases":actual})
    );
}

#[test]
fn all_three_original_simple_cash_references_remain_exact_inputs() {
    for row in reference()["original_simple_references"]
        .as_array()
        .unwrap()
    {
        let calc = BondRiskCalculator::from_cash_flows(
            array(row, "times"),
            array(row, "amounts"),
            number(row, "annual_yield"),
            Compounding::Simple,
            number(row, "present_value"),
            100.0,
        )
        .unwrap();
        close(
            calc.modified_duration().unwrap().as_f64(),
            number(row, "modified_duration"),
            1e-11,
        );
        close(
            calc.all_metrics().unwrap().modified_duration.as_f64(),
            number(row, "modified_duration"),
            1e-11,
        );
    }
}

#[test]
fn nonfinite_and_invalid_raw_inputs_never_become_successful_zero_risk() {
    let make = |times: Vec<f64>, cash: Vec<f64>, yield_rate, price, face| {
        BondRiskCalculator::from_cash_flows(
            times,
            cash,
            yield_rate,
            Compounding::SemiAnnual,
            price,
            face,
        )
    };
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(make(vec![1.0, 2.0], vec![100.0, 100.0], bad, 100.0, 100.0).is_err());
        assert!(make(vec![bad, 2.0], vec![100.0, 100.0], 0.05, 100.0, 100.0).is_err());
        assert!(make(vec![1.0, 2.0], vec![bad, 100.0], 0.05, 100.0, 100.0).is_err());
        assert!(make(vec![1.0, 2.0], vec![100.0, 100.0], 0.05, bad, 100.0).is_err());
        assert!(make(vec![1.0, 2.0], vec![100.0, 100.0], 0.05, 100.0, bad).is_err());
    }
    for times in [vec![], vec![0.0, 2.0], vec![-1.0, 2.0], vec![1.0; 4097]] {
        assert!(make(times, vec![100.0, 100.0], 0.05, 100.0, 100.0).is_err());
    }
    assert!(make(vec![1.0, 2.0], vec![0.0, 0.0], 0.05, 100.0, 100.0).is_err());
    assert!(make(vec![1.0; 4097], vec![100.0; 4097], 0.05, 100.0, 100.0).is_err());
    assert!(make(vec![1.0], vec![-100.0], 0.05, 100.0, 100.0).is_err());
    assert!(make(vec![1.0], vec![100.0], -2.0, 100.0, 100.0).is_err());
    assert!(make(vec![1.0], vec![100.0], 0.05, 0.0, 100.0).is_err());
    assert!(make(vec![1e-300], vec![1e-300], 0.05, 100.0, 100.0).is_err());
    assert!(BondRiskCalculator::from_cash_flows(
        vec![2.0],
        vec![100.0],
        -0.5,
        Compounding::Simple,
        100.0,
        100.0
    )
    .is_err());
}

#[test]
fn signed_cash_and_actual_signed_position_scaling_are_retained() {
    let calc = BondRiskCalculator::from_cash_flows(
        vec![1.0, 2.0],
        vec![230.0, -132.0],
        0.1,
        Compounding::Annual,
        100.0,
        -200.0,
    )
    .unwrap();
    close(
        calc.macaulay_duration().unwrap().as_f64(),
        -1.0 / 11.0,
        1e-11,
    );
    close(
        calc.modified_duration().unwrap().as_f64(),
        -10.0 / 121.0,
        1e-11,
    );
    close(
        calc.dv01().unwrap().as_f64(),
        calc.dv01_per_100().unwrap().as_f64() * -2.0,
        1e-11,
    );
    let zero = BondRiskCalculator::from_cash_flows(
        vec![1.0],
        vec![100.0],
        0.05,
        Compounding::Annual,
        100.0,
        0.0,
    )
    .unwrap();
    assert_eq!(zero.dv01().unwrap().value(), Decimal::ZERO);
    let tiny = BondRiskCalculator::from_cash_flows(
        vec![1.0],
        vec![100.0],
        0.05,
        Compounding::Annual,
        100.0,
        f64::from_bits(1),
    )
    .unwrap();
    assert!(tiny.dv01().is_err());
}

#[test]
fn aggregate_macaulay_conversion_cannot_invent_simple_cash_derivatives() {
    assert!(modified_from_macaulay(Duration::from(1.5), 0.1, Compounding::Simple).is_err());
    for rate in [f64::NAN, f64::INFINITY, -2.0] {
        assert!(
            modified_from_macaulay(Duration::from(1.5), rate, Compounding::SemiAnnual).is_err()
        );
    }
    close(
        modified_from_macaulay(Duration::from(5.0), 0.06, Compounding::SemiAnnual)
            .unwrap()
            .as_f64(),
        5.0 / 1.03,
        1e-11,
    );
    assert_eq!(
        modified_from_macaulay(Duration::from(5.0), -0.01, Compounding::Continuous).unwrap(),
        Duration::from(5.0)
    );
}

#[test]
fn bond_factory_retains_original_dated_laws_quote_frequency_and_face() {
    let pack: Value = serde_json::from_str(include_str!("fixtures/yas-quote-owner.json")).unwrap();
    for row in pack["cases"].as_array().unwrap() {
        let day_count = match row["day_count"].as_str().unwrap() {
            "Thirty360US" => DayCountConvention::Thirty360US,
            "Act365Fixed" => DayCountConvention::Act365Fixed,
            "ActActIcma" => DayCountConvention::ActActIcma,
            _ => panic!("fixture day count"),
        };
        let face = Decimal::from_str(row["face"].as_str().unwrap()).unwrap();
        let date = |s: &str| Date::parse(s).unwrap();
        let bond = FixedRateBond::builder()
            .cusip_unchecked("RISK-DATED")
            .face_value(face)
            .redemption_value(face)
            .coupon_rate(dec!(0.05))
            .frequency(Frequency::SemiAnnual)
            .day_count(day_count)
            .issue_date(date("2025-01-15"))
            .maturity(date("2035-01-15"))
            .calendar(CalendarId::weekend_only())
            .business_day_convention(BusinessDayConvention::Unadjusted)
            .end_of_month(false)
            .build()
            .unwrap();
        let frequency = match row["frequency"].as_u64().unwrap() {
            1 => Frequency::Annual,
            2 => Frequency::SemiAnnual,
            4 => Frequency::Quarterly,
            12 => Frequency::Monthly,
            _ => panic!("fixture frequency"),
        };
        let calc = BondRiskCalculator::from_bond(
            &bond,
            date("2025-04-15"),
            number(row, "dirty"),
            number(row, "annual_yield"),
            frequency,
        )
        .unwrap();
        let risk = calc.all_metrics().unwrap();
        close(
            risk.macaulay_duration.as_f64(),
            number(row, "macaulay_duration"),
            1e-8,
        );
        close(
            risk.modified_duration.as_f64(),
            number(row, "modified_duration"),
            1e-8,
        );
        close(risk.convexity.as_f64(), number(row, "convexity"), 1e-8);
        close(
            risk.dv01_per_100.as_f64(),
            number(row, "dv01_per_100"),
            1e-10,
        );
        close(risk.dv01.as_f64(), number(row, "dv01"), 1e-10);
        assert_eq!(calc.face_value(), face.to_f64().unwrap());
        assert!(
            BondRiskCalculator::from_bond(&bond, date("2024-12-31"), 100.0, 0.05, frequency)
                .is_err()
        );
    }
}

#[test]
fn actual_future_zero_time_cash_is_preserved_with_zero_derivatives() {
    let cash = [DatedYieldCash {
        date: Date::parse("2025-01-31").unwrap(),
        amount: ExactDecimal::new(dec!(100)),
    }];
    let prepared = PreparedDatedCash::new(
        &cash,
        Date::parse("2025-01-30").unwrap(),
        &DiscountTimeBasis::Calendar {
            day_count: DayCountConvention::Thirty360E,
        },
    )
    .unwrap();
    assert_eq!(prepared.discount_cash(), &[(0.0, 100.0)]);
    for basis in [
        Compounding::Annual,
        Compounding::SemiAnnual,
        Compounding::Quarterly,
        Compounding::Monthly,
        Compounding::Daily,
        Compounding::Continuous,
        Compounding::Simple,
    ] {
        let calc =
            BondRiskCalculator::from_prepared_cash(&prepared, 0.05, basis, 100.0, 100.0).unwrap();
        let result = calc.all_metrics().unwrap();
        assert_eq!(result.macaulay_duration.years(), Decimal::ZERO);
        assert_eq!(result.modified_duration.years(), Decimal::ZERO);
        assert_eq!(result.convexity.value(), Decimal::ZERO);
        assert_eq!(result.dv01_per_100.value(), Decimal::ZERO);
        assert_eq!(result.dv01.value(), Decimal::ZERO);
    }
    assert!(cash_yield_risk(&[(0.0, 100.0)], 0.05, Compounding::Annual).is_err());
}
