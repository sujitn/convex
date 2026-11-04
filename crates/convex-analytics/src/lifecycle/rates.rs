//! Original independent IRS/OIS leg cash, profile steps and history policies.
use super::*;
use super::{
    bond::{publications, steps},
    state::Required,
};
use convex_rates::{CashflowContext, SwapContract};

pub(super) fn swap_steps(c: &SwapContract, date: Date, effects: &mut Vec<LifecycleEffect>) {
    let t = c.terms();
    for (name, rows) in [
        ("fixed.notional", &t.fixed.leg.notionals),
        ("fixed.rate", &t.fixed.rates),
        ("floating.notional", &t.floating.leg().notionals),
        ("floating.spread", &t.floating.profiles().spreads),
        ("floating.gearing", &t.floating.profiles().gearings),
    ] {
        steps(name, rows, date, effects);
    }
}
impl LifecycleState {
    pub(super) fn swap_cash(
        &mut self,
        e: &LifecycleEvent,
        c: &SwapContract,
        inputs: &LifecycleInputs<'_>,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        let policy = c.terms().floating.fixing_policy();
        let context = CashflowContext {
            fixings: self.context(e, policy.final_only, policy.unpublished),
            on_payment: PaymentDateEntitlement::Include,
        };
        let history = self.history_series()?;
        let cash = publications(inputs.publications, |p| {
            c.completed_cashflows(context, &history, p)
        })?;
        for (kind, flows) in [
            (CashKind::FixedCoupon, &cash.fixed),
            (CashKind::FloatingCoupon, &cash.floating),
        ] {
            for f in flows {
                let ordinal = if kind == CashKind::FixedCoupon {
                    c.fixed_schedule()
                        .periods()
                        .iter()
                        .position(|p| p == &f.period)
                } else {
                    c.floating_coupons()
                        .iter()
                        .position(|p| p.period() == &f.period)
                }
                .ok_or_else(|| invalid("coupon", "original native period missing"))?;
                self.propose(
                    e,
                    ObligationKey {
                        kind,
                        effective_date: e.effective_date,
                        ordinal: ordinal as u32,
                    },
                    f.period.payment_date(),
                    self.scale(f.signed_amount)?,
                    effects,
                )?;
            }
        }
        if !self.required.iter().any(|(_, r)| *r == Required::Cash) {
            self.remaining = Decimal::ZERO;
            self.status = EconomicStatus::Matured;
        }
        Ok(())
    }
}
