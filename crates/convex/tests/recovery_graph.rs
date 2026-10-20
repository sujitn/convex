//! Actual native fits retain every full helper under a recovery scenario.
use convex::{
    cash_calibration as cash, credit, credit_calibration as cds, curve_contracts as curves,
    quote_risk as risk,
};
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    fixings::FixingSeries,
    Date,
};
use serde_json::Value;

#[test]
fn all_original_credit_and_cash_helpers_bind_the_actual_scenario_assumption() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/recovery-hedge-boundaries.json"
    ))
    .unwrap();
    let request = &corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "recovery_hedge.rebuilt.multiple_helpers.cash")
        .unwrap()["request"];
    let input: convex::library::CalibrationInput =
        serde_json::from_value(request["operation"]["graph"].clone()).unwrap();
    let recipes: Vec<curves::CurveRecipe> =
        serde_json::from_value(request["inputs"]["curves"].clone()).unwrap();
    let supplied = curves::build_curve_set(&recipes, &curves::CurveSet::new())
        .unwrap()
        .curves;
    let calendar_inputs = request["inputs"]["calendars"].as_array().unwrap();
    let calendars: Vec<_> = calendar_inputs
        .iter()
        .map(|v| {
            let holidays: Vec<Date> = serde_json::from_value(v["holidays"].clone()).unwrap();
            DynamicCalendar::from_dates(
                v["pin"]["id"].as_str().unwrap(),
                WeekendType::SaturdaySunday,
                holidays,
            )
        })
        .collect();
    let calendar = |pin: &Value| {
        &calendars[calendar_inputs
            .iter()
            .position(|c| c["pin"] == *pin)
            .unwrap()]
    };
    let credit_recipe = &input.credit[0];
    let credit_calendars: Vec<_> = credit_recipe
        .helpers
        .iter()
        .map(|h| credit::CdsCalendars {
            premium: credit::CalendarBinding {
                pin: &h.terms.premium_calendar,
                calendar: calendar(&serde_json::to_value(&h.terms.premium_calendar).unwrap()),
            },
            settlement: credit::CalendarBinding {
                pin: &h.terms.settlement_calendar,
                calendar: calendar(&serde_json::to_value(&h.terms.settlement_calendar).unwrap()),
            },
        })
        .collect();
    let cash_recipe = &input.cash[0].recipe;
    let cash::CashCalibrationHelper::Bond { input: h } = &cash_recipe.helpers[0] else {
        panic!("full bond helper required")
    };
    let pins = &request["operation"]["graph"]["cash"][0]["calendars"][0]["pins"];
    let cash_resources = [cash::CashHelperResources::Bond {
        ids: &h.terms.calendars,
        calendars: convex::bond_contracts::BondCalendars {
            payment: calendar(&pins["payment"]),
            reset: calendar(&pins["reset"]),
            observation: calendar(&pins["observation"]),
            exercise: calendar(&pins["exercise"]),
        },
    }];
    let credit_inputs = [cds::CdsCurveBuildInput {
        recipe: credit_recipe,
        calendars: &credit_calendars,
    }];
    let cash_inputs = [cash::CashCurveBuildInput {
        recipe: cash_recipe,
        resources: &cash_resources,
    }];
    let history = FixingSeries::new(vec![]).unwrap();
    let graph = risk::prepare_quote_risk_graph(
        risk::QuoteRiskGraphInput {
            rates: &[],
            credit: &credit_inputs,
            cash: &cash_inputs,
            supplied: &supplied,
            history: &history,
            rate_context: None,
            controls: input.controls,
        },
        |_, _| panic!("these complete helpers require no fixing publication"),
    )
    .unwrap();
    let factor: convex::workflow::GraphHedgeFactor =
        serde_json::from_value(request["operation"]["input"]["hedge"]["factors"][0].clone())
            .unwrap();
    let original = serde_json::to_value(credit_recipe).unwrap();
    for delta in [0.0, 0.0001, -0.0001, 0.00005, -0.00005] {
        let scenario = risk::evaluate_quote_graph(&graph, &factor.key, delta).unwrap();
        assert_eq!(scenario.calibrations.len(), 2);
        assert_eq!(scenario.repricing.len(), 3);
        assert!(scenario
            .repricing
            .iter()
            .all(|q| q.residual.abs() <= q.tolerance));
        let actual = 0.4 + delta;
        assert!(
            matches!(scenario.curves.get(&credit_recipe.curve.key).unwrap().recipe().role,curves::CurveRole::Credit{recovery,..} if recovery==actual)
        );
        for calibration in &scenario.calibrations {
            match calibration {
                risk::QuoteGraphCalibration::Credit(fit) => {
                    let mut expected = credit_recipe.clone();
                    if let curves::CurveRole::Credit { recovery, .. } = &mut expected.curve.role {
                        *recovery = actual;
                    }
                    for helper in &mut expected.helpers {
                        helper.terms.reference.recovery_fraction =
                            convex_core::contracts::FiniteF64::new(actual).unwrap();
                    }
                    assert_eq!(fit.recipe, expected);
                    assert_eq!(fit.recipe.helpers.len(), 2);
                }
                risk::QuoteGraphCalibration::Cash(fit) => {
                    let mut expected = cash_recipe.clone();
                    if let Some(cash::BondBasisModel::CreditParRecovery { recovery, .. }) =
                        &mut expected.basis_model
                    {
                        *recovery = actual;
                    }
                    assert_eq!(fit.recipe, expected);
                }
                _ => panic!("Unexpected fit family"),
            }
        }
        assert_eq!(serde_json::to_value(credit_recipe).unwrap(), original);
    }
    assert!(
        matches!(graph.baseline().curves.get(&credit_recipe.curve.key).unwrap().recipe().role,curves::CurveRole::Credit{recovery,..} if recovery==0.4)
    );
}
