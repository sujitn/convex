//! Complete supplied risk, native quantities, constraints and proposal diagnostics.
use crate::quote_risk::{QuoteRiskKey, QuoteRiskSettings, QuoteSensitivity, RiskPosition};
use convex_core::{contracts::*, Currency, Date};
use convex_credit::{CreditReference, ReferencePin};
use serde::{Deserialize, Serialize};

// Nullable inputs are explicit fields: absence cannot choose continuous lots,
// identity FX or a missing credit scope on the caller's behalf.
fn required_option<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
#[cfg(feature = "schemars")]
fn nullable_schema<T: schemars::JsonSchema>(
    generator: &mut schemars::SchemaGenerator,
) -> schemars::Schema {
    generator.subschema_for::<Option<T>>()
}

/// Closed original product families; this tag does not qualify their pricing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HedgeProduct {
    /// Actual cash security, including separate issuer basis when applicable.
    CashBond,
    /// Actual single-name CDS; reference side stays inside original risk.
    Cds,
    /// Actual single-currency swap; reference leg side stays inside original risk.
    Irs,
    /// Actual dated contract, never a continuous symbol or guessed CTD.
    Futures,
}
/// Same-snapshot monetary conversion only; this is not portfolio FX delta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeFxPin {
    /// Native risk and cost currency.
    pub from: Currency,
    /// Reporting currency.
    pub to: Currency,
    /// Positive reporting money per one native money unit.
    pub rate: ExactDecimal,
    /// Actual FX observation identity and revision.
    pub observation: ReferencePin,
    /// Exact same information set as every risk result.
    pub snapshot_id: SnapshotId,
}
/// Full original reference risk. No absent factor is interpreted as zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeRiskVector {
    /// Expected holding/candidate identity.
    pub id: TradeId,
    /// Original product family, without product repricing claims.
    pub product: HedgeProduct,
    /// Actual instrument and reference-data revision.
    pub instrument: ReferencePin,
    /// Pin of the complete original terms, state and market-input manifest.
    /// Resolving or independently qualifying that record is the caller's job.
    pub economics: ReferencePin,
    /// Pin of the original full pricing/model calculation, not a cached scalar delta.
    pub calculation: ReferencePin,
    /// Actual owning legal entity; no cross-entity netting authorization is inferred.
    pub entity: EntityId,
    /// Actual settlement/economic cut-off used by the reference calculation.
    pub settlement: Date,
    /// Every admitted credit identity whose nonzero risk this product carries.
    pub credit_scope: Vec<CreditReference>,
    /// Positive original reference size already included in each native risk PV.
    pub reference_position: RiskPosition,
    /// All required factors in the problem's original order, including explicit zeros.
    pub sensitivities: Vec<QuoteSensitivity>,
    /// Required only when original monetary currency differs from reporting.
    #[serde(deserialize_with = "required_option")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "nullable_schema::<HedgeFxPin>")
    )]
    pub fx: Option<HedgeFxPin>,
}
/// Complete supplied current and pending state, relative to the same native side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeHolding {
    /// Actual complete reference risk.
    pub risk: HedgeRiskVector,
    /// Signed actual holdings; never gross absolute exposure.
    pub current_quantity: ExactDecimal,
    /// Signed pending exposure supplied by the application; explicit zero is allowed.
    pub pending_quantity: ExactDecimal,
}
/// Target and limits in the full key's monetary normalization and reporting currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeFactor {
    /// Original curve/quote/node/entity/model/basis/unit/response identity.
    pub key: QuoteRiskKey,
    /// Original bump, stability and numerical policy; every vector must match.
    pub risk_settings: QuoteRiskSettings,
    /// Exact legal/recovery/document scope for credit, hazard or recovery factors.
    #[serde(deserialize_with = "required_option")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "nullable_schema::<CreditReference>")
    )]
    pub credit: Option<CreditReference>,
    /// Positive normalization in the factor's reported monetary units.
    pub scale: f64,
    /// Positive dimensionless squared-error weight.
    pub weight: f64,
    /// Desired final exposure, including supplied pending quantities.
    pub target: f64,
    /// Closed lower final factor bound.
    pub lower: f64,
    /// Closed upper final factor bound.
    pub upper: f64,
}
/// Caller-admitted candidate eligibility; this is not a freshness or order check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeEligibility {
    /// Supplied eligibility decision; false blocks solving.
    pub eligible: bool,
    /// Exact independent policy/reference decision revision.
    pub decision: ReferencePin,
    /// Actual decision explanation, retained even for eligible candidates.
    pub reason: String,
}
/// Actual nonnegative side costs per positive original reference size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeCosts {
    /// Original risk currency; same explicit FX conversion is applied once.
    pub currency: Currency,
    /// Buy cost, supplied as exact monetary input, including explicit zero.
    pub buy_per_reference: ExactDecimal,
    /// Sell cost, supplied independently.
    pub sell_per_reference: ExactDecimal,
    /// Original cost observation/assumption revision, separate from model price.
    pub observation: ReferencePin,
}
/// Native trade and final holding intervals, side liquidity and lot lattice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeCandidate {
    /// Full original reference risk; matching held IDs require exact vector equality.
    pub risk: HedgeRiskVector,
    /// Actual supplied eligibility; no live permission is inferred.
    pub eligibility: HedgeEligibility,
    /// Closed native signed trade interval.
    pub trade_lower: ExactDecimal,
    /// Closed native signed trade interval.
    pub trade_upper: ExactDecimal,
    /// Closed final native holding interval, including current and pending exposure.
    pub holding_lower: ExactDecimal,
    /// Closed final native holding interval.
    pub holding_upper: ExactDecimal,
    /// Nonnegative available buy size in original quantity units.
    pub buy_available: ExactDecimal,
    /// Nonnegative available sell size; distinct from long-side liquidity.
    pub sell_available: ExactDecimal,
    /// Positive zero-anchored trade lot. Contracts require a positive integer lot.
    /// None explicitly requests continuous notional quantities.
    #[serde(deserialize_with = "required_option")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "nullable_schema::<ExactDecimal>")
    )]
    pub lot: Option<ExactDecimal>,
    /// Original actual side costs; these are not execution prices or cash postings.
    pub costs: HedgeCosts,
}
/// Explicit scaled numerical and bounded work policy; no solver defaults inferred.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeSolveSettings {
    /// Exact caller-admitted numerical policy revision.
    pub policy_version: Version,
    /// Relative singular-value cutoff in (0,1e-3].
    pub rank_relative_tolerance: f64,
    /// Largest allowed scaled design condition number, in `[1,1e4]`.
    pub max_condition: f64,
    /// Dimensionless primal/KKT backward-error bound, in (0,1e-6].
    pub solver_tolerance: f64,
    /// Final absolute factor feasibility tolerance, in each original monetary unit.
    /// This is independent of solver stopping and factor scaling.
    pub factor_tolerance: f64,
    /// Final absolute reporting-money cost feasibility tolerance.
    pub cost_tolerance: f64,
    /// Positive reporting-money normalization for transaction costs.
    pub cost_scale: f64,
    /// Nonnegative dimensionless objective weight on normalized transaction costs.
    pub cost_weight: f64,
    /// 1..100000 actual active-set attempts plus discrete lattice points.
    pub max_work: usize,
    /// 1..1000 maximum iterations inside each bounded SVD decomposition.
    pub svd_iterations: usize,
}
/// Pure immutable complete-factor proposal request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeProblem {
    /// Actual common information set; reference risks and FX must match.
    pub snapshot_id: SnapshotId,
    /// One actual owning entity; this calculation does not authorize it.
    pub entity: EntityId,
    /// Actual common economic cut-off, without clock access or freshness inference.
    pub settlement: Date,
    /// Monetary reporting currency, distinct from factor quotation currency.
    pub reporting_currency: Currency,
    /// Exact current/pending application-state revision.
    pub position_state: ReferencePin,
    /// Complete expected holding IDs; missing, unrequested and duplicate entries fail.
    pub expected_holdings: Vec<TradeId>,
    /// Actual complete current and pending risk; at most256 holdings.
    pub holdings: Vec<HedgeHolding>,
    /// 1..16 complete factors in preserved original order.
    pub factors: Vec<HedgeFactor>,
    /// At most4 candidates, in preserved original order; empty is aggregate-only.
    pub candidates: Vec<HedgeCandidate>,
    /// Nonnegative maximum reporting-money transaction cost, including explicit zero.
    pub max_cost: f64,
    /// Original scaling/rank/work/feasibility policy.
    pub settings: HedgeSolveSettings,
}
/// Complete same-key current and pending sums in original factor order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeExposure {
    /// Full original problem, including every source, quantity, constraint and policy.
    pub problem: HedgeProblem,
    /// Scaled current reporting exposure per complete factor.
    pub current: Vec<f64>,
    /// Separately scaled pending reporting exposure.
    pub pending: Vec<f64>,
    /// Actual current plus pending before proposed trades.
    pub before: Vec<f64>,
}
/// Explicit qualification of a diagnostic; failed states carry no proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HedgeSolveStatus {
    /// Completed supplied-model solve and all final quantity/factor/cost checks.
    Feasible,
    /// Completed continuous search found no feasible point within declared tolerance.
    Infeasible,
    /// Continuous solution exists, but the complete admitted lot lattice is infeasible.
    DiscreteInfeasible,
    /// Full scaled candidate matrix is not identified; no ridge or pseudoinverse substitute.
    RankDeficient,
    /// Original scaled design exceeds the caller's conditioning policy.
    IllConditioned,
    /// Original work budget exhausted; no incumbent is promoted to a proposal.
    WorkExhausted,
    /// Bounded numerical solve or final postcondition remains unresolved.
    NumericalFailure,
}
/// Every proposal is a calculation, without executable order authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HedgeAuthority {
    /// No order submission, limit authorization, cash posting or provider certification.
    ProposalOnly,
}
/// Complete checked native quantity result; costs and risk are model calculations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeSolution {
    /// Exact emitted native signed trade quantities in candidate order.
    pub quantities: Vec<ExactDecimal>,
    /// Original native size ratios, evaluated once from emitted quantities.
    pub reference_multiples: Vec<f64>,
    /// Final native holdings, including supplied pending state.
    pub final_holdings: Vec<ExactDecimal>,
    /// Full factor exposure after all trades, in original factor order.
    pub after: Vec<f64>,
    /// After minus desired target, in each original monetary normalization.
    pub residual: Vec<f64>,
    /// Positive reporting cost per candidate, using actual supplied signed side.
    pub candidate_costs: Vec<f64>,
    /// Complete unrounded reporting-money transaction cost.
    pub total_cost: f64,
    /// One-half sum of weighted squared scaled target residuals.
    pub tracking_objective: f64,
    /// Tracking objective plus explicitly weighted normalized costs.
    pub objective: f64,
    /// Every original hard constraint, including exact native and lot checks.
    pub constraints: Vec<HedgeConstraintCheck>,
}
/// One measured hard postcondition; names identify original candidate or factor index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeConstraintCheck {
    /// Original indexed constraint, e.g. factor.0, holding.1 or lot.2.
    pub name: String,
    /// All emitted native quantities use Decimal; factor/cost checks use declared bounds.
    pub satisfied: bool,
    /// Measured nonnegative original-unit violation; zero for satisfied exact checks.
    pub violation: f64,
    /// Actual allowed numerical violation; zero for exact native constraints.
    pub tolerance: f64,
}
/// Actual scaled design diagnostics; no financial market/model certificate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeRank {
    /// Required candidate column count.
    pub columns: usize,
    /// Actual rank at the original relative singular-value threshold.
    pub rank: usize,
    /// Full scaled singular values, largest first.
    pub singular_values: Vec<f64>,
    /// Largest/smallest when finite and identified; absent for singular matrices.
    pub condition: Option<f64>,
}
/// Work is deterministic attempt counts, not elapsed time or latency assurance.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeWork {
    /// Actual active-set systems considered, including dependent systems.
    pub active_sets: usize,
    /// Actual discrete combinations visited, without guessed or rounded CTD quantities.
    pub discrete_points: usize,
    /// Actual bounded SVD calls, including rank and dependent KKT systems.
    pub svd_decompositions: usize,
}
/// Complete diagnostic; only Feasible carries a final proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeSolveReport {
    /// All original request data and separately aggregated current/pending exposure.
    pub exposure: HedgeExposure,
    /// Measured outcome, never an executable classification.
    pub status: HedgeSolveStatus,
    /// Actual diagnostic explanation.
    pub message: String,
    /// Explicit absence of order authority.
    pub authority: HedgeAuthority,
    /// Original scaled full-matrix rank/condition diagnostics.
    pub rank: HedgeRank,
    /// Continuous relaxation, separately checked without imposing trade lots.
    pub continuous: Option<HedgeSolution>,
    /// Final checked complete lot/continuous proposal, absent on every failed outcome.
    pub proposal: Option<HedgeSolution>,
    /// Actual work counts, retaining original maximum in exposure.problem.settings.
    pub work: HedgeWork,
}
