//! Engine composition uses the exact same supplied pure product implementation.
#[path = "../../convex-rates/tests/support/mod.rs"]
mod support;
use convex_analytics::swap_calibration::{
    build_swap_curve_set, SwapBootstrapSettings, SwapCalibrationHelper, SwapCurveBuildInput,
    SwapCurveRecipe,
};
use convex_core::fixings::FixingSeries;
use convex_curves::contracts::{
    CalibrationMode, CurveSet, CurveSource, FitNumericalControls, MarketQuote, QuoteUnit,
};
use support::*;
#[test]
fn supplied_engine_curve_entrypoint_has_native_repricing_and_failure_parity() {
    let case = case("regular-eom");
    let terms = terms(&case);
    let calendars = calendars(&case);
    let values = flat_curve(&terms, d("2024-01-15"), 0.02, 0.04);
    let supplied = CurveSet::new()
        .with_curve(
            values
                .get(&terms.collateral.discount_curve)
                .unwrap()
                .as_ref()
                .clone(),
        )
        .unwrap();
    let mut curve = values
        .get(terms.floating.projection_curve())
        .unwrap()
        .recipe()
        .clone();
    curve.dependencies = vec![terms.collateral.discount_curve.clone()];
    curve.source = CurveSource::Helpers(vec!["six-month".into()]);
    curve.calibration = CalibrationMode::Piecewise;
    let recipe = SwapCurveRecipe {
        curve,
        pillars: vec![d("2024-07-31")],
        helpers: vec![SwapCalibrationHelper {
            id: "six-month".into(),
            pillar: d("2024-07-31"),
            terms: terms.clone(),
            quote: MarketQuote {
                value: 0.04,
                unit: QuoteUnit::DecimalRate,
            },
            tolerance: 1e-12,
        }],
        overnight_projection: None,
        bootstrap: Some(SwapBootstrapSettings {
            lower_zero: -0.1,
            upper_zero: 0.15,
            max_iterations: 100,
        }),
    };
    let calendars = [bindings(&recipe.helpers[0].terms, &calendars)];
    let inputs = [SwapCurveBuildInput {
        recipe: &recipe,
        calendars: &calendars,
    }];
    let history = FixingSeries::new(vec![]).unwrap();
    let context = context(d("2024-01-15"));
    let controls = FitNumericalControls::default();
    let pure =
        build_swap_curve_set(&inputs, &supplied, &history, context, controls, publication).unwrap();
    let engine = convex_engine::curve_builder::build_supplied_swap_curves(
        &inputs,
        &supplied,
        &history,
        context,
        controls,
        publication,
    )
    .unwrap();
    assert_eq!(engine.build_order, pure.build_order);
    assert_eq!(
        engine.calibrations[0].curve.diagnostics(),
        pure.calibrations[0].curve.diagnostics()
    );
    assert_eq!(
        engine.calibrations[0].repriced_helpers[0].present_value,
        pure.calibrations[0].repriced_helpers[0].present_value
    );
    assert!(
        engine.calibrations[0].curve.diagnostics().quotes[0]
            .residual
            .abs()
            <= 1e-12
    );
    assert!(convex_engine::curve_builder::build_supplied_swap_curves(
        &inputs,
        &CurveSet::new(),
        &history,
        context,
        controls,
        publication
    )
    .is_err());
    assert_eq!(supplied.len(), 1);
}
