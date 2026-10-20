//! Actual native legal cash transfer admission and conserved receipt ownership.
#[path = "../../../tests/fixtures/packed.rs"]
mod packed;
use convex_analytics::{lifecycle::*, workflow::*};
use convex_bonds::contracts::{BondCalendars, BondContract, BondTerms};
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    contracts::*,
    Date,
};
use serde_json::{json, Value};

fn calendar(book: &Value) -> DynamicCalendar {
    let source = book["resources"]["calendars"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["pin"]["id"] == "CASH.SYNTHETIC.v1")
        .unwrap();
    assert_eq!(source["weekend_days"], json!([5, 6]));
    let mut c = DynamicCalendar::new("CASH.SYNTHETIC.v1:1", WeekendType::SaturdaySunday);
    c.add_holidays(
        source["holidays"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| Date::parse(d.as_str().unwrap()).unwrap()),
    );
    c
}
fn bond(terms: &Value, c: &DynamicCalendar) -> BondContract {
    serde_json::from_value::<BondTerms>(terms.clone())
        .unwrap()
        .build_with_calendars(&BondCalendars {
            payment: c,
            reset: c,
            observation: c,
            exercise: c,
        })
        .unwrap()
}
fn replay(h: &Value, c: &DynamicCalendar) -> LifecycleState {
    let life = &h["lifecycle"];
    let mut state = LifecycleState::new(
        serde_json::from_value(life["trade_id"].clone()).unwrap(),
        serde_json::from_value(life["version"].clone()).unwrap(),
        LifecycleInstrument::Bond {
            contract: Box::new(bond(&life["instrument"]["bond"]["terms"], c)),
            credit: None,
        },
        serde_json::from_value(life["position"].clone()).unwrap(),
        serde_json::from_value(life["known_at"].clone()).unwrap(),
        vec![],
    )
    .unwrap();
    for step in life["events"].as_array().unwrap() {
        let event: LifecycleEvent = serde_json::from_value(step["event"].clone()).unwrap();
        let amended = if let LifecycleAction::BondAmendment { terms, .. } = &event.action {
            Some(
                terms
                    .clone()
                    .build_with_calendars(&BondCalendars {
                        payment: c,
                        reset: c,
                        observation: c,
                        exercise: c,
                    })
                    .unwrap(),
            )
        } else {
            None
        };
        state = transition(
            &state,
            &event,
            &LifecycleInputs {
                amended_bond: amended.as_ref(),
                ..Default::default()
            },
        )
        .unwrap()
        .next;
    }
    state
}
#[test]
fn signed_cash_transfers_conserve_original_receipts_and_bind_full_prepared_states() {
    let corpus: Value =
        serde_json::from_str(packed::text("tests/fixtures/library/claim-pnl-bound.json")).unwrap();
    for k in [1, 2] {
        let spec = &corpus["cases"][k]["request"]["operation"]["input"];
        let b = &spec["book"];
        let c = calendar(b);
        let mut bindings = vec![&b["before"]];
        bindings.extend(b["steps"].as_array().unwrap().iter().map(|s| &s["after"]));
        bindings.extend(
            b["standalone"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["kind"] == "available")
                .map(|s| &s["information"]),
        );
        let mut next = b["steps"].as_array().unwrap().len() + 1;
        let input = CausalPnlInput {
            causes: b["steps"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| serde_json::from_value(s["cause"].clone()).unwrap())
                .collect(),
            points: bindings
                .iter()
                .map(|p| serde_json::from_value(p["point"].clone()).unwrap())
                .collect(),
            standalone: b["standalone"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    if s["kind"] == "available" {
                        let result = PnlStandalone::Available { point: next };
                        next += 1;
                        result
                    } else {
                        PnlStandalone::Unavailable {
                            reason: s["reason"].as_str().unwrap().into(),
                        }
                    }
                })
                .collect(),
            reporting_currency: convex_core::Currency::USD,
            max_points: 33,
            max_product_evaluations: 4224,
        };
        let states = bindings
            .iter()
            .map(|p| {
                p["holdings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| replay(&b["holdings"][i.as_u64().unwrap() as usize], &c))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let owners: Vec<CohortOwnership> = serde_json::from_value(spec["cohorts"].clone()).unwrap();
        let splits: Vec<CohortLotSplit> =
            serde_json::from_value(spec["lot_splits"].clone()).unwrap();
        let transfers: Vec<CohortClaimTransfer> =
            serde_json::from_value(spec["claim_transfers"].clone()).unwrap();
        let path =
            prepare_cohort_pnl_with_transfers(&input, &owners, &[], &splits, &transfers, &states)
                .unwrap();
        let history = convex_core::fixings::FixingSeries::default();
        let publications = PublicationSchedule::new(vec![]).unwrap();
        for (i, row) in states.iter().enumerate() {
            let recipes: Vec<convex_curves::contracts::CurveRecipe> = serde_json::from_value(
                b["markets"][bindings[i]["market"].as_u64().unwrap() as usize]["curves"].clone(),
            )
            .unwrap();
            let curves = convex_curves::contracts::build_curve_set(
                &recipes,
                &convex_curves::contracts::CurveSet::new(),
            )
            .unwrap()
            .curves;
            let mut recorded = rust_decimal::Decimal::ZERO;
            let mut outstanding = rust_decimal::Decimal::ZERO;
            let mut fraction = rust_decimal::Decimal::ZERO;
            for (j, state) in row.iter().enumerate() {
                let contract = match state.instrument() {
                    LifecycleInstrument::Bond { contract, .. } => contract,
                    _ => unreachable!(),
                };
                let pos = &input.points[i].positions[j];
                let cash = PreparedBondCash::fixed(
                    contract,
                    input.points[i].valuation_date,
                    convex_core::cashflows::PaymentDateEntitlement::Exclude,
                )
                .unwrap();
                let native = ValuationInputs::Bond {
                    cash: &cash,
                    method: BondPriceMethod::DiscountSpread {
                        curve: pos.cash_discount.clone(),
                        role: BondDiscountRole::Collateral,
                        decimal_spread: 0.0,
                    },
                    market: SuppliedMarket {
                        curves: &curves,
                        fixings: &history,
                        publications: &publications,
                    },
                };
                let discount = curves.get(&pos.cash_discount).unwrap();
                let mark = mark_causal_pnl_position(
                    &path,
                    i,
                    j,
                    state,
                    &CausalPnlProduct::Standard(&native),
                    discount,
                )
                .unwrap();
                for value in mark
                    .claims
                    .iter()
                    .filter_map(|c| c.transfer_ownership.as_ref())
                    .chain(mark.transferred_claims.iter())
                {
                    let own = &value.ownership;
                    fraction += own.fraction.decimal();
                    recorded += own.owned_recorded.value().decimal();
                    outstanding += own.owned_outstanding.value().decimal();
                    for receipt in &own.receipts {
                        assert!(row[0].payment_receipt_events().contains(&receipt.event));
                    }
                }
                if j == 0 && i == transfers[0].applied_points[0] + 1 {
                    let error = mark_causal_pnl_position(
                        &path,
                        i,
                        j,
                        &states[i - 1][j],
                        &CausalPnlProduct::Standard(&native),
                        discount,
                    )
                    .unwrap_err();
                    assert!(
                        error
                            .to_string()
                            .contains("same complete admitted original state"),
                        "{error}"
                    );
                }
            }
            let claim = &row[0].view().obligations[0];
            assert_eq!(fraction, rust_decimal::Decimal::ONE);
            assert_eq!(recorded, claim.recorded.value().decimal());
            assert_eq!(
                outstanding,
                claim.amount.value().decimal() - claim.recorded.value().decimal()
            );
        }
        for change in 0..6 {
            let mut bad = transfers.clone();
            match change {
                0 => bad[0].proposal_id = EventId::new("FOREIGN.PROPOSAL").unwrap(),
                1 => bad[0].cutoff.receipt_ids.clear(),
                2 => bad[1].fraction = ExactDecimal::parse("0.7").unwrap(),
                3 => bad[1].id = bad[0].id.clone(),
                4 => bad[1].applied_points.push(0),
                _ => bad[0].to = bad[0].from.clone(),
            }
            assert!(
                prepare_cohort_pnl_with_transfers(&input, &owners, &[], &splits, &bad, &states)
                    .is_err(),
                "admitted invalid fact {change}"
            );
        }
    }
}
