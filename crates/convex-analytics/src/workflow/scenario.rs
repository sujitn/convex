//! Bounded same-information-set full model marking and ordered attribution.
use super::*;
use crate::{
    cash_calibration as c, futures as f, quote_risk as q, risk::hedging::constrained::HedgeFxPin,
};
use convex_core::{
    contracts::{ExactDecimal, ModelMoney, SnapshotId},
    Currency,
};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde::{Deserialize, Serialize};

mod product;
pub use product::*;

/// Original failures; no partial book or invented missing-position zero.
#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    /// Original quantity/context/native valuation failure.
    #[error(transparent)]
    Workflow(#[from] WorkflowError),
    /// Original helper/ICMA/credit cash error.
    #[error(transparent)]
    Cash(#[from] c::CashCalibrationError),
    /// Original graph/factor admission or fit failure.
    #[error(transparent)]
    Risk(#[from] q::QuoteRiskError),
    /// Original exact supplied curve/domain failure.
    #[error(transparent)]
    Curve(#[from] convex_curves::contracts::CurveContractError),
}
/// All-or-error full model scenario report.
pub type ScenarioResult<T> = Result<T, ScenarioError>;

/// Signed current and hypothetical pending quantity in the same native units.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ScenarioPositionInput {
    /// Original security, economics/calculation manifests and legal entity.
    pub identity: GraphHedgeIdentity,
    /// Current native position, relative to the original complete trade.
    pub current: q::RiskPosition,
    /// Signed pending quantity, not an executed trade or posted balance.
    pub pending_quantity: ExactDecimal,
}
/// Explicit total graph/product reservation; original native bounds still apply.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ScenarioWorkPolicy {
    /// Baseline + every ordered prefix + every standalone move, at most33.
    pub max_graph_evaluations: usize,
    /// Complete product calls across all points/positions, at most4224.
    pub max_product_evaluations: usize,
}
/// Full immutable hypothetical scenario. Snapshot/event/time changes are absent
/// deliberately: those are different causal inputs and require lifecycle P&L.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BookScenarioInput {
    /// Original common immutable information set, never a scenario publication.
    pub snapshot_id: SnapshotId,
    /// Exact expected positions; omissions and duplicates fail.
    pub positions: Vec<ScenarioPositionInput>,
    /// Declared ordered path of1..16 distinct compatible original coordinates.
    pub moves: Vec<q::QuoteGraphMove>,
    /// Explicit monetary reporting currency.
    pub reporting_currency: Currency,
    /// Exact positive direct same-snapshot FX observation for each foreign currency.
    pub fx: Vec<HedgeFxPin>,
    /// Explicit bounded work admission before numerical scenario evaluation.
    pub policy: ScenarioWorkPolicy,
}

/// One complete actual graph.0 is baseline,1..F ordered prefixes,F+1..2F singles.
#[derive(Debug)]
pub struct BookScenarioPoint {
    /// All actual simultaneous coordinates on this point, in declared order.
    pub moves: Vec<q::QuoteGraphMove>,
    /// Full rebuilt/frozen graph, original fit and publication diagnostics.
    pub graph: q::QuoteGraphScenario,
}
/// Privately admitted path; cannot be fabricated from supplied monetary results.
#[derive(Debug)]
pub struct PreparedBookScenarios {
    input: BookScenarioInput,
    points: Vec<BookScenarioPoint>,
}
impl PreparedBookScenarios {
    /// Complete original immutable declaration.
    pub fn input(&self) -> &BookScenarioInput {
        &self.input
    }
    /// Complete actual baseline/prefix/standalone graph diagnostics.
    pub fn points(&self) -> &[BookScenarioPoint] {
        &self.points
    }
}

fn bad(field: &'static str, reason: &str) -> ScenarioError {
    invalid(field, reason).into()
}
fn quantity(input: &ScenarioPositionInput, pending: bool) -> ScenarioResult<q::RiskPosition> {
    let mut p = input.current.clone();
    p.quantity = if pending {
        input.pending_quantity
    } else {
        ExactDecimal::new(
            p.quantity
                .decimal()
                .checked_add(input.pending_quantity.decimal())
                .ok_or_else(|| bad("quantity", "projected Decimal addition overflow"))?,
        )
    };
    p.scale()?;
    Ok(p)
}
fn fx_rate(input: &BookScenarioInput, currency: Currency) -> ScenarioResult<f64> {
    if currency == input.reporting_currency {
        return Ok(1.0);
    }
    let pin = input
        .fx
        .iter()
        .find(|p| p.from == currency)
        .ok_or_else(|| bad("FX", "exact foreign-currency reporting observation missing"))?;
    pin.rate
        .decimal()
        .to_f64()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| bad("FX", "positive finite original conversion required"))
}
fn validate(input: &BookScenarioInput) -> ScenarioResult<()> {
    let n = input.positions.len();
    if n == 0 || n > 128 || input.moves.is_empty() || input.moves.len() > 16 {
        return Err(bad(
            "book",
            "1..128 positions and1..16 original moves required",
        ));
    }
    let count = 1 + 2 * input.moves.len();
    q::validate_quote_graph_moves(&input.moves)?;
    if input.policy.max_graph_evaluations == 0
        || input.policy.max_graph_evaluations > 33
        || count > input.policy.max_graph_evaluations
        || input.policy.max_product_evaluations == 0
        || input.policy.max_product_evaluations > 4224
        || count * n > input.policy.max_product_evaluations
    {
        return Err(bad(
            "scenario work",
            "full graph/product reservation exhausted",
        ));
    }
    for (i, p) in input.positions.iter().enumerate() {
        if input.positions[..i]
            .iter()
            .any(|v| v.identity.id == p.identity.id)
        {
            return Err(bad("positions", "duplicate original position identity"));
        }
        p.current.scale()?;
        quantity(p, true)?;
        quantity(p, false)?;
        fx_rate(input, p.current.currency)?;
    }
    for (i, p) in input.fx.iter().enumerate() {
        if p.from == input.reporting_currency
            || p.to != input.reporting_currency
            || p.snapshot_id != input.snapshot_id
            || p.rate.decimal() <= Decimal::ZERO
            || input.fx[..i].iter().any(|x| x.from == p.from)
            || !input.positions.iter().any(|x| x.current.currency == p.from)
        {
            return Err(bad(
                "FX",
                "one exact same-snapshot direct observation per consumed foreign currency",
            ));
        }
    }
    Ok(())
}
/// Prepare every complete nonlinear graph after total work/manifest admission.
/// Original graph helper/solver/history and publication limits remain unchanged.
pub fn prepare_book_scenarios(
    graph: &q::PreparedQuoteRiskGraph<'_>,
    input: &BookScenarioInput,
) -> ScenarioResult<PreparedBookScenarios> {
    validate(input)?;
    let mut points = vec![BookScenarioPoint {
        moves: Vec::new(),
        graph: graph.baseline().clone(),
    }];
    for i in 1..=input.moves.len() {
        let moves = input.moves[..i].to_vec();
        points.push(BookScenarioPoint {
            graph: q::evaluate_quote_graph_moves(graph, &moves)?,
            moves,
        });
    }
    for m in &input.moves {
        let moves = vec![m.clone()];
        points.push(BookScenarioPoint {
            graph: q::evaluate_quote_graph_moves(graph, &moves)?,
            moves,
        });
    }
    Ok(PreparedBookScenarios {
        input: input.clone(),
        points,
    })
}

/// Distinct current, pending and projected signed monetary amounts.
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioAmounts {
    /// Current holdings scaled once.
    pub current: ModelMoney,
    /// Hypothetical pending quantity, not realized cash.
    pub pending: ModelMoney,
    /// Exact Decimal current+pending quantity scaled once.
    pub projected: ModelMoney,
}
fn money(v: f64, c: Currency) -> ScenarioResult<ModelMoney> {
    Ok(q::risk_reference_pv(v, c)?)
}
impl ScenarioAmounts {
    fn map(&self, currency: Currency, operation: impl Fn(f64) -> f64) -> ScenarioResult<Self> {
        Ok(Self {
            current: money(operation(self.current.value().get()), currency)?,
            pending: money(operation(self.pending.value().get()), currency)?,
            projected: money(operation(self.projected.value().get()), currency)?,
        })
    }
    fn subtract(&self, other: &Self) -> ScenarioResult<Self> {
        let currency = self.current.currency();
        Ok(Self {
            current: money(
                self.current.value().get() - other.current.value().get(),
                currency,
            )?,
            pending: money(
                self.pending.value().get() - other.pending.value().get(),
                currency,
            )?,
            projected: money(
                self.projected.value().get() - other.projected.value().get(),
                currency,
            )?,
        })
    }
    fn add(&self, other: &Self) -> ScenarioResult<Self> {
        let currency = self.current.currency();
        Ok(Self {
            current: money(
                self.current.value().get() + other.current.value().get(),
                currency,
            )?,
            pending: money(
                self.pending.value().get() + other.pending.value().get(),
                currency,
            )?,
            projected: money(
                self.projected.value().get() + other.projected.value().get(),
                currency,
            )?,
        })
    }
}

/// Exact native amount meanings; full model values and diagnostics remain intact.
#[derive(Debug, Clone, Serialize)]
pub enum ScenarioValuation {
    /// Same original complete native cash/leg/history/model result.
    Product(Valuation),
    /// Full issuer promised/default-recovery model and actual entitled cash price.
    CashBasis(Box<c::BasisBondValuation>),
    /// Full delivery alternatives, individual models/residuals and reselected CTD.
    Delivery(Box<f::BondFuturesBasketValue>),
}
/// Complete original product valuation at one actual graph path point.
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioPositionValue {
    /// Full nonlinear model output, including CTD/model/known-fixing diagnostics.
    pub valuation: ScenarioValuation,
    /// Native original-reference economic PV or explicit futures quote level.
    pub reference_amount: ModelMoney,
    /// Native money; signs and quantity/reference unit applied exactly once.
    pub local: ScenarioAmounts,
    /// Explicit fixed same-snapshot FX conversion of each amount.
    pub reporting: ScenarioAmounts,
}
/// Ordered effects are path-dependent; standalone effects need not add to total.
#[derive(Debug, Clone, Serialize)]
pub struct ModelPnlAttribution {
    /// Actual marked baseline amounts.
    pub baseline: ScenarioAmounts,
    /// Actual full simultaneous scenario amounts.
    pub scenario: ScenarioAmounts,
    /// Full scenario minus baseline model marking.
    pub total_change: ScenarioAmounts,
    /// Each complete prefix minus previous prefix, in original declared order.
    pub ordered_effects: Vec<ScenarioAmounts>,
    /// Each actual standalone factor scenario minus original baseline.
    pub standalone_effects: Vec<ScenarioAmounts>,
    /// Full nonlinear change minus the sum of standalone effects.
    pub interaction: ScenarioAmounts,
    /// Actual total change minus sum of ordered effects; never clamped to zero.
    pub reconciliation_residual: ScenarioAmounts,
}
fn attribution(values: &[ScenarioAmounts], factors: usize) -> ScenarioResult<ModelPnlAttribution> {
    let baseline = values[0].clone();
    let scenario = values[factors].clone();
    let total_change = scenario.subtract(&baseline)?;
    let ordered_effects = (1..=factors)
        .map(|i| values[i].subtract(&values[i - 1]))
        .collect::<ScenarioResult<Vec<_>>>()?;
    let standalone_effects = values[factors + 1..]
        .iter()
        .map(|v| v.subtract(&baseline))
        .collect::<ScenarioResult<Vec<_>>>()?;
    let sum = |rows: &[ScenarioAmounts]| -> ScenarioResult<ScenarioAmounts> {
        rows.iter().try_fold(
            baseline.map(baseline.current.currency(), |_| 0.0)?,
            |sum, v| sum.add(v),
        )
    };
    Ok(ModelPnlAttribution {
        interaction: total_change.subtract(&sum(&standalone_effects)?)?,
        reconciliation_residual: total_change.subtract(&sum(&ordered_effects)?)?,
        baseline,
        scenario,
        total_change,
        ordered_effects,
        standalone_effects,
    })
}
/// Separate independent selection/fair winner sets, including ties at each step.
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioCtdTransition {
    /// Actual preceding ordered prefix state.
    pub before: f::BondFuturesCtdState,
    /// Actual next prefix state.
    pub after: f::BondFuturesCtdState,
    /// Complete selection winner/tie set changed.
    pub selection_changed: bool,
    /// Complete fair-value winner/tie set changed.
    pub fair_deliverable_changed: bool,
    /// Either endpoint has more than one selection/fair winner.
    pub ties_present: bool,
}
/// Complete privately produced position; results cannot be supplied as scalars.
#[derive(Debug, Serialize)]
pub struct ScenarioTradeResult<'p> {
    #[serde(skip)]
    path: &'p PreparedBookScenarios,
    input: ScenarioPositionInput,
    basis: ValuationBasis,
    values: Vec<ScenarioPositionValue>,
    local: ModelPnlAttribution,
    reporting: ModelPnlAttribution,
    ctd_transitions: Vec<ScenarioCtdTransition>,
    consumed_raw_factors: Vec<bool>,
}
impl ScenarioTradeResult<'_> {
    /// Full original admitted position and immutable source declaration.
    pub fn input(&self) -> &ScenarioPositionInput {
        &self.input
    }
    /// Full native baseline, ordered-prefix and standalone valuations.
    pub fn values(&self) -> &[ScenarioPositionValue] {
        &self.values
    }
    /// Native-currency model marking, ordered attribution and interactions.
    pub fn local(&self) -> &ModelPnlAttribution {
        &self.local
    }
    /// Explicit fixed same-snapshot reporting conversion and attribution.
    pub fn reporting(&self) -> &ModelPnlAttribution {
        &self.reporting
    }
    /// Every original independent CTD selection/fair transition and tie.
    pub fn ctd_transitions(&self) -> &[ScenarioCtdTransition] {
        &self.ctd_transitions
    }
}
/// Separate native currencies and measurement bases; no PV/quote-level netting.
#[derive(Debug, Serialize)]
pub struct ScenarioBookTotals {
    /// Discounted cash PV or unrounded indicative futures quote-level marking.
    pub basis: ValuationBasis,
    /// Actual local/reporting monetary currency.
    pub currency: Currency,
    /// Complete nonlinear marking and factor/interactions/residuals.
    pub attribution: ModelPnlAttribution,
}
/// Analytics-only deterministic hypothetical model P&L, never actual ledger P&L.
#[derive(Debug, Serialize)]
pub struct BookScenarioReport<'p> {
    /// Original declaration, all quantity/source pins and factor order.
    pub input: BookScenarioInput,
    /// All privately produced original full product revaluations.
    pub trades: Vec<ScenarioTradeResult<'p>>,
    /// Complete native totals separated by currency AND measurement basis.
    pub local_totals: Vec<ScenarioBookTotals>,
    /// Explicit fixed-FX reporting totals, separated by measurement basis.
    pub reporting_totals: Vec<ScenarioBookTotals>,
    /// Baseline + prefix + standalone graph points reserved upfront.
    pub graph_evaluations: usize,
    /// Full original product calls across every position/point.
    pub product_evaluations: usize,
    /// Actual native calibration/helper work across baseline and all scenarios.
    pub graph_price_evaluations: usize,
}

/// Complete exact coverage/FX aggregation from ONLY native-produced positions
/// bound to this original path. Missing/foreign results fail before aggregation.
pub fn aggregate_book_scenarios<'p>(
    path: &'p PreparedBookScenarios,
    trades: Vec<ScenarioTradeResult<'p>>,
) -> ScenarioResult<BookScenarioReport<'p>> {
    if trades.len() != path.input.positions.len()
        || trades
            .iter()
            .zip(&path.input.positions)
            .any(|(r, p)| !std::ptr::eq(r.path, path) || r.input.identity.id != p.identity.id)
    {
        return Err(bad(
            "book coverage",
            "all exact original positions/path in declared order required",
        ));
    }
    for (i, m) in path.input.moves.iter().enumerate() {
        if matches!(m.key.definition().coordinate, q::RiskCoordinate::Raw { .. })
            && m.key.definition().factor.definition().quote_type
                != convex_core::contracts::QuoteType::JointZeroRate
            && !trades.iter().any(|p| p.consumed_raw_factors[i])
        {
            return Err(bad(
                "raw factor",
                "no original complete product consumes the declared raw coordinate",
            ));
        }
    }
    let totals = |reporting: bool| -> ScenarioResult<Vec<ScenarioBookTotals>> {
        let mut groups: Vec<(ValuationBasis, Currency, Vec<ScenarioAmounts>)> = Vec::new();
        for trade in &trades {
            let currency = if reporting {
                path.input.reporting_currency
            } else {
                trade.input.current.currency
            };
            let amounts = trade
                .values
                .iter()
                .map(|v| {
                    if reporting {
                        v.reporting.clone()
                    } else {
                        v.local.clone()
                    }
                })
                .collect::<Vec<_>>();
            if let Some((_, _, sum)) = groups
                .iter_mut()
                .find(|(b, c, _)| *b == trade.basis && *c == currency)
            {
                for (s, v) in sum.iter_mut().zip(&amounts) {
                    *s = s.add(v)?;
                }
            } else {
                groups.push((trade.basis, currency, amounts));
            }
        }
        groups
            .into_iter()
            .map(|(basis, currency, amounts)| {
                Ok(ScenarioBookTotals {
                    basis,
                    currency,
                    attribution: attribution(&amounts, path.input.moves.len())?,
                })
            })
            .collect()
    };
    let local_totals = totals(false)?;
    let reporting_totals = totals(true)?;
    let graph_price_evaluations = path.points.iter().try_fold(0usize, |sum, p| {
        sum.checked_add(p.graph.price_evaluations)
            .ok_or_else(|| bad("scenario work", "native helper work count overflow"))
    })?;
    Ok(BookScenarioReport {
        input: path.input.clone(),
        graph_evaluations: path.points.len(),
        product_evaluations: path.points.len() * trades.len(),
        graph_price_evaluations,
        trades,
        local_totals,
        reporting_totals,
    })
}
