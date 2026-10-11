//! Reconstruct complete original checked state and replay supplied native events.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::{futures as f, lifecycle as l};
use convex_core::{contracts::*, futures::*};
use serde::{Deserialize, Serialize};

/// Original full economic instrument. Cached computed state is never decoded.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LifecycleInstrumentInput {
    /// Full original zero/bill face, absolute redemption and quotation terms.
    Zero {
        /// All original economic terms, reconstructed through the native builder.
        input: ZeroInput,
        /// Actual original outstanding-principal recovery mapping.
        credit: Option<l::BondCreditTerms>,
    },
    /// Full fixed/floating/stepped/amortizing/optional bond.
    Bond {
        /// Original complete terms and calendar revisions.
        bond: BondInput,
        /// Actual outstanding-principal-only legal recovery mapping.
        credit: Option<l::BondCreditTerms>,
    },
    /// Full original IRS/OIS terms.
    Swap {
        /// Original trade, profiles, reset and collateral.
        terms: convex_rates::SwapTerms,
    },
    /// Full original credit confirmation.
    Cds {
        /// Actual legal/entity/notional/date/recovery terms.
        terms: convex_credit::CdsTerms,
    },
    /// Actual dated exchange future and supplied opening trade/official mark.
    Future {
        /// Full original contract with expiry/delivery/accrual mechanics.
        contract: FuturesContract,
        /// Actual opening money level source/precision, never zero inferred.
        opening_mark: f::FuturesMark,
    },
}
/// Original actual futures clearing/calendar/source context for a supplied event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FuturesEventInputs {
    /// Exact original clearing rules and normal monetary method.
    pub clearing: f::FuturesClearingRules,
    /// Actual original calendar/source revision.
    pub calendar_stamp: ReferenceStamp,
    /// Actual supplied time/freshness/source-use decisions.
    pub context: EligibilityContext,
}
/// Explicit actual physical settlement, separate from CTD valuation/carry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeliveryEventInputs {
    /// Complete original actual published basket.
    pub basket: DeliveryBasket,
    /// Actual selected security/version.
    pub instrument: InstrumentPin,
    /// Original security source and availability.
    pub security_stamp: ReferenceStamp,
    /// Complete original checked cash-bond economics.
    pub bond: BondInput,
    /// Actual independent delivery-calendar source.
    pub calendar_stamp: ReferenceStamp,
    /// Actual supplied source/freshness/information decisions.
    pub context: EligibilityContext,
}
/// One original actual supplied event and its complete ancillary inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LifecycleStep {
    /// Strict original event, exact prior revision/IDs/economic/known-at times.
    pub event: l::LifecycleEvent,
    /// Explicit CDS contractual cash rounding where required.
    pub credit_rounding: Option<convex_core::cashflows::CashRounding>,
    /// Full original clearing context for monetary futures effects.
    pub futures: Option<FuturesEventInputs>,
    /// Actual physical delivery information where required.
    pub delivery: Option<DeliveryEventInputs>,
}
/// Full original opening terms, eligible opening history and actual event replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LifecycleInput {
    /// Actual original trade identity.
    pub trade_id: TradeId,
    /// Actual original economic revision.
    pub version: Version,
    /// Full original native economic terms.
    pub instrument: LifecycleInstrumentInput,
    /// Exact signed native original units, never a duration proxy.
    pub position: l::LifecyclePosition,
    /// Original opening cutoff; past payments are never assumed.
    pub known_at: UtcTimestamp,
    /// Actual relevant eligible opening observations only.
    pub history: Vec<convex_core::fixings::FixingObservation>,
    /// All actual prior events plus requested final transition, in supplied order.
    pub events: Vec<LifecycleStep>,
}
pub(super) fn native_error(e: l::LifecycleError) -> LibraryError {
    match e {
        l::LifecycleError::Workflow(e) => e.into(),
        l::LifecycleError::Bond(e) => e.into(),
        l::LifecycleError::Swap(e) => e.into(),
        l::LifecycleError::Futures(e) => e.into(),
        l::LifecycleError::Cashflow(e) => e.into(),
        l::LifecycleError::Unsupported(reason) => LibraryError {
            code: LibraryErrorCode::Unsupported,
            message: reason.into(),
            details: Value::Null,
        },
        e => LibraryError {
            code: LibraryErrorCode::InvalidInput,
            message: e.to_string(),
            details: serde_json::json!({"native_kind":format!("{e:?}")}),
        },
    }
}
pub(super) struct NativeReplay {
    pub state: l::LifecycleState,
    pub transitions: Vec<(l::LifecycleEvent, Vec<l::LifecycleEffect>)>,
}
pub(super) fn replay_native(
    input: &LifecycleInput,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<NativeReplay> {
    if input.events.len() > 4096 {
        return Err(LibraryError::limit(
            "at most4096 supplied replay events per request",
        ));
    }
    let (instrument, exercise) = match &input.instrument {
        LifecycleInstrumentInput::Zero { input, credit } => (
            l::LifecycleInstrument::ZeroCoupon {
                bond: Box::new(input.native(r)?),
                credit: credit.clone(),
            },
            None,
        ),
        LifecycleInstrumentInput::Bond { bond, credit } => (
            l::LifecycleInstrument::Bond {
                contract: Box::new(r.bond(bond)?),
                credit: credit.clone(),
            },
            Some((
                &bond.terms.calendars.exercise,
                r.calendar(&bond.calendars.exercise)?,
            )),
        ),
        LifecycleInstrumentInput::Swap { terms } => (
            l::LifecycleInstrument::Swap(Box::new(terms.clone().build(&r.swap_calendars(terms)?)?)),
            None,
        ),
        LifecycleInstrumentInput::Cds { terms } => (
            l::LifecycleInstrument::Cds(Box::new(
                terms.clone().build(&r.credit_calendars(terms)?)?,
            )),
            None,
        ),
        LifecycleInstrumentInput::Future {
            contract,
            opening_mark,
        } => (
            l::LifecycleInstrument::Futures {
                contract: Box::new(contract.clone()),
                opening_mark: Box::new(opening_mark.clone()),
            },
            None,
        ),
    };
    let mut state = l::LifecycleState::new(
        input.trade_id.clone(),
        input.version,
        instrument,
        input.position,
        input.known_at,
        input.history.clone(),
    )
    .map_err(native_error)?;
    let mut transitions = Vec::new();
    for step in &input.events {
        let delivery_bond = step
            .delivery
            .as_ref()
            .map(|d| r.bond(&d.bond))
            .transpose()?;
        let futures = step
            .futures
            .as_ref()
            .map(|f| {
                Ok::<_, LibraryError>(l::FuturesLifecycleInputs {
                    clearing: &f.clearing,
                    calendar: super::products::future_calendar(&f.calendar_stamp, r)?,
                    context: f.context,
                })
            })
            .transpose()?;
        let delivery = match (&step.delivery, &delivery_bond) {
            (Some(d), Some(b)) => Some(l::BondDeliveryInputs {
                basket: &d.basket,
                security: f::FuturesBond {
                    instrument: &d.instrument,
                    stamp: &d.security_stamp,
                    bond: b,
                },
                calendar: super::products::future_calendar(&d.calendar_stamp, r)?,
                context: d.context,
            }),
            _ => None,
        };
        let next = l::transition(
            &state,
            &step.event,
            &l::LifecycleInputs {
                publications: Some(&r.publications),
                exercise_calendar: exercise,
                credit_rounding: step.credit_rounding,
                futures,
                delivery,
            },
        )
        .map_err(native_error)?;
        transitions.push((next.event, next.effects));
        state = next.next;
    }
    Ok(NativeReplay { state, transitions })
}

pub(super) fn replay(input: &LifecycleInput, r: &ResolvedInputs<'_>) -> LibraryResult<Value> {
    let output = replay_native(input, r)?;
    let transitions = output
        .transitions
        .into_iter()
        .map(|(event, effects)| serde_json::json!({"event":event,"effects":effects}))
        .collect::<Vec<_>>();
    Ok(serde_json::json!({"state":output.state.view(),"transitions":transitions}))
}
