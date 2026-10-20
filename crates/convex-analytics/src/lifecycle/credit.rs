//! Actual credit event, pending auction and separately recorded recovery cash.
use super::state::{DefaultClaim, Required};
use super::*;
use convex_bonds::traits::Bond;
use convex_credit::{CdsContract, CdsSide};

fn denominator(c: &CdsContract) -> Decimal {
    if c.terms().premium_day_count == convex_core::daycounts::DayCountConvention::Act360 {
        Decimal::from(360)
    } else {
        Decimal::from(365)
    }
}
fn buyer_sign(c: &CdsContract) -> Decimal {
    if c.terms().side == CdsSide::ProtectionBuyer {
        Decimal::ONE
    } else {
        -Decimal::ONE
    }
}
fn premium(c: &CdsContract, days: Decimal) -> LifecycleResult<Decimal> {
    let t = c.terms();
    arithmetic(
        t.notional
            .decimal()
            .checked_mul(t.coupon_decimal.decimal())
            .and_then(|v| v.checked_mul(days))
            .and_then(|v| v.checked_div(denominator(c))),
        "credit premium",
    )
}
impl LifecycleState {
    pub(super) fn credit_cash(
        &mut self,
        e: &LifecycleEvent,
        c: &CdsContract,
        inputs: &LifecycleInputs<'_>,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        self.active()?;
        let rounding=inputs.credit_rounding.ok_or_else(||invalid("credit.rounding","actual cash precision must be supplied; pricing precision is not a posting convention"))?;
        rounding.apply(Decimal::ZERO)?;
        let t = c.terms();
        let sign = buyer_sign(c);
        if t.trade_date == e.effective_date {
            if let Some(upfront) = t.initial_clean_upfront_fraction {
                let days = c
                    .premium_periods()
                    .iter()
                    .find(|p| p.accrual_start <= t.step_in_date && t.step_in_date < p.accrual_end)
                    .map_or(0, |p| p.accrual_start.days_between(&t.step_in_date));
                let refund = premium(c, Decimal::from(days))?;
                let cash = arithmetic(
                    t.notional
                        .decimal()
                        .checked_mul(upfront.decimal())
                        .and_then(|u| u.checked_sub(refund)),
                    "initial credit cash",
                )?;
                let cash = rounding.apply(self.scale(arithmetic(
                    cash.checked_mul(-sign),
                    "initial cash direction",
                )?)?)?;
                self.propose(
                    e,
                    ObligationKey {
                        kind: CashKind::InitialCreditCash,
                        effective_date: e.effective_date,
                        ordinal: 0,
                    },
                    t.cash_settlement_date,
                    cash,
                    effects,
                )?;
            }
        }
        for (n, p) in c
            .premium_periods()
            .iter()
            .enumerate()
            .filter(|(_, p)| p.payment_date == e.effective_date)
        {
            let amount = arithmetic(
                premium(
                    c,
                    Decimal::from(p.accrual_start.days_between(&p.accrual_end)),
                )?
                .checked_mul(-sign),
                "premium direction",
            )?;
            self.propose(
                e,
                ObligationKey {
                    kind: CashKind::Premium,
                    effective_date: e.effective_date,
                    ordinal: n as u32,
                },
                p.payment_date,
                rounding.apply(self.scale(amount)?)?,
                effects,
            )?;
        }
        if !self.required.iter().any(|(_, r)| *r == Required::Cash) {
            self.remaining = Decimal::ZERO;
            self.status = EconomicStatus::Matured;
        }
        Ok(())
    }
    pub(super) fn default(
        &mut self,
        e: &LifecycleEvent,
        d: &CreditDefault,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        self.active()?;
        let fraction = d.accrual_day_fraction.decimal();
        if fraction < Decimal::ZERO || fraction > Decimal::ONE {
            return Err(invalid(
                "default.day_fraction",
                "explicit contractual fraction in[0,1] required",
            ));
        }
        d.rounding.apply(Decimal::ZERO)?;
        let (principal,accrual)=match &self.instrument {
            LifecycleInstrument::Cds(c)=>{
                let t=c.terms();
                if d.entity!=t.reference.entity_id || d.seniority!=t.reference.seniority || d.document!=t.reference.document || e.effective_date<t.protection_start_date || e.effective_date>t.premium_schedule.end_date { return Err(invalid("default.legal","exact eligible original entity/priority/document/protection dates required")); }
                let period=c.premium_periods().iter().enumerate().find(|(_,p)|p.accrual_start<=e.effective_date&&e.effective_date<p.accrual_end).ok_or_else(||invalid("default.period","original active premium period absent"))?;
                let key=ObligationKey { kind:CashKind::Premium,effective_date:period.1.payment_date,ordinal:period.0 as u32 };
                if self.obligations.contains_key(&key) { return Err(invalid("default.order","active-period scheduled premium already crystallized; replay default before conditional survival payment")); }
                let accrued=if t.pay_accrual_on_default { arithmetic(premium(c,arithmetic(Decimal::from(period.1.accrual_start.days_between(&e.effective_date)).checked_add(fraction),"default elapsed days")?)?.checked_mul(-buyer_sign(c)),"default accrued direction")? } else { Decimal::ZERO };
                (self.scale(arithmetic(t.notional.decimal().checked_mul(buyer_sign(c)),"protection direction")?)?,self.scale(accrued)?)
            },
            LifecycleInstrument::Bond { contract,credit }=>{
                require_bond_credit(d,credit.as_ref())?;
                if e.effective_date<contract.terms().schedule.start_date || e.effective_date>=contract.terms().schedule.end_date || self.required.contains(&(e.effective_date,Required::Step)) || self.required.contains(&(e.effective_date,Required::Cash)) { return Err(invalid("default.order","eligible bond life and crystallized same-day principal/coupon required")); }
                (self.scale(contract.outstanding_principal(e.effective_date,BoundarySide::After)?)?,Decimal::ZERO)
            },
            LifecycleInstrument::ZeroCoupon { bond,credit }=>{ require_bond_credit(d,credit.as_ref())?; if e.effective_date>=bond.maturity_date() { return Err(invalid("default.date","before original maturity required")); } (self.scale(bond.face_value())?,Decimal::ZERO) },
            _=>return Err(LifecycleError::Unsupported("credit default requires native single-name CDS or admitted principal-recovery bond mapping")),
        };
        // A premium earned before the default cutoff remains a claim, even
        // when its scheduled cash boundary is today or later. Current-period
        // default accrual is a separate obligation and cannot absorb arrears.
        if let LifecycleInstrument::Cds(c) = self.instrument.clone() {
            for (n, p) in c
                .premium_periods()
                .iter()
                .enumerate()
                .filter(|(_, p)| p.accrual_end <= e.effective_date)
            {
                let key = ObligationKey {
                    kind: CashKind::Premium,
                    effective_date: p.payment_date,
                    ordinal: n as u32,
                };
                if !self.obligations.contains_key(&key) {
                    let amount = arithmetic(
                        premium(
                            &c,
                            Decimal::from(p.accrual_start.days_between(&p.accrual_end)),
                        )?
                        .checked_mul(-buyer_sign(&c)),
                        "earned premium direction",
                    )?;
                    self.propose(
                        e,
                        key,
                        p.payment_date,
                        d.rounding.apply(self.scale(amount)?)?,
                        effects,
                    )?;
                }
            }
        }
        let before = self.signed_position()?;
        self.default_claim = Some(DefaultClaim {
            event: e.event_id.clone(),
            date: e.effective_date,
            principal,
            accrual,
            rounding: d.rounding,
            recovery: None,
        });
        self.remaining = Decimal::ZERO;
        self.status = EconomicStatus::DefaultPending;
        self.required.clear();
        self.notices.clear();
        effects.push(LifecycleEffect::DefaultPending(e.event_id.clone()));
        effects.push(LifecycleEffect::PositionChanged {
            before: ExactDecimal::new(before),
            after: ExactDecimal::new(Decimal::ZERO),
            unit: self.position,
        });
        Ok(())
    }
    pub(super) fn recovery(
        &mut self,
        e: &LifecycleEvent,
        r: &RecoverySettlement,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        let claim = self
            .default_claim
            .clone()
            .ok_or_else(|| invalid("recovery", "original unresolved default absent"))?;
        let fraction = r.recovery_fraction.decimal();
        if self.status != EconomicStatus::DefaultPending
            || claim.recovery.is_some()
            || r.default_event != claim.event
            || fraction < Decimal::ZERO
            || fraction > Decimal::ONE
            || r.cash_date < e.effective_date
            || claim.date.days_between(&r.cash_date) > 366
        {
            return Err(invalid("recovery","actual matching original default, bounded fractional result and future/current cash date required"));
        }
        let recovered = match self.instrument {
            LifecycleInstrument::Cds(_) => Decimal::ONE - fraction,
            _ => fraction,
        };
        let cash = claim.rounding.apply(arithmetic(
            claim.principal.checked_mul(recovered),
            "recovery cash",
        )?)?;
        self.propose(
            e,
            ObligationKey {
                kind: CashKind::Recovery,
                effective_date: claim.date,
                ordinal: 0,
            },
            r.cash_date,
            cash,
            effects,
        )?;
        if matches!(self.instrument, LifecycleInstrument::Cds(_)) {
            self.propose(
                e,
                ObligationKey {
                    kind: CashKind::DefaultAccrual,
                    effective_date: claim.date,
                    ordinal: 0,
                },
                r.cash_date,
                claim.rounding.apply(claim.accrual)?,
                effects,
            )?;
        }
        let pending = self
            .default_claim
            .as_mut()
            .ok_or_else(|| invalid("default", "original default absent"))?;
        pending.recovery = Some(r.clone());
        self.status = EconomicStatus::DefaultCashPending;
        effects.push(LifecycleEffect::RecoveryKnown(r.clone()));
        Ok(())
    }
}
fn require_bond_credit(d: &CreditDefault, credit: Option<&BondCreditTerms>) -> LifecycleResult<()> {
    let c=credit.ok_or(LifecycleError::Unsupported("bond default requires admitted entity/priority/document and outstanding-principal-only recovery terms"))?;
    if d.entity != c.entity
        || d.seniority != c.seniority
        || d.document != c.document
        || !d.accrual_day_fraction.decimal().is_zero()
    {
        return Err(invalid(
            "bond.default",
            "exact principal-only mapping with no invented coupon recovery required",
        ));
    }
    Ok(())
}
