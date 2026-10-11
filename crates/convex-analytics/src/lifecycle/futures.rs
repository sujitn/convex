//! Original rounded variation and separately confirmed physical delivery.
use super::*;
use crate::futures::{futures_settlement_effect, FuturesMark, FuturesMarkKind};
use convex_core::futures::{FuturesSettlement, InstrumentPin};
use convex_credit::ReferencePin;

impl LifecycleState {
    pub(super) fn futures_settlement(
        &mut self,
        e: &LifecycleEvent,
        mark: &FuturesMark,
        inputs: &LifecycleInputs<'_>,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        self.active()?;
        let LifecycleInstrument::Futures { contract, .. } = &self.instrument else {
            return Err(LifecycleError::Unsupported(
                "variation margin requires exact dated futures",
            ));
        };
        let LifecyclePosition::FuturesContracts(count) = self.position else {
            return Err(invalid("position", "integer contracts required"));
        };
        let supplied = inputs
            .futures
            .ok_or_else(|| invalid("futures.inputs", "actual clearing/calendar/context absent"))?;
        if mark.date.date() != e.effective_date
            || supplied.context.known_at != e.known_at
            || supplied.context.as_of.datetime().date_naive() != e.effective_date.as_naive_date()
        {
            return Err(invalid(
                "futures.context",
                "actual event date and information cutoff must agree",
            ));
        }
        match &contract.settlement {
            FuturesSettlement::Rate {
                final_settlement, ..
            } if mark.date > *final_settlement => {
                return Err(invalid("settlement.date", "past original final settlement"))
            }
            FuturesSettlement::Bond { delivery, .. }
                if mark.date > delivery.last_delivery
                    || mark.kind == FuturesMarkKind::FinalSettlement =>
            {
                return Err(invalid(
                    "settlement.date",
                    "bond delivery requires explicit physical asset/invoice event",
                ))
            }
            _ => (),
        }
        let previous = self
            .futures_mark
            .as_ref()
            .ok_or_else(|| invalid("futures.mark", "original cleared baseline absent"))?;
        let effect = futures_settlement_effect(
            contract,
            supplied.clearing,
            supplied.calendar,
            previous,
            mark,
            count,
            supplied.context,
        )?;
        self.propose(
            e,
            ObligationKey {
                kind: CashKind::VariationMargin,
                effective_date: e.effective_date,
                ordinal: 0,
            },
            e.effective_date,
            effect.proposed_variation.value().decimal(),
            effects,
        )?;
        self.futures_mark = Some(mark.clone());
        if mark.kind == FuturesMarkKind::FinalSettlement {
            self.remaining = Decimal::ZERO;
            self.status = EconomicStatus::Matured;
        }
        effects.push(LifecycleEffect::FuturesVariation(Box::new(effect)));
        Ok(())
    }
    pub(super) fn deliver(
        &mut self,
        e: &LifecycleEvent,
        deliverable: &InstrumentPin,
        price: ExactDecimal,
        _notice: &ReferencePin,
        inputs: &LifecycleInputs<'_>,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        self.active()?;
        let LifecycleInstrument::Futures { contract, .. } = &self.instrument else {
            return Err(LifecycleError::Unsupported(
                "physical delivery requires a dated bond future",
            ));
        };
        let FuturesSettlement::Bond {
            contract_face,
            delivery,
        } = &contract.settlement
        else {
            return Err(LifecycleError::Unsupported(
                "rate futures have no deliverable basket or asset",
            ));
        };
        let input = inputs.delivery.ok_or_else(|| {
            invalid(
                "delivery.inputs",
                "full original native basket/security/calendar inputs absent",
            )
        })?;
        let mark = self
            .futures_mark
            .as_ref()
            .ok_or_else(|| invalid("delivery.mark", "original cleared mark absent"))?;
        if input.security.instrument != deliverable
            || input.security.stamp.id.as_str() != deliverable.id.as_str()
            || input.security.stamp.version != deliverable.version
            || input.security.bond.terms().currency != contract.currency
            || input.calendar.stamp.id != delivery.calendar.id
            || input.calendar.stamp.version != delivery.calendar.version
            || !input.calendar.calendar.is_business_day(e.effective_date)
            || input.context.known_at != e.known_at
            || input.context.as_of.datetime().date_naive() != e.effective_date.as_naive_date()
            || e.effective_date < delivery.first_delivery.date()
            || e.effective_date > delivery.last_delivery.date()
            || mark.date.date() != e.effective_date.min(contract.last_trade.date())
            || mark.kind != FuturesMarkKind::DailySettlement
            || mark.price_points != price
        {
            return Err(invalid("delivery.context","exact selected security/calendar/date/cutoff and official final settled invoice points required"));
        }
        for stamp in [
            &contract.stamp,
            &input.basket.stamp,
            input.security.stamp,
            input.calendar.stamp,
        ] {
            stamp.validate()?;
            if stamp.available_at > e.known_at
                || stamp.source_at > input.context.as_of
                || stamp.valid_until <= input.context.as_of
            {
                return Err(invalid(
                    "delivery.reference",
                    "actual available nonexpired immutable inputs required",
                ));
            }
        }
        input.security.bond.require_plain_valuation()?;
        let terms = input.security.bond.terms();
        if !matches!(terms.coupon, convex_bonds::contracts::BondCoupon::Fixed)
            || terms.schedule.end_date <= e.effective_date
            || terms.dates.ex_coupon.is_some()
            || !terms.principal_prices.is_empty()
            || terms.redemption_multiplier != Decimal::ONE
            || terms
                .profiles
                .notional
                .iter()
                .any(|v| v.value != terms.face_value)
        {
            return Err(LifecycleError::Unsupported(
                "delivery requires original native fixed nominal bullet invoice conventions",
            ));
        }
        let cash = input
            .security
            .bond
            .fixed_projection(e.effective_date, PaymentDateEntitlement::Exclude)?;
        let accrued = arithmetic(
            cash.accrued
                .checked_mul(contract_face.decimal())
                .and_then(|v| v.checked_div(terms.face_value)),
            "delivery accrued",
        )?
        .round_dp_with_strategy(
            contract.daily_settlement.cash_decimal_places,
            rust_decimal::RoundingStrategy::MidpointAwayFromZero,
        );
        let invoice = convex_core::futures::bond_delivery_invoice(
            contract,
            input.basket,
            deliverable,
            e.effective_date.try_into()?,
            price,
            ExactDecimal::new(accrued),
        )?;
        let LifecyclePosition::FuturesContracts(count) = self.position else {
            return Err(invalid("position", "integer contracts required"));
        };
        let face = arithmetic(
            contract_face.decimal().checked_mul(Decimal::from(count)),
            "delivered face",
        )?;
        let cash = arithmetic(
            invoice.decimal().checked_mul(-Decimal::from(count)),
            "delivery invoice",
        )?;
        let key = ObligationKey {
            kind: CashKind::DeliveryInvoice,
            effective_date: e.effective_date,
            ordinal: 0,
        };
        self.propose(e, key.clone(), e.effective_date, cash, effects)?;
        self.delivery_pending = Some((key.clone(), deliverable.clone(), face));
        self.delivery_event = Some(e.clone());
        self.delivery_terms = Some(input.security.bond.terms().clone());
        self.remaining = Decimal::ZERO;
        self.status = EconomicStatus::Matured;
        effects.push(LifecycleEffect::SecurityDelivery {
            instrument: deliverable.clone(),
            face: ExactDecimal::new(face),
            key,
        });
        Ok(())
    }
    pub(super) fn security_receipt(
        &mut self,
        e: &LifecycleEvent,
        key: &ObligationKey,
        instrument: &InstrumentPin,
        face: ExactDecimal,
        receipt: &EventId,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        if self.receipts.len() >= MAX_LIFECYCLE_ITEMS
            || self.receipts.contains(receipt)
            || self.delivery_pending.as_ref()
                != Some(&(key.clone(), instrument.clone(), face.decimal()))
        {
            return Err(invalid(
                "security.receipt",
                "unique actual complete receipt for original proposed security/face required",
            ));
        }
        self.receipts.insert(receipt.clone());
        self.delivery_pending = None;
        self.security_receipt_event = Some(e.clone());
        effects.push(LifecycleEffect::SecurityRecorded {
            key: key.clone(),
            receipt_id: receipt.clone(),
        });
        Ok(())
    }
}
