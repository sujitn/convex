//! Explicit synthetic native terms used by the lifecycle example and fixtures.
#![allow(dead_code)]
use convex_analytics::{lifecycle::*, workflow::PublicationSchedule};
use convex_bonds::contracts::{BondContract, BondTerms};
use convex_core::{
    calendars::{Calendar, DynamicCalendar, WeekendType},
    cashflows::CashRounding,
    contracts::{ExactDecimal, IndexId, Money, SourceId, TradeId, UtcTimestamp, Version},
    fixings::{FixingObservation, FixingRequirement, FixingStatus},
    Currency, Date,
};
use convex_credit::{CdsCalendars, CdsContract, CdsTerms};
use convex_rates::{
    CalendarBinding, FloatingLegTerms, LegCalendars, SwapCalendars, SwapContract, SwapTerms,
};
use serde_json::{json, Value};
pub type Error = Box<dyn std::error::Error>;
pub fn d(s: &str) -> Result<Date, Error> {
    Ok(Date::parse(s)?)
}
pub fn n(s: &str) -> Result<ExactDecimal, Error> {
    Ok(ExactDecimal::parse(s)?)
}
pub fn at(s: &str) -> Result<UtcTimestamp, Error> {
    Ok(UtcTimestamp::parse(&format!("{s}T23:59:59Z"))?)
}
pub fn cash(s: &str) -> Result<Money, Error> {
    Ok(Money::new(n(s)?, Currency::USD))
}
pub fn pin(id: &str) -> Result<convex_credit::ReferencePin, Error> {
    Ok(convex_credit::ReferencePin {
        id: id.to_string().try_into()?,
        version: Version::new(1)?,
    })
}
pub struct Fixture {
    pub data: Value,
    pub calendar: DynamicCalendar,
}
impl Fixture {
    pub fn load() -> Result<Self, Error> {
        Ok(Self {
            data: serde_json::from_str(include_str!(
                "../../../../tests/fixtures/lifecycle/inputs.json"
            ))?,
            calendar: DynamicCalendar::new(
                "explicit synthetic weekend lifecycle resource",
                WeekendType::SaturdaySunday,
            ),
        })
    }
    pub fn bond(&self, name: &str) -> Result<BondContract, Error> {
        Ok(serde_json::from_value::<BondTerms>(self.data[name].clone())?.build()?)
    }
    pub fn swap(&self, name: &str) -> Result<SwapContract, Error> {
        let t: SwapTerms = serde_json::from_value(self.data[name].clone())?;
        let bind = |pin| CalendarBinding {
            pin,
            calendar: &self.calendar as &dyn Calendar,
        };
        let fixed = LegCalendars {
            accrual: bind(&t.fixed.leg.calendars.accrual),
            payment: bind(&t.fixed.leg.calendars.payment),
        };
        let floating = LegCalendars {
            accrual: bind(&t.floating.leg().calendars.accrual),
            payment: bind(&t.floating.leg().calendars.payment),
        };
        let (reset, fixing, observation) = match &t.floating {
            FloatingLegTerms::Term(f) => (
                Some(bind(&f.resets.reset_calendar)),
                Some(bind(&f.resets.fixing_calendar)),
                None,
            ),
            FloatingLegTerms::Overnight(f) => (None, None, Some(bind(&f.observation_calendar))),
        };
        let c = t.clone().build(&SwapCalendars {
            fixed,
            floating,
            reset,
            fixing,
            observation,
        })?;
        Ok(c)
    }
    pub fn cds(&self) -> Result<CdsContract, Error> {
        let t: CdsTerms = serde_json::from_value(self.data["cds"].clone())?;
        Ok(t.build(&CdsCalendars {
            premium: convex_credit::CalendarBinding {
                pin: &t.premium_calendar,
                calendar: &self.calendar,
            },
            settlement: convex_credit::CalendarBinding {
                pin: &t.settlement_calendar,
                calendar: &self.calendar,
            },
        })?)
    }
    pub fn publications(
        &self,
        index: &IndexId,
        dates: &[Date],
    ) -> Result<PublicationSchedule, Error> {
        Ok(PublicationSchedule::new(
            dates
                .iter()
                .map(|&date| {
                    Ok(FixingRequirement {
                        index_id: index.clone(),
                        observation_date: date,
                        expected_publication: UtcTimestamp::parse(&format!("{date}T08:00:00Z"))?,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?,
        )?)
    }
    pub fn observation(
        &self,
        index: &IndexId,
        date: Date,
        rate: &str,
        revision: u64,
    ) -> Result<FixingObservation, Error> {
        let ts = UtcTimestamp::parse(&format!("{date}T08:00:00Z"))?;
        Ok(FixingObservation {
            index_id: index.clone(),
            observation_date: date,
            rate: n(rate)?.decimal(),
            published_at: ts,
            known_at: ts,
            revision: Version::new(revision)?,
            status: FixingStatus::Final,
            source_id: SourceId::new("lifecycle.synthetic.administrator")?,
        })
    }
}
pub fn bond_state(f: &Fixture) -> Result<LifecycleState, Error> {
    Ok(LifecycleState::new(
        TradeId::new("lifecycle.synthetic.bond")?,
        Version::new(1)?,
        LifecycleInstrument::Bond {
            contract: Box::new(f.bond("bond")?),
            credit: Some(BondCreditTerms {
                entity: "synthetic.bond.entity".to_string().try_into()?,
                seniority: convex_credit::Seniority::SeniorUnsecured,
                document: pin("synthetic.bond.recovery-principal-only")?,
            }),
        },
        LifecyclePosition::BondFace(n("2000")?),
        at("2024-01-02")?,
        vec![],
    )?)
}
pub fn event(
    s: &LifecycleState,
    id: &str,
    date: &str,
    action: LifecycleAction,
) -> Result<LifecycleEvent, Error> {
    let v = s.view();
    Ok(LifecycleEvent {
        event_id: EventId::new(id)?,
        trade_id: v.trade_id,
        expected_revision: v.revision,
        effective_date: d(date)?,
        known_at: at(date)?,
        action,
    })
}
pub fn apply(
    s: &LifecycleState,
    id: &str,
    date: &str,
    action: LifecycleAction,
    inputs: &LifecycleInputs<'_>,
) -> Result<LifecycleTransition, Error> {
    Ok(transition(s, &event(s, id, date, action)?, inputs)?)
}
pub fn pay(
    s: &LifecycleState,
    id: &str,
    date: &str,
    key: ObligationKey,
    amount: Money,
) -> Result<LifecycleTransition, Error> {
    apply(
        s,
        id,
        date,
        LifecycleAction::Payment(PaymentReceipt {
            receipt_id: EventId::new(format!("receipt.{id}"))?,
            obligation: key,
            amount,
            value_date: d(date)?,
            source: SourceId::new("synthetic.externally-recorded-cash")?,
        }),
        &LifecycleInputs::default(),
    )
}
pub fn run() -> Result<Value, Error> {
    let f = Fixture::load()?;
    let empty = LifecycleInputs::default();
    let mut s = bond_state(&f)?;
    let mut phases = vec![s.view()];
    let mut effects = vec![];
    for (id, date, action) in [
        (
            "bond.step.rate",
            "2024-04-02",
            LifecycleAction::ContractStep,
        ),
        (
            "bond.amortization",
            "2024-07-02",
            LifecycleAction::ContractStep,
        ),
        (
            "bond.coupon1",
            "2024-07-02",
            LifecycleAction::ContractualCash,
        ),
    ] {
        let r = apply(&s, id, date, action, &empty)?;
        effects.extend(r.effects);
        s = r.next;
        phases.push(s.view());
    }
    for o in s.view().obligations {
        let r = pay(
            &s,
            &format!("pay.first.{:?}.{}", o.key.kind, o.key.ordinal),
            "2024-07-07",
            o.key,
            o.amount,
        )?;
        effects.extend(r.effects);
        s = r.next;
    }
    for (id, date, action) in [
        (
            "bond.redemption",
            "2025-01-02",
            LifecycleAction::ContractStep,
        ),
        (
            "bond.coupon2",
            "2025-01-02",
            LifecycleAction::ContractualCash,
        ),
    ] {
        let r = apply(&s, id, date, action, &empty)?;
        effects.extend(r.effects);
        s = r.next;
        phases.push(s.view());
    }
    for (i, o) in s
        .view()
        .obligations
        .into_iter()
        .filter(|o| !o.settled)
        .enumerate()
    {
        let r = pay(&s, &format!("pay.final.{i}"), "2025-01-07", o.key, o.amount)?;
        effects.extend(r.effects);
        s = r.next;
    }
    let mut c = LifecycleState::new(
        f.cds()?.terms().trade_id.clone(),
        Version::new(1)?,
        LifecycleInstrument::Cds(Box::new(f.cds()?)),
        LifecyclePosition::CdsNotional(n("1000000")?),
        at("2024-06-19")?,
        vec![],
    )?;
    let credit_inputs = LifecycleInputs {
        credit_rounding: Some(CashRounding::NearestEven(2)),
        ..Default::default()
    };
    c = apply(
        &c,
        "credit.original.upfront",
        "2024-06-19",
        LifecycleAction::ContractualCash,
        &credit_inputs,
    )?
    .next;
    c = apply(
        &c,
        "credit.premium1",
        "2024-06-20",
        LifecycleAction::ContractualCash,
        &credit_inputs,
    )?
    .next;
    let t = f.cds()?;
    let r = &t.terms().reference;
    c = apply(
        &c,
        "credit.actual.default",
        "2024-07-02",
        LifecycleAction::CreditDefault(CreditDefault {
            entity: r.entity_id.clone(),
            seniority: r.seniority,
            document: r.document.clone(),
            accrual_day_fraction: n("0.25")?,
            rounding: CashRounding::NearestEven(2),
        }),
        &empty,
    )?
    .next;
    let default_pending = c.view();
    c = apply(
        &c,
        "credit.actual.auction",
        "2024-07-03",
        LifecycleAction::Recovery(RecoverySettlement {
            default_event: EventId::new("credit.actual.default")?,
            recovery_fraction: n("0.4")?,
            reference: pin("synthetic.actual-auction-input")?,
            cash_date: d("2024-07-10")?,
        }),
        &empty,
    )?
    .next;
    let auction_pending = c.view();
    for (i, o) in c.view().obligations.into_iter().enumerate() {
        c = pay(
            &c,
            &format!("credit.pay.{i}"),
            "2024-07-10",
            o.key,
            o.amount,
        )?
        .next;
    }
    Ok(
        json!({"task":"CVX-062","certified":false,"qualification":"Synthetic supplied events, original native economic transitions. The library proposes effects and consumes explicit recorded receipts; it never posts a ledger or authorizes cash movement.","bond_phases":phases,"bond_effects":effects,"bond_closed":s.view(),"credit_default_pending":default_pending,"credit_auction_cash_pending":auction_pending,"credit_closed":c.view()}),
    )
}
