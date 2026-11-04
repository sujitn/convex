//! Original Decimal90/QuantLib dated controls and checked provider delegation.
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Date};
use convex_curves::{CurveError, CurveResult, RateCurve, RateCurveDyn, TermStructure, ValueType};
use serde_json::{json, Value};
use std::sync::Arc;

fn reference() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/native-dated-time.json"
    ))
    .unwrap()
}

fn date(text: &str) -> Date {
    Date::parse(text).unwrap()
}

fn basis(row: &Value) -> DayCountConvention {
    match row["day_count"].as_str().unwrap() {
        "Act360" => DayCountConvention::Act360,
        "Act365Fixed" => DayCountConvention::Act365Fixed,
        "ActActIsda" => DayCountConvention::ActActIsda,
        "Thirty360US" => DayCountConvention::Thirty360US,
        "Thirty360E" => DayCountConvention::Thirty360E,
        _ => panic!("unexpected original convention"),
    }
}

#[derive(Clone)]
struct Supplied {
    reference: Date,
    maximum: Date,
    maximum_tenor: f64,
    basis: DayCountConvention,
    initial: f64,
    slope: f64,
}

impl Supplied {
    fn from_row(row: &Value, initial: f64, slope: f64) -> Self {
        Self {
            reference: date(row["reference_date"].as_str().unwrap()),
            maximum: date(row["maximum_date"].as_str().unwrap()),
            maximum_tenor: row["maximum_tenor"].as_str().unwrap().parse().unwrap(),
            basis: basis(row),
            initial,
            slope,
        }
    }
}

impl TermStructure for Supplied {
    fn reference_date(&self) -> Date {
        self.reference
    }
    fn max_date(&self) -> Date {
        self.maximum
    }
    fn tenor_bounds(&self) -> (f64, f64) {
        (0.0, self.maximum_tenor)
    }
    fn value_type(&self) -> ValueType {
        ValueType::ZeroRate {
            compounding: Compounding::Continuous,
            day_count: self.basis,
        }
    }
    fn value_at(&self, tenor: f64) -> f64 {
        self.initial + self.slope * tenor
    }
    fn derivative_at(&self, _: f64) -> Option<f64> {
        Some(self.slope)
    }
}

fn compare(id: &str, actual: f64, expected: &Value, bound: &Value) {
    let expected: f64 = expected.as_str().unwrap().parse().unwrap();
    let bound: f64 = bound.as_str().unwrap().parse().unwrap();
    assert!(
        actual.is_finite() && (actual - expected).abs() <= bound,
        "{id}: {actual} != {expected}"
    );
    // A separate verifier compares these actual bits with the original full
    // Decimal90 answers, avoiding an expected-value binary64 rounding allowance.
    println!(
        "{}",
        json!({"independent_dated_case": id, "actual_bits": format!("{:016x}", actual.to_bits())})
    );
}

#[test]
fn independent_original_dated_times_and_discounts() {
    let reference = reference();
    let rows = reference["dated_cases"].as_array().unwrap();
    assert_eq!(rows.len(), 20);
    for row in rows {
        let source = Supplied::from_row(
            row,
            row["continuous_rate"].as_str().unwrap().parse().unwrap(),
            0.0,
        );
        let end = date(row["date"].as_str().unwrap());
        let id = row["id"].as_str().unwrap();
        compare(
            &format!("{id}:time"),
            source.date_to_tenor(end).unwrap(),
            &row["expected_time"],
            &row["absolute_time_bound"],
        );
        compare(
            &format!("{id}:discount"),
            RateCurve::new(source).discount_factor(end).unwrap(),
            &row["expected_discount"],
            &row["absolute_discount_bound"],
        );
    }
}

#[test]
fn independent_original_dated_forward_periods() {
    let reference = reference();
    let rows = reference["forward_cases"].as_array().unwrap();
    assert_eq!(rows.len(), 45);
    for row in rows {
        let source = Supplied::from_row(
            row,
            row["initial_rate"].as_f64().unwrap(),
            row["slope"].as_f64().unwrap(),
        );
        let compounding = match row["compounding"].as_str().unwrap() {
            "Continuous" => Compounding::Continuous,
            "Simple" => Compounding::Simple,
            "SemiAnnual" => Compounding::SemiAnnual,
            _ => panic!("unexpected original compounding"),
        };
        let actual = RateCurve::new(source)
            .forward_rate(
                date(row["start"].as_str().unwrap()),
                date(row["end"].as_str().unwrap()),
                compounding,
            )
            .unwrap();
        compare(
            row["id"].as_str().unwrap(),
            actual,
            &row["expected_rate"],
            &row["absolute_bound"],
        );
    }
}

#[test]
fn checked_dates_preserve_zero_time_and_require_context() {
    let source = Supplied {
        reference: date("2025-01-30"),
        maximum: date("2035-01-30"),
        maximum_tenor: 10.0,
        basis: DayCountConvention::Thirty360E,
        initial: 0.05,
        slope: 0.0,
    };
    assert_eq!(source.date_to_tenor(source.reference).unwrap(), 0.0);
    let next = date("2025-01-31");
    assert_eq!(source.date_to_tenor(next).unwrap(), 0.0);
    let rate = RateCurve::new(source.clone());
    assert_eq!(rate.discount_factor(next).unwrap(), 1.0);
    assert!(rate
        .forward_rate(source.reference, next, Compounding::Simple)
        .is_err());
    assert!(rate
        .forward_rate(next, source.reference, Compounding::Continuous)
        .is_err());
    assert!(source.date_to_tenor(date("2025-01-29")).is_err());
    assert!(source.date_to_tenor(date("2200-01-30")).is_err());
    assert!(source
        .date_to_tenor(Date::from_ymd(10000, 1, 1).unwrap())
        .is_err());
    for basis in [
        DayCountConvention::ActActIcma,
        DayCountConvention::Act365Leap,
        DayCountConvention::Thirty360EIsda,
    ] {
        let contextual = Supplied {
            basis,
            ..source.clone()
        };
        assert!(contextual.date_to_tenor(date("2026-01-30")).is_err());
        assert!(RateCurve::new(contextual)
            .discount_factor(date("2026-01-30"))
            .is_err());
    }
}

struct Provider {
    source: Supplied,
    deny_dates: bool,
}

impl TermStructure for Provider {
    fn reference_date(&self) -> Date {
        self.source.reference
    }
    fn max_date(&self) -> Date {
        self.source.maximum
    }
    fn tenor_bounds(&self) -> (f64, f64) {
        (0.0, 10.0)
    }
    // DF representation cannot itself declare the provider's ACT/360 clock.
    fn value_type(&self) -> ValueType {
        ValueType::DiscountFactor
    }
    fn tenor_day_count(&self) -> DayCountConvention {
        DayCountConvention::Act360
    }
    fn value_at(&self, _: f64) -> f64 {
        1.0
    }
    fn try_value_at(&self, _: f64) -> CurveResult<f64> {
        Err(CurveError::invalid_value("supplied provider unavailable"))
    }
    fn date_to_tenor(&self, end: Date) -> CurveResult<f64> {
        if self.deny_dates {
            Err(CurveError::invalid_value(
                "supplied date conversion unavailable",
            ))
        } else {
            convex_curves::contracts::date_fraction(
                self.reference_date(),
                end,
                self.tenor_day_count(),
            )
            .map_err(|error| CurveError::invalid_value(error.to_string()))
        }
    }
}

#[test]
fn wrappers_preserve_provider_time_and_checked_failures() {
    for deny_dates in [false, true] {
        let provider = Provider {
            source: Supplied {
                reference: date("2024-01-01"),
                maximum: date("2034-01-01"),
                maximum_tenor: 10.0,
                basis: DayCountConvention::Act360,
                initial: 0.0,
                slope: 0.0,
            },
            deny_dates,
        };
        let end = date("2024-12-26"); // Exactly 360 actual days.
        let shared: Arc<dyn TermStructure> = Arc::new(provider);
        let boxed: Box<dyn TermStructure> = Box::new(shared.clone());
        let borrowed: &dyn TermStructure = boxed.as_ref();
        for source in [&shared as &dyn TermStructure, &boxed, &borrowed] {
            assert_eq!(source.tenor_day_count(), DayCountConvention::Act360);
            if deny_dates {
                assert!(source
                    .date_to_tenor(end)
                    .unwrap_err()
                    .to_string()
                    .contains("date conversion unavailable"));
            } else {
                assert_eq!(source.date_to_tenor(end).unwrap(), 1.0);
            }
            assert!(source.value_at_date(end).is_err());
        }
        let rate = RateCurve::new(shared);
        let dynamic: &dyn RateCurveDyn = &rate;
        assert_eq!(dynamic.date_to_tenor(end).is_err(), deny_dates);
        assert!(rate.discount_factor(end).is_err());
    }
}
