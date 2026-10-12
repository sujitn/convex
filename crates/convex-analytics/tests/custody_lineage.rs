//! Actual native checked delivery, custody conservation and immutable invoice receipts.
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
fn successive_custody_actual_disposals_and_invoice_shares_conserve_exact_native_facts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/custody-lineage-pnl-admitted.json"
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
        let path = prepare_cohort_pnl_with_disposals(
            &input,
            &owners,
            &links,
            &[],
            &transfers,
            &partitions,
            &disposals,
            &states,
        )
        .unwrap();
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
            for (j, state) in row.iter().enumerate().skip(1) {
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
                for c in &mark.transferred_claims {
                    cash_received += c.ownership.owned_recorded.value().decimal();
                    cash_outstanding += c.ownership.owned_outstanding.value().decimal();
                    for r in &c.ownership.receipts {
                        assert!(row[0].payment_receipt_events().contains(&r.event));
                    }
                }
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
        for change in 0..5 {
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
            assert!(prepare_cohort_pnl_with_disposals(
                &input,
                &owners,
                &links,
                &[],
                &transfers,
                &invalid,
                &disposals,
                &states
            )
            .is_err());
        }
        for change in 0..5 {
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
                prepare_cohort_pnl_with_disposals(
                    &input,
                    &owners,
                    &links,
                    &[],
                    &transfers,
                    &partitions,
                    &invalid,
                    &states
                ),
                Err(ReconciliationError::Workflow(WorkflowError::Invalid { .. }))
            ));
        }
    }
}
