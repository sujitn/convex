//! Direct embedding binds native states, original quantities and private coverage.
use convex::{curve_contracts as c, lifecycle as l, workflow as w};
use convex_core::{
    calendars::WeekendCalendar, cashflows::PaymentDateEntitlement, fixings::FixingSeries,
};
use serde_json::{json, Value};

fn source() -> Value {
    let all: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/causal-pnl-cash-contracts.json"
    ))
    .unwrap();
    all["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "causal.time.stepped_delayed_partial.plain_contract")
        .unwrap()["request"]
        .clone()
}

#[test]
fn exact_native_replay_and_private_mark_coverage() {
    let request = source();
    let book: convex::library::MixedBookPnlInput =
        serde_json::from_value(request["operation"]["input"].clone()).unwrap();
    let mut points = vec![book.before.point.clone()];
    points.extend(book.steps.iter().map(|s| s.after.point.clone()));
    let input = w::CausalPnlInput {
        causes: book.steps.iter().map(|s| s.cause).collect(),
        points,
        standalone: book
            .standalone
            .iter()
            .map(|s| match s {
                convex::library::CausalPnlCounterfactual::Unavailable { reason } => {
                    w::PnlStandalone::Unavailable {
                        reason: reason.clone(),
                    }
                }
                _ => panic!("original complete time control has no independent counterfactual"),
            })
            .collect(),
        reporting_currency: book.reporting_currency,
        max_points: book.max_points,
        max_product_evaluations: book.max_product_evaluations,
    };
    let path = w::prepare_causal_pnl(&input).unwrap();
    let foreign = w::prepare_causal_pnl(&input).unwrap();
    let curves = c::build_curve_set(&book.markets[0].curves, &c::CurveSet::new())
        .unwrap()
        .curves;
    let first = &book.holdings[book.before.holdings[0]];
    let convex::library::BookScenarioProductInput::Product { product } = &first.product else {
        unreachable!()
    };
    let convex::library::RiskProductInput::Bond { bond, .. } = product.as_ref() else {
        unreachable!()
    };
    let calendars = convex::bond_contracts::BondCalendars {
        payment: &WeekendCalendar,
        reset: &WeekendCalendar,
        observation: &WeekendCalendar,
        exercise: &WeekendCalendar,
    };
    let contract = bond.terms.clone().build_with_calendars(&calendars).unwrap();
    let mut state = l::LifecycleState::new(
        first.id.clone(),
        first.lifecycle.version,
        l::LifecycleInstrument::Bond {
            contract: Box::new(contract.clone()),
            credit: None,
        },
        first.lifecycle.position,
        first.lifecycle.known_at,
        first.lifecycle.history.clone(),
    )
    .unwrap();
    for step in &first.lifecycle.events {
        state = l::transition(&state, &step.event, &l::LifecycleInputs::default())
            .unwrap()
            .next;
    }
    let publications = w::PublicationSchedule::new(Vec::new()).unwrap();
    let history = FixingSeries::default();
    let mut rows = Vec::new();
    let bindings = std::iter::once(&book.before)
        .chain(book.steps.iter().map(|s| &s.after))
        .collect::<Vec<_>>();
    for (i, binding) in bindings.iter().enumerate() {
        let holding = &book.holdings[binding.holdings[0]];
        if i > 0 {
            let previous = &book.holdings[bindings[i - 1].holdings[0]];
            for step in &holding.lifecycle.events[previous.lifecycle.events.len()..] {
                state = l::transition(&state, &step.event, &l::LifecycleInputs::default())
                    .unwrap()
                    .next;
            }
        }
        let cash = w::PreparedBondCash::fixed(
            &contract,
            binding.point.valuation_date,
            PaymentDateEntitlement::Exclude,
        )
        .unwrap();
        let native = w::ValuationInputs::Bond {
            cash: &cash,
            method: w::BondPriceMethod::DiscountSpread {
                curve: binding.point.positions[0].cash_discount.clone(),
                role: w::BondDiscountRole::Collateral,
                decimal_spread: 0.0,
            },
            market: w::SuppliedMarket {
                curves: &curves,
                fixings: &history,
                publications: &publications,
            },
        };
        let product = w::CausalPnlProduct::Standard(&native);
        let discount = curves
            .get(&binding.point.positions[0].cash_discount)
            .unwrap();
        if i == 0 {
            assert!(w::mark_causal_pnl_position(&path, 0, 1, &state, &product, discount).is_err());
            assert!(w::aggregate_causal_pnl(&path, Vec::new()).is_err());
            let other =
                w::mark_causal_pnl_position(&foreign, 0, 0, &state, &product, discount).unwrap();
            assert!(w::aggregate_causal_pnl(&path, vec![vec![other]]).is_err());
        }
        rows.push(w::mark_causal_pnl_position(&path, i, 0, &state, &product, discount).unwrap());
    }
    assert_eq!(
        rows[0].recorded_cash.value().decimal(),
        rust_decimal::Decimal::ZERO
    );
    assert_eq!(
        rows[3].recorded_cash.value().decimal(),
        rust_decimal::Decimal::from(-240)
    );
    let report = w::aggregate_causal_pnl(&path, vec![rows]).unwrap();
    assert_eq!(report.product_evaluations, 4);
    assert_eq!(report.trades.len(), 1);
    assert!(report.trades[0].local.interaction.is_none());
    assert!(
        report.trades[0]
            .local
            .residual
            .current_model_pnl
            .value()
            .get()
            .abs()
            <= 1e-7
    );
    assert!(report.trades[0].ctd_transitions.iter().all(Option::is_none));
}

#[test]
fn typed_native_request_enforces_bound_sources_and_exact_units() {
    let original = source();
    let native: convex::library::LibraryRequest = serde_json::from_value(original.clone()).unwrap();
    let result = convex::library::execute(&native).unwrap();
    let value = json!({"ok":"true","result":result});
    assert_eq!(value["result"]["data"]["report"]["product_evaluations"], 4);
    for mutation in ["work", "binding", "quantity"] {
        let mut input = original.clone();
        let d = &mut input["operation"]["input"];
        match mutation {
            "work" => d["max_points"] = json!(1),
            "binding" => d["before"]["market"] = json!(999),
            "quantity" => {
                let index = d["before"]["holdings"][0].as_u64().unwrap() as usize;
                d["holdings"][index]["lifecycle"]["position"]["value"] = json!("-900");
            }
            _ => unreachable!(),
        }
        let request: convex::library::LibraryRequest = serde_json::from_value(input).unwrap();
        assert!(convex::library::execute(&request).is_err(), "{mutation}");
    }
}
