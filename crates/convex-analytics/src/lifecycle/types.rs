//! Exact event, economic status and proposed effect contracts.
use super::*;
use convex_core::contracts::{EntityId, SourceId};
use convex_credit::{ReferencePin, Seniority};
use serde::{Deserialize, Serialize};

/// Validated local economic event/receipt identity, distinct from a market observation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(convex_core::contracts::ObservationId);
impl EventId {
    /// Admit a bounded1-128ASCII token using the canonical identity validator.
    pub fn new(value: impl Into<String>) -> Result<Self, convex_core::contracts::ContractError> {
        Ok(Self(convex_core::contracts::ObservationId::new(value)?))
    }
    /// Exact original opaque token; no normalization or latest lookup.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Closed product position units; native trade direction is scaled only once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "unit",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LifecyclePosition {
    /// Signed face in the bond currency.
    BondFace(ExactDecimal),
    /// Signed copies of the full native swap's original direction/notionals.
    TradeCopies(ExactDecimal),
    /// Signed notional multiplying the full CDS's original protection direction.
    CdsNotional(ExactDecimal),
    /// Signed integer futures, positive long.
    FuturesContracts(i64),
}
/// Contractual life and cash settlement are independent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EconomicStatus {
    /// Remaining original contractual exposure.
    Active,
    /// Exercise terminated future exposure; redemption cash remains separate.
    Exercised,
    /// Explicit agreement terminated all future exposure.
    Terminated,
    /// Contractual maturity has occurred, including delayed receivables/payables.
    Matured,
    /// Protection ended, recovery information remains absent.
    DefaultPending,
    /// Actual auction information supplied; cash still remains pending.
    DefaultCashPending,
    /// No future exposure, unresolved recovery or outstanding obligations.
    Closed,
}
/// Exact source/destination of an obligation; never a generic cash balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CashKind {
    /// Bond's full original coupon.
    BondCoupon,
    /// Effective principal reduction or final redemption.
    Principal,
    /// Native signed fixed leg coupon.
    FixedCoupon,
    /// Native signed floating leg coupon.
    FloatingCoupon,
    /// Contractual CDS premium, conditional on confirmed survival to payment.
    Premium,
    /// Original contractual upfront less original accrued refund.
    InitialCreditCash,
    /// Supplied exercise redemption and its separate partial accrued treatment.
    Exercise,
    /// Explicit agreed termination cash.
    Termination,
    /// Bond principal recovery or CDS protection claim.
    Recovery,
    /// Earned default premium, separate from protection cash.
    DefaultAccrual,
    /// Original native normal-futures rounded variation cash.
    VariationMargin,
    /// Official physically delivered security invoice.
    DeliveryInvoice,
}
/// Stable local obligation key bound to one immutable trade state.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObligationKey {
    /// Economic cash category.
    pub kind: CashKind,
    /// Original coupon/principal/event boundary, not receipt time.
    pub effective_date: Date,
    /// Native leg/period/provision ordinal; no approximate date matching.
    pub ordinal: u32,
}
/// Proposed receivable/payable; payment is represented only by supplied receipts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CashObligation {
    /// Original immutable key.
    pub key: ObligationKey,
    /// Actual original contractual payment date.
    pub due_date: Date,
    /// Signed exact contract-currency cash.
    pub amount: Money,
    /// Total actual supplied receipts applied to this obligation.
    pub recorded: Money,
    /// Explicit completion; even a zero claim needs a supplied settlement record.
    pub settled: bool,
    /// Proposal event identity, independent of payment receipt identities.
    pub generated_by: EventId,
}
/// Exact immutable legal bond credit mapping supplied separately from bond math.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BondCreditTerms {
    /// Actual reference entity, not inferred from a ticker.
    pub entity: EntityId,
    /// Obligation priority.
    pub seniority: Seniority,
    /// Admitted recovery document revision. This scoped route recovers outstanding
    /// principal only; coupon recovery, acceleration and other conventions fail.
    pub document: ReferencePin,
}
/// Explicit actual credit event facts; no assumed auction price or settlement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreditDefault {
    /// Exact supplied legal reference entity.
    pub entity: EntityId,
    /// Exact native/supplied priority.
    pub seniority: Seniority,
    /// Exact default document revision.
    pub document: ReferencePin,
    /// Actual contractual elapsed fraction of default day, in `[0,1]`. This is
    /// distinct from the standard model's expected half-day pricing integration.
    pub accrual_day_fraction: ExactDecimal,
    /// Explicit default-cash precision, not a pricing tolerance.
    pub rounding: CashRounding,
}
/// Actual externally supplied auction/recovery terms; these do not assert payment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverySettlement {
    /// Exact default event this auction resolves.
    pub default_event: EventId,
    /// Actual fractional recovery, including 0 and 1.
    pub recovery_fraction: ExactDecimal,
    /// Actual immutable source revision of the result.
    pub reference: ReferencePin,
    /// Actual contractual cash date, not inferred from a clock/calendar.
    pub cash_date: Date,
}
/// Supplied recorded cash, separate from economic proposal generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaymentReceipt {
    /// Immutable external receipt identity, locally unique to the state.
    pub receipt_id: EventId,
    /// Exact original obligation to settle.
    pub obligation: ObligationKey,
    /// Signed cash actually recorded externally, including partial payment.
    pub amount: Money,
    /// Actual value date, never earlier than the original contracted due date.
    pub value_date: Date,
    /// Original actual source of the payment record.
    pub source: SourceId,
}
/// Pure supplied event payload. These events do not authorize ledger writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "terms",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LifecycleAction {
    /// Ingest one actual fixing revision for this product's index.
    Fixing(FixingObservation),
    /// Apply all actual profile/principal changes on this exact day.
    ContractStep,
    /// Crystallize complete earned coupons/initial cash on this exact boundary.
    ContractualCash,
    /// Apply a supplied actual cash receipt to an existing obligation.
    Payment(PaymentReceipt),
    /// Actual security receipt/delivery, separate from the proposed asset effect.
    SecurityReceipt {
        /// Original proposed asset key.
        obligation: ObligationKey,
        /// Actual received/delivered security revision.
        instrument: convex_core::futures::InstrumentPin,
        /// Signed actual face; this initial route requires full delivery.
        face: ExactDecimal,
        /// Unique externally recorded receipt identity.
        receipt_id: EventId,
        /// Actual record source.
        source: SourceId,
    },
    /// Record permitted call/put notice; no price/model chooses exercise.
    ExerciseNotice {
        /// Exact original provision ordinal.
        provision: u32,
        /// Exact contracted future exercise date.
        exercise_date: Date,
    },
    /// Apply the already notified actual exercise and original cash conventions.
    Exercise {
        /// Exact original provision ordinal.
        provision: u32,
        /// Explicit observation-weight prefix for shifted overnight accrual.
        weight_end: Option<Date>,
    },
    /// Supplied agreement terminates future exposure and specifies signed cash.
    /// Previously crystallized arrears are retained; omitted residual cash fails.
    Terminate {
        /// Fraction of remaining economic exposure to terminate, in (0,1].
        fraction: ExactDecimal,
        /// Actual signed cash for this position, including explicit zero.
        cash: Money,
        /// Actual agreed cash date.
        cash_date: Date,
        /// Immutable agreement revision; no replacement price is inferred.
        agreement: ReferencePin,
    },
    /// Supplied default ends protection; actual recovery remains unknown.
    CreditDefault(CreditDefault),
    /// Actual auction/recovery terms create pending cash claims.
    Recovery(RecoverySettlement),
    /// Apply the original native rounded variation margin to an official mark.
    FuturesSettlement(crate::futures::FuturesMark),
    /// Supplied actual physical delivery choice; no CTD model chooses the asset.
    BondDelivery {
        /// Actual immutable selected basket member.
        deliverable: convex_core::futures::InstrumentPin,
        /// Official invoice futures points (separate from daily settled mark).
        invoice_price_points: ExactDecimal,
        /// Actual final exchange delivery notice revision.
        notice: ReferencePin,
    },
}
/// Ordered immutable economic input, with explicit knowledge cutoff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleEvent {
    /// Stable event identity, locally unique; authoritative dedup is the engine's.
    pub event_id: EventId,
    /// Exact original trade identity.
    pub trade_id: TradeId,
    /// Exact prior transition count; next state advances once on success.
    pub expected_revision: u64,
    /// Supplied economic date; cannot move backwards or skip required boundaries.
    pub effective_date: Date,
    /// Supplied information cutoff; cannot move backwards or precede effects.
    pub known_at: UtcTimestamp,
    /// Original actual event economics.
    pub action: LifecycleAction,
}
/// Every proposed effect retains its separate economic meaning.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum LifecycleEffect {
    /// Proposed payable/receivable, never posted cash.
    CashProposed(CashObligation),
    /// Actual externally recorded receipt applied locally, not written to a DB.
    CashRecorded(PaymentReceipt),
    /// Native original independently dated profile step.
    ProfileStep {
        /// Exact product/leg/profile name.
        profile: String,
        /// Original principal/rate/gearing/spread before the boundary.
        before: ExactDecimal,
        /// Original value after the boundary.
        after: ExactDecimal,
    },
    /// Exact original quantity exposure changed without cash inference.
    PositionChanged {
        /// Signed native position before the event.
        before: ExactDecimal,
        /// Signed native position after the event.
        after: ExactDecimal,
        /// Original product unit, retained in state.
        unit: LifecyclePosition,
    },
    /// Original actual fixing revision; forecast is never an event observation.
    FixingApplied(FixingObservation),
    /// Notification retains distinct notice, exercise and redemption dates.
    ExerciseNotified {
        /// Original last notice date.
        notice: Date,
        /// Contractual exercise date.
        exercise: Date,
        /// Original adjusted redemption date.
        settlement: Date,
    },
    /// Default claim has no fabricated recovery amount.
    DefaultPending(EventId),
    /// Original recovery source/precision/date, with cash still pending.
    RecoveryKnown(RecoverySettlement),
    /// Native complete normal variation and source/rounding report.
    FuturesVariation(Box<crate::futures::FuturesSettlementEffect>),
    /// Proposed delivery asset quantity, never a recorded security transfer.
    SecurityDelivery {
        /// Actual selected eligible security revision.
        instrument: convex_core::futures::InstrumentPin,
        /// Signed face received (positive long future) or delivered (negative).
        face: ExactDecimal,
        /// Stable key; actual delivery recording is a separate input.
        key: ObligationKey,
    },
    /// Exact original proposed security effect confirmed by supplied receipt.
    SecurityRecorded {
        /// Original asset effect key.
        key: ObligationKey,
        /// Actual receipt identity.
        receipt_id: EventId,
    },
}
