//! Direct native partial security receipt conservation and full provenance.
#[path = "../../convex/examples/support/lifecycle_fixture.rs"]
mod fixture;
#[path = "support/futures.rs"]
mod futures;
use convex_analytics::{futures::*, lifecycle::*};
use convex_core::{
    contracts::{TradeId, Version},
    futures::{EligibilityContext, ReferenceQuality},
};
use fixture::{apply, at, n, pay, pin, Error};
use rust_decimal::Decimal;

#[test]
fn partial_security_receipts_conserve_signed_native_face_and_independent_invoice(
) -> Result<(), Error> {
    for count in [-2, 2] {
        let mut pack = futures::BondPack::new();
        // Explicit separate synthetic revision, retaining the original fixture.
        pack.contract.stamp.version = Version::new(2)?;
        pack.basket.stamp.version = Version::new(2)?;
        pack.basket.contract.version = Version::new(2)?;
        if let convex_core::futures::FuturesSettlement::Bond { delivery, .. } =
            &mut pack.contract.settlement
        {
            delivery.basket.version = Version::new(2)?;
        }
        pack.contract.stamp.valid_until = at("2027-01-01")?;
        pack.contract.stamp.quality = ReferenceQuality::Synthetic;
        pack.basket.stamp.valid_until = at("2027-01-01")?;
        pack.basket.stamp.quality = ReferenceQuality::Synthetic;
        let opening = futures::mark(&pack.contract, "100", "2026-10-07", FuturesMarkKind::Trade);
        let mut state = LifecycleState::new(
            TradeId::new("RESIDUAL.NATIVE.DELIVERY")?,
            Version::new(1)?,
            LifecycleInstrument::Futures {
                contract: Box::new(pack.contract.clone()),
                opening_mark: Box::new(opening),
            },
            LifecyclePosition::FuturesContracts(count),
            at("2026-10-07")?,
            vec![],
        )?;
        let context = EligibilityContext {
            as_of: at("2026-12-08")?,
            known_at: at("2026-12-08")?,
        };
        let calendar = FuturesCalendar {
            stamp: &pack.calendar_stamp,
            calendar: &pack.calendar,
        };
        let clearing = futures::rules(&pack.contract, None);
        state = apply(
            &state,
            "CLEARING",
            "2026-12-08",
            LifecycleAction::FuturesSettlement(futures::mark(
                &pack.contract,
                "100",
                "2026-12-08",
                FuturesMarkKind::DailySettlement,
            )),
            &LifecycleInputs {
                futures: Some(FuturesLifecycleInputs {
                    clearing: &clearing,
                    calendar,
                    context,
                }),
                ..Default::default()
            },
        )?
        .next;
        let members = pack.deliverables();
        let selected = &members[0];
        state = apply(
            &state,
            "DELIVERY",
            "2026-12-08",
            LifecycleAction::BondDelivery {
                deliverable: selected.security.instrument.clone(),
                invoice_price_points: n("100")?,
                notice: pin("ACTUAL.RESIDUAL.NOTICE")?,
            },
            &LifecycleInputs {
                delivery: Some(BondDeliveryInputs {
                    basket: &pack.basket,
                    security: selected.security,
                    calendar,
                    context,
                }),
                ..Default::default()
            },
        )?
        .next;
        let (key, instrument, full) = state
            .view()
            .delivery_pending
            .clone()
            .ok_or("missing original asset")?;
        let before = state.view();
        let signed = Decimal::from(count.signum());
        let receipt = |amount: Decimal, id: &str| -> Result<LifecycleAction, Error> {
            Ok(LifecycleAction::SecurityReceipt {
                obligation: key.clone(),
                instrument: instrument.clone(),
                face: convex_core::contracts::ExactDecimal::new(amount),
                receipt_id: EventId::new(id)?,
                source: "actual.partial.recorder".to_owned().try_into()?,
            })
        };
        for (i, amount) in [
            Decimal::ZERO,
            -signed * Decimal::from(1),
            signed * Decimal::from(200001),
        ]
        .into_iter()
        .enumerate()
        {
            assert!(apply(
                &state,
                &format!("INVALID.{i}"),
                "2026-12-08",
                receipt(amount, &format!("BAD.{i}"))?,
                &Default::default()
            )
            .is_err());
            assert_eq!(state.view(), before);
            assert!(state.security_receipt_events().is_empty());
        }
        for (i, face) in [50000, 70000, 80000].into_iter().enumerate() {
            let amount = signed * Decimal::from(face);
            let step = apply(
                &state,
                &format!("PARTIAL.{i}"),
                "2026-12-08",
                receipt(amount, &format!("RECEIPT.{i}"))?,
                &Default::default(),
            )?;
            state = step.next;
            assert_eq!(state.security_receipt_events().len(), i + 1);
            assert_eq!(state.security_receipt_events()[i], step.event);
            let total = state
                .security_receipt_events()
                .iter()
                .map(|e| match e.action {
                    LifecycleAction::SecurityReceipt { face, .. } => face.decimal(),
                    _ => unreachable!(),
                })
                .sum::<Decimal>();
            let outstanding = state
                .view()
                .delivery_pending
                .map_or(Decimal::ZERO, |x| x.2.decimal());
            assert_eq!(total + outstanding, full.decimal());
            assert_eq!(state.view().status, EconomicStatus::Matured);
            let unchanged = state.view();
            assert!(apply(
                &state,
                "DUPLICATE",
                "2026-12-08",
                receipt(amount, &format!("RECEIPT.{i}"))?,
                &Default::default()
            )
            .is_err());
            assert_eq!(state.view(), unchanged);
        }
        for (i, claim) in state.view().obligations.into_iter().enumerate() {
            state = pay(
                &state,
                &format!("ACTUAL.CASH.{i}"),
                "2026-12-08",
                claim.key,
                claim.amount,
            )?
            .next;
        }
        assert_eq!(state.view().status, EconomicStatus::Closed);
        assert_eq!(state.payment_receipt_events().len(), 2);
        assert_eq!(state.security_receipt_events().len(), 3);
        assert!(state.view().delivery_pending.is_none());
    }
    Ok(())
}

#[test]
fn residual_acquisition_cannot_use_later_original_earned_information() -> Result<(), Error> {
    use convex_analytics::workflow as w;
    use serde_json::json;
    let pack = fixture::Fixture::load()?;
    let mut before = fixture::bond_state(&pack)?;
    before = apply(
        &before,
        "PROFILE",
        "2024-04-02",
        LifecycleAction::ContractStep,
        &Default::default(),
    )?
    .next;
    let mut earned = apply(
        &before,
        "PRINCIPAL",
        "2024-07-02",
        LifecycleAction::ContractStep,
        &Default::default(),
    )?
    .next;
    earned = apply(
        &earned,
        "EARNED",
        "2024-07-02",
        LifecycleAction::ContractualCash,
        &Default::default(),
    )?
    .next;
    let id = earned.view().trade_id;
    let point = |date: &str,
                 name: &str,
                 quantity: &str,
                 fills: serde_json::Value|
     -> Result<w::PnlPointInput, Error> {
        Ok(serde_json::from_value(
            json!({"snapshot_id":name,"manifest":{"id":name,"version":1},
            "valuation_date":date,"known_at":format!("{date}T12:00:00Z"),"positions":[{
                "identity":{"id":id,"instrument":{"id":"ORIGINAL.FIXED","version":1},
                    "economics":{"id":name,"version":1},"calculation":{"id":name,"version":1},"entity":"SUPPLIED.DESK"},
                "current":{"quantity":quantity,"reference_quantity":"1000","quantity_unit":"notional","currency":"USD"},
                "pending_quantity":"0","cash_discount":{"id":"ORIGINAL.DISCOUNT","version":"1"},"overdue":"nominal"}],
            "fills":fills,"fx":[]}),
        )?)
    };
    let mut input = w::CausalPnlInput {
        causes: vec![w::PnlCause::Time, w::PnlCause::Position],
        points: vec![
            point("2024-07-01", "NATIVE.BEFORE", "0", json!([]))?,
            point("2024-07-02", "NATIVE.EARNED", "0", json!([]))?,
            point(
                "2024-07-02",
                "NATIVE.ACQUIRED",
                "2000",
                json!([{
                "id":"ORIGINAL.ACQUISITION","trade_id":id,"instrument":{"id":"ORIGINAL.FIXED","version":1},
                "quantity":"2000","consideration":{"value":"-1900","currency":"USD","unit":"currency"},
                "fees":{"value":"0","currency":"USD","unit":"currency"},"date":"2024-07-02",
                "known_at":"2024-07-02T11:00:00Z","source":"actual.supplied.fill"}]),
            )?,
        ],
        standalone: vec![
            w::PnlStandalone::Unavailable {
                reason: "Original earned time boundary".to_owned(),
            },
            w::PnlStandalone::Unavailable {
                reason: "Original actual acquisition".to_owned(),
            },
        ],
        reporting_currency: convex_core::Currency::USD,
        max_points: 3,
        max_product_evaluations: 3,
    };
    let mut owners = vec![w::CohortOwnership {
        trade_id: id,
        quantity: n("2000")?,
        acquisition: w::CohortAcquisition::Fill {
            fill_id: EventId::new("ORIGINAL.ACQUISITION")?,
        },
        disposal_fill: None,
        entitlements: earned
            .view()
            .obligations
            .into_iter()
            .map(|o| {
                Ok::<_, Error>(w::CohortClaimEntitlement {
                    entitled: o.key.kind == CashKind::BondCoupon,
                    residual: if o.key.kind == CashKind::BondCoupon {
                        Some(w::CohortResidualClaim {
                            id: EventId::new("ORIGINAL.COUPON.RESIDUAL")?,
                            fraction: n("0.5")?,
                            cutoff: w::CohortCashCutoff {
                                date: fixture::d("2024-07-02")?,
                                known_at: convex_core::contracts::UtcTimestamp::parse(
                                    "2024-07-02T11:00:00Z",
                                )?,
                                receipt_ids: vec![],
                            },
                        })
                    } else {
                        None
                    },
                    key: o.key,
                    source: pin("ORIGINAL.LEGAL.ENTITLEMENT")?,
                })
            })
            .collect::<Result<_, _>>()?,
    }];
    let states = vec![vec![before], vec![earned.clone()], vec![earned]];
    let earned_known_at = at("2024-07-02")?;
    input.points[1].known_at = earned_known_at;
    input.points[2].known_at = earned_known_at;
    let error = w::prepare_cohort_pnl(&input, &owners, &[], &states).unwrap_err();
    assert!(error
        .to_string()
        .contains("original earned/recovery terms must be known at actual acquisition"));
    input.points[2].fills[0].known_at = earned_known_at;
    for e in &mut owners[0].entitlements {
        if let Some(residual) = &mut e.residual {
            residual.cutoff.known_at = earned_known_at;
        }
    }
    w::prepare_cohort_pnl(&input, &owners, &[], &states)?;
    assert_eq!(states[2][0].view().revision, 3);
    Ok(())
}
