//! Pure causal reconciliation. Native prices and replayed states cannot be
//! replaced by supplied scalar marks; operational authority remains external.
use super::*;
use crate::{lifecycle as l, quote_risk as q, risk::hedging::constrained::HedgeFxPin};
use convex_core::{
    contracts::{ExactDecimal, ModelMoney, Money, SnapshotId, SourceId, TradeId, UtcTimestamp},
    Currency,
};
use convex_credit::ReferencePin;
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde::{Deserialize, Serialize};
mod product;
pub use product::*;
mod cohorts;
pub use cohorts::*;

/// Original product and lifecycle failures, including explicitly unknown cash.
#[derive(Debug, thiserror::Error)]
pub enum ReconciliationError {
    /// Original financial error.
    #[error(transparent)]
    Workflow(#[from] WorkflowError),
    /// Original event/order/history/receipt error.
    #[error(transparent)]
    Lifecycle(#[from] l::LifecycleError),
    /// Original complete issuer/cash/ICMA error.
    #[error(transparent)]
    Cash(#[from] crate::cash_calibration::CashCalibrationError),
    /// Missing actual economic state, never supplied zero.
    #[error("incomplete reconciliation: {0}")]
    Incomplete(&'static str),
}
/// No partial accepted book on failure.
pub type ReconciliationResult<T> = Result<T, ReconciliationError>;
fn bad(field: &'static str, reason: &str) -> ReconciliationError {
    invalid(field, reason).into()
}
fn num(v: Decimal) -> ReconciliationResult<f64> {
    v.to_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| bad("amount", "finite Decimal conversion required"))
}
fn money(v: f64, c: Currency) -> ReconciliationResult<ModelMoney> {
    q::risk_reference_pv(v, c)
        .map_err(WorkflowError::from)
        .map_err(Into::into)
}
fn dec(v: Option<Decimal>) -> ReconciliationResult<Decimal> {
    v.ok_or_else(|| bad("Decimal arithmetic", "overflow"))
}
/// Declared order is part of the answer; events can couple to time boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PnlCause {
    /// Economic time/carry plus mandatory earned coupon/principal boundaries.
    Time,
    /// Newly eligible actual published observations and their knowledge cutoff.
    Fixing,
    /// Full versioned curves, calibration quotes and observed cash/repo markets.
    Market,
    /// Explicit model assumptions/OAS, retaining original numerical policies.
    Model,
    /// Actual direct reporting FX observation.
    Fx,
    /// Full prospective amended economics, subject to historical conservation.
    Amendment,
    /// Actual supplied fills, legal cash ownership and hypothetical pending quantities.
    Position,
    /// Actual receipt, notice, exercise, default, auction or clearing event.
    Event,
}
/// Supplied immutable execution fact; this API neither authenticates nor posts it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PnlFill {
    /// Locally unique actual fill identity.
    pub id: l::EventId,
    /// Original expected holding/cohort.
    pub trade_id: TradeId,
    /// Exact executed security revision.
    pub instrument: ReferencePin,
    /// Signed native quantity, positive acquisition of original trade direction.
    pub quantity: ExactDecimal,
    /// Signed cash consideration, or signed futures monetary quote consideration.
    /// For a future this is q*actual points*currency/point, NOT paid cash.
    pub consideration: Money,
    /// Explicit commission/fees, signed actual cash (normally negative).
    pub fees: Money,
    /// Supplied economic execution date.
    pub date: Date,
    /// Original receipt/knowledge timestamp.
    pub known_at: UtcTimestamp,
    /// Actual immutable record source.
    pub source: SourceId,
}
/// Native quantities and explicit unsettled-claim discount policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PnlPositionInput {
    /// Full original holding/terms/calculation/entity pins.
    pub identity: GraphHedgeIdentity,
    /// Complete signed native unit and original reference quantity.
    pub current: q::RiskPosition,
    /// Hypothetical remaining pending orders, separate from actual fills.
    pub pending_quantity: ExactDecimal,
    /// Exact collateral discount curve for crystallized receivables/payables.
    pub cash_discount: CurveKey,
    /// Explicit carrying convention for due/overdue admitted claims.
    pub overdue: OverdueClaimPolicy,
}
/// Contractual claims are not market recovery estimates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OverdueClaimPolicy {
    /// Due claims carried at their exact remaining nominal amount; no credit,
    /// penalty, collection cost, default discount or funding yield is inferred.
    Nominal,
    /// Require a separate complete overdue-claim model rather than infer value.
    Reject,
}
/// One complete caller-bound information set, separately priced/replayed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PnlPointInput {
    /// Distinct exact information-set declaration.
    pub snapshot_id: SnapshotId,
    /// Complete terms/market/model/history/event manifest declaration.
    pub manifest: ReferencePin,
    /// Actual supplied economic valuation date.
    pub valuation_date: Date,
    /// Actual supplied information cutoff; no system clock.
    pub known_at: UtcTimestamp,
    /// Exact ordered complete expected holding coverage.
    pub positions: Vec<PnlPositionInput>,
    /// All actual fills since the before point, in chronological ingress order.
    pub fills: Vec<PnlFill>,
    /// Exact direct observations for this point and its reporting currency.
    pub fx: Vec<HedgeFxPin>,
}
/// A causal counterfactual can be economically undefined without its prerequisites.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PnlStandalone {
    /// Full independently admitted native point index following ordered points.
    Available {
        /// Actual point, not a supplied scalar effect.
        point: usize,
    },
    /// Cannot be formed without due history, amended economics or prior events.
    Unavailable {
        /// Explicit original missing prerequisite, retained in report.
        reason: String,
    },
}
/// Baseline, every ordered endpoint and every available standalone endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CausalPnlInput {
    /// Declared ordered causal phases,1..16; an event sequence remains ordered.
    pub causes: Vec<PnlCause>,
    /// Baseline then ordered endpoints then full available counterfactuals.
    pub points: Vec<PnlPointInput>,
    /// Exactly one declaration per cause; absent counterfactuals are never zero.
    pub standalone: Vec<PnlStandalone>,
    /// Explicit common reporting currency.
    pub reporting_currency: Currency,
    /// Explicit full-information/native-mark ceiling,1..33.
    pub max_points: usize,
    /// Explicit product-call ceiling,1..4224.
    pub max_product_evaluations: usize,
}
/// Private admitted path; native results bind to its exact address and coverage.
#[derive(Debug)]
pub struct PreparedCausalPnl {
    input: CausalPnlInput,
    cohorts: Option<Vec<CohortOwnership>>,
    handoffs: Vec<Vec<Option<PhysicalDeliveryValue>>>,
    cohort_state_pins: Option<Vec<Vec<String>>>,
    lot_splits: Vec<CohortLotSplit>,
    claim_ownership: Vec<Vec<Vec<CohortClaimOwnershipValue>>>,
    physical_partitions: Vec<PhysicalDeliveryPartition>,
}
impl PreparedCausalPnl {
    /// Full immutable declarations.
    pub fn input(&self) -> &CausalPnlInput {
        &self.input
    }
}
fn fx(path: &PreparedCausalPnl, point: usize, currency: Currency) -> ReconciliationResult<f64> {
    if currency == path.input.reporting_currency {
        return Ok(1.0);
    }
    let p = path.input.points[point]
        .fx
        .iter()
        .find(|p| p.from == currency)
        .ok_or_else(|| bad("FX", "exact direct foreign currency observation missing"))?;
    let rate = num(p.rate.decimal())?;
    if rate <= 0.0 {
        return Err(bad("FX", "positive direct rate required"));
    }
    Ok(rate)
}
/// Admit exact coverage, work, monotone time/knowledge, quantities and fill facts
/// before pricing. Full source/cause conservation is additionally ingress-owned.
pub fn prepare_causal_pnl(input: &CausalPnlInput) -> ReconciliationResult<PreparedCausalPnl> {
    let f = input.causes.len();
    if !(1..=16).contains(&f)
        || input.standalone.len() != f
        || input.points.len() < f + 1
        || input.max_points == 0
        || input.max_points > 33
        || input.points.len() > input.max_points
        || input.max_product_evaluations == 0
        || input.max_product_evaluations > 4224
    {
        return Err(bad(
            "work",
            "complete bounded causal path/counterfactual declaration required",
        ));
    }
    let n = input.points[0].positions.len();
    if !(1..=128).contains(&n)
        || input.points.len() * n > input.max_product_evaluations
        || !input.points[0].fills.is_empty()
    {
        return Err(bad(
            "coverage",
            "1..128 complete holdings and original empty interval fill set required",
        ));
    }
    let mut next = f + 1;
    for s in &input.standalone {
        match s {
            PnlStandalone::Available { point } => {
                if *point != next {
                    return Err(bad(
                        "standalone",
                        "exact complete counterfactual order required",
                    ));
                }
                next += 1;
            }
            PnlStandalone::Unavailable { reason } => {
                if reason.is_empty() || reason.len() > 8192 {
                    return Err(bad(
                        "standalone",
                        "explicit bounded missing prerequisite required",
                    ));
                }
            }
        }
    }
    if next != input.points.len() {
        return Err(bad("coverage", "unconsumed full information set"));
    }
    let path = PreparedCausalPnl {
        input: input.clone(),
        cohorts: None,
        handoffs: Vec::new(),
        cohort_state_pins: None,
        lot_splits: Vec::new(),
        claim_ownership: Vec::new(),
        physical_partitions: Vec::new(),
    };
    for (i, p) in input.points.iter().enumerate() {
        if p.positions.len() != n
            || p.fills.len() > 4096
            || p.fx.len() > 128
            || p.known_at.datetime().date_naive() < p.valuation_date.as_naive_date()
            || !(1..=9999).contains(&p.valuation_date.year())
            || input.points[..i]
                .iter()
                .any(|v| v.snapshot_id == p.snapshot_id || v.manifest == p.manifest)
        {
            return Err(bad("information set", "distinct snapshots/manifests, complete coverage and available economic time required"));
        }
        let previous = if i > 0 && i <= f {
            &input.points[i - 1]
        } else {
            &input.points[0]
        };
        if p.valuation_date < previous.valuation_date || p.known_at < previous.known_at {
            return Err(bad(
                "time",
                "nondecreasing economic and known-at path required",
            ));
        }
        for (j, pos) in p.positions.iter().enumerate() {
            let original = &input.points[0].positions[j];
            if pos.identity.id != original.identity.id
                || pos.identity.entity != original.identity.entity
                || pos.current.currency != original.current.currency
                || pos.current.quantity_unit != original.current.quantity_unit
                || pos.current.reference_quantity != original.current.reference_quantity
                || p.positions[..j]
                    .iter()
                    .any(|v| v.identity.id == pos.identity.id)
            {
                return Err(bad(
                    "coverage",
                    "exact original identity/currency/unit/reference and entity required",
                ));
            }
            let executed = p
                .fills
                .iter()
                .filter(|x| x.trade_id == pos.identity.id)
                .try_fold(Decimal::ZERO, |a, x| {
                    dec(a.checked_add(x.quantity.decimal()))
                })?;
            if dec(original.current.quantity.decimal().checked_add(executed))?
                != pos.current.quantity.decimal()
            {
                return Err(bad(
                    "position",
                    "current quantity must equal opening plus actual supplied fills",
                ));
            }
            pos.current.scale().map_err(WorkflowError::from)?;
            let mut projected = pos.current.clone();
            projected.quantity = ExactDecimal::new(dec(pos
                .current
                .quantity
                .decimal()
                .checked_add(pos.pending_quantity.decimal()))?);
            projected.scale().map_err(WorkflowError::from)?;
            fx(&path, i, pos.current.currency)?;
        }
        for (j, x) in p.fills.iter().enumerate() {
            let pos = p
                .positions
                .iter()
                .find(|v| v.identity.id == x.trade_id)
                .ok_or_else(|| bad("fill", "unknown holding"))?;
            if x.instrument != pos.identity.instrument
                || x.consideration.currency() != pos.current.currency
                || x.fees.currency() != pos.current.currency
                || x.quantity.decimal().is_zero()
                || x.known_at > p.known_at
                || x.date > p.valuation_date
                || x.date < input.points[0].valuation_date
                || x.known_at < input.points[0].known_at
                || p.fills[..j].iter().any(|a| a.id == x.id)
                || (j > 0 && (x.date < p.fills[j - 1].date || x.known_at < p.fills[j - 1].known_at))
            {
                return Err(bad("fill", "exact units/revision, interval/knowledge, unique ordered actual facts required"));
            }
        }
        if i > 0 && !p.fills.starts_with(&previous.fills) {
            return Err(bad(
                "fills",
                "original interval facts cannot disappear or change",
            ));
        }
        for (j, x) in p.fx.iter().enumerate() {
            if x.to != input.reporting_currency
                || x.from == input.reporting_currency
                || x.snapshot_id != p.snapshot_id
                || x.rate.decimal() <= Decimal::ZERO
                || p.fx[..j].iter().any(|a| a.from == x.from)
                || !p.positions.iter().any(|a| a.current.currency == x.from)
            {
                return Err(bad(
                    "FX",
                    "one exact consumed direct observation per currency and snapshot required",
                ));
            }
        }
    }
    Ok(path)
}
/// Every original active value; terminal exposure is a native lifecycle conclusion.
#[derive(Debug, Clone, Serialize)]
pub enum LifecycleModelValue {
    /// Full original nonlinear native product diagnostics.
    Active(Box<ScenarioValuation>),
    /// Original checked fixed-price notice path; no model chooses the notice.
    Notified {
        /// Actual successfully applied notice, separate from its legal deadline.
        notice: Box<l::LifecycleEvent>,
        /// Full original legal cash path and native spread valuation.
        valuation: Box<BondValue>,
        /// Exact original notice/exercise/cash economics.
        exercise: WorkoutCashDetails,
    },
    /// Original lifecycle status proves future exposure has ended; claims remain.
    Ended(l::EconomicStatus),
}
/// One explicitly discounted original obligation, including actual partial receipts.
#[derive(Debug, Clone, Serialize)]
pub struct PnlClaimValue {
    /// Full original obligation with dates, source event and recorded receipts.
    pub obligation: l::CashObligation,
    /// Exact remaining amount, not expected recovery or probability-weighted cash.
    pub outstanding: Money,
    /// Original native discount ratio, or explicit due-at-nominal policy.
    pub discount_ratio: Option<f64>,
    /// Actual outstanding currency present value.
    pub present_value: Option<ModelMoney>,
    /// Exact full amount removed from active model to avoid double counting.
    pub removed_from_active_model: ModelMoney,
    /// Exact explicit legal entitlement in cohort mode; unowned claims remain
    /// visible in the original state and carry no owned value or received cash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entitled: Option<bool>,
    /// Exact residual legal ownership and checked excluded receipt prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub residual_ownership: Option<ResidualClaimValue>,
    /// Exact current ownership and historical recipients of transferred cash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_ownership: Option<PnlTransferredClaimValue>,
}
/// Original checked transferred cash, discounted by its original cash owner.
#[derive(Debug, Clone, Serialize)]
pub struct PnlTransferredClaimValue {
    /// Complete source, legal transfers and exact receipt allocations.
    pub ownership: CohortClaimOwnershipValue,
    /// Same original collateral ratio, or explicit nominal overdue convention.
    pub discount_ratio: f64,
    /// Exact owned outstanding cash valued by the original collateral curve.
    pub present_value: ModelMoney,
}
/// Privately generated full mark; no scalar result may be supplied at aggregation.
#[derive(Debug, Serialize)]
pub struct CausalPositionMark<'p> {
    /// Complete supplied security reorganizations applied to this original lot.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lot_splits: Vec<CohortLotSplit>,
    #[serde(skip)]
    path: &'p PreparedCausalPnl,
    point: usize,
    index: usize,
    /// Full actual replayed economic view.
    pub state: l::LifecycleStateView,
    /// Original full active/native ended model meaning.
    pub valuation: LifecycleModelValue,
    /// Explicit cash-PV versus futures-quote-level meaning.
    pub basis: ValuationBasis,
    /// Full original reference price level before scaling.
    pub reference_amount: ModelMoney,
    /// Current/pending/projected model amounts; claims are current-only.
    pub local: ScenarioAmounts,
    /// Explicit point FX conversion, preserving separate native bases.
    pub reporting: ScenarioAmounts,
    /// Complete outstanding claims, including partial/zero settlement state.
    pub claims: Vec<PnlClaimValue>,
    /// Separately sourced claims received without changing security exposure.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub transferred_claims: Vec<PnlTransferredClaimValue>,
    /// Exact cumulative externally recorded cash from full replay.
    pub recorded_cash: Money,
    /// Exact cumulative proposed claims from full replay, not cash received.
    pub proposed_cash: Money,
    /// Actual interval fills, separate from current and pending quantities.
    pub executed_quantity: ExactDecimal,
    /// Actual executed part of the current native model marking.
    pub executed_marking: ModelMoney,
    /// Exact cash consideration or distinct futures quote consideration, signed.
    pub fill_consideration: Money,
    /// Exact actual fees, never an inferred rate.
    pub fill_fees: Money,
    /// All original independent basket selection/fair CTD winners and ties.
    pub ctd: Option<crate::futures::BondFuturesCtdState>,
    /// Actual final rate-future quote anchor after exposure ends. This is an
    /// unrounded quote level, not remaining exposure or received margin cash.
    pub terminal_quote: Option<ModelMoney>,
    /// Exact explicit original cash entitlement and activation/disposal facts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cohort: Option<CohortOwnership>,
    /// Actual checked physical asset/invoice handoff on the cash owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_delivery: Option<PhysicalDeliveryValue>,
}
/// Cash-PV reconciliation includes incremental recorded receipts and actual fill
/// cash; futures quote reconciliation deducts executed quote consideration and
/// deliberately does not add margin receipts. Exact cash is separately retained.
#[derive(Debug, Clone, Serialize)]
pub struct CausalPnlAmounts {
    /// Mark-only current/pending/projected values or changes.
    pub marking: ScenarioAmounts,
    /// Interval received cash plus fill cash/fees, converted at this point's FX.
    pub cash: ModelMoney,
    /// Current model gain with actual cash economics, separate from pending.
    pub current_model_pnl: ModelMoney,
}
/// Original cause order plus actually admitted standalone effects and residuals.
#[derive(Debug, Serialize)]
pub struct CausalPnlAttribution {
    /// Complete actual interval change.
    pub total: CausalPnlAmounts,
    /// Complete prefix minus preceding prefix in declared cause order.
    pub ordered: Vec<CausalPnlAmounts>,
    /// Each actual standalone effect; absent values are explicitly unavailable.
    pub standalone: Vec<Option<CausalPnlAmounts>>,
    /// Total minus standalone sum; None if any counterfactual is unavailable.
    pub interaction: Option<CausalPnlAmounts>,
    /// Actual total minus sum of ordered effects, never clamped.
    pub residual: CausalPnlAmounts,
}
fn amounts(v: [f64; 3], c: Currency) -> ReconciliationResult<ScenarioAmounts> {
    Ok(ScenarioAmounts {
        current: money(v[0], c)?,
        pending: money(v[1], c)?,
        projected: money(v[2], c)?,
    })
}
fn values(a: &ScenarioAmounts) -> [f64; 3] {
    [
        a.current.value().get(),
        a.pending.value().get(),
        a.projected.value().get(),
    ]
}
fn combine(
    a: &CausalPnlAmounts,
    b: &CausalPnlAmounts,
    sign: f64,
) -> ReconciliationResult<CausalPnlAmounts> {
    let c = a.cash.currency();
    let x = values(&a.marking);
    let y = values(&b.marking);
    Ok(CausalPnlAmounts {
        marking: amounts(std::array::from_fn(|i| x[i] + sign * y[i]), c)?,
        cash: money(a.cash.value().get() + sign * b.cash.value().get(), c)?,
        current_model_pnl: money(
            a.current_model_pnl.value().get() + sign * b.current_model_pnl.value().get(),
            c,
        )?,
    })
}
fn explain(
    path: &PreparedCausalPnl,
    v: &[CausalPnlAmounts],
) -> ReconciliationResult<CausalPnlAttribution> {
    let n = path.input.causes.len();
    let total = combine(&v[n], &v[0], -1.0)?;
    let ordered = (1..=n)
        .map(|i| combine(&v[i], &v[i - 1], -1.0))
        .collect::<ReconciliationResult<Vec<_>>>()?;
    let standalone = path
        .input
        .standalone
        .iter()
        .map(|s| match s {
            PnlStandalone::Available { point } => Ok(Some(combine(&v[*point], &v[0], -1.0)?)),
            PnlStandalone::Unavailable { .. } => Ok(None),
        })
        .collect::<ReconciliationResult<Vec<_>>>()?;
    let zero = CausalPnlAmounts {
        marking: amounts([0.0; 3], total.cash.currency())?,
        cash: money(0.0, total.cash.currency())?,
        current_model_pnl: money(0.0, total.cash.currency())?,
    };
    let sum = |rows: &[CausalPnlAmounts]| {
        rows.iter()
            .try_fold(zero.clone(), |a, b| combine(&a, b, 1.0))
    };
    let interaction = if standalone.iter().all(Option::is_some) {
        Some(combine(
            &total,
            &sum(&standalone.iter().flatten().cloned().collect::<Vec<_>>())?,
            -1.0,
        )?)
    } else {
        None
    };
    Ok(CausalPnlAttribution {
        residual: combine(&total, &sum(&ordered)?, -1.0)?,
        total,
        ordered,
        standalone,
        interaction,
    })
}
fn effective(
    path: &PreparedCausalPnl,
    m: &CausalPositionMark<'_>,
    before: &CausalPositionMark<'_>,
    reporting: bool,
) -> ReconciliationResult<CausalPnlAmounts> {
    let c = if reporting {
        path.input.reporting_currency
    } else {
        m.local.current.currency()
    };
    let rate = if reporting {
        fx(path, m.point, m.local.current.currency())?
    } else {
        1.0
    };
    let recorded = dec(m
        .recorded_cash
        .value()
        .decimal()
        .checked_sub(before.recorded_cash.value().decimal()))?;
    let fill = m.fill_consideration.value().decimal();
    let fees = m.fill_fees.value().decimal();
    let cash = dec(dec(recorded.checked_add(
        if m.basis == ValuationBasis::CurrencyPresentValue {
            fill
        } else {
            Decimal::ZERO
        },
    ))?
    .checked_add(fees))?;
    let marking = if reporting {
        m.reporting.clone()
    } else {
        m.local.clone()
    };
    let adjustment = if m.basis == ValuationBasis::CurrencyPresentValue {
        num(cash)?
    } else {
        num(dec(fees.checked_sub(fill))?)?
            + m.terminal_quote.as_ref().map_or(0.0, |q| q.value().get())
    };
    Ok(CausalPnlAmounts {
        current_model_pnl: money(marking.current.value().get() + adjustment * rate, c)?,
        cash: money(num(cash)? * rate, c)?,
        marking,
    })
}
/// Every full original position and its causal financial explanation.
#[derive(Debug, Serialize)]
pub struct CausalPnlTrade<'p> {
    /// Exact original expected holding identity.
    pub id: TradeId,
    /// Baseline, each ordered endpoint and each available standalone mark.
    pub marks: Vec<CausalPositionMark<'p>>,
    /// Native-currency marking/cash/model-gain effects.
    pub local: CausalPnlAttribution,
    /// Exact changing-FX reporting effects.
    pub reporting: CausalPnlAttribution,
    /// Original full CTD transitions for every ordered endpoint.
    pub ctd_transitions: Vec<Option<ScenarioCtdTransition>>,
}
/// Separate native currency AND cash-PV/futures-quote meaning.
#[derive(Debug, Serialize)]
pub struct CausalPnlTotal {
    /// Financial measurement basis.
    pub basis: ValuationBasis,
    /// Actual native or reporting currency.
    pub currency: Currency,
    /// Actual full aggregate explanation.
    pub attribution: CausalPnlAttribution,
}
/// Pure deterministic reconciliation of supplied facts, never authoritative P&L.
#[derive(Debug, Serialize)]
pub struct CausalPnlReport<'p> {
    /// Complete actual before/after/path/FX/quantity/fill declarations.
    pub input: CausalPnlInput,
    /// Full native economics/states/claims/received/proposed/effects per holding.
    pub trades: Vec<CausalPnlTrade<'p>>,
    /// Separate native currencies and bases.
    pub local_totals: Vec<CausalPnlTotal>,
    /// Actual full point FX, with distinct bases.
    pub reporting_totals: Vec<CausalPnlTotal>,
    /// All logical native calls admitted up front, including zero quantities.
    pub product_evaluations: usize,
}
/// Aggregate only exact privately produced path/position/point results. No
/// omitted holding, fake result, foreign path, scalar callback or synthetic zero.
pub fn aggregate_causal_pnl<'p>(
    path: &'p PreparedCausalPnl,
    rows: Vec<Vec<CausalPositionMark<'p>>>,
) -> ReconciliationResult<CausalPnlReport<'p>> {
    let n = path.input.points[0].positions.len();
    let count = path.input.points.len();
    if rows.len() != n
        || rows.iter().enumerate().any(|(j, r)| {
            r.len() != count
                || r.iter()
                    .enumerate()
                    .any(|(i, m)| !std::ptr::eq(m.path, path) || m.point != i || m.index != j)
        })
    {
        return Err(bad(
            "coverage",
            "complete private original path/holding/point order required",
        ));
    }
    type PnlGroups = Vec<(ValuationBasis, Currency, Vec<CausalPnlAmounts>)>;
    let mut groups: [PnlGroups; 2] = [Vec::new(), Vec::new()];
    let mut trades = Vec::with_capacity(n);
    for (j, marks) in rows.into_iter().enumerate() {
        let local = marks
            .iter()
            .map(|m| effective(path, m, &marks[0], false))
            .collect::<ReconciliationResult<Vec<_>>>()?;
        let reporting = marks
            .iter()
            .map(|m| effective(path, m, &marks[0], true))
            .collect::<ReconciliationResult<Vec<_>>>()?;
        if marks.iter().any(|m| m.basis != marks[0].basis) {
            return Err(bad("basis", "original measurement basis cannot change"));
        }
        for (k, v) in [&local, &reporting].iter().enumerate() {
            let c = v[0].cash.currency();
            if let Some((_, _, sum)) = groups[k]
                .iter_mut()
                .find(|(b, currency, _)| *b == marks[0].basis && *currency == c)
            {
                for (s, x) in sum.iter_mut().zip(v.iter()) {
                    *s = combine(s, x, 1.0)?;
                }
            } else {
                groups[k].push((marks[0].basis, c, (*v).clone()));
            }
        }
        let ctd_transitions = (1..=path.input.causes.len())
            .map(|i| match (&marks[i - 1].ctd, &marks[i].ctd) {
                (Some(a), Some(b)) => Some(ScenarioCtdTransition {
                    before: a.clone(),
                    after: b.clone(),
                    selection_changed: a.selection_ties != b.selection_ties,
                    fair_deliverable_changed: a.fair_ties != b.fair_ties,
                    ties_present: a.selection_ties.len() > 1
                        || b.selection_ties.len() > 1
                        || a.fair_ties.len() > 1
                        || b.fair_ties.len() > 1,
                }),
                _ => None,
            })
            .collect();
        trades.push(CausalPnlTrade {
            id: path.input.points[0].positions[j].identity.id.clone(),
            local: explain(path, &local)?,
            reporting: explain(path, &reporting)?,
            marks,
            ctd_transitions,
        });
    }
    let totals = |g: Vec<(ValuationBasis, Currency, Vec<CausalPnlAmounts>)>| {
        g.into_iter()
            .map(|(basis, currency, v)| {
                Ok(CausalPnlTotal {
                    basis,
                    currency,
                    attribution: explain(path, &v)?,
                })
            })
            .collect::<ReconciliationResult<Vec<_>>>()
    };
    let [local, reporting] = groups;
    Ok(CausalPnlReport {
        input: path.input.clone(),
        trades,
        local_totals: totals(local)?,
        reporting_totals: totals(reporting)?,
        product_evaluations: n * count,
    })
}
