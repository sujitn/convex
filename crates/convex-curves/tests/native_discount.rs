//! Native discount admission, checked delegation and Decimal-boundary controls.
use convex_core::daycounts::DayCountConvention;
use convex_core::traits::YieldCurve;
use convex_core::types::{Compounding, Date};
use convex_curves::{CurveError, CurveResult, RateCurve, TermStructure, ValueType};
use rust_decimal::Decimal;
use std::sync::Arc;

#[derive(Clone)]
struct Supplied {
    value: f64,
    kind: ValueType,
    reject: bool,
}

impl TermStructure for Supplied {
    fn reference_date(&self) -> Date {
        Date::from_ymd(2025, 1, 15).unwrap()
    }
    fn max_date(&self) -> Date {
        Date::from_ymd(2100, 1, 15).unwrap()
    }
    fn value_at(&self, _: f64) -> f64 {
        self.value
    }
    fn try_value_at(&self, _: f64) -> CurveResult<f64> {
        if self.reject {
            Err(CurveError::invalid_value("supplied checked query rejected"))
        } else {
            Ok(self.value)
        }
    }
    fn tenor_bounds(&self) -> (f64, f64) {
        (0.0, 150.0)
    }
    fn value_type(&self) -> ValueType {
        self.kind.clone()
    }
}

fn stored(value: f64) -> Supplied {
    Supplied {
        value,
        kind: ValueType::DiscountFactor,
        reject: false,
    }
}

#[test]
fn discount_queries_require_finite_positive_values_and_valid_tenors() {
    for value in [0.7, 1.0, 1.25] {
        assert_eq!(
            RateCurve::new(stored(value))
                .discount_factor_at_tenor(1.0)
                .unwrap(),
            value
        );
    }
    for value in [0.0, -0.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(RateCurve::new(stored(value))
            .discount_factor_at_tenor(1.0)
            .is_err());
    }
    let curve = RateCurve::new(stored(0.95));
    assert_eq!(curve.discount_factor_at_tenor(0.0).unwrap(), 1.0);
    for tenor in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(curve.discount_factor_at_tenor(tenor).is_err());
    }
    for (rate, compounding) in [
        (1e18, Compounding::Continuous),
        (-1e18, Compounding::Continuous),
        (-1.0, Compounding::Annual),
        (-1.0, Compounding::Simple),
    ] {
        let curve = RateCurve::new(Supplied {
            value: rate,
            kind: ValueType::ZeroRate {
                compounding,
                day_count: DayCountConvention::Act365Fixed,
            },
            reject: false,
        });
        assert!(curve.discount_factor_at_tenor(1.0).is_err());
    }
}

#[test]
fn wrappers_preserve_the_underlying_checked_query_error() {
    let supplied = Supplied {
        reject: true,
        ..stored(0.95)
    };
    for curve in [
        &supplied as &dyn TermStructure,
        &Box::new(supplied.clone()) as &dyn TermStructure,
        &Arc::new(supplied.clone()) as &dyn TermStructure,
        &RateCurve::new(supplied.clone()) as &dyn TermStructure,
    ] {
        assert_eq!(curve.value_at(1.0), 0.95);
        let error = RateCurve::new(curve)
            .discount_factor_at_tenor(1.0)
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("supplied checked query rejected"));
    }
}

#[test]
fn core_decimal_adapter_rejects_erased_nonzero_discount_values() {
    let date = Date::from_ymd(2026, 1, 15).unwrap();
    let healthy = RateCurve::new(stored(0.95));
    assert_eq!(
        YieldCurve::discount_factor(&healthy, date).unwrap(),
        Decimal::from_f64_retain(0.95).unwrap()
    );
    let tiny = RateCurve::new(stored(1e-100));
    assert_eq!(tiny.discount_factor_at_tenor(1.0).unwrap(), 1e-100);
    assert!(YieldCurve::discount_factor(&tiny, date).is_err());
}

#[test]
fn bumped_views_preserve_base_tail_policy_and_checked_errors() {
    use convex_curves::bumping::{KeyRateBump, ParallelBump, Scenario, ScenarioBump};
    use convex_curves::{DiscreteCurve, InterpolationMethod};
    fn check<T: TermStructure>(base: Arc<T>, rejected: bool) {
        let parallel = ParallelBump::new(1.0);
        let key_rate = KeyRateBump::new(5.0, 1.0);
        let scenario = Scenario::new("supplied").with_bump(ScenarioBump::parallel(1.0));
        let borrowed_parallel = parallel.apply(base.as_ref());
        let owned_parallel = parallel.apply_arc(base.clone());
        let borrowed_key_rate = key_rate.apply(base.as_ref());
        let owned_key_rate = key_rate.apply_arc(base.clone());
        let borrowed_scenario = scenario.apply(base.as_ref());
        let owned_scenario = scenario.clone().apply_arc(base.clone());
        let views: [&dyn TermStructure; 6] = [
            &borrowed_parallel,
            &owned_parallel,
            &borrowed_key_rate,
            &owned_key_rate,
            &borrowed_scenario,
            &owned_scenario,
        ];
        for view in views {
            for tenor in [0.25, 30.0] {
                let result = RateCurve::new(view).discount_factor_at_tenor(tenor);
                if rejected {
                    assert!(result
                        .unwrap_err()
                        .to_string()
                        .contains("supplied checked query rejected"));
                } else {
                    assert!(result.unwrap().is_finite());
                }
            }
        }
    }
    let base = DiscreteCurve::new(
        Date::from_ymd(2025, 1, 15).unwrap(),
        vec![1.0, 10.0],
        vec![0.04, 0.04],
        ValueType::ZeroRate {
            compounding: Compounding::Continuous,
            day_count: DayCountConvention::Act365Fixed,
        },
        InterpolationMethod::Linear,
    )
    .unwrap();
    check(Arc::new(base), false);
    check(
        Arc::new(Supplied {
            reject: true,
            ..stored(0.95)
        }),
        true,
    );
}
