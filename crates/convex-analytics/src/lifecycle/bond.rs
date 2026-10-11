//! Original profile, principal, coupon and exercise economics.
use super::state::Required;
use super::*;
use convex_bonds::{
    contracts::{BondProjectionContext, ExerciseAccrued},
    traits::Bond,
};
use convex_core::cashflows::DatedValue;

impl LifecycleState {
    pub(super) fn contract_step(
        &mut self,
        e: &LifecycleEvent,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        if !self.required.remove(&(e.effective_date, Required::Step)) {
            return Err(invalid(
                "step",
                "no original unprocessed profile/principal boundary",
            ));
        }
        let instrument = self.instrument.clone();
        match instrument {
            LifecycleInstrument::Bond { contract, .. } => {
                let t = contract.terms();
                for (name, rows) in [
                    ("bond.coupon", &t.profiles.coupon),
                    ("bond.spread", &t.profiles.spread),
                    ("bond.gearing", &t.profiles.gearing),
                    ("bond.principal", &t.profiles.notional),
                ] {
                    steps(name, rows, e.effective_date, effects);
                }
                for (n, p) in contract
                    .principal_cashflows()
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.effective_date == e.effective_date)
                {
                    self.propose(
                        e,
                        ObligationKey {
                            kind: CashKind::Principal,
                            effective_date: p.effective_date,
                            ordinal: n as u32,
                        },
                        p.payment_date,
                        self.scale(p.amount)?,
                        effects,
                    )?;
                    let before = self.scale(p.principal.before)?;
                    let after = self.scale(p.principal.after)?;
                    effects.push(LifecycleEffect::PositionChanged {
                        before: ExactDecimal::new(before),
                        after: ExactDecimal::new(after),
                        unit: self.position,
                    });
                    if p.is_redemption {
                        self.remaining = Decimal::ZERO;
                        self.status = EconomicStatus::Matured;
                    }
                }
            }
            LifecycleInstrument::ZeroCoupon { bond, .. } => {
                let before = self.signed_position()?;
                self.propose(
                    e,
                    ObligationKey {
                        kind: CashKind::Principal,
                        effective_date: e.effective_date,
                        ordinal: 0,
                    },
                    e.effective_date,
                    self.scale(bond.redemption_value())?,
                    effects,
                )?;
                self.remaining = Decimal::ZERO;
                self.status = EconomicStatus::Matured;
                effects.push(LifecycleEffect::PositionChanged {
                    before: ExactDecimal::new(before),
                    after: ExactDecimal::new(Decimal::ZERO),
                    unit: self.position,
                });
            }
            LifecycleInstrument::Swap(c) => super::rates::swap_steps(&c, e.effective_date, effects),
            _ => {
                return Err(LifecycleError::Unsupported(
                    "contract step is not applicable to this product",
                ))
            }
        }
        Ok(())
    }
    pub(super) fn contractual_cash(
        &mut self,
        e: &LifecycleEvent,
        inputs: &LifecycleInputs<'_>,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        if !self.required.remove(&(e.effective_date, Required::Cash)) {
            return Err(invalid(
                "cash",
                "no original unprocessed coupon/initial-cash boundary",
            ));
        }
        let instrument = self.instrument.clone();
        match instrument {
            LifecycleInstrument::Bond { contract, .. } => {
                // A maturity step may already have ended future exposure; earned
                // same-day coupons retain the pre-redemption remaining fraction.
                let old_remaining = self.remaining;
                if self.status == EconomicStatus::Matured && old_remaining.is_zero() {
                    self.remaining = Decimal::ONE;
                }
                let context = BondProjectionContext {
                    settlement: e.effective_date,
                    on_payment: PaymentDateEntitlement::Include,
                    fixing: self.context(e, true, UnpublishedFixingPolicy::RequirePublished),
                    overnight_accrual_cutoffs: Vec::new(),
                };
                let history = self.history_series()?;
                let cash = publications(inputs.publications, |p| {
                    contract.completed_cashflows(&context, &history, p)
                })?;
                for (n, c) in contract
                    .periods()
                    .enumerate()
                    .filter(|(_, p)| p.accrual_dates().1 == e.effective_date)
                {
                    let value = cash
                        .coupons
                        .iter()
                        .find(|p| {
                            p.accrual_start == c.accrual_dates().0
                                && p.accrual_end == c.accrual_dates().1
                        })
                        .ok_or_else(|| invalid("coupon", "original completed cash absent"))?;
                    self.propose(
                        e,
                        ObligationKey {
                            kind: CashKind::BondCoupon,
                            effective_date: e.effective_date,
                            ordinal: n as u32,
                        },
                        value.payment_date,
                        self.scale(value.amount)?,
                        effects,
                    )?;
                }
                self.remaining = old_remaining;
            }
            LifecycleInstrument::Swap(c) => self.swap_cash(e, &c, inputs, effects)?,
            LifecycleInstrument::Cds(c) => self.credit_cash(e, &c, inputs, effects)?,
            _ => {
                return Err(LifecycleError::Unsupported(
                    "scheduled coupon cash is not applicable to this product",
                ))
            }
        }
        Ok(())
    }
    fn exercise_dates(
        &self,
        provision: u32,
        date: Date,
        inputs: &LifecycleInputs<'_>,
    ) -> LifecycleResult<convex_bonds::contracts::ExerciseDates> {
        let LifecycleInstrument::Bond { contract, .. } = &self.instrument else {
            return Err(LifecycleError::Unsupported(
                "exercise requires full original bond exercise terms",
            ));
        };
        let (id, calendar) = inputs
            .exercise_calendar
            .ok_or_else(|| invalid("exercise.calendar", "actual bound calendar absent"))?;
        if id != &contract.terms().calendars.exercise {
            return Err(invalid(
                "exercise.calendar",
                "exact original resource differs",
            ));
        }
        let terms = contract
            .terms()
            .exercise
            .get(provision as usize)
            .ok_or_else(|| invalid("exercise.provision", "not contracted"))?;
        Ok(terms.dates(date, calendar)?)
    }
    pub(super) fn notice(
        &mut self,
        e: &LifecycleEvent,
        provision: u32,
        date: Date,
        inputs: &LifecycleInputs<'_>,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        self.active()?;
        check_date(date)?;
        let dates = self.exercise_dates(provision, date, inputs)?;
        if e.effective_date > dates.notice
            || e.effective_date > date
            || self.notices.contains_key(&(provision, date))
        {
            return Err(invalid("notice", "timely unique notice required"));
        }
        if !self.notices.is_empty() {
            return Err(invalid(
                "notice",
                "conflicting outstanding exercise requires explicit cancellation/replay",
            ));
        }
        self.notices.insert((provision, date), dates.clone());
        self.notice_event = Some(e.clone());
        effects.push(LifecycleEffect::ExerciseNotified {
            notice: dates.notice,
            exercise: dates.exercise,
            settlement: dates.settlement,
        });
        Ok(())
    }
    pub(super) fn exercise(
        &mut self,
        e: &LifecycleEvent,
        provision: u32,
        weight_end: Option<Date>,
        inputs: &LifecycleInputs<'_>,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        self.active()?;
        let dates = self.exercise_dates(provision, e.effective_date, inputs)?;
        if self.notices.get(&(provision, e.effective_date)) != Some(&dates) {
            return Err(invalid("exercise", "original timely notice required"));
        }
        if self
            .required
            .first()
            .is_some_and(|(d, _)| *d == e.effective_date)
        {
            return Err(invalid(
                "exercise.order",
                "retain completed coupon/principal before exercise",
            ));
        }
        let LifecycleInstrument::Bond { contract, .. } = self.instrument.clone() else {
            return Err(LifecycleError::Unsupported("exercise product"));
        };
        let t = &contract.terms().exercise[provision as usize];
        let redemption = arithmetic(
            contract
                .outstanding_principal(e.effective_date, BoundarySide::After)?
                .checked_mul(t.price_per_100)
                .and_then(|a| a.checked_div(Decimal::from(100))),
            "exercise redemption",
        )?;
        let accrued = if t.accrued == ExerciseAccrued::PayAccrued {
            let history = self.history_series()?;
            publications(inputs.publications, |p| {
                contract.exercise_accrued(
                    e.effective_date,
                    weight_end,
                    &history,
                    self.context(e, true, UnpublishedFixingPolicy::RequirePublished),
                    p,
                )
            })?
        } else {
            if weight_end.is_some() {
                return Err(invalid(
                    "exercise.weight_end",
                    "unused included-accrued cutoff",
                ));
            }
            Decimal::ZERO
        };
        let cash = contract.terms().rounding.apply(arithmetic(
            redemption.checked_add(accrued),
            "exercise cash",
        )?)?;
        let before = self.signed_position()?;
        self.propose(
            e,
            ObligationKey {
                kind: CashKind::Exercise,
                effective_date: e.effective_date,
                ordinal: provision,
            },
            dates.settlement,
            self.scale(cash)?,
            effects,
        )?;
        self.remaining = Decimal::ZERO;
        self.status = EconomicStatus::Exercised;
        self.required.clear();
        self.notices.clear();
        effects.push(LifecycleEffect::PositionChanged {
            before: ExactDecimal::new(before),
            after: ExactDecimal::new(Decimal::ZERO),
            unit: self.position,
        });
        Ok(())
    }
}
pub(super) fn steps(
    name: &str,
    rows: &[DatedValue],
    date: Date,
    effects: &mut Vec<LifecycleEffect>,
) {
    for pair in rows.windows(2).filter(|p| p[1].start == date) {
        effects.push(LifecycleEffect::ProfileStep {
            profile: name.to_owned(),
            before: ExactDecimal::new(pair[0].value),
            after: ExactDecimal::new(pair[1].value),
        });
    }
}
pub(super) fn publications<T, E: Into<LifecycleError>>(
    schedule: Option<&crate::workflow::PublicationSchedule>,
    f: impl FnOnce(
        &mut dyn FnMut(
            &convex_core::contracts::IndexId,
            Date,
        ) -> Result<UtcTimestamp, CashflowError>,
    ) -> Result<T, E>,
) -> LifecycleResult<T> {
    let mut missing = None;
    let mut callback = |i: &convex_core::contracts::IndexId, d: Date| {
        let value = match schedule {
            Some(s) => s.get(i, d),
            None => Err(crate::workflow::WorkflowError::MissingInput(vec![
                crate::workflow::MissingInput::Publication(i.clone(), d),
            ])),
        };
        value.map_err(|err| {
            missing = Some(err);
            CashflowError::InvalidTerms {
                field: "publication",
                reason: "actual administrator decision absent".into(),
            }
        })
    };
    let result = f(&mut callback);
    if let Some(err) = missing {
        return Err(err.into());
    }
    result.map_err(Into::into)
}
