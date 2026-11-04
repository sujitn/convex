//! Pure original contractual lifecycle. Supply actual events and immutable data;
//! obtain next economic state and proposed effects. No ledger, timer or authority.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod bond;
mod credit;
mod futures;
mod rates;
mod state;
mod types;
use convex_core::{
    cashflows::{BoundarySide, CashRounding, CashflowError, PaymentDateEntitlement},
    contracts::{ExactDecimal, Money, TradeId, UtcTimestamp, Version},
    fixings::{FixingContext, FixingObservation, FixingSeries, UnpublishedFixingPolicy},
    Currency, Date,
};
use rust_decimal::Decimal;
pub use state::{LifecycleInstrument, LifecycleState, LifecycleStateView, LifecycleTransition};
use std::collections::{BTreeMap, BTreeSet};
pub use types::*;

/// Maximum observations, receipts, effects or transition records in one state.
pub const MAX_LIFECYCLE_ITEMS: usize = 65_536;
/// Failure never returns a partial accepted state or effects.
#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    /// Economic identity, amount, date, ordering or bounded-resource violation.
    #[error("invalid lifecycle {field}: {reason}")]
    Invalid {
        /// Exact failed input.
        field: &'static str,
        /// Concrete failure.
        reason: String,
    },
    /// Contract/event combination requires different complete economic terms.
    #[error("unsupported lifecycle: {0}")]
    Unsupported(&'static str),
    /// Original common cash/fixing/Decimal failure, retaining missing requirements.
    #[error(transparent)]
    Cashflow(#[from] CashflowError),
    /// Original reference/date/unit validation failure.
    #[error(transparent)]
    Reference(#[from] convex_core::contracts::ContractError),
    /// Native complete bond failure.
    #[error(transparent)]
    Bond(#[from] convex_bonds::contracts::BondContractError),
    /// Native complete swap failure.
    #[error(transparent)]
    Swap(#[from] convex_rates::SwapError),
    /// Native futures guard/source/rounding failure.
    #[error(transparent)]
    Futures(#[from] crate::futures::FuturesError),
    /// Original typed publication schedule failure.
    #[error(transparent)]
    Workflow(#[from] crate::workflow::WorkflowError),
}
/// Result with explicit failure and no accepted partial state.
pub type LifecycleResult<T> = Result<T, LifecycleError>;
/// Immutable resolved ancillary inputs. Only the event's owning data is consumed.
#[derive(Default)]
pub struct LifecycleInputs<'a> {
    /// Complete checked successor bond, required only for BondAmendment.
    /// Build with the same actual versioned calendars as the original bond.
    pub amended_bond: Option<&'a convex_bonds::contracts::BondContract>,
    /// Actual administrator schedule for fixing-aware coupons/exercise.
    pub publications: Option<&'a crate::workflow::PublicationSchedule>,
    /// Original exercise calendar name and actual data, bound by the caller.
    pub exercise_calendar: Option<(
        &'a convex_bonds::types::CalendarId,
        &'a dyn convex_core::calendars::Calendar,
    )>,
    /// Full native clearing/calendar/source context for variation settlement.
    pub futures: Option<FuturesLifecycleInputs<'a>>,
    /// Explicit contractual CDS cash rounding, required for scheduled credit cash.
    pub credit_rounding: Option<CashRounding>,
    /// Exact supplied physical basket/selected security for invoice settlement.
    pub delivery: Option<BondDeliveryInputs<'a>>,
}
/// Actual physical settlement needs its selected asset, factor and calendar;
/// financing/carry and CTD choice belong to valuation, not invoice settlement.
#[derive(Clone, Copy)]
pub struct BondDeliveryInputs<'a> {
    /// Complete original immutable published delivery basket.
    pub basket: &'a convex_core::futures::DeliveryBasket,
    /// Actual explicitly selected complete checked security, never a proxy bond.
    pub security: crate::futures::FuturesBond<'a>,
    /// Original delivery-calendar binding.
    pub calendar: crate::futures::FuturesCalendar<'a>,
    /// Actual supplied reference use and information cutoff.
    pub context: convex_core::futures::EligibilityContext,
}

/// Supplied original native settlement rules; no library freshness clock.
#[derive(Clone, Copy)]
pub struct FuturesLifecycleInputs<'a> {
    /// Exact original clearing revision and method.
    pub clearing: &'a crate::futures::FuturesClearingRules,
    /// Exact clearing-calendar binding.
    pub calendar: crate::futures::FuturesCalendar<'a>,
    /// Supplied reference availability/freshness/time decisions.
    pub context: convex_core::futures::EligibilityContext,
}
/// Deterministic closed dispatch. Input state is immutable, including on failure.
pub fn transition(
    previous: &LifecycleState,
    event: &LifecycleEvent,
    inputs: &LifecycleInputs<'_>,
) -> LifecycleResult<LifecycleTransition> {
    previous.check_event(event)?;
    let mut next = previous.clone();
    let mut effects = Vec::new();
    match &event.action {
        LifecycleAction::BondAmendment { .. } => {
            next.amend_bond(event, inputs.amended_bond, &mut effects)?
        }
        LifecycleAction::Fixing(o) => next.fixing(event, o, &mut effects)?,
        LifecycleAction::ContractStep => next.contract_step(event, &mut effects)?,
        LifecycleAction::ContractualCash => next.contractual_cash(event, inputs, &mut effects)?,
        LifecycleAction::Payment(r) => next.payment(event, r, &mut effects)?,
        LifecycleAction::SecurityReceipt {
            obligation,
            instrument,
            face,
            receipt_id,
            source: _,
        } => next.security_receipt(
            event,
            obligation,
            instrument,
            *face,
            receipt_id,
            &mut effects,
        )?,
        LifecycleAction::ExerciseNotice {
            provision,
            exercise_date,
        } => next.notice(event, *provision, *exercise_date, inputs, &mut effects)?,
        LifecycleAction::Exercise {
            provision,
            weight_end,
        } => next.exercise(event, *provision, *weight_end, inputs, &mut effects)?,
        LifecycleAction::Terminate {
            fraction,
            cash,
            cash_date,
            agreement,
        } => next.terminate(event, *fraction, cash, *cash_date, agreement, &mut effects)?,
        LifecycleAction::CreditDefault(d) => next.default(event, d, &mut effects)?,
        LifecycleAction::Recovery(r) => next.recovery(event, r, &mut effects)?,
        LifecycleAction::FuturesSettlement(mark) => {
            next.futures_settlement(event, mark, inputs, &mut effects)?
        }
        LifecycleAction::BondDelivery {
            deliverable,
            invoice_price_points,
            notice,
        } => next.deliver(
            event,
            deliverable,
            *invoice_price_points,
            notice,
            inputs,
            &mut effects,
        )?,
    }
    if effects.len() > MAX_LIFECYCLE_ITEMS {
        return Err(invalid("effects", "bounded per-event work exceeded"));
    }
    next.revision = next
        .revision
        .checked_add(1)
        .ok_or_else(|| invalid("revision", "overflow"))?;
    next.effective_date = event.effective_date;
    next.known_at = event.known_at;
    next.events.insert(event.event_id.clone());
    next.close_if_settled();
    Ok(LifecycleTransition {
        event: event.clone(),
        next,
        effects,
    })
}
fn invalid(field: &'static str, reason: impl Into<String>) -> LifecycleError {
    LifecycleError::Invalid {
        field,
        reason: reason.into(),
    }
}
fn arithmetic(value: Option<Decimal>, name: &'static str) -> LifecycleResult<Decimal> {
    value.ok_or_else(|| CashflowError::Arithmetic(name).into())
}
fn check_date(date: Date) -> LifecycleResult<()> {
    if (1..=9999).contains(&date.year()) {
        Ok(())
    } else {
        Err(invalid("date", "outside0001..9999"))
    }
}
fn money(amount: Decimal, currency: Currency) -> Money {
    Money::new(ExactDecimal::new(amount), currency)
}
