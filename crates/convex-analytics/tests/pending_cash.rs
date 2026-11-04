//! Actual native checked delivery, custody conservation and immutable invoice receipts.
#[path = "../../../tests/fixtures/packed.rs"]
mod packed;
use convex_analytics::{futures::*, lifecycle::*, workflow::*};
use convex_bonds::contracts::{BondCalendars, BondContract, BondTerms};
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    Date,
};
use rust_decimal::Decimal;
use serde_json::Value;
fn bond(v: &Value, c: &DynamicCalendar) -> BondContract {
    serde_json::from_value::<BondTerms>(v.clone())
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
    let v = &life["instrument"];
    let instrument = if v["kind"] == "future" {
        LifecycleInstrument::Futures {
            contract: Box::new(serde_json::from_value(v["contract"].clone()).unwrap()),
            opening_mark: Box::new(serde_json::from_value(v["opening_mark"].clone()).unwrap()),
        }
    } else {
        LifecycleInstrument::Bond {
            contract: Box::new(bond(&v["bond"]["terms"], c)),
            credit: None,
        }
    };
    let mut state = LifecycleState::new(
        serde_json::from_value(life["trade_id"].clone()).unwrap(),
        serde_json::from_value(life["version"].clone()).unwrap(),
        instrument,
        serde_json::from_value(life["position"].clone()).unwrap(),
        serde_json::from_value(life["known_at"].clone()).unwrap(),
        vec![],
    )
    .unwrap();
    for s in life["events"].as_array().unwrap() {
        let event: LifecycleEvent = serde_json::from_value(s["event"].clone()).unwrap();
        let f = &s["futures"];
        let d = &s["delivery"];
        let clearing: Option<FuturesClearingRules> = if f.is_null() {
            None
        } else {
            Some(serde_json::from_value(f["clearing"].clone()).unwrap())
        };
        let stamp: Option<convex_core::futures::ReferenceStamp> = if f.is_null() {
            None
        } else {
            Some(serde_json::from_value(f["calendar_stamp"].clone()).unwrap())
        };
        let delivery_stamp: Option<convex_core::futures::ReferenceStamp> = if d.is_null() {
            None
        } else {
            Some(serde_json::from_value(d["calendar_stamp"].clone()).unwrap())
        };
        let security: Option<convex_core::futures::ReferenceStamp> = if d.is_null() {
            None
        } else {
            Some(serde_json::from_value(d["security_stamp"].clone()).unwrap())
        };
        let instrument: Option<convex_core::futures::InstrumentPin> = if d.is_null() {
            None
        } else {
            Some(serde_json::from_value(d["instrument"].clone()).unwrap())
        };
        let basket: Option<convex_core::futures::DeliveryBasket> = if d.is_null() {
            None
        } else {
            Some(serde_json::from_value(d["basket"].clone()).unwrap())
        };
        let delivered = if d.is_null() {
            None
        } else {
            Some(bond(&d["bond"]["terms"], c))
        };
        let inputs = LifecycleInputs {
            futures: clearing.as_ref().map(|clearing| FuturesLifecycleInputs {
                clearing,
                calendar: FuturesCalendar {
                    stamp: stamp.as_ref().unwrap(),
                    calendar: c,
                },
                context: serde_json::from_value(f["context"].clone()).unwrap(),
            }),
            delivery: delivered.as_ref().map(|b| BondDeliveryInputs {
                basket: basket.as_ref().unwrap(),
                security: FuturesBond {
                    instrument: instrument.as_ref().unwrap(),
                    stamp: security.as_ref().unwrap(),
                    bond: b,
                },
                calendar: FuturesCalendar {
                    stamp: delivery_stamp.as_ref().unwrap(),
                    calendar: c,
                },
                context: serde_json::from_value(d["context"].clone()).unwrap(),
            }),
            ..Default::default()
        };
        state = transition(&state, &event, &inputs).unwrap().next;
    }
    state
}
#[test]
fn pending_delivery_cash_and_independent_legal_facts_conserve_native_facts() {
    let corpus: Value = serde_json::from_str(packed::text(
        "tests/fixtures/library/pending-cash-pnl-bound.json",
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap().iter().filter(|c| {
        c["expected_error"].is_null() && !c["id"].as_str().unwrap().contains("regression")
    }) {
        let spec = &case["request"]["operation"]["input"];
        let b = &spec["book"];
        let source = b["resources"]["calendars"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["pin"]["id"] == "WEEKEND")
            .unwrap();
        let mut calendar = DynamicCalendar::new("WEEKEND:2", WeekendType::SaturdaySunday);
        calendar.add_holidays(
            source["holidays"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| Date::parse(d.as_str().unwrap()).unwrap()),
        );
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
                .map(|s| serde_json::from_value(s["point"].clone()).unwrap())
                .collect(),
            standalone: b["standalone"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    if s["kind"] == "available" {
                        let p = PnlStandalone::Available { point: next };
                        next += 1;
                        p
                    } else {
                        PnlStandalone::Unavailable {
                            reason: s["reason"].as_str().unwrap().into(),
                        }
                    }
                })
                .collect(),
            reporting_currency: serde_json::from_value(b["reporting_currency"].clone()).unwrap(),
            max_points: 33,
            max_product_evaluations: 4224,
        };
        let states = bindings
            .iter()
            .map(|s| {
                s["holdings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| replay(&b["holdings"][i.as_u64().unwrap() as usize], &calendar))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let owners: Vec<CohortOwnership> = serde_json::from_value(spec["cohorts"].clone()).unwrap();
        let links: Vec<PhysicalDeliveryLink> =
            serde_json::from_value(spec["physical_deliveries"].clone()).unwrap();
        let partitions: Vec<PhysicalDeliveryPartition> =
            serde_json::from_value(spec["physical_partitions"].clone()).unwrap();
        let transfers: Vec<CohortClaimTransfer> =
            serde_json::from_value(spec["claim_transfers"].clone()).unwrap();
        let disposals: Vec<PhysicalDeliveryDisposal> =
            serde_json::from_value(spec["physical_disposals"].clone()).unwrap();
        let dated: Vec<CohortDatedEntitlement> =
            serde_json::from_value(spec["dated_entitlements"].clone()).unwrap();
        let path = prepare_cohort_pnl_with_dated_entitlements(
            &input,
            &owners,
            &links,
            &[],
            &transfers,
            &partitions,
            &disposals,
            &dated,
            &states,
        )
        .unwrap();
        let independent = input.causes.len() + 1;
        assert!(dated[0].applied_points.contains(&independent));
        assert_eq!(input.causes[0], PnlCause::Position);
        assert_eq!(states[0][1].view(), states[independent][1].view());
        assert_eq!(
            states[0][1].payment_receipt_events(),
            states[independent][1].payment_receipt_events()
        );
        assert_eq!(
            states[0][1].cash_proposal_event(&dated[0].key),
            states[independent][1].cash_proposal_event(&dated[0].key)
        );
        let received: Decimal = states[1][0]
            .security_receipt_events()
            .iter()
            .map(|e| match &e.action {
                LifecycleAction::SecurityReceipt { face, .. } => face.decimal(),
                _ => unreachable!(),
            })
            .sum();
        assert!(received.abs() < owners[1].quantity.decimal().abs());
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
            let history = convex_core::fixings::FixingSeries::default();
            let publications = PublicationSchedule::new(vec![]).unwrap();
            let mut proposed = Decimal::ZERO;
            let mut disposed = Decimal::ZERO;
            let mut received = Decimal::ZERO;
            let mut outstanding = Decimal::ZERO;
            let mut cash_received = Decimal::ZERO;
            let mut cash_outstanding = Decimal::ZERO;
            let first_bond = usize::from(matches!(
                row[0].instrument(),
                LifecycleInstrument::Futures { .. }
            ));
            let mut dated_recorded = Decimal::ZERO;
            let mut dated_outstanding = Decimal::ZERO;
            for (j, state) in row.iter().enumerate().skip(first_bond) {
                let LifecycleInstrument::Bond { contract, .. } = state.instrument() else {
                    unreachable!()
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
                if let Some(h) = &mark.physical_delivery {
                    proposed += h.proposed_face.decimal();
                    disposed += h
                        .disposals
                        .iter()
                        .map(|d| d.face.decimal())
                        .sum::<Decimal>();
                    if !h.invoice_excluded {
                        cash_received += h.invoice.recorded.value().decimal();
                        cash_outstanding += h.outstanding_invoice.value().decimal();
                    }
                    received += h.received_face.decimal();
                    outstanding += h.outstanding_face.decimal();
                    for event in &h.security_receipts {
                        assert!(row[0].security_receipt_events().contains(event));
                    }
                }
                for c in &mark.claims {
                    if let Some(v) = &c.transfer_ownership {
                        if v.ownership.origin == dated[0].origin
                            && v.ownership.obligation.key == dated[0].key
                        {
                            dated_recorded += v.ownership.owned_recorded.value().decimal();
                            dated_outstanding += v.ownership.owned_outstanding.value().decimal();
                        }
                    }
                }
                for c in &mark.transferred_claims {
                    if c.ownership.origin == dated[0].origin
                        && c.ownership.obligation.key == dated[0].key
                    {
                        dated_recorded += c.ownership.owned_recorded.value().decimal();
                        dated_outstanding += c.ownership.owned_outstanding.value().decimal();
                    }
                    if c.ownership.origin == row[0].view().trade_id {
                        cash_received += c.ownership.owned_recorded.value().decimal();
                        cash_outstanding += c.ownership.owned_outstanding.value().decimal();
                    }
                    let original = if c.ownership.origin == row[0].view().trade_id {
                        &row[0]
                    } else {
                        &row[1]
                    };
                    for r in &c.ownership.receipts {
                        assert!(original.payment_receipt_events().contains(&r.event));
                    }
                }
            }
            if dated[0].applied_points.contains(&i) {
                let origin = input.points[i]
                    .positions
                    .iter()
                    .position(|p| p.identity.id == dated[0].origin)
                    .unwrap();
                let claim = row[origin]
                    .view()
                    .obligations
                    .into_iter()
                    .find(|o| o.key == dated[0].key)
                    .unwrap();
                let prior: Decimal = row[origin]
                    .payment_receipt_events()
                    .iter()
                    .filter_map(|e| match &e.action {
                        LifecycleAction::Payment(r)
                            if dated[0].residual.cutoff.receipt_ids.contains(&r.receipt_id) =>
                        {
                            Some(r.amount.value().decimal())
                        }
                        _ => None,
                    })
                    .sum();
                assert_eq!(
                    dated_recorded,
                    (claim.recorded.value().decimal() - prior)
                        * dated[0].residual.fraction.decimal()
                );
                assert_eq!(
                    dated_outstanding,
                    (claim.amount.value().decimal() - claim.recorded.value().decimal())
                        * dated[0].residual.fraction.decimal()
                );
            } else {
                assert_eq!(dated_recorded, Decimal::ZERO);
                assert_eq!(dated_outstanding, Decimal::ZERO);
            }
            if row[0].delivery_event().is_some() {
                assert_eq!(proposed + disposed, owners[1].quantity.decimal());
                assert_eq!(received + outstanding, proposed);
                let actual = row[0]
                    .security_receipt_events()
                    .iter()
                    .map(|e| match &e.action {
                        LifecycleAction::SecurityReceipt { face, .. } => face.decimal(),
                        _ => unreachable!(),
                    })
                    .sum::<Decimal>();
                assert_eq!(received + disposed, actual);
                let invoice = row[0]
                    .view()
                    .obligations
                    .into_iter()
                    .find(|c| c.key.kind == CashKind::DeliveryInvoice)
                    .unwrap();
                assert_eq!(cash_received, invoice.recorded.value().decimal());
                assert_eq!(
                    cash_outstanding,
                    invoice.amount.value().decimal() - invoice.recorded.value().decimal()
                );
            }
        }
        for change in 0..if partitions.is_empty() { 0 } else { 5 } {
            let mut invalid = partitions.clone();
            match change {
                0 => invalid[0].receipt_ids.push(EventId::new("ABSENT").unwrap()),
                1 => invalid[0].delivery_id = EventId::new("ABSENT").unwrap(),
                2 => {
                    invalid[0].split.children.pop();
                }
                3 => invalid[0].split.applied_points.push(0),
                _ => invalid[0].split.children[1] = invalid[0].split.children[0].clone(),
            };
            assert!(prepare_cohort_pnl_with_dated_entitlements(
                &input,
                &owners,
                &links,
                &[],
                &transfers,
                &invalid,
                &disposals,
                &dated,
                &states
            )
            .is_err());
        }
        for change in 0..if disposals.is_empty() { 0 } else { 5 } {
            let mut invalid = disposals.clone();
            match change {
                0 => invalid[0].receipts[0].receipt_id = EventId::new("ABSENT").unwrap(),
                1 => invalid[0].delivery_id = EventId::new("ABSENT").unwrap(),
                2 => invalid[0].fill_id = EventId::new("ABSENT").unwrap(),
                3 => {
                    invalid[0].receipts[0].face = convex_core::contracts::ExactDecimal::new(
                        -invalid[0].receipts[0].face.decimal(),
                    )
                }
                _ => invalid[0].applied_points.push(0),
            }
            assert!(matches!(
                prepare_cohort_pnl_with_dated_entitlements(
                    &input,
                    &owners,
                    &links,
                    &[],
                    &transfers,
                    &partitions,
                    &invalid,
                    &dated,
                    &states
                ),
                Err(ReconciliationError::Workflow(WorkflowError::Invalid { .. }))
            ));
        }
        for change in 0..6 {
            let mut invalid = dated.clone();
            match change {
                0 => invalid[0].proposal_id = EventId::new("ABSENT").unwrap(),
                1 => {
                    if invalid[0].residual.cutoff.receipt_ids.is_empty() {
                        invalid[0]
                            .residual
                            .cutoff
                            .receipt_ids
                            .push(EventId::new("ABSENT").unwrap());
                    } else {
                        invalid[0].residual.cutoff.receipt_ids.clear();
                    }
                }
                2 => {
                    invalid[0].residual.fraction =
                        convex_core::contracts::ExactDecimal::new(Decimal::ZERO)
                }
                3 => invalid.push(invalid[0].clone()),
                4 => invalid[0].applied_points.push(0),
                _ => invalid[0].residual.id = invalid[0].proposal_id.clone(),
            }
            assert!(prepare_cohort_pnl_with_dated_entitlements(
                &input,
                &owners,
                &links,
                &[],
                &transfers,
                &partitions,
                &disposals,
                &invalid,
                &states
            )
            .is_err());
        }
    }
}
