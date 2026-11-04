//! Independent frozen exponential tail controls and checked native domains.
use convex_math::extrapolation::{Extrapolator, UfrConvergence};
#[path = "../../../tests/fixtures/library/ufr_reference.rs"]
mod reference;

#[test]
fn independent_positive_negative_small_alpha_and_boundary_integrals() {
    for &[llp, zero, forward, ufr, alpha, t, expected_zero, expected_forward, expected_discount] in
        reference::POINTS
    {
        let kernel = UfrConvergence::try_new(ufr, alpha, llp).unwrap();
        let z = kernel.try_zero_rate(t, zero, forward).unwrap();
        let f = kernel.try_forward_rate(t, forward).unwrap();
        assert!(
            (z - expected_zero).abs() <= reference::RATE_BOUND,
            "{t}: zero {z}"
        );
        assert!(
            (f - expected_forward).abs() <= reference::RATE_BOUND,
            "{t}: forward {f}"
        );
        assert!(
            ((-z * t).exp() - expected_discount).abs()
                <= reference::DISCOUNT_ABSOLUTE
                    + reference::DISCOUNT_RELATIVE * expected_discount.abs()
        );
        let original = kernel
            .try_extrapolate(t, llp, zero, (forward - zero) / llp)
            .unwrap();
        assert!((original - z).abs() <= reference::RATE_BOUND);
    }
}

#[test]
fn checked_tail_revalidates_mutable_parameters_and_all_finite_inputs() {
    let kernel = UfrConvergence::try_new(0.042, 0.1, 20.0).unwrap();
    for t in [-1.0, 0.0, 19.0, f64::NAN, f64::INFINITY] {
        assert!(kernel.try_zero_rate(t, 0.025, 0.03).is_err());
        assert!(kernel.try_forward_rate(t, 0.03).is_err());
    }
    for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(kernel.try_zero_rate(21.0, rate, 0.03).is_err());
        assert!(kernel.try_zero_rate(21.0, 0.025, rate).is_err());
        assert!(kernel.try_forward_rate(21.0, rate).is_err());
    }
    for invalid in [
        UfrConvergence {
            convergence_speed: 0.0,
            ..kernel
        },
        UfrConvergence {
            ultimate_forward_rate: f64::NAN,
            ..kernel
        },
        UfrConvergence {
            last_liquid_point: -1.0,
            ..kernel
        },
    ] {
        assert!(invalid.try_zero_rate(21.0, 0.025, 0.03).is_err());
        assert!(invalid.try_forward_rate(21.0, 0.03).is_err());
    }
    assert_eq!(kernel.try_zero_rate(20.0, 0.025, 0.03).unwrap(), 0.025);
    assert_eq!(kernel.try_forward_rate(20.0, 0.03).unwrap(), 0.03);
}

#[test]
fn subnormal_and_overflow_decay_limits_preserve_the_original_integral() {
    // alpha*tau rounds to zero, but the integrated decay retains tau.
    let slow = UfrConvergence::try_new(0.04, f64::from_bits(1), 1.0).unwrap();
    let t = 1.000001;
    let zero = slow.try_zero_rate(t, 0.02, 0.03).unwrap();
    assert!((zero - (0.02 + 0.03 * (t - 1.0)) / t).abs() < 1e-15);
    assert!((slow.try_forward_rate(t, 0.03).unwrap() - 0.03).abs() < 1e-15);
    // alpha*tau overflows while the converged forward/integral is finite.
    let fast = UfrConvergence::try_new(0.04, f64::MAX, 1.0).unwrap();
    assert_eq!(fast.try_forward_rate(150.0, 0.03).unwrap(), 0.04);
    assert!(
        (fast.try_zero_rate(150.0, 0.02, 0.03).unwrap() - (0.02 + 0.04 * 149.0) / 150.0).abs()
            < 1e-15
    );
}
