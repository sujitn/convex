//! Failure and invariant coverage for CVX-039; exercises public pure APIs.

use convex_math::bounds::{MAX_DATA_POINTS, MAX_MATRIX_DIMENSION, MAX_SOLVER_ITERATIONS};
use convex_math::extrapolation::{Extrapolator, UfrConvergence};
use convex_math::interpolation::{FlatForward, Interpolator, LinearInterpolator};
use convex_math::linear_algebra::solve_tridiagonal;
use convex_math::optimization::golden_section_checked;
use convex_math::solvers::{bisection, brent, hybrid, newton_raphson, secant, SolverConfig};
use proptest::prelude::*;

#[test]
fn empty_and_one_row_tridiagonal_systems_do_not_panic() {
    assert_eq!(
        solve_tridiagonal(&[], &[], &[], &[]).unwrap(),
        Vec::<f64>::new()
    );
    assert_eq!(
        solve_tridiagonal(&[], &[2.0], &[], &[6.0]).unwrap(),
        vec![3.0]
    );
    assert!(solve_tridiagonal(&[], &[0.0], &[], &[1.0]).is_err());
    assert!(solve_tridiagonal(&[], &[f64::NAN], &[], &[1.0]).is_err());
}

#[test]
fn solver_configuration_is_rejected_before_evaluation() {
    for tolerance in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        let config = SolverConfig::new(tolerance, 10);
        assert!(brent(
            |_| panic!("invalid configuration evaluated"),
            0.0,
            1.0,
            &config
        )
        .is_err());
        assert!(bisection(
            |_| panic!("invalid configuration evaluated"),
            0.0,
            1.0,
            &config
        )
        .is_err());
        assert!(secant(
            |_| panic!("invalid configuration evaluated"),
            0.0,
            1.0,
            &config
        )
        .is_err());
        assert!(newton_raphson(
            |_| panic!("invalid configuration evaluated"),
            |_| 1.0,
            0.0,
            &config
        )
        .is_err());
        assert!(hybrid(
            |_| panic!("invalid configuration evaluated"),
            |_| 1.0,
            0.0,
            Some((0.0, 1.0)),
            &config
        )
        .is_err());
    }
}

#[test]
fn nonfinite_evaluations_never_become_successful_roots() {
    let config = SolverConfig::default();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(brent(|_| value, 0.0, 1e-12, &config).is_err());
        assert!(bisection(|_| value, 0.0, 1e-12, &config).is_err());
        assert!(newton_raphson(|_| 1.0, |_| value, 0.0, &config).is_err());
        assert!(secant(|x| if x == 0.0 { 1.0 } else { value }, 0.0, 1.0, &config).is_err());
    }
}

#[test]
fn malformed_origin_data_is_rejected_before_indexing() {
    assert!(FlatForward::with_origin(vec![1.0], vec![]).is_err());
    assert!(FlatForward::with_origin(vec![-1.0, 1.0], vec![0.02, 0.03]).is_err());
}

#[test]
fn interpolation_rejects_nonfinite_nodes_and_queries() {
    assert!(LinearInterpolator::new(vec![0.0, f64::NAN], vec![0.0, 1.0]).is_err());
    assert!(LinearInterpolator::new(vec![0.0, 1.0], vec![0.0, f64::INFINITY]).is_err());
    let interp = LinearInterpolator::new(vec![0.0, 1.0], vec![0.0, 1.0])
        .unwrap()
        .with_extrapolation();
    assert!(interp.interpolate(f64::NAN).is_err());
    assert!(interp.derivative(f64::INFINITY).is_err());
}

#[test]
fn iteration_and_allocation_limits_fail_before_work() {
    for budget in [0, MAX_SOLVER_ITERATIONS + 1, u32::MAX] {
        assert!(brent(
            |_| panic!("out-of-budget callback"),
            0.0,
            1.0,
            &SolverConfig::new(1e-10, budget)
        )
        .is_err());
    }
    assert!(LinearInterpolator::new(
        vec![0.0; MAX_DATA_POINTS + 1],
        vec![1.0; MAX_DATA_POINTS + 1]
    )
    .is_err());
    let matrix = nalgebra::DMatrix::identity(MAX_MATRIX_DIMENSION + 1, MAX_MATRIX_DIMENSION + 1);
    assert!(convex_math::linear_algebra::lu_decomposition(&matrix).is_err());
}

#[test]
fn nonconvergence_and_stagnation_are_explicit_and_bounded() {
    let evaluations = std::cell::Cell::new(0);
    let result = bisection(
        |x| {
            evaluations.set(evaluations.get() + 1);
            if x < 0.3 {
                -1.0
            } else {
                1.0
            }
        },
        0.0,
        1.0,
        &SolverConfig::new(1e-300, 8),
    );
    assert!(result.is_err());
    assert!(evaluations.get() <= 8 + 3);
    assert!(newton_raphson(|_| 1e30, |_| 1e308, 0.0, &SolverConfig::default()).is_err());
    // Multiplying signs underflows; comparing signs must still refuse this bracket.
    assert!(brent(|_| 1e-250, 0.0, 1.0, &SolverConfig::new(1e-300, 8)).is_err());
    assert!(hybrid(
        |_| f64::NAN,
        |_| 1.0,
        0.0,
        Some((-1.0, 1.0)),
        &SolverConfig::default()
    )
    .is_err());
}

#[test]
fn checked_compatibility_apis_reject_invalid_models_and_queries() {
    assert!(UfrConvergence::try_new(0.04, 0.0, 20.0).is_err());
    assert!(UfrConvergence::try_new(f64::NAN, 0.1, 20.0).is_err());
    let mut ufr = UfrConvergence::try_new(-0.01, 0.1, 20.0).unwrap();
    assert!(ufr
        .try_extrapolate(30.0, 20.0, -0.02, 0.0)
        .unwrap()
        .is_finite());
    ufr.convergence_speed = f64::NAN;
    assert!(ufr.try_extrapolate(30.0, 20.0, 0.02, 0.0).is_err());
    assert!(golden_section_checked(|_| f64::NAN, -1.0, 1.0, 1e-10, 100).is_err());
    assert!(golden_section_checked(|x| x * x, -1.0, 1.0, 1e-10, 1).is_err());
    assert!(golden_section_checked(
        |_| panic!("invalid budget evaluated"),
        -1.0,
        1.0,
        1e-10,
        usize::MAX
    )
    .is_err());
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, rng_seed: proptest::test_runner::RngSeed::Fixed(0x039), .. ProptestConfig::default() })]

    #[test]
    fn signed_affine_roots_roundtrip(root in -0.09f64..0.90, slope in 0.25f64..10.0) {
        let f = |x: f64| slope * (x - root);
        let config = SolverConfig::default();
        for result in [brent(f, -0.10, 1.0, &config), bisection(f, -0.10, 1.0, &config),
            newton_raphson(f, |_| slope, 0.05, &config), secant(f, 0.0, 0.2, &config)] {
            let result = result.unwrap();
            prop_assert!(result.root.is_finite() && result.residual.is_finite());
            prop_assert!((result.root - root).abs() <= config.tolerance / slope);
            prop_assert!(result.iterations <= config.max_iterations);
        }
    }

    #[test]
    fn diagonally_dominant_system_recovers_known_solution(solution in prop::collection::vec(-100.0f64..100.0, 1..64)) {
        let n = solution.len();
        let off = vec![-1.0; n - 1];
        let diagonal = vec![4.0; n];
        let rhs: Vec<_> = (0..n).map(|i| 4.0 * solution[i] - if i>0 { solution[i-1] } else { 0.0 }
            - if i+1<n { solution[i+1] } else { 0.0 }).collect();
        let actual = solve_tridiagonal(&off, &diagonal, &off, &rhs).unwrap();
        for (actual, expected) in actual.iter().zip(&solution) { prop_assert!((actual - expected).abs() < 1e-10); }
    }

    #[test]
    fn arbitrary_float_patterns_are_errors_or_finite_results(a in any::<u64>(), b in any::<u64>(), c in any::<u64>(), query in any::<u64>()) {
        let xs = vec![0.0, 1.0, 2.0];
        let ys = vec![f64::from_bits(a), f64::from_bits(b), f64::from_bits(c)];
        let query = f64::from_bits(query);
        if let Ok(interpolator) = LinearInterpolator::new(xs, ys) {
            for value in [interpolator.interpolate(query), interpolator.derivative(query)].into_iter().flatten() {
                prop_assert!(value.is_finite());
            }
        }
    }
}
