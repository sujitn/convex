//! CVX-060 synthetic analytic fits, native-unit failures and bounded work.
use convex_math::optimization::*;
use proptest::{
    prelude::*,
    test_runner::{Config, RngSeed},
};

fn parameter(initial: f64) -> FitParameter {
    FitParameter {
        initial,
        lower: -1.0,
        upper: 1.0,
    }
}
fn residual(scale: f64) -> FitResidual {
    FitResidual {
        scale,
        weight: 1.0,
        tolerance: 1e-9 * scale,
    }
}
fn linear(target: f64) -> Result<LeastSquaresOutput, FitFailure<&'static str>> {
    bounded_least_squares(
        &[parameter(0.2)],
        &[residual(1.0)],
        LeastSquaresSettings::default(),
        |x| Ok(vec![x[0] - target]),
    )
}

#[test]
fn identified_negative_solution_meets_native_and_objective_bounds() {
    let output = linear(-0.04).unwrap();
    assert!((output.parameters[0] + 0.04).abs() < 1e-10);
    assert_eq!(output.diagnostics.rank, Some(1));
    assert_eq!(output.diagnostics.condition_estimate, Some(1.0));
    assert!(output.diagnostics.iterations <= 100);
    assert!(output.diagnostics.evaluations <= 1 + 100 + 2 * 101);
}

#[test]
fn native_unit_scaling_preserves_the_fit_and_keeps_native_residuals() {
    let mut outputs = Vec::new();
    for scale in [1.0, 100.0, 10_000.0] {
        let output = bounded_least_squares(
            &[parameter(0.0)],
            &[residual(scale)],
            LeastSquaresSettings::default(),
            |x| Ok::<_, &'static str>(vec![(x[0] + 0.02) * scale]),
        )
        .unwrap();
        assert!(output.diagnostics.native_residuals[0].abs() <= 1e-9 * scale);
        outputs.push(output.parameters[0]);
    }
    assert!(outputs.windows(2).all(|p| (p[0] - p[1]).abs() < 1e-12));
}

#[test]
fn exact_start_still_requires_identification_and_reports_no_steps() {
    let output = bounded_least_squares(
        &[parameter(-1.0)],
        &[residual(1.0)],
        LeastSquaresSettings::default(),
        |x| Ok::<_, &'static str>(vec![x[0] + 1.0]),
    )
    .unwrap();
    assert_eq!(output.parameters, [-1.0]);
    assert_eq!(output.diagnostics.iterations, 0);
    assert_eq!(output.diagnostics.active_bounds, 1);
    assert_eq!(output.diagnostics.evaluations, 3);
    let failed = bounded_least_squares(
        &[parameter(0.0)],
        &[residual(1.0)],
        LeastSquaresSettings::default(),
        |_| Ok::<_, &'static str>(vec![0.0]),
    )
    .unwrap_err();
    assert!(matches!(failed.reason, FitFailureReason::IllConditioned));
    assert_eq!(failed.diagnostics.rank, Some(0));
}

#[test]
fn nearly_singular_and_correlated_jacobians_fail_without_parameters() {
    for coefficient in [0.0, 1e-12] {
        let failed = bounded_least_squares(
            &[parameter(0.0), parameter(0.0)],
            &[residual(1.0); 2],
            LeastSquaresSettings::default(),
            |x| {
                Ok::<_, &'static str>(vec![
                    x[0] + x[1] - 0.1,
                    x[0] + x[1] + coefficient * x[1] - 0.1,
                ])
            },
        )
        .unwrap_err();
        assert!(matches!(failed.reason, FitFailureReason::IllConditioned));
        assert!(failed.diagnostics.rank.unwrap() < 2);
    }
}

#[test]
fn invalid_dimensions_and_budgets_never_evaluate() {
    for size in [0, MAX_FIT_DIMENSION + 1] {
        let failed = bounded_least_squares(
            &vec![parameter(0.0); size],
            &vec![residual(1.0); size],
            LeastSquaresSettings::default(),
            |_| -> Result<Vec<f64>, &'static str> { panic!("invalid input evaluated") },
        )
        .unwrap_err();
        assert!(matches!(failed.reason, FitFailureReason::InvalidInput(_)));
        assert_eq!(failed.diagnostics.evaluations, 0);
    }
    for budget in [0, 10_001, usize::MAX] {
        for settings in [
            LeastSquaresSettings {
                max_iterations: budget,
                ..Default::default()
            },
            LeastSquaresSettings {
                svd_iterations: budget,
                ..Default::default()
            },
        ] {
            let failed = bounded_least_squares(
                &[parameter(0.0)],
                &[residual(1.0)],
                settings,
                |_| -> Result<Vec<f64>, &'static str> { panic!("invalid settings evaluated") },
            )
            .unwrap_err();
            assert_eq!(failed.diagnostics.evaluations, 0);
        }
    }
}

#[test]
fn invalid_bounds_and_native_contracts_fail_before_callbacks() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
        let failed = bounded_least_squares(
            &[parameter(0.0)],
            &[FitResidual {
                scale: value,
                ..residual(1.0)
            }],
            LeastSquaresSettings::default(),
            |_| -> Result<Vec<f64>, &'static str> { panic!("bad scale evaluated") },
        )
        .unwrap_err();
        assert_eq!(failed.diagnostics.evaluations, 0);
    }
    for p in [
        parameter(f64::NAN),
        parameter(2.0),
        FitParameter {
            initial: 0.0,
            lower: 1.0,
            upper: 1.0,
        },
        FitParameter {
            initial: 0.0,
            lower: -f64::MAX,
            upper: f64::MAX,
        },
    ] {
        let failed = bounded_least_squares(
            &[p],
            &[residual(1.0)],
            LeastSquaresSettings::default(),
            |_| -> Result<Vec<f64>, &'static str> { panic!("bad bound evaluated") },
        )
        .unwrap_err();
        assert_eq!(failed.diagnostics.evaluations, 0);
    }
    for contract in [
        FitResidual {
            weight: 0.0,
            ..residual(1.0)
        },
        FitResidual {
            weight: -1.0,
            ..residual(1.0)
        },
        FitResidual {
            tolerance: 0.0,
            ..residual(1.0)
        },
    ] {
        assert!(bounded_least_squares(
            &[parameter(0.0)],
            &[contract],
            LeastSquaresSettings::default(),
            |_| Ok::<_, &'static str>(vec![0.0])
        )
        .is_err());
    }
}

#[test]
fn callback_error_is_preserved_and_nonfinite_or_wrong_dimension_never_converges() {
    let failed = bounded_least_squares(
        &[parameter(0.0)],
        &[residual(1.0)],
        LeastSquaresSettings::default(),
        |_| Err::<Vec<f64>, _>("missing fixing: INDEX/2024-01-02"),
    )
    .unwrap_err();
    assert!(matches!(
        failed.reason,
        FitFailureReason::Callback("missing fixing: INDEX/2024-01-02")
    ));
    assert_eq!(failed.diagnostics.evaluations, 1);
    for output in [vec![], vec![0.0, 0.0], vec![f64::NAN], vec![f64::INFINITY]] {
        assert!(bounded_least_squares(
            &[parameter(0.0)],
            &[residual(1.0)],
            LeastSquaresSettings::default(),
            |_| Ok::<_, &'static str>(output.clone())
        )
        .is_err());
    }
}

#[test]
fn actual_iteration_limit_retains_measured_native_failure() {
    let failed = bounded_least_squares(
        &[parameter(0.1)],
        &[residual(1.0)],
        LeastSquaresSettings {
            max_iterations: 1,
            ..Default::default()
        },
        |x| Ok::<_, &'static str>(vec![x[0] * x[0] - 0.25]),
    )
    .unwrap_err();
    assert!(matches!(failed.reason, FitFailureReason::IterationLimit));
    assert_eq!(failed.diagnostics.iterations, 1);
    assert!(failed.diagnostics.scaled_rms.unwrap() > 1e-10);
}

#[test]
fn infeasible_bounds_and_zero_weight_quotes_cannot_certify_a_fit() {
    let failed = bounded_least_squares(
        &[parameter(0.0)],
        &[residual(1.0)],
        LeastSquaresSettings::default(),
        |x| Ok::<_, &'static str>(vec![x[0] - 2.0]),
    )
    .unwrap_err();
    assert!(matches!(failed.reason, FitFailureReason::Stagnation));
    assert_eq!(failed.diagnostics.active_bounds, 1);
    let failed = bounded_least_squares(
        &[parameter(0.0)],
        &[
            residual(1.0),
            FitResidual {
                weight: 0.0,
                ..residual(1.0)
            },
        ],
        LeastSquaresSettings::default(),
        |x| Ok::<_, &'static str>(vec![x[0] - 0.2, 1.0]),
    )
    .unwrap_err();
    assert!(matches!(
        failed.reason,
        FitFailureReason::Stagnation | FitFailureReason::IterationLimit
    ));
    assert_eq!(failed.diagnostics.native_residuals[1], 1.0);
}

#[test]
fn overflow_in_scaling_and_jacobian_is_a_failure() {
    let failed = bounded_least_squares(
        &[parameter(0.0)],
        &[residual(f64::MIN_POSITIVE)],
        LeastSquaresSettings::default(),
        |_| Ok::<_, &'static str>(vec![f64::MAX]),
    )
    .unwrap_err();
    assert!(matches!(
        failed.reason,
        FitFailureReason::Numerical("scaled residual")
    ));
}

#[test]
fn bounded_svd_reports_decomposition_limit() {
    let failed = bounded_least_squares(
        &[parameter(0.0); 4],
        &[residual(1.0); 6],
        LeastSquaresSettings {
            svd_iterations: 1,
            ..Default::default()
        },
        |x| {
            Ok::<_, &'static str>(
                (0..6)
                    .map(|i| {
                        x.iter()
                            .enumerate()
                            .map(|(j, value)| value * ((i * 4 + j + 1) as f64).sin())
                            .sum::<f64>()
                            - 0.1
                    })
                    .collect(),
            )
        },
    )
    .unwrap_err();
    assert!(matches!(
        failed.reason,
        FitFailureReason::DecompositionLimit
    ));
}

proptest! {
    #![proptest_config(Config { cases: 256, rng_seed: RngSeed::Fixed(0x060), ..Config::default() })]
    #[test]
    fn scaled_linear_properties(target in -9000_i32..9000, unit in 0_u8..3) {
        let target = f64::from(target) / 10_000.0;
        let scale = [1.0, 100.0, 10_000.0][usize::from(unit)];
        let result = bounded_least_squares(&[parameter(0.0)], &[residual(scale)], LeastSquaresSettings::default(),
            |x| Ok::<_, &'static str>(vec![(x[0] - target) * scale])).unwrap();
        prop_assert!((result.parameters[0] - target).abs() <= 1e-9);
        prop_assert!(result.parameters[0] >= -1.0 && result.parameters[0] <= 1.0);
        prop_assert!(result.diagnostics.native_residuals[0].abs() <= 1e-9 * scale);
        prop_assert_eq!(result.diagnostics.rank, Some(1));
    }
}
