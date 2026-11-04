//! Execute original rejected curve recipes through the facade native curve API.
use convex::curve_contracts::{
    build_curve_set, fit_parametric_curve, CurveFitError, CurveRecipe, CurveSet,
    ParametricFitSettings,
};
use convex::library::LibraryError;
use convex_math::optimization::FitFailureReason;
use serde_json::Value;

fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!(
        "../../../tests/fixtures/library/curve-families.json"
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn original_native_build_failures_preserve_complete_recipe_and_error_kind() {
    let mut count = 0;
    for case in cases()
        .into_iter()
        .filter(|c| c["request"]["operation"]["kind"] == "curves" && !c["expected_error"].is_null())
    {
        let input = case["request"]["inputs"]["curves"].clone();
        let recipes: Vec<CurveRecipe> = serde_json::from_value(input.clone()).unwrap();
        let supplied = CurveSet::new();
        let error = build_curve_set(&recipes, &supplied).unwrap_err();
        let error = LibraryError::from(error);
        assert_eq!(
            serde_json::to_value(error.code).unwrap(),
            case["expected_error"]
        );
        assert!(!error.message.is_empty());
        assert_eq!(serde_json::to_value(&recipes).unwrap(), input);
        assert!(supplied.is_empty());
        count += 1;
    }
    assert_eq!(count, 29);
}

#[test]
fn original_native_parametric_fit_failures_return_no_curve_and_keep_inputs() {
    let mut count = 0;
    for case in cases().into_iter().filter(|c| {
        c["request"]["operation"]["kind"] == "parametric_fit" && !c["expected_error"].is_null()
    }) {
        let input = case["request"]["operation"]["recipe"].clone();
        let settings_input = case["request"]["operation"]["settings"].clone();
        let recipe: CurveRecipe = serde_json::from_value(input.clone()).unwrap();
        let settings: ParametricFitSettings =
            serde_json::from_value(settings_input.clone()).unwrap();
        match fit_parametric_curve(&recipe, &settings).unwrap_err() {
            CurveFitError::Contract(error) => {
                let actual = LibraryError::from(error);
                assert_eq!(
                    serde_json::to_value(actual.code).unwrap(),
                    case["expected_error"]
                );
                assert!(!actual.message.is_empty());
            }
            CurveFitError::Solver(error) => {
                let expected = case["expected_error"].as_str().unwrap();
                let valid = match &error.reason {
                    FitFailureReason::InvalidInput(_) => expected == "invalid_input",
                    FitFailureReason::IllConditioned => expected == "non_unique",
                    FitFailureReason::Callback(native) => {
                        serde_json::to_value(LibraryError::from(native.clone()).code).unwrap()
                            == case["expected_error"]
                    }
                    FitFailureReason::Numerical(_)
                    | FitFailureReason::DecompositionLimit
                    | FitFailureReason::IterationLimit
                    | FitFailureReason::Stagnation => expected == "no_convergence",
                };
                assert!(valid, "{}: {}", case["id"], error);
                assert!(!error.to_string().is_empty());
            }
        }
        assert_eq!(serde_json::to_value(recipe).unwrap(), input);
        assert_eq!(serde_json::to_value(settings).unwrap(), settings_input);
        count += 1;
    }
    assert_eq!(count, 9);
}
