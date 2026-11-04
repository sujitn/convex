//! Strict complete native single-name economics. No ticker selects conventions.
use convex_core::{
    calendars::Calendar,
    cashflows::ScheduleSpec,
    contracts::{EntityId, ExactDecimal, FiniteF64, ReferenceDataId, TradeId, Version},
    daycounts::DayCountConvention,
    Currency, Date,
};
use convex_curves::contracts::CurveKey;
use serde::{Deserialize, Serialize};

/// Exact supplied reference/calendar/document revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ReferencePin {
    /// Provider-neutral resource identity.
    pub id: ReferenceDataId,
    /// Immutable revision; never a latest lookup.
    pub version: Version,
}
/// Actual calendar bound to its declared version by the caller.
#[derive(Clone, Copy)]
pub struct CalendarBinding<'a> {
    /// Actual immutable identity/version.
    pub pin: &'a ReferencePin,
    /// Calendar code/data. Custom callbacks must terminate without panic.
    pub calendar: &'a dyn Calendar,
}
/// Independent premium and cash-settlement calendar resources.
pub struct CdsCalendars<'a> {
    /// Adjust premium accrual/payment boundaries.
    pub premium: CalendarBinding<'a>,
    /// Count the explicitly supplied trade cash-settlement lag.
    pub settlement: CalendarBinding<'a>,
}
/// Initial closed single-name seniorities. Unknown wire values are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Seniority {
    /// Senior unsecured obligation.
    SeniorUnsecured,
    /// Senior secured obligation.
    SeniorSecured,
    /// Subordinated obligation.
    Subordinated,
}
impl Seniority {
    /// Exact provider-neutral value required in the supplied credit curve role.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SeniorUnsecured => "senior_unsecured",
            Self::SeniorSecured => "senior_secured",
            Self::Subordinated => "subordinated",
        }
    }
}
/// Complete reference obligation identity; provider mappings are outside pricing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CreditReference {
    /// Exact single reference entity.
    pub entity_id: EntityId,
    /// Debt priority, checked against the survival curve.
    pub seniority: Seniority,
    /// Actual document/confirmation revision.
    pub document: ReferencePin,
    /// Actual restructuring-clause revision; no jurisdiction preset is inferred.
    pub restructuring_clause: ReferencePin,
    /// Finite fractional recovery, explicit even at zero/full recovery.
    pub recovery_fraction: FiniteF64,
}
/// Direction; unsigned notional is independent of protection direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CdsSide {
    /// Receive protection, pay premium and positive initial cash.
    ProtectionBuyer,
    /// Pay protection, receive premium and positive initial cash.
    ProtectionSeller,
}
impl CdsSide {
    pub(crate) const fn sign(self) -> f64 {
        match self {
            Self::ProtectionBuyer => 1.0,
            Self::ProtectionSeller => -1.0,
        }
    }
}
/// Explicit standard-model version, including its half-day default accrual rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CdsModel {
    /// ISDA CDS Standard Model 1.8.3 convention, independently implemented in Rust.
    Standard183,
}
/// Maturity supplied as an actual date; no CDS2015 tenor/date mapping is inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CdsMaturityRule {
    /// Contract ends on an unadjusted March/June/September/December twentieth.
    QuarterlyTwentieth,
    /// Explicit off-cycle maturity with a supplied stub; still unadjusted protection.
    ExplicitDate,
}
/// Surviving pre-credit-event contract. Default/auction/successor lifecycle states
/// require a separate lifecycle route and cannot be smuggled into these inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CdsTerms {
    /// Internal trade identity.
    pub trade_id: TradeId,
    /// Exact definition revision.
    pub version: Version,
    /// Signed protection direction.
    pub side: CdsSide,
    /// Settlement and premium currency.
    pub currency: Currency,
    /// Strict positive face amount, exact decimal string on the wire.
    pub notional: ExactDecimal,
    /// Complete single-name credit reference.
    pub reference: CreditReference,
    /// Explicit mathematical convention/version.
    pub model: CdsModel,
    /// End-of-day original trade date.
    pub trade_date: Date,
    /// Original step-in: trade plus one calendar day, including weekends.
    pub step_in_date: Date,
    /// Inclusive protection start, at the beginning of this day.
    pub protection_start_date: Date,
    /// Trade cash-settlement date, independently checked against supplied lag.
    pub cash_settlement_date: Date,
    /// Explicit nonnegative cash-settlement lag in business days, at most 366.
    pub settlement_lag_business_days: i32,
    /// Complete premium grid with stubs and actual unadjusted maturity.
    pub premium_schedule: ScheduleSpec,
    /// Maturity date constraint, distinct from premium business-day adjustment.
    pub maturity_rule: CdsMaturityRule,
    /// Actual coupon fraction per year; exact decimal string, not bp/percent.
    pub coupon_decimal: ExactDecimal,
    /// ACT/360 or ACT/365F; other day counts are explicitly unsupported.
    pub premium_day_count: DayCountConvention,
    /// Whether accrued premium is paid at default under the standard model.
    pub pay_accrual_on_default: bool,
    /// Exact premium calendar version. Following adjustment is this model's rule.
    pub premium_calendar: ReferencePin,
    /// Exact cash-settlement calendar version.
    pub settlement_calendar: ReferencePin,
    /// Same-currency collateral discount dependency; no government curve proxy.
    pub discount_curve: CurveKey,
    /// Exact supplied credit dependency for model pricing. Conversion declares
    /// its separate fitted flat hazard and does not invent a curve revision.
    pub survival_curve: CurveKey,
    /// Optional contractual initial CLEAN upfront fraction from buyer to seller.
    /// Initial CASH is N * fraction minus the trade's accrued-premium refund.
    pub initial_clean_upfront_fraction: Option<ExactDecimal>,
}
/// Caller-supplied payment state. Future cash cannot be labelled already settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum InitialPaymentState {
    /// The terms contain no contractual initial payment.
    NotContracted,
    /// Initial cash remains due on the terms' cash-settlement date.
    Pending,
    /// Recorded externally as paid; excluded from outstanding trade PV.
    Settled,
}
/// Explicit current valuation boundaries; no system clock or hidden trade state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CdsPricingContext {
    /// End-of-day valuation, conditional on a surviving reference entity.
    pub valuation_date: Date,
    /// Current protection step-in, at least one calendar day after valuation.
    pub step_in_date: Date,
    /// Date for fair clean/dirty upfront conversion and accrued-premium refund.
    pub cash_settlement_date: Date,
    /// Actual contractual initial cash state, independent of expected cash.
    pub initial_payment_state: InitialPaymentState,
}
impl CdsTerms {
    /// Construct the original trade context, supplying recorded cash state.
    pub fn trade_context(&self, initial_payment_state: InitialPaymentState) -> CdsPricingContext {
        CdsPricingContext {
            valuation_date: self.trade_date,
            step_in_date: self.step_in_date,
            cash_settlement_date: self.cash_settlement_date,
            initial_payment_state,
        }
    }
}
