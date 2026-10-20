//! Direct embedding retains original owners and rejects foreign/incomplete paths.
use convex::{curve_contracts as c, quote_risk as q, workflow as w};
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    cashflows::PaymentDateEntitlement,
    contracts::ExactDecimal,
    fixings::FixingSeries,
};
use serde_json::Value;

#[test]
fn complete_native_marking_and_private_path_coverage() {
    let document: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/mixed-scenario-api-01.json"
    ))
    .unwrap();
    let request = &document["cases"][0]["request"];
    let input: convex::library::MixedBookScenarioInput =
        serde_json::from_value(request["operation"]["action"]["input"].clone()).unwrap();
    let recipes: Vec<c::CurveRecipe> =
        serde_json::from_value(request["inputs"]["curves"].clone()).unwrap();
    let curves = c::build_curve_set(&recipes, &c::CurveSet::new())
        .unwrap()
        .curves;
    let history = FixingSeries::default();
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
        |_, _| panic!("fixed supplied curves have no publication requirement"),
    )
    .unwrap();
    let product = &request["operation"]["action"]["input"]["holdings"][0]["product"]["product"];
    let terms: convex::bond_contracts::BondTerms =
        serde_json::from_value(product["bond"]["terms"].clone()).unwrap();
    let calendar = DynamicCalendar::from_dates(
        "CASH.SYNTHETIC.v1",
        WeekendType::SaturdaySunday,
        serde_json::from_value::<Vec<convex::Date>>(
            request["inputs"]["calendars"][0]["holidays"].clone(),
        )
        .unwrap(),
    );
    let calendars = convex::bond_contracts::BondCalendars {
        payment: &calendar,
        reset: &calendar,
        observation: &calendar,
        exercise: &calendar,
    };
    let contract = terms.build_with_calendars(&calendars).unwrap();
    let cash = w::PreparedBondCash::fixed(
        &contract,
        serde_json::from_value(product["context"]["settlement"].clone()).unwrap(),
        PaymentDateEntitlement::Exclude,
    )
    .unwrap();
    let publications = w::PublicationSchedule::new(Vec::new()).unwrap();
    let native = w::ValuationInputs::Bond {
        cash: &cash,
        method: w::BondPriceMethod::DiscountSpread {
            curve: serde_json::from_value(product["method"]["curve"].clone()).unwrap(),
            role: w::BondDiscountRole::Collateral,
            decimal_spread: product["method"]["decimal_spread"].as_f64().unwrap(),
        },
        market: w::SuppliedMarket {
            curves: &curves,
            fixings: &history,
            publications: &publications,
        },
    };
    let mut book = input.book;
    book.positions.truncate(1);
    book.moves.truncate(2);
    book.positions[0].pending_quantity =
        ExactDecimal::new(-book.positions[0].current.quantity.decimal());
    let path = w::prepare_book_scenarios(&graph, &book).unwrap();
    let foreign = w::prepare_book_scenarios(&graph, &book).unwrap();
    assert!(
        w::revalue_book_position(&path, 1, &w::BookScenarioProduct::Standard(&native)).is_err()
    );
    assert!(w::aggregate_book_scenarios(&path, Vec::new()).is_err());
    let other =
        w::revalue_book_position(&foreign, 0, &w::BookScenarioProduct::Standard(&native)).unwrap();
    assert!(w::aggregate_book_scenarios(&path, vec![other]).is_err());
    let trade =
        w::revalue_book_position(&path, 0, &w::BookScenarioProduct::Standard(&native)).unwrap();
    assert_eq!(
        trade.values()[0].reference_amount,
        native.monetary_value().unwrap()
    );
    assert_eq!(trade.input().identity.id, book.positions[0].identity.id);
    for value in trade.values() {
        assert_eq!(value.local.projected.value().get(), 0.0);
        assert_eq!(
            value.local.current.value().get(),
            -value.local.pending.value().get()
        );
    }
    assert!(trade.ctd_transitions().is_empty());
    let report = w::aggregate_book_scenarios(&path, vec![trade]).unwrap();
    assert_eq!(report.product_evaluations, 5);
    assert_eq!(report.graph_evaluations, 5);
    assert_eq!(report.local_totals.len(), 1);
    assert_eq!(report.reporting_totals.len(), 1);
    assert_eq!(
        report.local_totals[0]
            .attribution
            .total_change
            .projected
            .value()
            .get(),
        0.0
    );
    // An unused raw coordinate is a failed declaration, never a supplied zero.
    book.moves.push(
        serde_json::from_value(request["operation"]["action"]["input"]["book"]["moves"][9].clone())
            .unwrap(),
    );
    let raw = w::prepare_book_scenarios(&graph, &book).unwrap();
    let trade =
        w::revalue_book_position(&raw, 0, &w::BookScenarioProduct::Standard(&native)).unwrap();
    assert!(w::aggregate_book_scenarios(&raw, vec![trade]).is_err());
}

#[test]
fn optional_scenarios_bind_actual_materialized_values_and_original_model() {
    use convex::optional_bond as o;
    use convex_core::calendars::WeekendCalendar;
    let cases: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/optional-graph-hedge-resolved.json"
    ))
    .unwrap();
    let request = &cases["cases"][0]["request"];
    let input = &request["operation"]["product"]["input"];
    let terms: convex::bond_contracts::BondTerms =
        serde_json::from_value(input["bond"]["terms"].clone()).unwrap();
    let bond = terms.build().unwrap();
    let mut recipe: c::CurveRecipe =
        serde_json::from_value(request["inputs"]["curves"][0].clone()).unwrap();
    let c::CurveSource::Nodes(nodes) = recipe.source.clone() else {
        panic!("original full grid required")
    };
    recipe.source = c::CurveSource::Helpers(vec!["original-full-helper".into()]);
    recipe.calibration = c::CalibrationMode::Piecewise;
    let fit = |bump| {
        let mut nodes = nodes.clone();
        for n in &mut nodes {
            n.quote.value += bump;
            let c::CurvePoint::Tenor(time) = n.pillar else {
                panic!("original full tenor grid required")
            };
            n.pillar = c::CurvePoint::Date(
                recipe
                    .reference_date
                    .checked_add_days((time * 365.0) as i64)
                    .unwrap(),
            );
        }
        let quote = c::MarketQuote {
            unit: c::QuoteUnit::DecimalRate,
            value: (nodes[1].quote.value * 50.0).exp_m1() / 50.0,
        };
        let repricing = c::QuoteRepricing {
            helper_id: "original-full-helper".into(),
            pillar: recipe.reference_date.checked_add_days(50 * 365).unwrap(),
            input: quote,
            model: quote,
            residual: 0.0,
            tolerance: 1e-12,
        };
        c::build_fitted_rate_curve(&recipe, &nodes, vec![repricing], 0, &c::CurveSet::new())
            .unwrap()
    };
    let baseline = c::CurveSet::new().with_curve(fit(0.0)).unwrap();
    let foreign = c::CurveSet::new().with_curve(fit(0.001)).unwrap();
    assert_eq!(
        baseline.get(&recipe.key).unwrap().recipe(),
        foreign.get(&recipe.key).unwrap().recipe()
    );
    let model = o::OptionalBondModel::assumed(
        serde_json::from_value(input["model"]["key"].clone()).unwrap(),
        serde_json::from_value(input["model"]["parameters"].clone()).unwrap(),
        input["model"]["reason"].as_str().unwrap().into(),
    )
    .unwrap();
    let prepared = o::prepare_fixed(
        &bond,
        serde_json::from_value(input["context"].clone()).unwrap(),
        o::OptionalBondInputs {
            discount: baseline.get(&recipe.key).unwrap(),
            projection: None,
            model: &model,
            exercise_calendar: &WeekendCalendar,
        },
        serde_json::from_value(input["settings"].clone()).unwrap(),
    )
    .unwrap();
    q::admit_optional_curve_set(&prepared, &baseline).unwrap();
    prepared.admit_scenario_baseline(&baseline).unwrap();
    assert!(prepared.admit_scenario_baseline(&foreign).is_err());
    q::admit_optional_scenario_curves(&prepared, &baseline, &baseline, &[]).unwrap();
    assert!(matches!(
        q::admit_optional_scenario_curves(&prepared, &baseline, &foreign, &[]),
        Err(q::QuoteRiskError::Optional(
            o::OptionalBondError::Unsupported { .. }
        ))
    ));
    let oas = request["operation"]["product"]["oas_decimal"]
        .as_f64()
        .unwrap();
    assert_eq!(
        serde_json::to_value(prepared.price(oas).unwrap()).unwrap(),
        serde_json::to_value(
            prepared
                .price_combined_scenario(oas, o::CurveShift::BASE, 0.0, 0.0)
                .unwrap()
        )
        .unwrap()
    );
}
