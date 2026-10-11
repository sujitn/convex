//! Actual native boundary admission and exact historical-state conservation.
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
fn control(index: usize) -> Value {
    serde_json::from_str::<Value>(include_str!(
        "../../../tests/fixtures/library/lot-pnl-reference.json"
    ))
    .unwrap()["cases"][index]
        .clone()
}
#[test]
fn boundary_amendment_preserves_earned_cash_and_rejects_retroactive_or_unchecked_terms() {
    let case = control(0);
    let book = &case["request"]["operation"]["input"]["book"];
    let c = calendar(book);
    let before_index = book["steps"][0]["after"]["holdings"][0].as_u64().unwrap() as usize;
    let state = replay(&book["holdings"][before_index], &c);
    let after_index = book["steps"][1]["after"]["holdings"][0].as_u64().unwrap() as usize;
    let after = &book["holdings"][after_index];
    let event: LifecycleEvent = serde_json::from_value(
        after["lifecycle"]["events"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["event"]
            .clone(),
    )
    .unwrap();
    let amended = bond(&after["product"]["product"]["bond"]["terms"], &c);
    let result = transition(
        &state,
        &event,
        &LifecycleInputs {
            amended_bond: Some(&amended),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(state.view().obligations, result.next.view().obligations);
    assert_eq!(
        state.payment_receipt_events(),
        result.next.payment_receipt_events()
    );
    assert_eq!(result.next.view().amendments, vec![event.clone()]);
    assert_eq!(
        result.next.view().version.get(),
        state.view().version.get() + 1
    );
    let original = state.view();
    assert!(transition(&state, &event, &LifecycleInputs::default()).is_err());
    let mut mid = event.clone();
    mid.effective_date = Date::parse("2026-10-08").unwrap();
    mid.known_at = UtcTimestamp::parse("2026-10-08T12:00:00Z").unwrap();
    assert!(transition(
        &state,
        &mid,
        &LifecycleInputs {
            amended_bond: Some(&amended),
            ..Default::default()
        }
    )
    .is_err());
    let mut retro = event.clone();
    if let LifecycleAction::BondAmendment { terms, .. } = &mut retro.action {
        terms.profiles.coupon[0].value = rust_decimal::Decimal::new(9, 2);
    }
    let bad_terms = match &retro.action {
        LifecycleAction::BondAmendment { terms, .. } => terms,
        _ => unreachable!(),
    };
    let changed = bad_terms
        .clone()
        .build_with_calendars(&BondCalendars {
            payment: &c,
            reset: &c,
            observation: &c,
            exercise: &c,
        })
        .unwrap();
    assert!(transition(
        &state,
        &retro,
        &LifecycleInputs {
            amended_bond: Some(&changed),
            ..Default::default()
        }
    )
    .is_err());
    assert_eq!(state.view(), original);
    // The next full original coupon is earned under the amended profiles. Any
    // intervening actual profile boundary is processed by its original owner.
    let future: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/lot-pnl-future.json"
    ))
    .unwrap();
    let end = Date::parse(future["accrual_end"].as_str().unwrap()).unwrap();
    let mut state = result.next;
    while let Some((date, kind)) = state
        .view()
        .pending_boundaries
        .first()
        .cloned()
        .filter(|(date, _)| *date <= end)
    {
        let event = LifecycleEvent {
            event_id: EventId::new(format!("FUTURE.CASH.{}", state.view().revision)).unwrap(),
            trade_id: state.view().trade_id,
            expected_revision: state.view().revision,
            effective_date: date,
            known_at: UtcTimestamp::parse("2027-04-07T12:00:00Z").unwrap(),
            action: if kind == "contract_step" {
                LifecycleAction::ContractStep
            } else {
                LifecycleAction::ContractualCash
            },
        };
        state = transition(&state, &event, &LifecycleInputs::default())
            .unwrap()
            .next;
    }
    let claim = state
        .view()
        .obligations
        .into_iter()
        .find(|o| o.key.kind == CashKind::BondCoupon && o.key.effective_date == end)
        .unwrap();
    let expected: rust_decimal::Decimal = future["signed_currency_coupon"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((claim.amount.value().decimal() - expected).abs() <= rust_decimal::Decimal::new(1, 10));
}
#[test]
fn signed_successive_lots_require_complete_conserved_original_facts() {
    for k in [1, 2] {
        let case = control(k);
        let spec = &case["request"]["operation"]["input"];
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
            standalone: vec![PnlStandalone::Available {
                point: b["steps"].as_array().unwrap().len() + 1,
            }]
            .into_iter()
            .chain(
                (1..b["steps"].as_array().unwrap().len()).map(|_| PnlStandalone::Unavailable {
                    reason: "exact original sequence".into(),
                }),
            )
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
        let path = prepare_cohort_pnl_with_splits(&input, &owners, &[], &splits, &states).unwrap();
        // Same audit view and earned cash cannot conceal different future terms.
        let first = &b["holdings"][b["before"]["holdings"][0].as_u64().unwrap() as usize];
        let mut foreign = first.clone();
        let rows = foreign["lifecycle"]["instrument"]["bond"]["terms"]["profiles"]["coupon"]
            .as_array_mut()
            .unwrap();
        rows[1]["value"] = json!("0.09");
        let wrong = replay(&foreign, &c);
        assert_eq!(wrong.view(), states[0][0].view());
        let contract = match wrong.instrument() {
            LifecycleInstrument::Bond { contract, .. } => contract,
            _ => unreachable!(),
        };
        let recipes: Vec<convex_curves::contracts::CurveRecipe> =
            serde_json::from_value(b["markets"][0]["curves"].clone()).unwrap();
        let curves = convex_curves::contracts::build_curve_set(
            &recipes,
            &convex_curves::contracts::CurveSet::new(),
        )
        .unwrap()
        .curves;
        let cash = PreparedBondCash::fixed(
            contract,
            input.points[0].valuation_date,
            convex_core::cashflows::PaymentDateEntitlement::Exclude,
        )
        .unwrap();
        let history = convex_core::fixings::FixingSeries::default();
        let publications = PublicationSchedule::new(vec![]).unwrap();
        let native = ValuationInputs::Bond {
            cash: &cash,
            method: BondPriceMethod::DiscountSpread {
                curve: input.points[0].positions[0].cash_discount.clone(),
                role: BondDiscountRole::Collateral,
                decimal_spread: 0.0,
            },
            market: SuppliedMarket {
                curves: &curves,
                fixings: &history,
                publications: &publications,
            },
        };
        let discount = curves
            .get(&input.points[0].positions[0].cash_discount)
            .unwrap();
        let error = mark_causal_pnl_position(
            &path,
            0,
            0,
            &wrong,
            &CausalPnlProduct::Standard(&native),
            discount,
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("same complete admitted original state"));
        // Equal term labels and historical views cannot substitute the actual
        // calendar materialized into future coupons and principal cash dates.
        let mut foreign_calendar = c.clone();
        foreign_calendar.add_holidays([Date::parse("2027-04-07").unwrap()]);
        let calendar_state = replay(first, &foreign_calendar);
        assert_eq!(calendar_state.view(), states[0][0].view());
        let calendar_contract = match calendar_state.instrument() {
            LifecycleInstrument::Bond { contract, .. } => contract,
            _ => unreachable!(),
        };
        let original_contract = match states[0][0].instrument() {
            LifecycleInstrument::Bond { contract, .. } => contract,
            _ => unreachable!(),
        };
        assert_eq!(calendar_contract.terms(), original_contract.terms());
        assert!(!calendar_contract.periods().eq(original_contract.periods()));
        let calendar_cash = PreparedBondCash::fixed(
            calendar_contract,
            input.points[0].valuation_date,
            convex_core::cashflows::PaymentDateEntitlement::Exclude,
        )
        .unwrap();
        let calendar_native = ValuationInputs::Bond {
            cash: &calendar_cash,
            method: BondPriceMethod::DiscountSpread {
                curve: input.points[0].positions[0].cash_discount.clone(),
                role: BondDiscountRole::Collateral,
                decimal_spread: 0.0,
            },
            market: SuppliedMarket {
                curves: &curves,
                fixings: &history,
                publications: &publications,
            },
        };
        for (state, message) in [
            (&calendar_state, "same complete admitted original state"),
            (
                &states[0][0],
                "exact same full native bond economics required",
            ),
        ] {
            let error = mark_causal_pnl_position(
                &path,
                0,
                0,
                state,
                &CausalPnlProduct::Standard(&calendar_native),
                discount,
            )
            .unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
        }
        let acquisition = splits[0].applied_points[0];
        let child = owners
            .iter()
            .position(|o| o.trade_id == splits[0].children[0])
            .unwrap();
        let h = &b["holdings"][bindings[acquisition]["holdings"][child].as_u64().unwrap() as usize];
        let mut foreign_states = states.clone();
        foreign_states[acquisition][child] = replay(h, &foreign_calendar);
        let error = prepare_cohort_pnl_with_splits(&input, &owners, &[], &splits, &foreign_states)
            .unwrap_err();
        assert!(error.to_string().contains("split economics"), "{error}");
        let mut lost = owners.clone();
        lost[2].quantity = ExactDecimal::parse("1").unwrap();
        assert!(prepare_cohort_pnl_with_splits(&input, &lost, &[], &splits, &states).is_err());
        let mut orphan = splits.clone();
        orphan.pop();
        assert!(prepare_cohort_pnl_with_splits(&input, &owners, &[], &orphan, &states).is_err());
    }
}
