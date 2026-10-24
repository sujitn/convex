//! CVX-062 exact economic boundaries, deterministic replay and supplied receipts.
#[path = "../../convex/examples/support/lifecycle_fixture.rs"]
mod fixture;
#[path = "support/futures.rs"]
mod futures;
use convex_analytics::lifecycle::*;
use convex_bonds::{
    contracts::{BondCoupon, BondProfiles, ExerciseStyle, OvernightRateTerms},
    instruments::ZeroCouponBond,
};
use convex_core::{
    cashflows::*,
    contracts::{IndexId, Money, TradeId, UtcTimestamp, Version},
    fixings::*,
    Currency,
};
use convex_credit::{CdsSide, Seniority};
use fixture::{apply, at, bond_state, cash, d, event, n, pay, pin, Error, Fixture};
use rust_decimal::Decimal;
use serde_json::{json, Value};
fn expected() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/lifecycle/expected.json"
    ))
    .unwrap()
}
fn value(v: &Value, name: &str) -> Decimal {
    v[name].as_str().unwrap().parse().unwrap()
}
fn bond_cash_ready(f: &Fixture) -> Result<LifecycleState, Error> {
    let mut s = bond_state(f)?;
    for (id, date, a) in [
        ("step.rate", "2024-04-02", LifecycleAction::ContractStep),
        (
            "step.principal",
            "2024-07-02",
            LifecycleAction::ContractStep,
        ),
        ("coupon.1", "2024-07-02", LifecycleAction::ContractualCash),
    ] {
        s = apply(&s, id, date, a, &Default::default())?.next;
    }
    Ok(s)
}
fn amount(s: &LifecycleState, kind: CashKind, ordinal: u32) -> Decimal {
    s.view()
        .obligations
        .iter()
        .find(|o| o.key.kind == kind && o.key.ordinal == ordinal)
        .unwrap()
        .amount
        .value()
        .decimal()
}
#[test]
fn repeated_full_state_event_and_inputs_are_identical_and_failures_are_atomic() -> Result<(), Error>
{
    let f = Fixture::load()?;
    let s = bond_state(&f)?;
    let e = event(&s, "step.rate", "2024-04-02", LifecycleAction::ContractStep)?;
    let a = transition(&s, &e, &Default::default())?;
    let b = transition(&s, &e, &Default::default())?;
    assert_eq!(a.next.view(), b.next.view());
    assert_eq!(a.effects, b.effects);
    assert_eq!(a.next.view().revision, 1);
    assert_eq!(s.view().revision, 0);
    for which in 0..5 {
        let mut bad = e.clone();
        match which {
            0 => bad.expected_revision = 2,
            1 => bad.trade_id = TradeId::new("another.trade")?,
            2 => bad.effective_date = d("2023-12-31")?,
            3 => bad.known_at = at("2024-01-01")?,
            _ => bad.effective_date = d("2024-04-03")?,
        };
        assert!(transition(&s, &bad, &Default::default()).is_err());
        assert_eq!(s.view().revision, 0);
    }
    let mut duplicate = e;
    duplicate.expected_revision = 1;
    assert!(transition(&a.next, &duplicate, &Default::default()).is_err());
    Ok(())
}
#[test]
fn earned_coupon_and_effective_amortization_keep_delayed_cash_and_exact_signed_face(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    let s = bond_cash_ready(&f)?;
    let reference = expected();
    assert_eq!(
        amount(&s, CashKind::BondCoupon, 0),
        value(&reference["bond"], "coupon_1") * Decimal::from(2)
    );
    assert_eq!(amount(&s, CashKind::Principal, 0), Decimal::from(800));
    for o in s.view().obligations {
        assert_eq!(o.due_date, d("2024-07-07")?);
        assert_eq!(o.recorded.value().decimal(), Decimal::ZERO);
        assert!(!o.settled);
    }
    let mut terms = f.bond("bond")?.terms().clone();
    terms.dates.ex_coupon = Some(ExCouponRule {
        offset: DateOffset::CalendarDays(-3),
        anchor: ExDateAnchor::PaymentDate,
        boundary: ExCouponBoundary::OnDate,
    });
    let c = terms.build()?;
    let mut short = LifecycleState::new(
        TradeId::new("short")?,
        Version::new(1)?,
        LifecycleInstrument::Bond {
            contract: Box::new(c),
            credit: None,
        },
        LifecyclePosition::BondFace(n("-2000")?),
        at("2024-01-02")?,
        vec![],
    )?;
    for (id, date, a) in [
        ("r", "2024-04-02", LifecycleAction::ContractStep),
        ("p", "2024-07-02", LifecycleAction::ContractStep),
        ("c", "2024-07-02", LifecycleAction::ContractualCash),
    ] {
        short = apply(&short, id, date, a, &Default::default())?.next;
    }
    assert_eq!(
        amount(&short, CashKind::BondCoupon, 0),
        -amount(&s, CashKind::BondCoupon, 0)
    );
    Ok(())
}
#[test]
fn partial_receipts_do_not_change_proposed_cash_and_duplicates_overpayment_sign_currency_fail(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    let s = bond_cash_ready(&f)?;
    let o = s
        .view()
        .obligations
        .into_iter()
        .find(|o| o.key.kind == CashKind::Principal)
        .ok_or("missing principal")?;
    let first = pay(
        &s,
        "cash.partial",
        "2024-07-07",
        o.key.clone(),
        cash("300")?,
    )?
    .next;
    let current = first
        .view()
        .obligations
        .into_iter()
        .find(|c| c.key == o.key)
        .ok_or("missing")?;
    assert_eq!(current.amount, o.amount);
    assert_eq!(current.recorded, cash("300")?);
    assert!(!current.settled);
    for (id, date, amount) in [
        ("cash.partial", "2024-07-07", cash("300")?),
        ("wrong.sign", "2024-07-07", cash("-1")?),
        ("too.much", "2024-07-07", cash("501")?),
        ("early", "2024-07-02", cash("1")?),
        ("fx", "2024-07-07", Money::new(n("1")?, Currency::EUR)),
    ] {
        assert!(pay(&first, id, date, o.key.clone(), amount).is_err());
    }
    let finished = pay(&first, "cash.rest", "2024-07-07", o.key, cash("500")?)?.next;
    assert!(
        finished
            .view()
            .obligations
            .iter()
            .find(|c| c.key.kind == CashKind::Principal)
            .ok_or("missing")?
            .settled
    );
    Ok(())
}
#[test]
fn redemption_ends_exposure_and_retains_earned_coupon_until_every_receipt_is_supplied(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    let mut s = bond_cash_ready(&f)?;
    for (i, o) in s.view().obligations.into_iter().enumerate() {
        s = pay(&s, &format!("first.{i}"), "2024-07-07", o.key, o.amount)?.next;
    }
    s = apply(
        &s,
        "final.step",
        "2025-01-02",
        LifecycleAction::ContractStep,
        &Default::default(),
    )?
    .next;
    assert_eq!(s.view().status, EconomicStatus::Matured);
    s = apply(
        &s,
        "final.coupon",
        "2025-01-02",
        LifecycleAction::ContractualCash,
        &Default::default(),
    )?
    .next;
    assert_eq!(
        amount(&s, CashKind::BondCoupon, 1),
        value(&expected()["bond"], "coupon_2") * Decimal::from(2)
    );
    assert_eq!(amount(&s, CashKind::Principal, 1), Decimal::from(1200));
    for (i, o) in s
        .view()
        .obligations
        .into_iter()
        .filter(|o| !o.settled)
        .enumerate()
    {
        s = pay(&s, &format!("last.{i}"), "2025-01-07", o.key, o.amount)?.next;
    }
    assert_eq!(s.view().status, EconomicStatus::Closed);
    assert!(s.view().obligations.iter().all(|o| o.settled));
    assert!(apply(
        &s,
        "repeat",
        "2025-01-08",
        LifecycleAction::ContractualCash,
        &Default::default()
    )
    .is_err());
    Ok(())
}
#[test]
fn notice_exercise_and_settlement_preserve_original_dates_price_and_earned_partial_cash(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    let original = f.bond("bond")?;
    let id = &original.terms().calendars.exercise;
    let inputs = LifecycleInputs {
        exercise_calendar: Some((id, &f.calendar)),
        ..Default::default()
    };
    let mut s = bond_state(&f)?;
    s = apply(
        &s,
        "step",
        "2024-04-02",
        LifecycleAction::ContractStep,
        &inputs,
    )?
    .next;
    assert!(apply(
        &s,
        "missing.notice",
        "2024-04-15",
        LifecycleAction::Exercise {
            provision: 0,
            weight_end: None
        },
        &inputs
    )
    .is_err());
    assert!(apply(
        &s,
        "late.notice",
        "2024-04-14",
        LifecycleAction::ExerciseNotice {
            provision: 0,
            exercise_date: d("2024-04-15")?
        },
        &inputs
    )
    .is_err());
    s = apply(
        &s,
        "notice",
        "2024-04-12",
        LifecycleAction::ExerciseNotice {
            provision: 0,
            exercise_date: d("2024-04-15")?,
        },
        &inputs,
    )?
    .next;
    s = apply(
        &s,
        "exercise",
        "2024-04-15",
        LifecycleAction::Exercise {
            provision: 0,
            weight_end: None,
        },
        &inputs,
    )?
    .next;
    assert_eq!(s.view().status, EconomicStatus::Exercised);
    assert_eq!(
        amount(&s, CashKind::Exercise, 0),
        value(&expected()["bond"], "exercise_cash") * Decimal::from(2)
    );
    let o = s.view().obligations[0].clone();
    assert_eq!(o.due_date, d("2024-04-18")?);
    assert!(pay(&s, "too.early", "2024-04-17", o.key.clone(), o.amount).is_err());
    s = pay(&s, "exercise.receipt", "2024-04-18", o.key, o.amount)?.next;
    assert_eq!(s.view().status, EconomicStatus::Closed);
    Ok(())
}
#[test]
fn actual_term_fixing_revisions_and_prefix_exercise_never_require_future_coupon_rates(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    let c = f.bond("frn")?;
    let index = IndexId::new("USD.TERM")?;
    let mut s = LifecycleState::new(
        TradeId::new("frn")?,
        Version::new(1)?,
        LifecycleInstrument::Bond {
            contract: Box::new(c.clone()),
            credit: None,
        },
        LifecyclePosition::BondFace(n("1000")?),
        at("2024-01-02")?,
        vec![f.observation(&index, d("2024-01-02")?, "0.03", 1)?],
    )?;
    let revision = f.observation(&index, d("2024-01-02")?, "0.04", 2)?;
    s = apply(
        &s,
        "revised",
        "2024-01-03",
        LifecycleAction::Fixing(revision.clone()),
        &Default::default(),
    )?
    .next;
    assert!(apply(
        &s,
        "duplicate.revision",
        "2024-01-03",
        LifecycleAction::Fixing(revision),
        &Default::default()
    )
    .is_err());
    let dates = [d("2024-01-02")?, d("2024-02-02")?];
    let publications = f.publications(&index, &dates)?;
    let inputs = LifecycleInputs {
        publications: Some(&publications),
        exercise_calendar: Some((&c.terms().calendars.exercise, &f.calendar)),
        ..Default::default()
    };
    s = apply(
        &s,
        "reset2",
        "2024-02-02",
        LifecycleAction::Fixing(f.observation(&index, d("2024-02-02")?, "-0.005", 1)?),
        &inputs,
    )?
    .next;
    s = apply(
        &s,
        "frn.notice",
        "2024-02-12",
        LifecycleAction::ExerciseNotice {
            provision: 0,
            exercise_date: d("2024-02-15")?,
        },
        &inputs,
    )?
    .next;
    s = apply(
        &s,
        "frn.exercise",
        "2024-02-15",
        LifecycleAction::Exercise {
            provision: 0,
            weight_end: None,
        },
        &inputs,
    )?
    .next;
    assert_eq!(s.view().fixings.len(), 3);
    assert_eq!(s.view().status, EconomicStatus::Exercised);
    // Exact native prefix:31days at.04+.001,13days at-.005+.001.
    let accrued = c.terms().rounding.apply(
        (Decimal::from(1000)
            * (n("0.041")?.decimal() * Decimal::from(31)
                + n("-0.004")?.decimal() * Decimal::from(13)))
            / Decimal::from(365),
    )?;
    assert_eq!(
        amount(&s, CashKind::Exercise, 0),
        Decimal::from(1010) + accrued
    );
    Ok(())
}
#[test]
fn term_coupon_missing_publication_or_due_history_has_no_accepted_state() -> Result<(), Error> {
    let f = Fixture::load()?;
    let c = f.bond("frn")?;
    let mut s = LifecycleState::new(
        TradeId::new("frn")?,
        Version::new(1)?,
        LifecycleInstrument::Bond {
            contract: Box::new(c),
            credit: None,
        },
        LifecyclePosition::BondFace(n("1000")?),
        at("2024-01-02")?,
        vec![],
    )?;
    let index = IndexId::new("USD.TERM")?;
    let dates = [d("2024-01-02")?, d("2024-02-02")?, d("2024-03-02")?];
    let pubs = f.publications(&index, &dates)?;
    assert!(matches!(
        apply(
            &s,
            "missing",
            "2024-04-02",
            LifecycleAction::ContractualCash,
            &Default::default()
        )
        .unwrap_err()
        .downcast_ref::<LifecycleError>(),
        Some(LifecycleError::Workflow(_))
    ));
    let inputs = LifecycleInputs {
        publications: Some(&pubs),
        ..Default::default()
    };
    assert!(apply(
        &s,
        "missing",
        "2024-04-02",
        LifecycleAction::ContractualCash,
        &inputs
    )
    .is_err());
    assert_eq!(s.view().revision, 0);
    for (i, date) in dates.into_iter().enumerate() {
        s = apply(
            &s,
            &format!("fix.{i}"),
            &date.to_string(),
            LifecycleAction::Fixing(f.observation(&index, date, "0.03", 1)?),
            &inputs,
        )?
        .next;
    }
    let r = apply(
        &s,
        "coupon",
        "2024-04-02",
        LifecycleAction::ContractualCash,
        &inputs,
    )?;
    assert_eq!(r.next.view().obligations.len(), 1);
    assert!(r.next.view().obligations.iter().all(|o| !o.settled));
    Ok(())
}
#[test]
fn irs_due_selection_keeps_original_rounding_profiles_and_missing_future_resets_separate(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    let index = IndexId::new("USD.TERM")?;
    // The first actual fixing predates opening; supplied history must be available.
    let c = f.swap("irs")?;
    let mut s = LifecycleState::new(
        c.terms().trade_id.clone(),
        c.terms().version,
        LifecycleInstrument::Swap(Box::new(c.clone())),
        LifecyclePosition::TradeCopies(n("1")?),
        at("2024-01-31")?,
        vec![f.observation(&index, d("2024-01-29")?, "0.05", 1)?],
    )?;
    let pubs = f.publications(&index, &[d("2024-01-29")?, d("2024-02-27")?])?;
    let inputs = LifecycleInputs {
        publications: Some(&pubs),
        ..Default::default()
    };
    s = apply(
        &s,
        "steps",
        "2024-02-29",
        LifecycleAction::ContractStep,
        &inputs,
    )?
    .next;
    s = apply(
        &s,
        "coupon1",
        "2024-02-29",
        LifecycleAction::ContractualCash,
        &inputs,
    )?
    .next;
    assert_eq!(
        amount(&s, CashKind::FixedCoupon, 0),
        value(&expected()["irs"], "fixed_1")
    );
    assert_eq!(
        amount(&s, CashKind::FloatingCoupon, 0),
        value(&expected()["irs"], "float_1")
    );
    assert_eq!(s.view().obligations[0].due_date, d("2024-03-04")?);
    let ctx = convex_rates::CashflowContext {
        fixings: FixingContext {
            valuation_date: d("2024-03-04")?,
            known_at: at("2024-03-04")?,
            unpublished: UnpublishedFixingPolicy::Forecast,
            final_only: true,
        },
        on_payment: PaymentDateEntitlement::Include,
    };
    let hist = FixingSeries::new(s.view().fixings)?;
    let due = c.due_cashflows(ctx, &hist, pubs.callback())?;
    assert_eq!(due.fixed.len(), 1);
    assert_eq!(due.floating.len(), 1);
    assert!(c
        .cashflows(ctx, &hist, pubs.callback(), |r| Err(
            CashflowError::MissingFixings(vec![r.requirement.clone()])
        ))
        .is_err());
    Ok(())
}
#[test]
fn partial_swap_termination_at_joint_boundary_keeps_old_arrears_and_reprices_remaining_quantity(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    let c = f.swap("irs")?;
    let index = IndexId::new("USD.TERM")?;
    let observations = vec![f.observation(&index, d("2024-01-29")?, "0.05", 1)?];
    let mut s = LifecycleState::new(
        c.terms().trade_id.clone(),
        c.terms().version,
        LifecycleInstrument::Swap(Box::new(c)),
        LifecyclePosition::TradeCopies(n("1")?),
        at("2024-01-31")?,
        observations,
    )?;
    let pubs = f.publications(&index, &[d("2024-01-29")?, d("2024-02-27")?])?;
    let inputs = LifecycleInputs {
        publications: Some(&pubs),
        ..Default::default()
    };
    s = apply(
        &s,
        "second.fix",
        "2024-02-27",
        LifecycleAction::Fixing(f.observation(&index, d("2024-02-27")?, "-0.005", 1)?),
        &inputs,
    )?
    .next;
    assert!(apply(
        &s,
        "bad.partial",
        "2024-02-28",
        LifecycleAction::Terminate {
            fraction: n("0.5")?,
            cash: cash("0")?,
            cash_date: d("2024-03-01")?,
            agreement: pin("agreement")?
        },
        &inputs
    )
    .is_err());
    s = apply(
        &s,
        "step",
        "2024-02-29",
        LifecycleAction::ContractStep,
        &inputs,
    )?
    .next;
    s = apply(
        &s,
        "first",
        "2024-02-29",
        LifecycleAction::ContractualCash,
        &inputs,
    )?
    .next;
    let before = s.view().obligations;
    s = apply(
        &s,
        "partial",
        "2024-02-29",
        LifecycleAction::Terminate {
            fraction: n("0.5")?,
            cash: cash("12.00")?,
            cash_date: d("2024-03-04")?,
            agreement: pin("actual.synthetic.agreement")?,
        },
        &inputs,
    )?
    .next;
    for o in before {
        assert!(s.view().obligations.contains(&o));
    }
    s = apply(
        &s,
        "remaining",
        "2024-03-31",
        LifecycleAction::ContractualCash,
        &inputs,
    )?
    .next;
    assert_eq!(
        amount(&s, CashKind::FixedCoupon, 1),
        value(&expected()["irs"], "fixed_2") * n("0.5")?.decimal()
    );
    assert_eq!(
        amount(&s, CashKind::FloatingCoupon, 1),
        value(&expected()["irs"], "float_2") * n("0.5")?.decimal()
    );
    Ok(())
}
#[test]
fn shifted_ois_crystallizes_original_weighted_known_coupon_without_future_forecasts(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    let c = f.swap("ois")?;
    let index = IndexId::new("USD.ON")?;
    let mut s = LifecycleState::new(
        c.terms().trade_id.clone(),
        c.terms().version,
        LifecycleInstrument::Swap(Box::new(c)),
        LifecyclePosition::TradeCopies(n("1")?),
        at("2024-04-04")?,
        vec![
            f.observation(&index, d("2024-04-02")?, "0.05", 1)?,
            f.observation(&index, d("2024-04-03")?, "0.05", 1)?,
            f.observation(&index, d("2024-04-04")?, "0.05", 1)?,
        ],
    )?;
    let dates = [
        "2024-04-02",
        "2024-04-03",
        "2024-04-04",
        "2024-04-05",
        "2024-04-08",
    ]
    .iter()
    .map(|s| d(s))
    .collect::<Result<Vec<_>, _>>()?;
    let pubs = f.publications(&index, &dates)?;
    let inputs = LifecycleInputs {
        publications: Some(&pubs),
        ..Default::default()
    };
    for date in ["2024-04-05", "2024-04-08"] {
        s = apply(
            &s,
            &format!("fix.{date}"),
            date,
            LifecycleAction::Fixing(f.observation(&index, d(date)?, "0.05", 1)?),
            &inputs,
        )?
        .next;
    }
    s = apply(
        &s,
        "ois.earned",
        "2024-04-11",
        LifecycleAction::ContractualCash,
        &inputs,
    )?
    .next;
    assert_eq!(
        amount(&s, CashKind::FixedCoupon, 0),
        value(&expected()["ois"], "fixed")
    );
    assert_eq!(
        amount(&s, CashKind::FloatingCoupon, 0),
        value(&expected()["ois"], "float")
    );
    assert_eq!(s.view().status, EconomicStatus::Matured);
    assert!(s.view().obligations.iter().all(|o| !o.settled));
    Ok(())
}
fn credit_start(f: &Fixture, side: CdsSide) -> Result<LifecycleState, Error> {
    let original = f.cds()?;
    let mut t = original.terms().clone();
    t.side = side;
    let c = t.build(&convex_credit::CdsCalendars {
        premium: convex_credit::CalendarBinding {
            pin: &t.premium_calendar,
            calendar: &f.calendar,
        },
        settlement: convex_credit::CalendarBinding {
            pin: &t.settlement_calendar,
            calendar: &f.calendar,
        },
    })?;
    Ok(LifecycleState::new(
        t.trade_id.clone(),
        t.version,
        LifecycleInstrument::Cds(Box::new(c)),
        LifecyclePosition::CdsNotional(t.notional),
        at("2024-06-19")?,
        vec![],
    )?)
}
fn default_event(f: &Fixture) -> Result<CreditDefault, Error> {
    let c = f.cds()?;
    let r = &c.terms().reference;
    Ok(CreditDefault {
        entity: r.entity_id.clone(),
        seniority: r.seniority,
        document: r.document.clone(),
        accrual_day_fraction: n("0.25")?,
        rounding: CashRounding::NearestEven(2),
    })
}
fn credit_before_default(f: &Fixture, side: CdsSide) -> Result<LifecycleState, Error> {
    let mut s = credit_start(f, side)?;
    let inputs = LifecycleInputs {
        credit_rounding: Some(CashRounding::NearestEven(2)),
        ..Default::default()
    };
    s = apply(
        &s,
        "initial",
        "2024-06-19",
        LifecycleAction::ContractualCash,
        &inputs,
    )?
    .next;
    s = apply(
        &s,
        "premium.first",
        "2024-06-20",
        LifecycleAction::ContractualCash,
        &inputs,
    )?
    .next;
    Ok(s)
}
#[test]
fn default_retains_initial_and_earned_arrears_and_keeps_unknown_auction_and_cash_pending(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    let s = credit_before_default(&f, CdsSide::ProtectionBuyer)?;
    assert_eq!(
        amount(&s, CashKind::InitialCreditCash, 0),
        value(&expected()["cds"], "initial_cash")
    );
    assert_eq!(
        amount(&s, CashKind::Premium, 0),
        value(&expected()["cds"], "first_premium")
    );
    let a = apply(
        &s,
        "default",
        "2024-07-02",
        LifecycleAction::CreditDefault(default_event(&f)?),
        &Default::default(),
    )?;
    assert_eq!(a.next.view().status, EconomicStatus::DefaultPending);
    assert_eq!(a.next.view().obligations, s.view().obligations);
    assert!(a.next.view().recovery.is_none());
    let auction = RecoverySettlement {
        default_event: EventId::new("default")?,
        recovery_fraction: n("0.4")?,
        reference: pin("actual.auction")?,
        cash_date: d("2024-07-10")?,
    };
    let mut c = apply(
        &a.next,
        "auction",
        "2024-07-03",
        LifecycleAction::Recovery(auction),
        &Default::default(),
    )?
    .next;
    assert_eq!(c.view().status, EconomicStatus::DefaultCashPending);
    assert_eq!(
        amount(&c, CashKind::Recovery, 0),
        value(&expected()["cds"], "default_protection")
    );
    assert_eq!(
        amount(&c, CashKind::DefaultAccrual, 0),
        value(&expected()["cds"], "default_accrual")
    );
    assert!(apply(
        &c,
        "future.premium",
        "2024-09-20",
        LifecycleAction::ContractualCash,
        &Default::default()
    )
    .is_err());
    for (i, o) in c.view().obligations.into_iter().enumerate() {
        c = pay(&c, &format!("actual.{i}"), "2024-07-10", o.key, o.amount)?.next;
    }
    assert_eq!(c.view().status, EconomicStatus::Closed);
    Ok(())
}
#[test]
fn credit_sides_offset_and_zero_or_full_recovery_still_requires_explicit_settlement(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    for recovery in ["0", "0.4", "1"] {
        let mut results = vec![];
        for side in [CdsSide::ProtectionBuyer, CdsSide::ProtectionSeller] {
            let s = credit_before_default(&f, side)?;
            let s = apply(
                &s,
                "default",
                "2024-07-02",
                LifecycleAction::CreditDefault(default_event(&f)?),
                &Default::default(),
            )?
            .next;
            let s = apply(
                &s,
                "auction",
                "2024-07-03",
                LifecycleAction::Recovery(RecoverySettlement {
                    default_event: EventId::new("default")?,
                    recovery_fraction: n(recovery)?,
                    reference: pin("auction")?,
                    cash_date: d("2024-07-10")?,
                }),
                &Default::default(),
            )?
            .next;
            assert_eq!(s.view().status, EconomicStatus::DefaultCashPending);
            assert!(
                !s.view()
                    .obligations
                    .iter()
                    .find(|o| o.key.kind == CashKind::Recovery)
                    .unwrap()
                    .settled
            );
            results.push(s);
        }
        for o in results[0].view().obligations {
            let other = results[1]
                .view()
                .obligations
                .into_iter()
                .find(|c| c.key == o.key)
                .unwrap();
            assert_eq!(o.amount.value().decimal(), -other.amount.value().decimal());
        }
    }
    Ok(())
}
#[test]
fn wrong_credit_mapping_auction_identity_date_fraction_and_default_order_fail() -> Result<(), Error>
{
    let f = Fixture::load()?;
    let s = credit_before_default(&f, CdsSide::ProtectionBuyer)?;
    for which in 0..4 {
        let mut bad = default_event(&f)?;
        match which {
            0 => bad.entity = "wrong.entity".to_string().try_into()?,
            1 => bad.seniority = Seniority::Subordinated,
            2 => bad.document.version = Version::new(2)?,
            _ => bad.accrual_day_fraction = n("1.01")?,
        };
        assert!(apply(
            &s,
            "default",
            "2024-07-02",
            LifecycleAction::CreditDefault(bad),
            &Default::default()
        )
        .is_err());
    }
    let pending = apply(
        &s,
        "default",
        "2024-07-02",
        LifecycleAction::CreditDefault(default_event(&f)?),
        &Default::default(),
    )?
    .next;
    for which in 0..4 {
        let mut a = RecoverySettlement {
            default_event: EventId::new("default")?,
            recovery_fraction: n("0.4")?,
            reference: pin("auction")?,
            cash_date: d("2024-07-10")?,
        };
        match which {
            0 => a.default_event = EventId::new("another.default")?,
            1 => a.recovery_fraction = n("-0.1")?,
            2 => a.cash_date = d("2024-07-01")?,
            _ => a.cash_date = d("2026-07-01")?,
        };
        assert!(apply(
            &pending,
            "auction",
            "2024-07-03",
            LifecycleAction::Recovery(a),
            &Default::default()
        )
        .is_err());
    }
    Ok(())
}
#[test]
fn bond_default_requires_actual_principal_only_mapping_and_preserves_matured_claims(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    let s = bond_cash_ready(&f)?;
    let mut terms = default_event(&f)?;
    terms.entity = "synthetic.bond.entity".to_string().try_into()?;
    terms.seniority = Seniority::SeniorUnsecured;
    terms.document = pin("synthetic.bond.recovery-principal-only")?;
    terms.accrual_day_fraction = n("0")?;
    let default = apply(
        &s,
        "bond.default",
        "2024-07-03",
        LifecycleAction::CreditDefault(terms),
        &Default::default(),
    )?;
    assert!(default.effects.iter().any(|effect| matches!(effect,
        LifecycleEffect::PositionChanged { before, after, .. }
        if *before == n("1200").unwrap() && *after == n("0").unwrap())));
    let pending = default.next;
    let auction = apply(
        &pending,
        "recovery",
        "2024-07-04",
        LifecycleAction::Recovery(RecoverySettlement {
            default_event: EventId::new("bond.default")?,
            recovery_fraction: n("0.4")?,
            reference: pin("bond.auction")?,
            cash_date: d("2024-07-10")?,
        }),
        &Default::default(),
    )?
    .next;
    assert_eq!(amount(&auction, CashKind::Recovery, 0), Decimal::from(480));
    assert_eq!(amount(&auction, CashKind::Principal, 0), Decimal::from(800));
    assert_eq!(
        amount(&auction, CashKind::BondCoupon, 0),
        amount(&s, CashKind::BondCoupon, 0)
    );
    Ok(())
}
#[test]
fn zero_coupon_uses_full_absolute_redemption_and_rejects_infallible_invalid_shape(
) -> Result<(), Error> {
    let bond = ZeroCouponBond::new("synthetic.zero", d("2024-07-02")?, Currency::USD)
        .with_issue_date(d("2024-01-02")?)
        .with_face_value(Decimal::from(2500));
    let s = LifecycleState::new(
        TradeId::new("zero")?,
        Version::new(1)?,
        LifecycleInstrument::ZeroCoupon {
            bond: Box::new(bond),
            credit: None,
        },
        LifecyclePosition::BondFace(n("-5000")?),
        at("2024-01-02")?,
        vec![],
    )?;
    let s = apply(
        &s,
        "redemption",
        "2024-07-02",
        LifecycleAction::ContractStep,
        &Default::default(),
    )?
    .next;
    assert_eq!(amount(&s, CashKind::Principal, 0), Decimal::from(-5000));
    assert_eq!(s.view().status, EconomicStatus::Matured);
    let bad = ZeroCouponBond::new("bad", d("2024-07-02")?, Currency::USD);
    assert!(LifecycleState::new(
        TradeId::new("bad")?,
        Version::new(1)?,
        LifecycleInstrument::ZeroCoupon {
            bond: Box::new(bad),
            credit: None
        },
        LifecyclePosition::BondFace(n("1")?),
        at("2024-07-02")?,
        vec![]
    )
    .is_err());
    Ok(())
}
#[test]
fn immutable_history_order_and_native_position_units_are_checked_at_ingress() -> Result<(), Error> {
    let f = Fixture::load()?;
    for quantity in ["0", "1000000000000000001"] {
        assert!(LifecycleState::new(
            TradeId::new("b")?,
            Version::new(1)?,
            LifecycleInstrument::Bond {
                contract: Box::new(f.bond("bond")?),
                credit: None
            },
            LifecyclePosition::BondFace(n(quantity)?),
            at("2024-01-02")?,
            vec![]
        )
        .is_err());
    }
    assert!(LifecycleState::new(
        TradeId::new("b")?,
        Version::new(1)?,
        LifecycleInstrument::Bond {
            contract: Box::new(f.bond("bond")?),
            credit: None
        },
        LifecyclePosition::TradeCopies(n("1")?),
        at("2024-01-02")?,
        vec![]
    )
    .is_err());
    let index = IndexId::new("USD.TERM")?;
    let future = f.observation(&index, d("2024-02-02")?, "0.03", 1)?;
    assert!(LifecycleState::new(
        TradeId::new("b")?,
        Version::new(1)?,
        LifecycleInstrument::Bond {
            contract: Box::new(f.bond("frn")?),
            credit: None
        },
        LifecyclePosition::BondFace(n("1000")?),
        at("2024-01-02")?,
        vec![future]
    )
    .is_err());
    Ok(())
}
#[test]
fn events_roundtrip_exact_money_and_reject_unknown_terms_units_and_invalid_ids() -> Result<(), Error>
{
    let f = Fixture::load()?;
    let s = bond_state(&f)?;
    let original = event(&s, "event.1", "2024-04-02", LifecycleAction::ContractStep)?;
    let mut wire = serde_json::to_value(&original)?;
    assert_eq!(
        serde_json::from_value::<LifecycleEvent>(wire.clone())?,
        original
    );
    wire["unexpected"] = json!(true);
    assert!(serde_json::from_value::<LifecycleEvent>(wire).is_err());
    assert!(EventId::new(" bad").is_err());
    assert!(EventId::new("x".repeat(129)).is_err());
    assert!(serde_json::from_value::<LifecycleAction>(
        json!({"kind":"contract_step","terms":{"invented_rate":5}})
    )
    .is_err());
    Ok(())
}
#[test]
fn futures_daily_variation_advances_cleared_level_but_never_double_books_cash() -> Result<(), Error>
{
    use convex_analytics::futures::*;
    let p = futures::RatePack::new(&futures::fixture()["rates"]["cases"][0]);
    let opening = futures::mark(&p.contract, "100", "2026-10-07", FuturesMarkKind::Trade);
    let mut s = LifecycleState::new(
        TradeId::new("rate.future")?,
        Version::new(1)?,
        LifecycleInstrument::Futures {
            contract: Box::new(p.contract.clone()),
            opening_mark: Box::new(opening),
        },
        LifecyclePosition::FuturesContracts(-3),
        at("2026-10-07")?,
        vec![],
    )?;
    let context = convex_core::futures::EligibilityContext {
        as_of: at("2026-10-07")?,
        known_at: at("2026-10-07")?,
    };
    let inputs = LifecycleInputs {
        futures: Some(FuturesLifecycleInputs {
            clearing: &p.clearing,
            calendar: FuturesCalendar {
                stamp: &p.calendar_stamp,
                calendar: &p.calendar,
            },
            context,
        }),
        ..Default::default()
    };
    let first = futures::mark(
        &p.contract,
        "99.970009",
        "2026-10-07",
        FuturesMarkKind::DailySettlement,
    );
    let r = apply(
        &s,
        "first.margin",
        "2026-10-07",
        LifecycleAction::FuturesSettlement(first.clone()),
        &inputs,
    )?;
    s = r.next;
    let native = match &r.effects[1] {
        LifecycleEffect::FuturesVariation(n) => n,
        _ => return Err("native report missing".into()),
    };
    let independent = expected();
    let independent = &independent["futures"];
    assert_eq!(
        native.previous_contract_value.value().decimal(),
        value(independent, "previous_rounded_contract")
    );
    assert_eq!(
        native.current_contract_value.value().decimal(),
        value(independent, "current_rounded_contract")
    );
    assert_eq!(
        amount(&s, CashKind::VariationMargin, 0),
        value(independent, "variation")
    );
    assert!(!s.view().obligations[0].settled);
    assert!(apply(
        &s,
        "duplicate.margin",
        "2026-10-07",
        LifecycleAction::FuturesSettlement(first.clone()),
        &inputs
    )
    .is_err());
    // A supplied opening clearing level keeps its official precision. It is
    // neither rounded to an order tick nor treated as already received cash.
    let cleared = LifecycleState::new(
        TradeId::new("rate.future")?,
        Version::new(1)?,
        LifecycleInstrument::Futures {
            contract: Box::new(p.contract.clone()),
            opening_mark: Box::new(first),
        },
        LifecyclePosition::FuturesContracts(-3),
        at("2026-10-07")?,
        vec![],
    )?;
    assert!(cleared.view().obligations.is_empty());
    let context = convex_core::futures::EligibilityContext {
        as_of: at("2026-10-08")?,
        known_at: at("2026-10-08")?,
    };
    let inputs = LifecycleInputs {
        futures: Some(FuturesLifecycleInputs {
            clearing: &p.clearing,
            calendar: FuturesCalendar {
                stamp: &p.calendar_stamp,
                calendar: &p.calendar,
            },
            context,
        }),
        ..Default::default()
    };
    s = apply(
        &s,
        "second.margin",
        "2026-10-08",
        LifecycleAction::FuturesSettlement(futures::mark(
            &p.contract,
            "99.98",
            "2026-10-08",
            FuturesMarkKind::DailySettlement,
        )),
        &inputs,
    )?
    .next;
    let total = s
        .view()
        .obligations
        .iter()
        .map(|o| o.amount.value().decimal())
        .sum::<Decimal>();
    assert_eq!(total, Decimal::from(150));
    assert!(s.view().obligations.iter().all(|o| !o.settled));
    let restarted = apply(
        &cleared,
        "following.margin",
        "2026-10-08",
        LifecycleAction::FuturesSettlement(s.view().futures_mark.unwrap()),
        &inputs,
    )?
    .next;
    assert_eq!(
        amount(&restarted, CashKind::VariationMargin, 0),
        n("-74.94")?.decimal()
    );
    Ok(())
}
#[test]
fn official_final_rate_future_closes_only_after_each_actual_margin_receipt() -> Result<(), Error> {
    use convex_analytics::futures::*;
    let p = futures::RatePack::new(&futures::fixture()["rates"]["cases"][0]);
    let opening = futures::mark(&p.contract, "100", "2026-10-07", FuturesMarkKind::Trade);
    let s = LifecycleState::new(
        TradeId::new("final.rate.future")?,
        Version::new(1)?,
        LifecycleInstrument::Futures {
            contract: Box::new(p.contract.clone()),
            opening_mark: Box::new(opening),
        },
        LifecyclePosition::FuturesContracts(2),
        at("2026-10-07")?,
        vec![],
    )?;
    let context = convex_core::futures::EligibilityContext {
        as_of: at("2026-12-16")?,
        known_at: at("2026-12-16")?,
    };
    let inputs = LifecycleInputs {
        futures: Some(FuturesLifecycleInputs {
            clearing: &p.clearing,
            calendar: FuturesCalendar {
                stamp: &p.calendar_stamp,
                calendar: &p.calendar,
            },
            context,
        }),
        ..Default::default()
    };
    let r = apply(
        &s,
        "final.mark",
        "2026-12-16",
        LifecycleAction::FuturesSettlement(futures::mark(
            &p.contract,
            "96",
            "2026-12-16",
            FuturesMarkKind::FinalSettlement,
        )),
        &inputs,
    )?;
    let mut s = r.next;
    assert_eq!(s.view().status, EconomicStatus::Matured);
    let o = s.view().obligations[0].clone();
    assert_eq!(o.amount, cash("-20000")?);
    s = pay(&s, "actual.final.cash", "2026-12-16", o.key, o.amount)?.next;
    assert_eq!(s.view().status, EconomicStatus::Closed);
    Ok(())
}
#[test]
fn actual_bond_delivery_retains_published_factor_native_accrued_invoice_and_separate_asset_receipt(
) -> Result<(), Error> {
    use convex_analytics::futures::*;
    let mut p = futures::BondPack::new();
    // New explicit synthetic reference revision available through delivery. Original
    // published capture/expiry and every factor/identity fixture remain unchanged.
    p.contract.stamp.version = Version::new(2)?;
    p.contract.stamp.valid_until = at("2027-01-01")?;
    p.contract.stamp.quality = convex_core::futures::ReferenceQuality::Synthetic;
    p.contract.stamp.evidence =
        "Explicit synthetic revision2 delivery availability control, no market admission".into();
    p.basket.stamp.version = Version::new(2)?;
    p.basket.stamp.valid_until = at("2027-01-01")?;
    p.basket.stamp.quality = convex_core::futures::ReferenceQuality::Synthetic;
    p.basket.stamp.evidence = p.contract.stamp.evidence.clone();
    p.basket.contract.version = Version::new(2)?;
    if let convex_core::futures::FuturesSettlement::Bond { delivery, .. } =
        &mut p.contract.settlement
    {
        delivery.basket.version = Version::new(2)?;
    }
    let opening = futures::mark(&p.contract, "100", "2026-10-07", FuturesMarkKind::Trade);
    let mut s = LifecycleState::new(
        TradeId::new("physical.bond.future")?,
        Version::new(1)?,
        LifecycleInstrument::Futures {
            contract: Box::new(p.contract.clone()),
            opening_mark: Box::new(opening),
        },
        LifecyclePosition::FuturesContracts(-2),
        at("2026-10-07")?,
        vec![],
    )?;
    let clearing = futures::rules(&p.contract, None);
    let context = convex_core::futures::EligibilityContext {
        as_of: at("2026-12-08")?,
        known_at: at("2026-12-08")?,
    };
    let calendar = FuturesCalendar {
        stamp: &p.calendar_stamp,
        calendar: &p.calendar,
    };
    let inputs = LifecycleInputs {
        futures: Some(FuturesLifecycleInputs {
            clearing: &clearing,
            calendar,
            context,
        }),
        ..Default::default()
    };
    s = apply(
        &s,
        "last.margin",
        "2026-12-08",
        LifecycleAction::FuturesSettlement(futures::mark(
            &p.contract,
            "100",
            "2026-12-08",
            FuturesMarkKind::DailySettlement,
        )),
        &inputs,
    )?
    .next;
    let members = p.deliverables();
    let selected = &members[0];
    // Physical settlement can follow last trading. The invoice uses actual
    // delivery accrual/date while the official price remains the last mark.
    let last_context = convex_core::futures::EligibilityContext {
        as_of: at("2026-12-21")?,
        known_at: at("2026-12-21")?,
    };
    let last = apply(
        &s,
        "last.trading.margin",
        "2026-12-21",
        LifecycleAction::FuturesSettlement(futures::mark(
            &p.contract,
            "100",
            "2026-12-21",
            FuturesMarkKind::DailySettlement,
        )),
        &LifecycleInputs {
            futures: Some(FuturesLifecycleInputs {
                clearing: &clearing,
                calendar,
                context: last_context,
            }),
            ..Default::default()
        },
    )?
    .next;
    let delivery_action = LifecycleAction::BondDelivery {
        deliverable: selected.security.instrument.clone(),
        invoice_price_points: n("100")?,
        notice: pin("actual.synthetic.late.notice")?,
    };
    for date in ["2026-12-22", "2027-01-01"] {
        let delivery_inputs = LifecycleInputs {
            delivery: Some(BondDeliveryInputs {
                basket: &p.basket,
                security: selected.security,
                calendar,
                context: convex_core::futures::EligibilityContext {
                    as_of: at(date)?,
                    known_at: at(date)?,
                },
            }),
            ..Default::default()
        };
        let outcome = apply(
            &last,
            "dated.delivery",
            date,
            delivery_action.clone(),
            &delivery_inputs,
        );
        if date == "2026-12-22" {
            let late = outcome?.next;
            let claim = late
                .view()
                .obligations
                .into_iter()
                .find(|o| o.key.kind == CashKind::DeliveryInvoice)
                .ok_or("invoice absent")?;
            assert_eq!(claim.due_date, d(date)?);
            assert!(claim.amount.value().decimal() > Decimal::from(92564) * Decimal::from(2));
            assert_eq!(late.view().delivery_pending.unwrap().2, n("-200000")?);
        } else {
            assert!(matches!(
                outcome.unwrap_err().downcast_ref::<LifecycleError>(),
                Some(LifecycleError::Invalid {
                    field: "delivery.context",
                    ..
                })
            ));
        }
    }
    let inputs = LifecycleInputs {
        delivery: Some(BondDeliveryInputs {
            basket: &p.basket,
            security: selected.security,
            calendar,
            context,
        }),
        ..Default::default()
    };
    s = apply(
        &s,
        "actual.delivery",
        "2026-12-08",
        LifecycleAction::BondDelivery {
            deliverable: selected.security.instrument.clone(),
            invoice_price_points: n("100")?,
            notice: pin("actual.synthetic.delivery.notice")?,
        },
        &inputs,
    )?
    .next;
    let actual = futures::fixture();
    let expected_invoice = actual["bond"]["members"][0]["expected"]
        ["delivery_invoice_per_contract"]
        .as_str()
        .ok_or("missing original independent invoice")?;
    assert_eq!(
        amount(&s, CashKind::DeliveryInvoice, 0),
        expected_invoice.parse::<Decimal>()? * Decimal::from(2)
    );
    assert_eq!(s.view().status, EconomicStatus::Matured);
    let delivery = s.view().delivery_pending.ok_or("pending security absent")?;
    assert_eq!(delivery.2, n("-200000")?);
    for (i, o) in s.view().obligations.into_iter().enumerate() {
        s = pay(&s, &format!("cash.{i}"), "2026-12-08", o.key, o.amount)?.next;
    }
    assert_eq!(s.view().status, EconomicStatus::Matured);
    let wrong = LifecycleAction::SecurityReceipt {
        obligation: delivery.0.clone(),
        instrument: delivery.1.clone(),
        face: n("200000")?,
        receipt_id: EventId::new("asset.receipt")?,
        source: "actual.record".to_string().try_into()?,
    };
    assert!(apply(&s, "wrong.asset", "2026-12-08", wrong, &Default::default()).is_err());
    s = apply(
        &s,
        "actual.asset",
        "2026-12-08",
        LifecycleAction::SecurityReceipt {
            obligation: delivery.0,
            instrument: delivery.1,
            face: delivery.2,
            receipt_id: EventId::new("asset.receipt")?,
            source: "actual.record".to_string().try_into()?,
        },
        &Default::default(),
    )?
    .next;
    assert_eq!(s.view().status, EconomicStatus::Closed);
    Ok(())
}
#[test]
fn overnight_prefix_preserves_original_shift_and_lockout_weights_and_missing_history(
) -> Result<(), Error> {
    use convex_core::calendars::{DynamicCalendar, WeekendType};
    let calendar = DynamicCalendar::new("supplied weekend", WeekendType::SaturdaySunday);
    let schedule = OvernightSchedule::generate(
        d("2024-04-04")?,
        d("2024-04-11")?,
        convex_core::daycounts::DayCountConvention::Act360,
        &calendar,
        OvernightConvention {
            lookback_business_days: 2,
            observation_shift: true,
            lockout_business_days: 1,
        },
    )?;
    let prefix = schedule.prefix(d("2024-04-09")?, d("2024-04-05")?)?;
    assert!(prefix
        .observations()
        .iter()
        .all(|o| o.weight_end <= d("2024-04-05").unwrap()));
    assert_eq!(prefix.observations()[0], schedule.observations()[0]);
    assert!(schedule.prefix(d("2024-04-04")?, d("2024-04-05")?).is_err());
    assert!(schedule.prefix(d("2024-04-09")?, d("2024-05-01")?).is_err());
    let index = IndexId::new("USD.ON")?;
    let req = prefix.requirements(&index, |d| {
        Ok(UtcTimestamp::parse(&format!("{d}T08:00:00Z")).unwrap())
    })?;
    assert!(!req.is_empty());
    let context = FixingContext {
        valuation_date: d("2024-04-09")?,
        known_at: at("2024-04-09")?,
        unpublished: UnpublishedFixingPolicy::RequirePublished,
        final_only: true,
    };
    assert!(prefix
        .compound(
            &index,
            &FixingSeries::default(),
            context,
            |d| Ok(UtcTimestamp::parse(&format!("{d}T08:00:00Z")).unwrap()),
            |_, _| panic!("missing history must not invoke forecasts")
        )
        .is_err());
    Ok(())
}
#[test]
fn coupon_boundary_exercise_retains_full_earned_delayed_cash_and_does_not_pay_accrued_twice(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    let original = f.bond("bond")?;
    let inputs = LifecycleInputs {
        exercise_calendar: Some((&original.terms().calendars.exercise, &f.calendar)),
        ..Default::default()
    };
    let mut s = bond_state(&f)?;
    s = apply(
        &s,
        "rate",
        "2024-04-02",
        LifecycleAction::ContractStep,
        &inputs,
    )?
    .next;
    s = apply(
        &s,
        "notice",
        "2024-06-29",
        LifecycleAction::ExerciseNotice {
            provision: 0,
            exercise_date: d("2024-07-02")?,
        },
        &inputs,
    )?
    .next;
    assert!(apply(
        &s,
        "bad.exercise",
        "2024-07-02",
        LifecycleAction::Exercise {
            provision: 0,
            weight_end: None
        },
        &inputs
    )
    .is_err());
    s = apply(
        &s,
        "principal",
        "2024-07-02",
        LifecycleAction::ContractStep,
        &inputs,
    )?
    .next;
    s = apply(
        &s,
        "earned",
        "2024-07-02",
        LifecycleAction::ContractualCash,
        &inputs,
    )?
    .next;
    s = apply(
        &s,
        "exercise",
        "2024-07-02",
        LifecycleAction::Exercise {
            provision: 0,
            weight_end: None,
        },
        &inputs,
    )?
    .next;
    assert_eq!(amount(&s, CashKind::Principal, 0), Decimal::from(800));
    assert_eq!(
        amount(&s, CashKind::BondCoupon, 0),
        value(&expected()["bond"], "coupon_1") * Decimal::from(2)
    );
    assert_eq!(amount(&s, CashKind::Exercise, 0), Decimal::from(1212));
    assert_eq!(s.view().obligations.len(), 3);
    Ok(())
}
#[test]
fn callable_overnight_exercise_uses_only_published_original_shifted_prefix() -> Result<(), Error> {
    let f = Fixture::load()?;
    let mut t = f.bond("bond")?.terms().clone();
    let start = d("2024-04-04")?;
    let end = d("2024-04-11")?;
    t.issue_date = start;
    t.face_value = Decimal::from(1000);
    t.schedule.start_date = start;
    t.schedule.end_date = end;
    t.schedule.frequency = convex_core::types::Frequency::Monthly;
    t.schedule.stub_type = StubType::ShortLast;
    t.profiles = BondProfiles::constant(
        start,
        end,
        Decimal::ZERO,
        n("0.001")?.decimal(),
        n("1.25")?.decimal(),
        Decimal::from(1000),
    );
    t.day_count = convex_core::daycounts::DayCountConvention::Act360;
    t.coupon = BondCoupon::Overnight(OvernightRateTerms {
        index_id: IndexId::new("USD.ON")?,
        convention: OvernightConvention {
            lookback_business_days: 2,
            observation_shift: true,
            lockout_business_days: 0,
        },
    });
    t.exercise[0].style = ExerciseStyle::European {
        date: d("2024-04-09")?,
    };
    let c = t.build()?;
    let index = IndexId::new("USD.ON")?;
    let dates = [d("2024-04-02")?, d("2024-04-03")?, d("2024-04-04")?];
    let hist = dates
        .iter()
        .map(|&date| f.observation(&index, date, "0.05", 1))
        .collect::<Result<Vec<_>, _>>()?;
    let pubs = f.publications(&index, &dates)?;
    let mut s = LifecycleState::new(
        TradeId::new("overnight.call")?,
        Version::new(1)?,
        LifecycleInstrument::Bond {
            contract: Box::new(c.clone()),
            credit: None,
        },
        LifecyclePosition::BondFace(n("1000")?),
        at("2024-04-04")?,
        hist,
    )?;
    let inputs = LifecycleInputs {
        publications: Some(&pubs),
        exercise_calendar: Some((&c.terms().calendars.exercise, &f.calendar)),
        ..Default::default()
    };
    s = apply(
        &s,
        "notice",
        "2024-04-07",
        LifecycleAction::ExerciseNotice {
            provision: 0,
            exercise_date: d("2024-04-09")?,
        },
        &inputs,
    )?
    .next;
    assert!(apply(
        &s,
        "missing.cutoff",
        "2024-04-09",
        LifecycleAction::Exercise {
            provision: 0,
            weight_end: None
        },
        &inputs
    )
    .is_err());
    s = apply(
        &s,
        "prefix.exercise",
        "2024-04-09",
        LifecycleAction::Exercise {
            provision: 0,
            weight_end: Some(d("2024-04-05")?),
        },
        &inputs,
    )?
    .next;
    let daily = Decimal::ONE + n("0.05")?.decimal() / Decimal::from(360);
    let factor = daily * daily * daily;
    let accrued = CashRounding::NearestEven(2).apply(
        Decimal::from(1000)
            * (n("1.25")?.decimal() * (factor - Decimal::ONE)
                + n("0.001")?.decimal() * Decimal::from(5) / Decimal::from(360)),
    )?;
    assert_eq!(
        amount(&s, CashKind::Exercise, 0),
        Decimal::from(1010) + accrued
    );
    Ok(())
}
#[test]
fn default_on_premium_roll_keeps_previous_earned_coupon_and_separate_actual_day_fraction(
) -> Result<(), Error> {
    let f = Fixture::load()?;
    let mut s = credit_start(&f, CdsSide::ProtectionBuyer)?;
    let inputs = LifecycleInputs {
        credit_rounding: Some(CashRounding::NearestEven(2)),
        ..Default::default()
    };
    s = apply(
        &s,
        "upfront",
        "2024-06-19",
        LifecycleAction::ContractualCash,
        &inputs,
    )?
    .next;
    let pending = apply(
        &s,
        "default.on.roll",
        "2024-06-20",
        LifecycleAction::CreditDefault(default_event(&f)?),
        &inputs,
    )?
    .next;
    assert_eq!(
        amount(&pending, CashKind::Premium, 0),
        value(&expected()["cds"], "first_premium")
    );
    assert_eq!(pending.view().status, EconomicStatus::DefaultPending);
    let auction = apply(
        &pending,
        "auction",
        "2024-06-21",
        LifecycleAction::Recovery(RecoverySettlement {
            default_event: EventId::new("default.on.roll")?,
            recovery_fraction: n("0.4")?,
            reference: pin("auction")?,
            cash_date: d("2024-06-24")?,
        }),
        &inputs,
    )?
    .next;
    assert_eq!(
        amount(&auction, CashKind::DefaultAccrual, 0),
        n("-6.94")?.decimal()
    );
    assert_eq!(auction.view().obligations.len(), 4);
    Ok(())
}
