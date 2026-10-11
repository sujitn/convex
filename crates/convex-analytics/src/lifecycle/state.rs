//! Checked economic state bound to complete original native contracts.
use super::*;
use convex_bonds::{contracts::BondContract, instruments::ZeroCouponBond, traits::Bond};
use convex_core::contracts::IndexId;
use convex_credit::{CdsContract, ReferencePin};
use serde::Serialize;

/// Closed owned original checked product. Deserializing cached computed state is
/// deliberately unavailable; rebuild native terms and replay supplied events.
#[derive(Debug, Clone)]
pub enum LifecycleInstrument {
    /// Full fixed/term/overnight stepped/amortizing/optional bond economics.
    Bond {
        /// Actual checked contract.
        contract: Box<BondContract>,
        /// Optional admitted outstanding-principal-only recovery mapping.
        credit: Option<BondCreditTerms>,
    },
    /// Full actual zero-coupon/bill face and absolute redemption.
    ZeroCoupon {
        /// Original complete native bond; ingress rechecks economic bounds.
        bond: Box<ZeroCouponBond>,
        /// Optional actual principal-recovery mapping.
        credit: Option<BondCreditTerms>,
    },
    /// Complete native original IRS/OIS economics and checked schedules.
    Swap(Box<convex_rates::SwapContract>),
    /// Complete original native legal CDS confirmation and premium periods.
    Cds(Box<CdsContract>),
    /// Complete actual dated exchange contract and supplied opening trade mark.
    Futures {
        /// Exact dated native reference facts.
        contract: Box<convex_core::futures::FuturesContract>,
        /// Actual initial trade/official mark, not an inferred zero baseline.
        opening_mark: Box<crate::futures::FuturesMark>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(super) enum Required {
    Step,
    Cash,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct DefaultClaim {
    pub event: EventId,
    pub date: Date,
    pub principal: Decimal,
    pub accrual: Decimal,
    pub rounding: CashRounding,
    pub recovery: Option<RecoverySettlement>,
}
/// Private immutable checked state; economic life is independent of payment.
#[derive(Debug, Clone)]
pub struct LifecycleState {
    pub(super) instrument: LifecycleInstrument,
    pub(super) trade_id: TradeId,
    pub(super) version: Version,
    pub(super) position: LifecyclePosition,
    pub(super) remaining: Decimal,
    pub(super) revision: u64,
    pub(super) effective_date: Date,
    pub(super) known_at: UtcTimestamp,
    pub(super) status: EconomicStatus,
    pub(super) required: BTreeSet<(Date, Required)>,
    pub(super) obligations: BTreeMap<ObligationKey, CashObligation>,
    pub(super) cash_proposals: BTreeMap<ObligationKey, LifecycleEvent>,
    pub(super) history: Vec<FixingObservation>,
    pub(super) events: BTreeSet<EventId>,
    pub(super) receipts: BTreeSet<EventId>,
    pub(super) notices: BTreeMap<(u32, Date), convex_bonds::contracts::ExerciseDates>,
    pub(super) notice_event: Option<LifecycleEvent>,
    pub(super) default_claim: Option<DefaultClaim>,
    pub(super) futures_mark: Option<crate::futures::FuturesMark>,
    pub(super) delivery_pending:
        Option<(ObligationKey, convex_core::futures::InstrumentPin, Decimal)>,
    pub(super) delivery_event: Option<LifecycleEvent>,
    pub(super) delivery_terms: Option<convex_bonds::contracts::BondTerms>,
    pub(super) security_receipt_events: Vec<LifecycleEvent>,
    pub(super) payment_receipt_events: Vec<LifecycleEvent>,
}
/// Exact public audit view. Original contract remains accessible separately;
/// this view cannot be decoded into an unchecked mutable economic state.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LifecycleStateView {
    /// Original economic identity.
    pub trade_id: TradeId,
    /// Immutable original trade terms revision.
    pub version: Version,
    /// Signed physical quantity and explicit native units.
    pub position: LifecyclePosition,
    /// Remaining contractual fraction; matured principal lives in obligations.
    pub remaining_fraction: ExactDecimal,
    /// Successful locally applied event count.
    pub revision: u64,
    /// Supplied last economic event date.
    pub effective_date: Date,
    /// Actual last supplied information cutoff.
    pub known_at: UtcTimestamp,
    /// Economic exposure and pending settlement status.
    pub status: EconomicStatus,
    /// Pending original economic boundaries, in deterministic date/kind order.
    pub pending_boundaries: Vec<(Date, String)>,
    /// Actual applied local event identities, in deterministic token order.
    pub event_ids: Vec<EventId>,
    /// Actual consumed external receipt identities, in deterministic token order.
    pub receipt_ids: Vec<EventId>,
    /// Outstanding provision, notice, exercise and cash dates.
    pub exercise_notices: Vec<(u32, Date, Date, Date)>,
    /// All original proposed cash, including partially/fully recorded receipts.
    pub obligations: Vec<CashObligation>,
    /// Actual source revisions in deterministic ingress order.
    pub fixings: Vec<FixingObservation>,
    /// Exact pending credit event; stays visible before/after auction information.
    pub default_event: Option<EventId>,
    /// Actual auction source and cash date; None is unknown recovery.
    pub recovery: Option<RecoverySettlement>,
    /// Actual latest cleared reference level, distinct from realized cash.
    pub futures_mark: Option<crate::futures::FuturesMark>,
    /// Security receipt remains unknown until separately supplied.
    pub delivery_pending: Option<(
        ObligationKey,
        convex_core::futures::InstrumentPin,
        ExactDecimal,
    )>,
}
/// A deterministic proposed local transition, never authoritative persistence.
#[derive(Debug, Clone)]
pub struct LifecycleTransition {
    /// Exact original event, including actual source/agreement/notice inputs.
    pub event: LifecycleEvent,
    /// Complete next checked economic state.
    pub next: LifecycleState,
    /// Separate proposed contractual and supplied recorded-cash effects.
    pub effects: Vec<LifecycleEffect>,
}
impl LifecycleState {
    /// Start before the original contract's first effect. Seasoned states are
    /// replayed from actual events; past payments are never inferred as paid.
    /// Futures alone have an explicit opening mark. Supplied history is checked
    /// by the original native series and must be available at this cutoff.
    pub fn new(
        trade_id: TradeId,
        version: Version,
        instrument: LifecycleInstrument,
        position: LifecyclePosition,
        known_at: UtcTimestamp,
        history: Vec<FixingObservation>,
    ) -> LifecycleResult<Self> {
        let (start, required) = prepare(&instrument, &trade_id, version, position)?;
        check_date(start)?;
        if known_at.datetime().date_naive() < start.as_naive_date()
            || history.iter().any(|h| h.known_at > known_at)
        {
            return Err(invalid(
                "known_at",
                "opening effects/history are not yet known",
            ));
        }
        FixingSeries::new(history.clone())?;
        let index = product_index(&instrument);
        if history.iter().any(|h| Some(&h.index_id) != index.as_ref()) {
            return Err(invalid("fixings", "unrelated product index"));
        }
        let futures_mark = match &instrument {
            LifecycleInstrument::Futures { opening_mark, .. } => {
                opening_mark.stamp.validate()?;
                if opening_mark.stamp.available_at > known_at {
                    return Err(invalid(
                        "futures.mark",
                        "opening mark unavailable at supplied cutoff",
                    ));
                }
                Some((**opening_mark).clone())
            }
            _ => None,
        };
        Ok(Self {
            instrument,
            trade_id,
            version,
            position,
            remaining: Decimal::ONE,
            revision: 0,
            effective_date: start,
            known_at,
            status: EconomicStatus::Active,
            required,
            obligations: BTreeMap::new(),
            cash_proposals: BTreeMap::new(),
            history,
            events: BTreeSet::new(),
            receipts: BTreeSet::new(),
            notices: BTreeMap::new(),
            notice_event: None,
            default_claim: None,
            futures_mark,
            delivery_pending: None,
            delivery_event: None,
            delivery_terms: None,
            security_receipt_events: Vec::new(),
            payment_receipt_events: Vec::new(),
        })
    }
    /// Exact immutable native product; no detached approximate economic terms.
    pub fn instrument(&self) -> &LifecycleInstrument {
        &self.instrument
    }
    /// Exact successfully applied actual notice. The contractual last-notice
    /// deadline in the audit view is distinct from this issuance event.
    pub fn exercise_notice(&self) -> Option<&LifecycleEvent> {
        self.notice_event.as_ref()
    }
    /// Successfully checked actual delivery, separate from model-selected CTD.
    pub fn delivery_event(&self) -> Option<&LifecycleEvent> {
        self.delivery_event.as_ref()
    }
    /// Complete original security terms checked by the actual invoice owner.
    pub fn delivered_bond_terms(&self) -> Option<&convex_bonds::contracts::BondTerms> {
        self.delivery_terms.as_ref()
    }
    /// Latest successfully checked actual security receipt, including partial face.
    pub fn security_receipt_event(&self) -> Option<&LifecycleEvent> {
        self.security_receipt_events.last()
    }
    /// Every full original checked security receipt, in actual replay order.
    pub fn security_receipt_events(&self) -> &[LifecycleEvent] {
        &self.security_receipt_events
    }
    /// Every full original checked cash receipt, in actual replay order.
    pub fn payment_receipt_events(&self) -> &[LifecycleEvent] {
        &self.payment_receipt_events
    }
    /// Original checked event that generated this exact earned cash obligation.
    pub fn cash_proposal_event(&self, key: &ObligationKey) -> Option<&LifecycleEvent> {
        self.cash_proposals.get(key)
    }
    /// Applied original event count without cloning the public audit view.
    pub fn applied_event_count(&self) -> usize {
        self.events.len()
    }
    /// Deterministic full audit projection; no implicit cash total or reporting FX.
    pub fn view(&self) -> LifecycleStateView {
        LifecycleStateView {
            trade_id: self.trade_id.clone(),
            version: self.version,
            position: self.position,
            remaining_fraction: ExactDecimal::new(self.remaining),
            revision: self.revision,
            effective_date: self.effective_date,
            known_at: self.known_at,
            status: self.status,
            pending_boundaries: self
                .required
                .iter()
                .map(|(d, k)| {
                    (
                        *d,
                        match k {
                            Required::Step => "contract_step",
                            Required::Cash => "earned_cash",
                        }
                        .to_owned(),
                    )
                })
                .collect(),
            event_ids: self.events.iter().cloned().collect(),
            receipt_ids: self.receipts.iter().cloned().collect(),
            exercise_notices: self
                .notices
                .iter()
                .map(|((n, _), d)| (*n, d.notice, d.exercise, d.settlement))
                .collect(),
            obligations: self.obligations.values().cloned().collect(),
            fixings: self.history.clone(),
            default_event: self.default_claim.as_ref().map(|c| c.event.clone()),
            recovery: self.default_claim.as_ref().and_then(|c| c.recovery.clone()),
            futures_mark: self.futures_mark.clone(),
            delivery_pending: self
                .delivery_pending
                .as_ref()
                .map(|(k, p, q)| (k.clone(), p.clone(), ExactDecimal::new(*q))),
        }
    }
    pub(super) fn currency(&self) -> Currency {
        match &self.instrument {
            LifecycleInstrument::Bond { contract, .. } => contract.terms().currency,
            LifecycleInstrument::ZeroCoupon { bond, .. } => bond.currency(),
            LifecycleInstrument::Swap(c) => c.terms().currency,
            LifecycleInstrument::Cds(c) => c.terms().currency,
            LifecycleInstrument::Futures { contract, .. } => contract.currency,
        }
    }
    pub(super) fn scale(&self, amount: Decimal) -> LifecycleResult<Decimal> {
        let (q, reference) = match (&self.instrument, self.position) {
            (LifecycleInstrument::Bond { contract, .. }, LifecyclePosition::BondFace(q)) => {
                (q.decimal(), contract.terms().face_value)
            }
            (LifecycleInstrument::ZeroCoupon { bond, .. }, LifecyclePosition::BondFace(q)) => {
                (q.decimal(), bond.face_value())
            }
            (LifecycleInstrument::Swap(_), LifecyclePosition::TradeCopies(q)) => {
                (q.decimal(), Decimal::ONE)
            }
            (LifecycleInstrument::Cds(c), LifecyclePosition::CdsNotional(q)) => {
                (q.decimal(), c.terms().notional.decimal())
            }
            (LifecycleInstrument::Futures { .. }, LifecyclePosition::FuturesContracts(q)) => {
                (Decimal::from(q), Decimal::ONE)
            }
            _ => return Err(invalid("position", "wrong original physical unit")),
        };
        arithmetic(
            amount
                .checked_mul(q)
                .and_then(|a| a.checked_mul(self.remaining))
                .and_then(|a| a.checked_div(reference)),
            "position cash",
        )
    }
    pub(super) fn signed_position(&self) -> LifecycleResult<Decimal> {
        if let LifecycleInstrument::Bond { contract, .. } = &self.instrument {
            return self
                .scale(contract.outstanding_principal(self.effective_date, BoundarySide::After)?);
        }
        let q = match self.position {
            LifecyclePosition::BondFace(q)
            | LifecyclePosition::TradeCopies(q)
            | LifecyclePosition::CdsNotional(q) => q.decimal(),
            LifecyclePosition::FuturesContracts(q) => Decimal::from(q),
        };
        arithmetic(q.checked_mul(self.remaining), "remaining position")
    }
    pub(super) fn check_event(&self, e: &LifecycleEvent) -> LifecycleResult<()> {
        check_date(e.effective_date)?;
        if e.trade_id != self.trade_id
            || e.expected_revision != self.revision
            || self.events.contains(&e.event_id)
            || e.effective_date < self.effective_date
            || e.known_at < self.known_at
            || e.known_at.datetime().date_naive() < e.effective_date.as_naive_date()
        {
            return Err(invalid("event", "exact trade/prior revision, unique identity and nondecreasing economic/known-at order required"));
        }
        if self.events.len() >= MAX_LIFECYCLE_ITEMS {
            return Err(invalid("events", "bounded state full"));
        }
        if self
            .required
            .first()
            .is_some_and(|(d, _)| *d < e.effective_date)
        {
            return Err(invalid(
                "event.order",
                "unprocessed original contractual boundary; replay it before advancing",
            ));
        }
        if self.status == EconomicStatus::Closed {
            return Err(invalid(
                "status",
                "closed state has no admissible economic event",
            ));
        }
        Ok(())
    }
    pub(super) fn active(&self) -> LifecycleResult<()> {
        if self.status == EconomicStatus::Active {
            Ok(())
        } else {
            Err(invalid("status", "remaining active economics required"))
        }
    }
    pub(super) fn history_series(&self) -> LifecycleResult<FixingSeries> {
        Ok(FixingSeries::new(self.history.clone())?)
    }
    pub(super) fn context(
        &self,
        e: &LifecycleEvent,
        final_only: bool,
        unpublished: UnpublishedFixingPolicy,
    ) -> FixingContext {
        FixingContext {
            valuation_date: e.effective_date,
            known_at: e.known_at,
            unpublished,
            final_only,
        }
    }
    pub(super) fn propose(
        &mut self,
        e: &LifecycleEvent,
        key: ObligationKey,
        due: Date,
        amount: Decimal,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        check_date(due)?;
        if due < key.effective_date
            || self.obligations.len() >= MAX_LIFECYCLE_ITEMS
            || self.obligations.contains_key(&key)
        {
            return Err(invalid(
                "obligation",
                "ordered original cash date, unique key and bounded state required",
            ));
        }
        let obligation = CashObligation {
            key: key.clone(),
            due_date: due,
            amount: money(amount, self.currency()),
            recorded: money(Decimal::ZERO, self.currency()),
            settled: false,
            generated_by: e.event_id.clone(),
        };
        self.cash_proposals.insert(key.clone(), e.clone());
        self.obligations.insert(key, obligation.clone());
        effects.push(LifecycleEffect::CashProposed(obligation));
        Ok(())
    }
    pub(super) fn fixing(
        &mut self,
        e: &LifecycleEvent,
        o: &FixingObservation,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        if product_index(&self.instrument).as_ref() != Some(&o.index_id)
            || o.known_at > e.known_at
            || o.observation_date > e.effective_date
        {
            return Err(invalid(
                "fixing",
                "actual product observation and available revision required",
            ));
        }
        let mut observations = self.history.clone();
        observations.push(o.clone());
        FixingSeries::new(observations.clone())?;
        self.history = observations;
        effects.push(LifecycleEffect::FixingApplied(o.clone()));
        Ok(())
    }
    pub(super) fn payment(
        &mut self,
        e: &LifecycleEvent,
        r: &PaymentReceipt,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        check_date(r.value_date)?;
        if self.receipts.len() >= MAX_LIFECYCLE_ITEMS
            || self.receipts.contains(&r.receipt_id)
            || r.value_date > e.effective_date
            || r.amount.currency() != self.currency()
        {
            return Err(invalid(
                "receipt",
                "unique identity, available value date and exact currency required",
            ));
        }
        let o = self
            .obligations
            .get_mut(&r.obligation)
            .ok_or_else(|| invalid("receipt", "no original proposed obligation"))?;
        let amount = r.amount.value().decimal();
        let original = o.amount.value().decimal();
        let paid = o.recorded.value().decimal();
        let next = arithmetic(paid.checked_add(amount), "recorded cash")?;
        if r.value_date < o.due_date
            || (amount.is_zero() && !original.is_zero())
            || o.settled
            || amount.is_sign_negative() != original.is_sign_negative()
            || next.abs() > original.abs()
        {
            return Err(invalid(
                "receipt",
                "contract due date, same signed direction and no overpayment required",
            ));
        }
        o.recorded = money(next, o.amount.currency());
        o.settled = next == original;
        self.receipts.insert(r.receipt_id.clone());
        self.payment_receipt_events.push(e.clone());
        effects.push(LifecycleEffect::CashRecorded(r.clone()));
        Ok(())
    }
    pub(super) fn close_if_settled(&mut self) {
        let cash_clear = self.obligations.values().all(|o| o.settled);
        if self.status != EconomicStatus::Active
            && self.status != EconomicStatus::DefaultPending
            && self.required.is_empty()
            && self.delivery_pending.is_none()
            && cash_clear
        {
            self.status = EconomicStatus::Closed;
        }
    }
    pub(super) fn terminate(
        &mut self,
        e: &LifecycleEvent,
        fraction: ExactDecimal,
        cash: &Money,
        cash_date: Date,
        _agreement: &ReferencePin,
        effects: &mut Vec<LifecycleEffect>,
    ) -> LifecycleResult<()> {
        self.active()?;
        let f = fraction.decimal();
        if f <= Decimal::ZERO
            || f > Decimal::ONE
            || cash.currency() != self.currency()
            || cash_date < e.effective_date
        {
            return Err(invalid(
                "termination",
                "explicit positive fraction <=1 and actual signed cash/date/currency required",
            ));
        }
        if self
            .required
            .first()
            .is_some_and(|(d, _)| *d == e.effective_date)
        {
            return Err(invalid(
                "termination.order",
                "crystallize same-day original steps/coupons before terminating",
            ));
        }
        if matches!(self.instrument, LifecycleInstrument::Futures { .. }) {
            return Err(LifecycleError::Unsupported(
                "futures position changes require supplied fills and original mark accounting",
            ));
        }
        if f < Decimal::ONE {
            let boundary = match &self.instrument {
                LifecycleInstrument::Swap(c) => {
                    c.fixed_schedule()
                        .periods()
                        .iter()
                        .any(|p| p.accrual_dates().0 == e.effective_date)
                        && c.floating_coupons()
                            .iter()
                            .any(|p| p.period().accrual_dates().0 == e.effective_date)
                }
                _ => false,
            };
            if !boundary {
                return Err(LifecycleError::Unsupported("partial termination requires a joint IRS/OIS accrual boundary; mid-period residual terms must be amended explicitly"));
            }
        }
        let before = self.signed_position()?;
        self.propose(
            e,
            ObligationKey {
                kind: CashKind::Termination,
                effective_date: e.effective_date,
                ordinal: 0,
            },
            cash_date,
            cash.value().decimal(),
            effects,
        )?;
        self.remaining = arithmetic(
            self.remaining.checked_mul(Decimal::ONE - f),
            "terminated fraction",
        )?;
        if self.remaining.is_zero() {
            self.status = EconomicStatus::Terminated;
            self.required.clear();
            self.notices.clear();
        }
        effects.push(LifecycleEffect::PositionChanged {
            before: ExactDecimal::new(before),
            after: ExactDecimal::new(self.signed_position()?),
            unit: self.position,
        });
        Ok(())
    }
}
pub(super) fn product_index(i: &LifecycleInstrument) -> Option<IndexId> {
    match i {
        LifecycleInstrument::Bond { contract, .. } => match &contract.terms().coupon {
            convex_bonds::contracts::BondCoupon::Fixed => None,
            convex_bonds::contracts::BondCoupon::Term(t) => Some(t.index_id.clone()),
            convex_bonds::contracts::BondCoupon::Overnight(t) => Some(t.index_id.clone()),
        },
        LifecycleInstrument::Swap(c) => Some(c.terms().floating.index_id().clone()),
        LifecycleInstrument::Futures { contract, .. } => match &contract.settlement {
            convex_core::futures::FuturesSettlement::Rate { accrual, .. } => match accrual {
                convex_core::futures::RateAccrual::Overnight { index, .. }
                | convex_core::futures::RateAccrual::Term { index, .. } => Some(index.clone()),
            },
            _ => None,
        },
        _ => None,
    }
}
fn profiles(
    rows: &[convex_core::cashflows::DatedValue],
    required: &mut BTreeSet<(Date, Required)>,
) {
    for r in rows.iter().skip(1) {
        required.insert((r.start, Required::Step));
    }
}
fn prepare(
    i: &LifecycleInstrument,
    trade: &TradeId,
    version: Version,
    position: LifecyclePosition,
) -> LifecycleResult<(Date, BTreeSet<(Date, Required)>)> {
    let mut required = BTreeSet::new();
    let start = match i {
        LifecycleInstrument::Bond { contract, .. } => {
            let t = contract.terms();
            if !matches!(position, LifecyclePosition::BondFace(_)) {
                return Err(invalid("position", "bond face required"));
            }
            for p in contract.periods() {
                if p.payment_date() < p.accrual_dates().1 {
                    return Err(LifecycleError::Unsupported(
                        "payment before earned coupon boundary needs explicit prepayment terms",
                    ));
                }
                required.insert((p.accrual_dates().1, Required::Cash));
            }
            for p in contract.principal_cashflows() {
                required.insert((p.effective_date, Required::Step));
            }
            for rows in [
                &t.profiles.coupon,
                &t.profiles.spread,
                &t.profiles.gearing,
                &t.profiles.notional,
            ] {
                profiles(rows, &mut required);
            }
            t.issue_date
        }
        LifecycleInstrument::ZeroCoupon { bond, .. } => {
            if !matches!(position, LifecyclePosition::BondFace(_))
                || bond.face_value() <= Decimal::ZERO
                || bond.redemption_value() <= Decimal::ZERO
                || bond.face_value() > Decimal::from(1_000_000_000_000_000_000_u64)
                || bond.redemption_value() > Decimal::from(10_000_000_000_000_000_000_u64)
                || bond.get_issue_date() >= bond.maturity_date()
            {
                return Err(invalid(
                    "zero_coupon",
                    "actual positive bounded face/redemption and distinct issue/maturity required",
                ));
            }
            required.insert((bond.maturity_date(), Required::Step));
            bond.get_issue_date()
        }
        LifecycleInstrument::Swap(c) => {
            let t = c.terms();
            if t.trade_id != *trade
                || t.version != version
                || !matches!(position, LifecyclePosition::TradeCopies(_))
            {
                return Err(invalid(
                    "swap",
                    "exact original identity/version and trade copies required",
                ));
            }
            for p in c
                .fixed_schedule()
                .periods()
                .iter()
                .chain(c.floating_coupons().iter().map(|p| p.period()))
            {
                if p.payment_date() < p.accrual_dates().1 {
                    return Err(LifecycleError::Unsupported(
                        "swap prepaid coupon requires separate complete agreement",
                    ));
                }
                required.insert((p.accrual_dates().1, Required::Cash));
            }
            profiles(&t.fixed.leg.notionals, &mut required);
            profiles(&t.fixed.rates, &mut required);
            profiles(&t.floating.leg().notionals, &mut required);
            profiles(&t.floating.profiles().spreads, &mut required);
            profiles(&t.floating.profiles().gearings, &mut required);
            t.effective_date
        }
        LifecycleInstrument::Cds(c) => {
            let t = c.terms();
            if t.trade_id != *trade
                || t.version != version
                || !matches!(position, LifecyclePosition::CdsNotional(_))
            {
                return Err(invalid(
                    "cds",
                    "exact original identity/version and notional required",
                ));
            }
            for p in c.premium_periods() {
                required.insert((p.payment_date, Required::Cash));
            }
            if t.initial_clean_upfront_fraction.is_some() {
                required.insert((t.trade_date, Required::Cash));
            }
            t.trade_date
        }
        LifecycleInstrument::Futures {
            contract,
            opening_mark,
        } => {
            contract.validate()?;
            if !matches!(position, LifecyclePosition::FuturesContracts(_))
                || opening_mark.contract.id != contract.instrument_id
                || opening_mark.contract.version != contract.stamp.version
                || opening_mark.kind == crate::futures::FuturesMarkKind::FinalSettlement
                || opening_mark.date < contract.listed_from
                || opening_mark.date > contract.last_trade
            {
                return Err(invalid(
                    "futures",
                    "exact unexpired original opening mark and integer contracts required",
                ));
            }
            if opening_mark.kind == crate::futures::FuturesMarkKind::Trade {
                contract.validate_order_units(opening_mark.date, 1, opening_mark.price_points)?;
            } else if matches!(
                contract.settlement,
                convex_core::futures::FuturesSettlement::Rate { .. }
            ) {
                contract.rate_from_price(opening_mark.price_points)?;
            } else if opening_mark.price_points.decimal() <= Decimal::ZERO
                || opening_mark.price_points.decimal() > Decimal::from(1000)
            {
                return Err(invalid(
                    "futures.mark",
                    "bond official points outside(0,1000]",
                ));
            }
            opening_mark.date.date()
        }
    };
    let amount = match position {
        LifecyclePosition::BondFace(q)
        | LifecyclePosition::TradeCopies(q)
        | LifecyclePosition::CdsNotional(q) => q.decimal(),
        LifecyclePosition::FuturesContracts(q) => Decimal::from(q),
    };
    if amount.is_zero()
        || amount.abs() > Decimal::from(1_000_000_000_000_000_000_u64)
        || matches!(position,LifecyclePosition::FuturesContracts(q) if q.unsigned_abs()>1_000_000_000)
        || required.len() > MAX_LIFECYCLE_ITEMS
    {
        return Err(invalid(
            "position/bounds",
            "nonzero bounded original quantity and work required",
        ));
    }
    if required.first().is_some_and(|(d, _)| *d < start) {
        return Err(invalid(
            "opening",
            "past economic effects require actual replay",
        ));
    }
    Ok((start, required))
}
