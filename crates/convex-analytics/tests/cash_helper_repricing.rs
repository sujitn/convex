//! Direct diagnostics reuse original full cash, legal/recovery and quote guards.
#[path = "../../convex/examples/support/cash_fixture.rs"]
mod fixture;
use convex_analytics::cash_calibration::*;
use convex_curves::contracts::*;
use fixture::*;

fn direct_market(f: &Fixture) -> CurveSet {
    let recipes = f
        .recipes
        .iter()
        .map(|r| {
            let mut c = r.curve.clone();
            let parameters = f.row["expected_parameters"][&c.key.id].as_array().unwrap();
            c.calibration = CalibrationMode::Direct;
            c.dependencies.clear();
            c.source = CurveSource::Nodes(
                std::iter::once((c.reference_date, &parameters[0]))
                    .chain(r.pillars.iter().copied().zip(parameters))
                    .map(|(date, value)| CurveNode {
                        pillar: CurvePoint::Date(date),
                        quote: MarketQuote {
                            value: number(value).unwrap(),
                            unit: QuoteUnit::DecimalRate,
                        },
                    })
                    .collect(),
            );
            c
        })
        .collect::<Vec<_>>();
    build_curve_set(&recipes, &f.supplied().unwrap())
        .unwrap()
        .curves
}

#[test]
fn original_independent_cash_repo_quotes_and_full_cash_reprice_without_fitting() {
    for id in ["positive", "negative"] {
        let f = Fixture::load(id).unwrap();
        let market = direct_market(&f);
        for recipes in [f.recipes.clone(), global(&f.recipes)] {
            let resources = f.resources(&recipes);
            for (r, resources) in recipes.iter().zip(&resources) {
                let before = market.get(&r.curve.key).unwrap().clone();
                let out = reprice_cash_helpers(
                    &CashCurveBuildInput {
                        recipe: r,
                        resources,
                    },
                    &market,
                )
                .unwrap();
                assert_eq!(out.recipe, *r);
                assert!(out.within_tolerance);
                assert_eq!(out.price_evaluations, r.helpers.len());
                assert!(std::sync::Arc::ptr_eq(
                    &before,
                    market.get(&r.curve.key).unwrap()
                ));
                let group = match r.curve.role {
                    CurveRole::Government { .. } => Some("government_expected"),
                    CurveRole::IssuerSpread { .. } => Some("basis_expected"),
                    _ => None,
                };
                for (i, (h, q)) in r.helpers.iter().zip(&out.quotes).enumerate() {
                    assert_eq!(q.helper_id, h.id());
                    assert_eq!(q.input, h.quote());
                    assert_eq!(q.model.unit, h.quote().unit);
                    assert_eq!(q.residual, q.model.value - q.input.value);
                    assert!(q.residual.abs() <= h.tolerance());
                    if let Some(group) = group {
                        let CashHelperValuation::Bond { value, .. } = &out.valuations[i] else {
                            panic!("missing full original cash");
                        };
                        compare_price(value, &f.row[group][i], &f.data["tolerances"]).unwrap();
                    } else {
                        let CashHelperValuation::Repo {
                            start,
                            end,
                            repo_discount_ratio,
                            decimal_rate,
                            ..
                        } = &out.valuations[i]
                        else {
                            panic!("missing original loan");
                        };
                        assert!(end > start && *repo_discount_ratio > 0.0);
                        assert_eq!(decimal_rate.is_sign_negative(), id == "negative");
                    }
                }
            }
        }
    }
}

#[test]
fn original_credit_basis_recovery_is_separate_from_promised_cash() {
    let f = CreditFixture::load().unwrap();
    let recipes = vec![f.recipe.clone()];
    let resources = resources(&recipes, &f.calendar);
    let fitted = build(&recipes, &resources, &f.supplied).unwrap();
    let out = reprice_cash_helpers(
        &CashCurveBuildInput {
            recipe: &recipes[0],
            resources: &resources[0],
        },
        &fitted.curves,
    )
    .unwrap();
    assert!(out.within_tolerance);
    assert_eq!(out.price_evaluations, 2);
    for (actual, expected) in out
        .valuations
        .iter()
        .zip(f.data["credit_case"]["expected"].as_array().unwrap())
    {
        let CashHelperValuation::Bond { value, basis, .. } = actual else {
            panic!("missing full credit cash");
        };
        compare_price(value, expected, &f.data["tolerances"]).unwrap();
        let basis = basis.as_ref().unwrap();
        assert!(basis.recovery_pv > 0.0);
        assert_eq!(
            basis.price.present_value,
            basis.promised_pv + basis.recovery_pv
        );
        compare(
            basis.recovery_pv,
            number(&expected["recovery_pv"]).unwrap(),
            1e-6,
            1e-11,
        )
        .unwrap();
    }
}

#[test]
fn off_market_diagnostics_preserve_binding_contract_and_builder_failures() {
    let f = Fixture::load("positive").unwrap();
    let market = direct_market(&f);
    let mut r = f.recipes[2].clone();
    let CashCalibrationHelper::Bond { input: h } = &mut r.helpers[0] else {
        panic!("original government helper");
    };
    h.quote.value += 1.0;
    let recipes = vec![r.clone()];
    let resources = f.resources(&recipes);
    let input = CashCurveBuildInput {
        recipe: &r,
        resources: &resources[0],
    };
    let out = reprice_cash_helpers(&input, &market).unwrap();
    assert!(!out.within_tolerance);
    assert!(out.quotes[0].residual < -0.9);
    assert!(build_cash_curve_set(&[input], &market, FitNumericalControls::default()).is_err());
    assert!(reprice_cash_helpers(
        &CashCurveBuildInput {
            recipe: &r,
            resources: &[]
        },
        &market
    )
    .is_err());
    assert!(matches!(
        reprice_cash_helpers(
            &CashCurveBuildInput {
                recipe: &r,
                resources: &resources[0]
            },
            &CurveSet::new()
        ),
        Err(CashCalibrationError::Curve(
            CurveContractError::MissingDependency(_)
        ))
    ));
    // A failure in the last complete original contract returns no partial output.
    let mut bad = r.clone();
    let CashCalibrationHelper::Bond { input: h } = bad.helpers.last_mut().unwrap() else {
        panic!("original final government helper");
    };
    h.settlement = h.settlement.checked_add_days(1).unwrap();
    let recipes = vec![bad.clone()];
    let resources = f.resources(&recipes);
    assert!(reprice_cash_helpers(
        &CashCurveBuildInput {
            recipe: &bad,
            resources: &resources[0]
        },
        &market
    )
    .is_err());
    // Exact supplied representation/method metadata must match the original recipe.
    bad = r.clone();
    bad.curve.extrapolation = convex_curves::ExtrapolationMethod::Flat;
    let recipes = vec![bad.clone()];
    let resources = f.resources(&recipes);
    assert!(reprice_cash_helpers(
        &CashCurveBuildInput {
            recipe: &bad,
            resources: &resources[0]
        },
        &market
    )
    .is_err());
}
