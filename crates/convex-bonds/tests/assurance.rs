use convex_bonds::options::swaption_hw1f::{payer_swaption_hw1f, try_payer_swaption_hw1f};

#[test]
fn jamshidian_failure_is_an_error_and_never_a_zero_rate_fallback() {
    // Negative fixed-leg coefficients cannot bracket a unit bond value.
    let discount = |_: f64| 1.0;
    assert!(try_payer_swaption_hw1f(&discount, 0.03, 0.01, 0.0, 1.0, 4.0, -2.0).is_err());
    assert!(payer_swaption_hw1f(&discount, 0.03, 0.01, 0.0, 1.0, 4.0, -2.0).is_nan());
}

#[test]
fn checked_hw1f_rejects_nonfinite_discounts_and_unbounded_payment_counts() {
    let unavailable = |_: f64| f64::NAN;
    assert!(try_payer_swaption_hw1f(&unavailable, 0.03, 0.01, 0.04, 1.0, 4.0, 0.04).is_err());
    let finite = |t: f64| (-0.04 * t).exp();
    for tail in [f64::NAN, f64::INFINITY, 0.0, 151.0] {
        assert!(try_payer_swaption_hw1f(&finite, 0.03, 0.01, 0.04, 1.0, tail, 0.04).is_err());
    }
}
