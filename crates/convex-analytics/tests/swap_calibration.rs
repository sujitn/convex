//! Full product fitting, dependency and failure regressions.
#[path = "../../convex-rates/tests/support/mod.rs"]
mod support;
use convex_analytics::swap_calibration::*;
use convex_core::{contracts::IndexId, fixings::FixingSeries};
use convex_curves::contracts::{
    CalibrationMode, CurveKey, CurveRole, CurveSet, CurveSource, FitNumericalControls,
    GlobalFitSettings, MarketQuote, QuoteUnit,
};
use convex_rates::{FloatingLegTerms, SwapCalendars};
use rust_decimal::{prelude::FromPrimitive, Decimal};
use serde_json::json;
use support::*;

fn key(id: &str) -> CurveKey {
    CurveKey {
        id: id.into(),
        version: "v1".into(),
    }
}
fn helper(
    id: &str,
    source: &str,
    pillar: &str,
    quote: MarketQuote,
    overnight: bool,
) -> SwapCalibrationHelper {
    let mut value = case(source);
    let floating = value["terms"]["floating"].clone();
    if overnight {
        value["terms"]["floating"] = json!({"kind":"overnight","leg":floating["leg"],"index_id":"USD.ON","profiles":floating["profiles"],"projection_curve":key("USD.ON.PROJ"),"fixing_policy":floating["fixing_policy"],"observation_calendar":{"id":"OBSERVATION","version":1},"convention":{"lookback_business_days":0,"observation_shift":false,"lockout_business_days":0}});
    }
    let mut terms = terms(&value);
    let normalized = quote.decimal_rate().unwrap();
    for rate in &mut terms.fixed.rates {
        rate.value = Decimal::from_f64(normalized).unwrap();
    }
    SwapCalibrationHelper {
        id: id.into(),
        pillar: d(pillar),
        terms,
        quote,
        tolerance: match quote.unit {
            QuoteUnit::DecimalRate => 1e-12,
            QuoteUnit::PercentRate => 1e-10,
            QuoteUnit::BasisPoints => 1e-8,
            _ => panic!("rate quote required"),
        },
    }
}
fn recipe(overnight: bool, global: bool) -> SwapCurveRecipe {
    let helpers = vec![
        helper(
            "six-month",
            "regular-eom",
            "2024-08-02",
            MarketQuote {
                value: 0.04,
                unit: QuoteUnit::DecimalRate,
            },
            overnight,
        ),
        helper(
            "longer",
            "independent-leg-frequency",
            if overnight {
                "2025-02-11"
            } else {
                "2025-02-07"
            },
            MarketQuote {
                value: 4.2,
                unit: QuoteUnit::PercentRate,
            },
            overnight,
        ),
    ];
    let pillars = helpers.iter().map(|h| h.pillar).collect();
    let fixed = &helpers[0].terms;
    let nodes = flat_curve(fixed, d("2024-01-15"), 0.02, 0.04);
    let output_key = if overnight {
        &fixed.collateral.discount_curve
    } else {
        fixed.floating.projection_curve()
    };
    let mut curve = nodes.get(output_key).unwrap().recipe().clone();
    curve.key = output_key.clone();
    curve.source = CurveSource::Helpers(helpers.iter().map(|h| h.id.clone()).collect());
    if overnight {
        curve.role = CurveRole::Discount {
            collateral_currency: fixed.currency,
        };
    } else {
        curve.dependencies = vec![fixed.collateral.discount_curve.clone()];
    }
    curve.calibration = if global {
        CalibrationMode::Global(GlobalFitSettings {
            max_iterations: 100,
            objective_tolerance: 1e-10,
            initial_zeros: vec![0.025, 0.04],
            lower_zero: -0.1,
            upper_zero: 0.15,
            quote_scales: vec![0.01, 1.0],
            weights: vec![1.0, 1.0],
        })
    } else {
        CalibrationMode::Piecewise
    };
    SwapCurveRecipe {
        curve,
        pillars,
        helpers,
        overnight_projection: if overnight {
            Some(key("USD.ON.PROJ"))
        } else {
            None
        },
        bootstrap: if global {
            None
        } else {
            Some(SwapBootstrapSettings {
                lower_zero: -0.1,
                upper_zero: 0.15,
                max_iterations: 100,
            })
        },
    }
}
fn run(
    recipes: &[SwapCurveRecipe],
    supplied: &CurveSet,
) -> Result<SwapCurveBuildOutput, SwapCalibrationError> {
    let calendars = calendars(&case("regular-eom"));
    let sets: Vec<Vec<SwapCalendars<'_>>> = recipes
        .iter()
        .map(|r| {
            r.helpers
                .iter()
                .map(|h| bindings(&h.terms, &calendars))
                .collect()
        })
        .collect();
    let inputs: Vec<_> = recipes
        .iter()
        .zip(&sets)
        .map(|(recipe, calendars)| SwapCurveBuildInput { recipe, calendars })
        .collect();
    build_swap_curve_set(
        &inputs,
        supplied,
        &FixingSeries::new(vec![]).unwrap(),
        context(d("2024-01-15")),
        FitNumericalControls::default(),
        publication,
    )
}
fn assert_native_repricing(output: &SwapCurveBuildOutput) {
    assert!(output.price_evaluations > 0 && output.price_evaluations < MAX_SWAP_PRICE_EVALUATIONS);
    for fit in &output.calibrations {
        assert_eq!(fit.curve.recipe(), &fit.recipe.curve);
        for ((quote, helper), price) in fit
            .curve
            .diagnostics()
            .quotes
            .iter()
            .zip(&fit.recipe.helpers)
            .zip(&fit.repriced_helpers)
        {
            assert_eq!(quote.input, helper.quote);
            assert_eq!(quote.tolerance, helper.tolerance);
            assert_eq!(quote.residual, quote.model.value - quote.input.value);
            assert!(quote.residual.abs() <= helper.tolerance);
            let convex_rates::pricing::SwapParQuotes::Available { fixed_annuity, .. } = price.par
            else {
                panic!("expected par");
            };
            let scale = match helper.quote.unit {
                QuoteUnit::DecimalRate => 1.0,
                QuoteUnit::PercentRate => 100.0,
                QuoteUnit::BasisPoints => 10_000.0,
                _ => panic!("rate"),
            };
            assert!(
                price.present_value.value().get().abs()
                    <= fixed_annuity.value().get() * helper.tolerance / scale + 1e-7
            );
        }
    }
}

#[test]
fn dependent_ois_discount_then_index_projection_reprice_piecewise_and_global() {
    for global in [false, true] {
        let discount = recipe(true, global);
        let projection = recipe(false, global);
        let output = run(&[projection.clone(), discount.clone()], &CurveSet::new()).unwrap();
        assert_native_repricing(&output);
        assert_eq!(
            output.build_order,
            vec![key("USD.OIS"), key("USD.ON.PROJ"), key("USD.TERM.PROJ")]
        );
        assert_eq!(
            output
                .curves
                .get(&key("USD.ON.PROJ"))
                .unwrap()
                .recipe()
                .source,
            CurveSource::OvernightProjection {
                discount: key("USD.OIS"),
                index_id: "USD.ON".into()
            }
        );
        let ordered = run(&[discount, projection], &CurveSet::new()).unwrap();
        for (a, b) in output.calibrations.iter().zip(&ordered.calibrations) {
            assert_eq!(a.curve.diagnostics(), b.curve.diagnostics());
        }
        if global {
            for fit in &output.calibrations {
                let SwapSolverDiagnostics::Global(diag) = &fit.solver else {
                    panic!("global")
                };
                assert_eq!(diag.rank, Some(2));
                assert!(diag.evaluations > 0);
            }
        }
    }
}

#[test]
fn original_quote_order_and_units_are_preserved_without_changing_fit() {
    for global in [false, true] {
        let discount = recipe(true, global);
        let baseline = run(std::slice::from_ref(&discount), &CurveSet::new()).unwrap();
        let mut changed = discount.clone();
        changed.helpers.reverse();
        changed.curve.source =
            CurveSource::Helpers(changed.helpers.iter().map(|h| h.id.clone()).collect());
        if let CalibrationMode::Global(settings) = &mut changed.curve.calibration {
            settings.quote_scales.reverse();
            settings.weights.reverse();
        }
        let reverse = run(&[changed], &CurveSet::new()).unwrap();
        assert_native_repricing(&reverse);
        let at = convex_curves::contracts::CurveQuery::DiscountFactor {
            point: convex_curves::contracts::CurvePoint::Date(d("2024-10-01")),
        };
        let a = baseline
            .curves
            .get(&key("USD.OIS"))
            .unwrap()
            .query(&at)
            .unwrap()
            .value;
        let b = reverse
            .curves
            .get(&key("USD.OIS"))
            .unwrap()
            .query(&at)
            .unwrap()
            .value;
        assert!((a - b).abs() < 1e-12);
        assert_eq!(
            reverse.calibrations[0].curve.diagnostics().quotes[0].helper_id,
            "longer"
        );
    }
}

#[test]
fn missing_wrong_version_mappings_and_cycles_fail_without_mutating_supplied() {
    let projection = recipe(false, false);
    let supplied = CurveSet::new();
    assert!(matches!(
        run(std::slice::from_ref(&projection), &supplied),
        Err(SwapCalibrationError::Curve(
            convex_curves::contracts::CurveContractError::MissingDependency(_)
        ))
    ));
    assert!(supplied.is_empty());
    for change in 0..4 {
        let mut discount = recipe(true, false);
        match change {
            0 => discount.overnight_projection = None,
            1 => discount.overnight_projection = Some(key("WRONG")),
            2 => discount.curve.dependencies.push(key("UNUSED")),
            _ => {
                if let FloatingLegTerms::Overnight(t) = &mut discount.helpers[0].terms.floating {
                    t.index_id = IndexId::new("OTHER").unwrap();
                }
            }
        }
        assert!(run(&[discount], &supplied).is_err());
    }
    let mut discount = recipe(true, false);
    discount.curve.key.version = "v2".into();
    for helper in &mut discount.helpers {
        helper.terms.collateral.discount_curve.version = "v2".into();
    }
    assert!(matches!(
        run(&[projection.clone(), discount], &supplied),
        Err(SwapCalibrationError::Curve(
            convex_curves::contracts::CurveContractError::MissingDependency(_)
        ))
    ));
    let mut other = projection.clone();
    other.curve.key = key("USD.SECOND.PROJ");
    for helper in &mut other.helpers {
        if let FloatingLegTerms::Term(t) = &mut helper.terms.floating {
            t.projection_curve = other.curve.key.clone();
        }
        helper.terms.collateral.discount_curve = projection.curve.key.clone();
    }
    other.curve.dependencies = vec![projection.curve.key.clone()];
    let mut first = projection;
    for helper in &mut first.helpers {
        helper.terms.collateral.discount_curve = other.curve.key.clone();
    }
    first.curve.dependencies = vec![other.curve.key.clone()];
    assert!(matches!(
        run(&[first, other], &supplied),
        Err(SwapCalibrationError::Curve(
            convex_curves::contracts::CurveContractError::DependencyCycle(_)
        ))
    ));
}

#[test]
fn full_index_tenor_horizon_and_payment_lag_are_required_pillars() {
    let mut recipe = recipe(false, false);
    recipe.helpers[1] = helper(
        "longer",
        "full-index-tenor-stub",
        "2024-11-14",
        MarketQuote {
            value: 4.2,
            unit: QuoteUnit::PercentRate,
        },
        false,
    );
    recipe.pillars[1] = recipe.helpers[1].pillar;
    let fixed = &recipe.helpers[0].terms;
    let supplied = flat_curve(fixed, d("2024-01-15"), 0.02, 0.04);
    let discount = supplied
        .get(&fixed.collateral.discount_curve)
        .unwrap()
        .as_ref()
        .clone();
    let supplied = CurveSet::new().with_curve(discount).unwrap();
    assert!(run(&[recipe.clone()], &supplied).is_err());
    recipe.helpers[1].pillar = d("2025-01-31");
    recipe.pillars[1] = d("2025-01-31");
    assert_native_repricing(&run(&[recipe], &supplied).unwrap());
    let mut ois = crate::recipe(true, false);
    ois.helpers[0].pillar = d("2024-07-31");
    ois.pillars[0] = d("2024-07-31");
    assert!(run(&[ois], &CurveSet::new()).is_err());
}

#[test]
fn calendar_and_whole_batch_history_fail_before_projection_or_fitting() {
    let mut recipe = recipe(true, false);
    let calendars = calendars(&case("regular-eom"));
    let mut sets: Vec<_> = recipe
        .helpers
        .iter()
        .map(|h| bindings(&h.terms, &calendars))
        .collect();
    sets[0].observation = None;
    let input = SwapCurveBuildInput {
        recipe: &recipe,
        calendars: &sets,
    };
    assert!(build_swap_curve_set(
        &[input],
        &CurveSet::new(),
        &FixingSeries::new(vec![]).unwrap(),
        context(d("2024-01-15")),
        FitNumericalControls::default(),
        publication
    )
    .is_err());
    drop(sets);
    recipe.helpers[0].terms.effective_date = d("2024-01-31"); // Explicitly retain full original contracts.
    let sets: Vec<_> = recipe
        .helpers
        .iter()
        .map(|h| bindings(&h.terms, &calendars))
        .collect();
    let input = SwapCurveBuildInput {
        recipe: &recipe,
        calendars: &sets,
    };
    assert!(matches!(
        build_swap_curve_set(
            &[input],
            &CurveSet::new(),
            &FixingSeries::new(vec![]).unwrap(),
            context(d("2024-03-15")),
            FitNumericalControls::default(),
            publication
        ),
        Err(SwapCalibrationError::Swap(
            convex_rates::SwapError::Cashflow(
                convex_core::cashflows::CashflowError::MissingFixings(_)
            )
        ))
    ));
}

#[test]
fn impossible_brackets_bad_dimensions_zero_weight_rank_and_solver_limits_fail() {
    for change in 0..5 {
        let mut input = recipe(true, true);
        let CalibrationMode::Global(settings) = &mut input.curve.calibration else {
            panic!("global")
        };
        match change {
            0 => settings.weights = vec![0.0, 0.0],
            1 => settings.initial_zeros.pop().map(|_| ()).unwrap(),
            2 => settings.max_iterations = 0,
            3 => settings.weights[1] = 0.0,
            _ => settings.lower_zero = f64::NAN,
        }
        assert!(run(&[input], &CurveSet::new()).is_err());
    }
    let mut input = recipe(true, false);
    input.bootstrap.as_mut().unwrap().lower_zero = 0.10;
    input.bootstrap.as_mut().unwrap().upper_zero = 0.11;
    assert!(matches!(
        run(&[input], &CurveSet::new()),
        Err(SwapCalibrationError::Root(_))
    ));
    let mut input = recipe(true, true);
    if let CalibrationMode::Global(settings) = &mut input.curve.calibration {
        settings.max_iterations = 1;
    }
    assert!(run(&[input], &CurveSet::new()).is_err());
    let mut input = recipe(true, false);
    input.helpers[0].quote.unit = QuoteUnit::PricePer100;
    assert!(run(&[input], &CurveSet::new()).is_err());
}

#[test]
fn publications_are_frozen_once_per_unique_index_date_across_iterations() {
    let recipe = recipe(true, true);
    let calendars = calendars(&case("regular-eom"));
    let sets: Vec<_> = recipe
        .helpers
        .iter()
        .map(|h| bindings(&h.terms, &calendars))
        .collect();
    let input = SwapCurveBuildInput {
        recipe: &recipe,
        calendars: &sets,
    };
    let mut calls = 0;
    let output = build_swap_curve_set(
        &[input],
        &CurveSet::new(),
        &FixingSeries::new(vec![]).unwrap(),
        context(d("2024-01-15")),
        FitNumericalControls::default(),
        |index, at| {
            calls += 1;
            publication(index, at)
        },
    )
    .unwrap();
    assert_eq!(calls, output.publications.len());
    assert_native_repricing(&output);
}
