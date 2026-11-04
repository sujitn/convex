//! Independent supplied-period conversions and native curve admission.
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Date};
use convex_curves::{CurveError, CurveResult, RateCurve, TermStructure, ValueConverter, ValueType};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn compounding(value: &Value) -> Compounding {
    match value.as_str().unwrap() {
        "Continuous" => Compounding::Continuous,
        "Simple" => Compounding::Simple,
        "Annual" => Compounding::Annual,
        "SemiAnnual" => Compounding::SemiAnnual,
        "Quarterly" => Compounding::Quarterly,
        "Monthly" => Compounding::Monthly,
        "Daily" => Compounding::Daily,
        _ => panic!("unknown independent fixture convention"),
    }
}
fn number(row: &Value, key: &str) -> f64 {
    row[key].as_f64().unwrap()
}
fn scalar(row: &Value) -> CurveResult<f64> {
    let p = &row["parameters"];
    match row["operation"].as_str().unwrap() {
        "convert_compounding" => ValueConverter::convert_compounding(
            number(p, "rate"),
            number(p, "time"),
            compounding(&p["source"]),
            compounding(&p["target"]),
        ),
        "df_to_zero" => ValueConverter::df_to_zero(
            number(p, "discount"),
            number(p, "time"),
            compounding(&p["compounding"]),
        ),
        "zero_to_df" => ValueConverter::zero_to_df(
            number(p, "rate"),
            number(p, "time"),
            compounding(&p["compounding"]),
        ),
        "forward_rate_from_dfs" => ValueConverter::forward_rate_from_dfs(
            number(p, "first_discount"),
            number(p, "second_discount"),
            number(p, "start"),
            number(p, "end"),
            compounding(&p["compounding"]),
        ),
        "forward_rate_from_zeros" => ValueConverter::forward_rate_from_zeros(
            number(p, "first_rate"),
            number(p, "second_rate"),
            number(p, "start"),
            number(p, "end"),
        ),
        "instantaneous_forward" => ValueConverter::instantaneous_forward(
            number(p, "rate"),
            number(p, "derivative"),
            number(p, "time"),
            Compounding::Continuous,
        ),
        "hazard_to_survival" => {
            ValueConverter::hazard_to_survival(number(p, "hazard"), number(p, "time"))
        }
        "implied_hazard_rate" => {
            ValueConverter::implied_hazard_rate(number(p, "survival"), number(p, "time"))
        }
        "survival_to_hazard" => {
            ValueConverter::survival_to_hazard(number(p, "survival"), number(p, "derivative"))
        }
        "risky_discount_factor" => ValueConverter::risky_discount_factor(
            number(p, "discount"),
            number(p, "survival"),
            number(p, "recovery"),
        ),
        _ => panic!("unknown independent fixture operation"),
    }
}
fn check(row: &Value, actual: f64) {
    let expected: f64 = row["expected_decimal"].as_str().unwrap().parse().unwrap();
    let bound: f64 = row["absolute_bound"].as_str().unwrap().parse().unwrap();
    assert!(
        actual.is_finite() && (actual - expected).abs() <= bound,
        "{}: {actual} != {expected}",
        row["id"]
    );
    // The separate Decimal90 verifier checks the exact binary64 result against
    // every full-precision answer, including the original absolute bound.
    println!(
        "{}",
        json!({"independent_curve_case":row["id"],"actual_bits":format!("{:016x}",actual.to_bits())})
    );
}
#[derive(Clone)]
struct Supplied {
    rate: f64,
    derivative: Option<f64>,
    anchor: f64,
    kind: ValueType,
    calls: Arc<AtomicUsize>,
}
impl TermStructure for Supplied {
    fn reference_date(&self) -> Date {
        Date::from_ymd(2025, 1, 15).unwrap()
    }
    fn value_at(&self, t: f64) -> f64 {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if t == self.anchor {
            self.rate
        } else {
            self.rate + self.derivative.unwrap_or(0.0) * (t - self.anchor)
        }
    }
    fn tenor_bounds(&self) -> (f64, f64) {
        (0.0, 10.0)
    }
    fn value_type(&self) -> ValueType {
        self.kind.clone()
    }
    fn derivative_at(&self, _t: f64) -> Option<f64> {
        self.derivative
    }
    fn max_date(&self) -> Date {
        Date::from_ymd(2035, 1, 13).unwrap()
    }
}
fn supplied(comp: Compounding) -> Supplied {
    Supplied {
        rate: 0.05,
        derivative: Some(0.005),
        anchor: 2.0,
        kind: ValueType::ZeroRate {
            compounding: comp,
            day_count: DayCountConvention::Act365Fixed,
        },
        calls: Arc::new(AtomicUsize::new(0)),
    }
}
#[test]
fn independent_scalar_conversions() {
    let reference: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/native-curve-conversions.json"
    ))
    .unwrap();
    let rows = reference["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 640);
    for row in rows {
        check(row, scalar(row).unwrap());
    }
}
#[test]
fn independent_analytic_instantaneous_forwards() {
    let reference: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/native-curve-derivatives.json"
    ))
    .unwrap();
    let rows = reference["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 192);
    for row in rows {
        let mut source = supplied(compounding(&row["compounding"]));
        source.rate = number(row, "rate");
        source.derivative = Some(number(row, "derivative"));
        source.anchor = number(row, "time");
        if row["value_type"] == "discount_factor" {
            source.kind = ValueType::DiscountFactor;
        }
        check(
            row,
            RateCurve::new(source)
                .instantaneous_forward_at_tenor(number(row, "time"))
                .unwrap(),
        );
    }
}
#[test]
fn checked_scalar_domains() {
    for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(ValueConverter::df_to_zero(0.9, t, Compounding::Continuous).is_err());
        assert!(ValueConverter::convert_compounding(
            0.05,
            t,
            Compounding::Continuous,
            Compounding::Continuous
        )
        .is_err());
        assert!(ValueConverter::implied_hazard_rate(0.9, t).is_err());
    }
    for value in [-1.0, 0.0, f64::NAN, f64::INFINITY] {
        assert!(ValueConverter::df_to_zero(value, 1.0, Compounding::Continuous).is_err());
        assert!(ValueConverter::forward_rate_from_dfs(
            value,
            0.9,
            0.0,
            1.0,
            Compounding::Continuous
        )
        .is_err());
    }
    for rate in [f64::NAN, f64::INFINITY] {
        assert!(ValueConverter::zero_to_df(rate, 0.0, Compounding::Continuous).is_err());
        assert!(ValueConverter::convert_compounding(
            rate,
            1.0,
            Compounding::Continuous,
            Compounding::Continuous
        )
        .is_err());
    }
    assert!(ValueConverter::zero_to_df(-2.0, 1.0, Compounding::Annual).is_err());
    assert!(ValueConverter::zero_to_df(-2.0, 1.0, Compounding::Simple).is_err());
    assert!(ValueConverter::zero_to_df(1e308, 1.0, Compounding::Continuous).is_err());
    assert!(ValueConverter::forward_rate_from_zeros(0.04, 0.05, 1.0, 1.0).is_err());
    assert!(
        ValueConverter::forward_rate_from_dfs(1.0, 0.9, 1.0, 1.0, Compounding::Simple).is_err()
    );
}
#[test]
fn checked_native_admission() {
    let source = supplied(Compounding::Continuous);
    let curve = RateCurve::new(source.clone());
    for t in [f64::NAN, f64::INFINITY, -1.0, 11.0] {
        assert!(curve
            .zero_rate_at_tenor(t, Compounding::Continuous)
            .is_err());
        assert!(curve.instantaneous_forward_at_tenor(t).is_err());
        assert!(source.try_value_at(t).is_err());
    }
    assert_eq!(source.calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        curve
            .zero_rate_at_tenor(0.0, Compounding::Continuous)
            .unwrap(),
        0.04
    );
    assert_eq!(curve.discount_factor_at_tenor(0.0).unwrap(), 1.0);
    let mut bad = source.clone();
    bad.rate = f64::NAN;
    assert!(RateCurve::new(bad)
        .instantaneous_forward_at_tenor(2.0)
        .is_err());
    let mut bad = source;
    bad.derivative = Some(f64::NAN);
    assert!(RateCurve::new(bad)
        .instantaneous_forward_at_tenor(2.0)
        .is_err());
    for comp in [
        Compounding::Continuous,
        Compounding::Simple,
        Compounding::Annual,
        Compounding::SemiAnnual,
        Compounding::Quarterly,
        Compounding::Monthly,
        Compounding::Daily,
    ] {
        let mut source = supplied(comp);
        source.derivative = None;
        assert!(matches!(
            RateCurve::new(source).instantaneous_forward_at_tenor(2.0),
            Err(CurveError::DerivativeNotAvailable { .. })
        ));
    }
}
#[test]
fn independent_supplied_origin_quotations() {
    let reference: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/native-curve-origins.json"
    ))
    .unwrap();
    let rows = reference["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 168);
    for row in rows {
        let mut source = supplied(compounding(&row["source"]));
        source.anchor = 0.0;
        source.rate = number(row, "value");
        source.derivative = None;
        if row["kind"] == "discount_factor" {
            source.kind = ValueType::DiscountFactor;
            source.derivative = Some(number(row, "derivative"));
        }
        let calls = source.calls.clone();
        check(
            row,
            RateCurve::new(source)
                .zero_rate_at_tenor(0.0, compounding(&row["target"]))
                .unwrap(),
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
#[test]
fn origin_quotes_require_genuine_source_information() {
    let mut source = supplied(Compounding::Continuous);
    source.anchor = 0.0;
    source.kind = ValueType::DiscountFactor;
    source.rate = 1.0;
    source.derivative = None;
    assert!(matches!(
        RateCurve::new(source.clone()).zero_rate_at_tenor(0.0, Compounding::Continuous),
        Err(CurveError::DerivativeNotAvailable { .. })
    ));
    source.derivative = Some(f64::NAN);
    assert!(RateCurve::new(source.clone())
        .zero_rate_at_tenor(0.0, Compounding::Continuous)
        .is_err());
    source.derivative = Some(-0.05);
    source.rate = 0.99;
    assert!(RateCurve::new(source)
        .zero_rate_at_tenor(0.0, Compounding::Continuous)
        .is_err());
    let mut simple = supplied(Compounding::Simple);
    simple.anchor = 0.0;
    simple.rate = -2.0;
    simple.derivative = None;
    assert_eq!(
        RateCurve::new(simple)
            .zero_rate_at_tenor(0.0, Compounding::Continuous)
            .unwrap(),
        -2.0
    );
    let mut invalid_periodic = supplied(Compounding::Annual);
    invalid_periodic.anchor = 0.0;
    invalid_periodic.rate = -1.0;
    assert!(RateCurve::new(invalid_periodic)
        .zero_rate_at_tenor(0.0, Compounding::Annual)
        .is_err());
}
#[derive(Clone)]
struct CheckedFailure(Supplied);
impl TermStructure for CheckedFailure {
    fn reference_date(&self) -> Date {
        self.0.reference_date()
    }
    fn value_at(&self, _t: f64) -> f64 {
        panic!("checked callers must not use raw fallback")
    }
    fn try_value_at(&self, _t: f64) -> CurveResult<f64> {
        Err(CurveError::invalid_value("supplied source failure"))
    }
    fn tenor_bounds(&self) -> (f64, f64) {
        self.0.tenor_bounds()
    }
    fn value_type(&self) -> ValueType {
        self.0.value_type()
    }
    fn derivative_at(&self, t: f64) -> Option<f64> {
        self.0.derivative_at(t)
    }
    fn max_date(&self) -> Date {
        self.0.max_date()
    }
}
fn failure_is_preserved<T: TermStructure>(source: T) {
    let curve = RateCurve::new(source);
    assert!(curve
        .zero_rate_at_tenor(2.0, Compounding::Continuous)
        .unwrap_err()
        .to_string()
        .contains("supplied source failure"));
    assert!(curve
        .forward_rate_at_tenors(1.0, 2.0, Compounding::Continuous)
        .is_err());
    assert!(curve.instantaneous_forward_at_tenor(2.0).is_err());
}
#[test]
fn checked_source_errors_delegate_through_shared_views() {
    let source = CheckedFailure(supplied(Compounding::Continuous));
    failure_is_preserved(source.clone());
    failure_is_preserved(&source);
    failure_is_preserved(Arc::new(source.clone()));
    failure_is_preserved(Box::new(source));
}
#[test]
fn identity_conversion_retains_original_rate_bits() {
    for comp in [
        Compounding::Continuous,
        Compounding::Simple,
        Compounding::Annual,
        Compounding::SemiAnnual,
        Compounding::Quarterly,
        Compounding::Monthly,
        Compounding::Daily,
    ] {
        for rate in [-0.01_f64, 0.0, 0.05] {
            for t in [0.25, 1.0, 2.0] {
                assert_eq!(
                    ValueConverter::convert_compounding(rate, t, comp, comp)
                        .unwrap()
                        .to_bits(),
                    rate.to_bits()
                );
            }
        }
    }
}
#[test]
fn constant_intensity_and_terminal_payoff_domains() {
    for q in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
        assert!(ValueConverter::implied_hazard_rate(q, 1.0).is_err());
        assert!(ValueConverter::risky_discount_factor(0.9, q, 0.4).is_err());
    }
    assert!(ValueConverter::hazard_to_survival(-0.1, 1.0).is_err());
    assert!(ValueConverter::survival_to_hazard(0.9, 0.1).is_err());
    assert!(ValueConverter::survival_to_hazard(0.0, -0.1).is_err());
    assert!(ValueConverter::risky_discount_factor(0.9, 0.8, -0.1).is_err());
    assert!(ValueConverter::risky_discount_factor(0.9, 0.8, 1.1).is_err());
    assert_eq!(ValueConverter::hazard_to_survival(0.02, 0.0).unwrap(), 1.0);
    assert_eq!(
        ValueConverter::risky_discount_factor(0.9, 0.0, 0.0).unwrap(),
        0.0
    );
}
