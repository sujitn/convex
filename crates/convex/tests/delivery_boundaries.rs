//! Direct embedding preserves complete source recipes and original cash/carry owners.
#[path = "../../../tests/fixtures/packed.rs"]
mod packed;
use convex::{curve_contracts as c, futures as f, quote_risk as q};
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    fixings::FixingSeries,
};
use serde_json::Value;

// Two independently specified one-year deposit fits can share every recipe
// field while retaining different actual calibrated values and observations.
fn deposit_fit(template: &c::CurveRecipe, quote: f64) -> c::BuiltCurve {
    let pillar = convex::Date::from_ymd(2027, 10, 7).unwrap();
    let mut recipe = template.clone();
    recipe.source = c::CurveSource::Helpers(vec!["original-deposit".into()]);
    recipe.calibration = c::CalibrationMode::Piecewise;
    let zero = (1.0 + quote).ln();
    let nodes: Vec<_> = [recipe.reference_date, pillar]
        .into_iter()
        .map(|date| c::CurveNode {
            pillar: c::CurvePoint::Date(date),
            quote: c::MarketQuote {
                unit: c::QuoteUnit::DecimalRate,
                value: zero,
            },
        })
        .collect();
    let native = c::MarketQuote {
        unit: c::QuoteUnit::DecimalRate,
        value: quote,
    };
    c::build_fitted_rate_curve(
        &recipe,
        &nodes,
        vec![c::QuoteRepricing {
            helper_id: "original-deposit".into(),
            pillar,
            input: native,
            model: native,
            residual: 0.0,
            tolerance: 1e-12,
        }],
        0,
        &c::CurveSet::new(),
    )
    .unwrap()
}

#[test]
fn full_native_basket_owners_and_exact_original_recipe_admission() {
    let document: Value = serde_json::from_str(packed::text(
        "tests/fixtures/library/delivery-hedge-decimal.json",
    ))
    .unwrap();
    let history = FixingSeries::default();
    let mut checked = 0;
    for case in document["cases"].as_array().unwrap().iter().filter(|c| {
        c["expected_error"].is_null()
            && c["request"]["operation"]["action"]["kind"] == "product_risk"
    }) {
        let request = &case["request"];
        let action = &request["operation"]["action"];
        let product = &action["product"];
        let input: convex::library::BondFutureInput =
            serde_json::from_value(product["input"].clone()).unwrap();
        let recipe: c::CurveRecipe =
            serde_json::from_value(request["inputs"]["curves"][0].clone()).unwrap();
        let curves = c::build_curve_set(std::slice::from_ref(&recipe), &c::CurveSet::new())
            .unwrap()
            .curves;
        let discount = curves.get(&recipe.key).unwrap();
        let stamp: convex_core::futures::ReferenceStamp =
            serde_json::from_value(product["discount"]["stamp"].clone()).unwrap();
        let calendar = DynamicCalendar::from_dates(
            "original-embedding-control",
            WeekendType::SaturdaySunday,
            vec![
                convex::Date::from_ymd(2026, 11, 9).unwrap(),
                convex::Date::from_ymd(2026, 12, 1).unwrap(),
            ],
        );
        let calendars = convex::bond_contracts::BondCalendars {
            payment: &calendar,
            reset: &calendar,
            observation: &calendar,
            exercise: &calendar,
        };
        let bonds: Vec<_> = input
            .members
            .iter()
            .map(|m| {
                m.bond
                    .terms
                    .clone()
                    .build_with_calendars(&calendars)
                    .unwrap()
            })
            .collect();
        let deliverables: Vec<_> = input
            .members
            .iter()
            .zip(&bonds)
            .map(|(m, b)| f::BondFutureDeliverable {
                security: f::FuturesBond {
                    instrument: &m.instrument,
                    stamp: &m.stamp,
                    bond: b,
                },
                clean_price_per100: m.clean_price_per100,
                quote_stamp: &m.quote_stamp,
                financing: &m.financing,
                own_cash: m.own_cash.as_ref(),
                coupon_carry: &m.coupon_carry,
            })
            .collect();
        let native = f::BondFuturesInputs {
            contract: &input.contract,
            basket: &input.basket,
            delivery_calendar: f::FuturesCalendar {
                stamp: &input.calendar_stamp,
                calendar: &calendar,
            },
            settlement: input.settlement,
            delivery: input.delivery,
            market: &input.market,
            deliverables: &deliverables,
            selection: input.selection,
            context: input.context,
        };
        let graph = q::prepare_quote_risk_graph(
            q::QuoteRiskGraphInput {
                rates: &[],
                credit: &[],
                cash: &[],
                supplied: &curves,
                history: &history,
                rate_context: None,
                controls: serde_json::from_value(request["operation"]["input"]["controls"].clone())
                    .unwrap(),
            },
            |_, _| panic!("fixed cash has no fixing publication request"),
        )
        .unwrap();
        let original = f::price_bond_future(&native).unwrap();
        let risk_request = f::BondFuturesGraphRequest {
            key: serde_json::from_value(action["key"].clone()).unwrap(),
            position: serde_json::from_value(action["position"].clone()).unwrap(),
            snapshot_id: serde_json::from_value(action["snapshot_id"].clone()).unwrap(),
            settings: serde_json::from_value(action["settings"].clone()).unwrap(),
            policy: serde_json::from_value(product["policy"].clone()).unwrap(),
        };
        let source = f::FuturesCurve {
            curve: discount,
            stamp: &stamp,
        };
        let actual = f::bond_futures_graph_risk(&native, source, &graph, &risk_request).unwrap();
        assert_eq!(
            actual.evaluations[0].valuation.alternatives,
            original.alternatives
        );
        assert_eq!(actual.evaluations[0].valuation.selected, original.selected);
        assert_eq!(
            actual.evaluations[0].valuation.fair_deliverable,
            original.fair_deliverable
        );
        let expected: Value =
            serde_json::from_str(&convex::library::execute_json(&request.to_string())).unwrap();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            expected["result"]["data"]
        );
        let before = serde_json::to_value(&original).unwrap();
        assert_eq!(
            before,
            serde_json::to_value(f::price_bond_future(&native).unwrap()).unwrap()
        );
        // Repricing cannot legitimize a different complete recipe at the same ID/revision.
        let mut changed = recipe.clone();
        if let c::CurveSource::Nodes(nodes) = &mut changed.source {
            nodes[1].quote.value += 0.001;
        }
        let other = c::build_curve_set(&[changed], &c::CurveSet::new())
            .unwrap()
            .curves;
        assert!(f::admit_bond_futures_graph(&native, source, &other).is_err());
        if checked == 0 {
            let fitted = c::CurveSet::new()
                .with_curve(deposit_fit(&recipe, 0.035))
                .unwrap();
            let original = fitted.get(&recipe.key).unwrap();
            let foreign = deposit_fit(&recipe, 0.04);
            assert_eq!(original.recipe(), foreign.recipe());
            assert_ne!(
                original.interpolation_grid().unwrap(),
                foreign.interpolation_grid().unwrap()
            );
            let original_source = f::FuturesCurve {
                curve: original,
                stamp: &stamp,
            };
            assert!(f::admit_bond_futures_graph(&native, original_source, &fitted).is_ok());
            let foreign_source = f::FuturesCurve {
                curve: &foreign,
                stamp: &stamp,
            };
            assert!(f::admit_bond_futures_graph(&native, foreign_source, &fitted).is_err());
        }
        if action["key"]["coordinate"]["kind"] == "zero"
            && action["key"]["coordinate"]["node"].is_null()
        {
            for (h, up, down, value, frozen) in [
                (
                    risk_request.settings.bump,
                    1,
                    2,
                    actual.sensitivity.value,
                    actual.frozen_deliverable_sensitivity,
                ),
                (
                    risk_request.settings.bump / 2.,
                    3,
                    4,
                    actual.sensitivity.half_value,
                    actual.frozen_deliverable_half_sensitivity,
                ),
            ] {
                let original = f::bond_futures_risk(
                    &native,
                    f::BondFuturesRiskAxis::DiscountZeros { discount: source },
                    action["position"]["quantity"]
                        .as_str()
                        .unwrap()
                        .parse()
                        .unwrap(),
                    convex::contracts::ExactDecimal::parse(&h.to_string()).unwrap(),
                )
                .unwrap();
                assert!(
                    (original
                        .reselected_sensitivity
                        .value()
                        .decimal()
                        .to_string()
                        .parse::<f64>()
                        .unwrap()
                        - value)
                        .abs()
                        < 1e-5
                );
                assert!(
                    (original
                        .frozen_deliverable_sensitivity
                        .value()
                        .decimal()
                        .to_string()
                        .parse::<f64>()
                        .unwrap()
                        - frozen)
                        .abs()
                        < 1e-5
                );
                assert_eq!(
                    original.up.fair_deliverable,
                    actual.evaluations[up].valuation.fair_deliverable
                );
                assert_eq!(
                    original.down.fair_deliverable,
                    actual.evaluations[down].valuation.fair_deliverable
                );
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 14);
}
