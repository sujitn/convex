//! Direct native embedding checks complete original baseline and rate guards.
use convex::{curve_contracts as c, optional_bond as o, quote_risk as q};
use convex_core::{calendars::WeekendCalendar, fixings::FixingSeries};
use serde_json::Value;

#[test]
fn native_optional_graph_rejects_different_full_recipe_and_unresolved_half_move() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/optional-graph-hedge-resolved.json"
    ))
    .unwrap();
    let request = &cases["cases"][0]["request"];
    let input = &request["operation"]["product"]["input"];
    let terms: convex::bond_contracts::BondTerms =
        serde_json::from_value(input["bond"]["terms"].clone()).unwrap();
    let bond = terms.build().unwrap();
    let recipe: c::CurveRecipe =
        serde_json::from_value(request["inputs"]["curves"][0].clone()).unwrap();
    let original = c::build_curve_set(std::slice::from_ref(&recipe), &c::CurveSet::new())
        .unwrap()
        .curves;
    let discount = original.get(&recipe.key).unwrap();
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
            discount: discount.as_ref(),
            projection: None,
            model: &model,
            exercise_calendar: &WeekendCalendar,
        },
        serde_json::from_value(input["settings"].clone()).unwrap(),
    )
    .unwrap();
    let history = FixingSeries::default();
    let controls =
        serde_json::from_value(request["operation"]["graph"]["controls"].clone()).unwrap();
    let graph = |supplied| {
        q::prepare_quote_risk_graph(
            q::QuoteRiskGraphInput {
                rates: &[],
                credit: &[],
                cash: &[],
                supplied,
                history: &history,
                rate_context: None,
                controls,
            },
            |_, _| panic!("fixed cash has no fixing publication request"),
        )
    };
    let baseline = graph(&original).unwrap();
    q::admit_optional_graph(&prepared, &baseline).unwrap();
    // Same ID/version and shape is insufficient: original actual values matter.
    let mut changed = recipe.clone();
    if let c::CurveSource::Nodes(nodes) = &mut changed.source {
        nodes[1].quote.value += 0.001;
    }
    let other = c::build_curve_set(&[changed], &c::CurveSet::new())
        .unwrap()
        .curves;
    assert!(q::admit_optional_graph(&prepared, &graph(&other).unwrap()).is_err());
    let policies: q::OptionalRiskPolicies =
        serde_json::from_value(request["operation"]["product"]["policies"].clone()).unwrap();
    let factor = &request["operation"]["input"]["factors"][0];
    let mut settings: q::QuoteRiskSettings =
        serde_json::from_value(factor["settings"].clone()).unwrap();
    settings.bump = 0.004;
    // The full step is resolved; h/2 must independently fail the original floor.
    prepared
        .risk_resolution(settings.bump, policies.rates)
        .unwrap();
    assert!(matches!(
        q::optional_factor_sensitivity(
            &prepared,
            serde_json::from_value(factor["key"].clone()).unwrap(),
            serde_json::from_value(request["operation"]["input"]["reference_position"].clone())
                .unwrap(),
            serde_json::from_value(request["operation"]["input"]["snapshot_id"].clone()).unwrap(),
            settings,
            request["operation"]["product"]["oas_decimal"]
                .as_f64()
                .unwrap(),
            policies
        ),
        Err(q::QuoteRiskError::Optional(
            o::OptionalBondError::RiskResolution { .. }
        ))
    ));
}
