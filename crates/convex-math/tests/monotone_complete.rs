//! Frozen independent complete-regime economics and checked-domain controls.
use convex_math::interpolation::{Interpolator, MonotoneConvex};
#[rustfmt::skip]
#[path = "../../../tests/fixtures/library/monotone_reference.rs"]
mod controls;

#[test]
fn complete_native_regimes_match_independent_forward_integral_and_derivative() {
    for model in controls::MODELS {
        let native = MonotoneConvex::new(model.knots.to_vec(), model.zeros.to_vec())
            .unwrap()
            .with_extrapolation();
        for (actual, expected) in native.discrete_forwards().iter().zip(model.means) {
            assert!((actual - expected).abs() <= controls::RATE_BOUND);
        }
        for (actual, expected) in native.node_forward_rates().iter().zip(model.node_forwards) {
            assert!((actual - expected).abs() <= controls::RATE_BOUND);
        }
        for &[time, zero, forward, derivative, integral, discount] in model.points {
            let z = native.interpolate(time).unwrap();
            let f = native.forward_rate(time).unwrap();
            let d = native.derivative(time).unwrap();
            let area = native.integrated_forward(time).unwrap();
            assert!(
                (z - zero).abs() <= controls::RATE_BOUND,
                "{} z({time}): {z} vs {zero}",
                model.id
            );
            assert!(
                (f - forward).abs() <= controls::RATE_BOUND,
                "{} f({time}): {f} vs {forward}",
                model.id
            );
            assert!(
                (d - derivative).abs() <= controls::DERIVATIVE_BOUND,
                "{} dz({time}): {d} vs {derivative}",
                model.id
            );
            assert!((area - integral).abs() <= controls::INTEGRAL_BOUND);
            assert!(
                ((-area).exp() - discount).abs()
                    <= controls::DISCOUNT_ABSOLUTE + controls::DISCOUNT_RELATIVE * discount
            );
            assert!(f >= 0.0);
        }
    }
}

#[test]
fn supplied_nodes_reprice_and_nonnegative_forwards_preserve_discount_order() {
    for model in controls::MODELS {
        let native = MonotoneConvex::new(model.knots.to_vec(), model.zeros.to_vec()).unwrap();
        for (i, &time) in model.knots.iter().enumerate() {
            assert_eq!(native.interpolate(time).unwrap(), model.zeros[i]);
            assert_eq!(
                native.integrated_forward(time).unwrap(),
                time * model.zeros[i]
            );
        }
        let mut previous = 1.0;
        // A fixed epsilon does not bound steep near-axis gradients. Check the
        // actual economic invariant over the entire native domain instead.
        for i in 0..=2048 {
            let t = native.max_x() * f64::from(i) / 2048.0;
            let discount = (-native.integrated_forward(t).unwrap()).exp();
            assert!(native.forward_rate(t).unwrap() >= 0.0);
            assert!(discount > 0.0 && discount <= previous);
            previous = discount;
        }
        assert!(native.interpolate(native.max_x() + 1.0).is_err());
        assert!(native.derivative(native.max_x()).is_ok());
        assert!(native.derivative(0.0).is_ok());
    }
}

#[test]
fn negative_first_interval_nonfinite_and_invalid_times_fail_before_results() {
    for (times, zeros) in [
        (vec![1.0, 2.0], vec![-0.01, 0.02]),
        (vec![1.0, 2.0], vec![0.05, 0.01]),
        (vec![0.0, 2.0], vec![0.02, 0.03]),
        (vec![1.0, 1.0], vec![0.02, 0.03]),
        (vec![1.0, 2.0], vec![f64::NAN, 0.03]),
        (vec![1.0, 2.0], vec![f64::MAX, f64::MAX]),
    ] {
        assert!(MonotoneConvex::new(times, zeros).is_err());
    }
    let native = MonotoneConvex::new(vec![1.0, 2.0], vec![0.0, 0.0])
        .unwrap()
        .with_extrapolation();
    for bad in [-1.0, f64::INFINITY, f64::NAN] {
        assert!(native.interpolate(bad).is_err());
        assert!(native.forward_rate(bad).is_err());
        assert!(native.derivative(bad).is_err());
        assert!(native.integrated_forward(bad).is_err());
    }
    assert_eq!(native.interpolate(150.0).unwrap(), 0.0);
    assert_eq!(native.forward_rate(150.0).unwrap(), 0.0);
    assert_eq!(native.derivative(1e-300).unwrap(), 0.0);
}
