//! Closed operation contracts; unsupported economics never become proxy inputs.
use super::*;
use convex_analytics::workflow::{BondInverseSettings, BondPriceTarget, DatedYieldConvention};
use convex_bonds::contracts::BondProjectionContext;
use convex_core::{cashflows::*, fixings::FixingContext};
use convex_rates::{CashflowContext, SwapTerms};
use serde::{Deserialize, Serialize};

/// Complete versioned pure request, independent of service/registry state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LibraryRequest {
    /// Exactly LIBRARY_SCHEMA_VERSION; no guessed migration.
    pub schema_version: u32,
    /// All actual immutable resource values, not provider/repository handles.
    pub inputs: LibraryInputs,
    /// One closed financial operation.
    pub operation: LibraryOperation,
}
/// Explicit price representation and financial role.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BondMethod {
    /// Full fixed entitled cash at an explicit dated yield.
    Yield {
        /// Annualized decimal yield.
        decimal_rate: f64,
        /// Explicit compounding and time basis.
        convention: DatedYieldConvention,
    },
    /// Full fixed/projected cash on an exact reference and a constant Z/DM.
    DiscountSpread {
        /// Exact supplied curve revision.
        curve: CurveKey,
        /// Exact collateral/government meaning.
        role: BondReferenceRole,
        /// Annualized decimal spread, not rounded basis points.
        decimal_spread: f64,
    },
}
/// Reference meaning is separate from curve ID and from projection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BondReferenceRole {
    /// Same-currency collateral curve.
    Collateral {},
    /// Actual named government curve role.
    Government {
        /// Exact sovereign role, not a zero-rate proxy for G spread.
        sovereign: String,
    },
}
impl BondReferenceRole {
    pub(super) fn native(&self) -> convex_analytics::workflow::BondDiscountRole {
        match self {
            Self::Collateral {} => convex_analytics::workflow::BondDiscountRole::Collateral,
            Self::Government { sovereign } => {
                convex_analytics::workflow::BondDiscountRole::Government(sovereign.clone())
            }
        }
    }
}
/// Applicable full bond operations. Construction keeps optional terms; straight
/// pricing of an optional contract fails in the original native pricer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BondOperation {
    /// Full checked original contract and generated dates/principal.
    Construct {},
    /// Actual missing/published/future fixing requirements without guessing rates.
    Requirements {},
    /// Complete projected expected cash, accrued and entitlement.
    Cashflows {},
    /// Full original native PV/clean/dirty/accrued and consumed lineage.
    Price {
        /// Original quotation.
        method: BondMethod,
    },
    /// Full original cash quote risk, with native central and half-step stability.
    QuoteRisk {
        /// Explicit yield or frozen-cash spread representation.
        method: BondMethod,
        /// Actual raw instrument quote identity and native units.
        key: Box<convex_analytics::quote_risk::QuoteRiskKey>,
        /// Original signed face/trade quantity, scaled once.
        position: convex_analytics::quote_risk::RiskPosition,
        /// Exact immutable source snapshot.
        snapshot_id: convex_core::contracts::SnapshotId,
        /// Original native bump and stability policy.
        settings: convex_analytics::quote_risk::QuoteRiskSettings,
    },
    /// Full actual fixed-cash dated yield inverse with final native residual.
    YieldInverse {
        /// Original clean/dirty target.
        target: BondPriceTarget,
        /// Explicit quotation.
        convention: DatedYieldConvention,
        /// Original closed domain and both price/parameter tolerances.
        settings: BondInverseSettings,
    },
    /// Constant Z or frozen-projection DM inverse.
    SpreadInverse {
        /// Original clean/dirty target.
        target: BondPriceTarget,
        /// Exact discount reference.
        curve: CurveKey,
        /// Actual curve meaning.
        role: BondReferenceRole,
        /// Original unchanged domain/native residual bounds.
        settings: BondInverseSettings,
    },
}
/// IRS/OIS operations retain original leg signs, cash and par restrictions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SwapOperation {
    /// Original full native contract and independently generated dates.
    Construct {},
    /// Original complete eligible-history manifest.
    Requirements {},
    /// Actual native cash projection and known/forecast origins.
    Cashflows {},
    /// Native leg/net PV, accrued and applicable fair rate/spread.
    Price {},
}
/// Single-name credit operations preserve law, survival, initial cash and signs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CdsOperation {
    /// Full original terms and premium dates.
    Construct {},
    /// Signed premium/protection/default accrual/upfront PV and native cash detail.
    Price {},
    /// Running/clean/dirty conversion using the original complete pricer.
    Convert {
        /// Explicit quote and native unit.
        quote: convex_credit::CdsQuote,
        /// Unchanged caller domain and native residual/work bounds.
        settings: convex_credit::ConversionSettings,
    },
}
/// Complete pure R1 requests. Product models and server operations remain distinct.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LibraryOperation {
    /// Direct contextual own-yield sensitivity or positive reference hedge column.
    IcmaYieldRisk {
        /// Complete original economics, source/context/calendar/quantity revisions.
        input: Box<IcmaQuotationInput>,
        /// Exact original contextual time/compounding and bounded preparation.
        convention: convex_analytics::workflow::IcmaYieldConvention,
        /// Original cash selection; direct risk admits only unconditional plain cash.
        cash: convex_analytics::workflow::IcmaCashSelection,
        /// Full five-price sensitivity or existing original hedge vector.
        action: IcmaYieldRiskOperation,
    },
    /// Full supplied contextual ICMA fixed/workout quotations, without a Street alias.
    IcmaQuotation {
        /// Complete original terms, context, independent revisions and signed face.
        input: Box<IcmaQuotationInput>,
        /// Exact contextual time/compounding and bounded preparation policy.
        convention: convex_analytics::workflow::IcmaYieldConvention,
        /// Original plain cash or legally permitted conditional workout.
        cash: convex_analytics::workflow::IcmaCashSelection,
        /// Full price or unchanged closed-domain native inverse.
        action: IcmaQuotationOperation,
    },
    /// Complete contextual ICMA fixed legal-path prices/yields and defined YTW.
    IcmaWorkout {
        /// Full original economics, source/calendar/reference/quantity revisions.
        input: Box<IcmaQuotationInput>,
        /// Exact original contextual yield convention and per-path work bound.
        convention: convex_analytics::workflow::IcmaYieldConvention,
        /// Full actual purchase/information/notice state.
        context: convex_analytics::workflow::WorkoutContext,
        /// Complete legal-path price or yield/worst composition.
        action: IcmaWorkoutOperation,
    },
    /// Actual stochastic bounds on original term accrual/profile/reset segments.
    TermCouponOptions {
        /// Full economics, exact calendars/history/curves and model sources.
        input: Box<TermCouponOptionInput>,
    },
    /// Full hypothetical native coupon-model parameter scenario.
    TermCouponModelScenario {
        /// Complete original terms, eligible history, model and curves.
        input: Box<TermCouponOptionInput>,
        /// Exact original source/revision/index/observation/payment parameter.
        coordinate: convex_analytics::floating_coupon::CouponParameterCoordinate,
        /// Explicit finite move in the parameter's own native unit.
        delta: f64,
    },
    /// Full same-contract native coupon parameter first/second h/h2 derivatives.
    TermCouponParameterDerivatives {
        /// Complete original terms, histories, models and actual curves.
        input: Box<TermCouponOptionInput>,
        /// Exact original source/point/revision/parameter and native units.
        coordinate: convex_analytics::floating_coupon::CouponParameterCoordinate,
        /// Explicit native bump and first/second currency refinement policies.
        settings: convex_analytics::floating_coupon::CouponParameterDerivativeSettings,
    },
    /// Actual full coupon-model graph sensitivity with five native valuations.
    TermCouponGraphRisk {
        /// Original complete calibration graph and fit controls.
        graph: Box<super::curves::CalibrationInput>,
        /// Full original supplied coupon terms/history/model/curve bindings.
        input: Box<TermCouponOptionInput>,
        /// Original graph factor/quote-unit/response identity.
        key: Box<convex_analytics::quote_risk::QuoteRiskKey>,
        /// Explicit original face and signed holding units.
        position: convex_analytics::quote_risk::RiskPosition,
        /// Actual immutable caller information set.
        snapshot_id: convex_core::contracts::SnapshotId,
        /// Original central and mandatory half-step stability policy.
        settings: convex_analytics::quote_risk::QuoteRiskSettings,
    },
    /// Full native coupon-model column in existing constrained hedge units.
    TermCouponHedgeRisk {
        /// Original complete graph and fit policy.
        graph: Box<super::curves::CalibrationInput>,
        /// Full original coupon-model contract.
        input: Box<TermCouponOptionInput>,
        /// Identity, positive face reference and every intended common factor.
        risk: Box<convex_analytics::workflow::GraphHedgeRiskInput>,
    },
    /// Full original floating conditional cash and frozen-projection DM inverses.
    FloatingWorkout {
        /// Complete original floating terms, resources, revisions and signed face.
        input: Box<FloatingWorkoutInput>,
        /// Original information/purchase state and actual legal notice knowledge.
        context: convex_analytics::workflow::WorkoutContext,
        /// Full conditional price, single DM or every eligible original legal path.
        action: FloatingWorkoutOperation,
    },
    /// Complete supplied common-factor aggregation or constrained native-lot proposals.
    Hedge {
        /// Original risk vectors, revisions, current/pending native quantities and limits.
        problem: Box<convex_analytics::risk::hedging::constrained::HedgeProblem>,
        /// Actual pure operation; no live-order authority.
        action: super::hedges::HedgeOperation,
    },
    /// Full legal notice/principal-aware dated cash workout quotations.
    Workout {
        /// Complete original contract, calendars and independent source revisions.
        input: Box<WorkoutInput>,
        /// Actual information/purchase state and explicit notice policy or facts.
        context: convex_analytics::workflow::WorkoutContext,
        /// Explicit dated-cash compounding; no Street convention is inferred.
        convention: DatedYieldConvention,
        /// Original native single-path price/yield or complete all-date quotations.
        action: WorkoutOperation,
    },
    /// Full original cash quotation and signed settlement consideration.
    CashSecurity {
        /// Full native helper, quote revision and independent calendars.
        input: CashSecurityInput,
        /// Actual supplied information origin.
        reference_date: Date,
        /// Explicit dated-cash output yield convention.
        convention: DatedYieldConvention,
        /// Original closed inverse controls and price postcondition.
        settings: BondInverseSettings,
        /// Original quote or quantity-scaled invoice consideration.
        action: CashSecurityOperation,
    },
    /// G spread from actual full securities, never a zero-curve proxy.
    GovernmentSpread {
        /// Complete original target cash quotation.
        target: CashSecurityInput,
        /// Source securities and independent original calendar versions.
        benchmarks: Vec<GovernmentSecurityInput>,
        /// Exact supplied information origin.
        reference_date: Date,
        /// Common quotation, original inverse and explicit selection policy.
        settings: convex_analytics::cash_quotation::GovernmentSpreadSettings,
    },
    /// Full independently contextual ICMA actual-government-security G-spread.
    IcmaGovernmentSpread {
        /// Complete observed target terms, quote and independent context/pins.
        target: Box<convex_analytics::cash_quotation::IcmaCashSecurityQuote>,
        /// Every actual benchmark with independently supplied nominal mesh.
        benchmarks: Vec<convex_analytics::cash_quotation::IcmaGovernmentSecurityQuote>,
        /// Actual supplied information origin.
        reference_date: Date,
        /// Common output representation, selection and original bounded inverses.
        settings: convex_analytics::cash_quotation::IcmaGovernmentSpreadSettings,
    },
    /// Full contextual ICMA G-spread-to-price with price/spread postconditions.
    IcmaGovernmentPrice {
        /// Full actual target without a fabricated observed price or yield.
        target: Box<convex_analytics::cash_quotation::IcmaGovernmentSpreadTarget>,
        /// Every actual original government security/context/source revision.
        benchmarks: Vec<convex_analytics::cash_quotation::IcmaGovernmentSecurityQuote>,
        /// Actual supplied information origin.
        reference_date: Date,
        /// Complete common convention, inverse and separate spread residual bound.
        settings: convex_analytics::cash_quotation::IcmaGovernmentSpreadPriceSettings,
        /// Actual independent spread quote/revision and DecimalRate/BasisPoints unit.
        quote: convex_analytics::cash_quotation::GovernmentSpreadQuote,
    },
    /// Complete dated-cash price/invoice from an actual-security G-spread quote.
    GovernmentSpreadPrice {
        /// Actual target reference/terms/settlement and independent calendars.
        target: GovernmentSpreadTargetInput,
        /// Every full actual benchmark quotation in its original source order.
        benchmarks: Vec<GovernmentSecurityInput>,
        /// Exact supplied information origin, separate from settlement.
        reference_date: Date,
        /// Common quotation/inverse policy and explicit decimal spread residual.
        settings: convex_analytics::cash_quotation::GovernmentSpreadPriceSettings,
        /// Original fractional spread observation and its separate quote revision.
        quote: convex_analytics::cash_quotation::GovernmentSpreadQuote,
        /// Exact signed original-face notional and currency, applied once.
        position: convex_analytics::quote_risk::RiskPosition,
    },
    /// Discover actual public routes and explicit native/client limitations.
    Capabilities {},
    /// Validate and echo complete supplied inputs after checked reconstruction.
    Roundtrip {},
    /// Build and inspect complete native supplied curve recipes.
    Curves {},
    /// Fallible native date/tenor/DF/forward/survival/hazard queries.
    CurveQuery {
        /// Exact supplied immutable curve.
        curve: CurveKey,
        /// Actual native query convention and period.
        query: CurveQuery,
    },
    /// Convert actual dates and model tenors using the supplied curve time basis.
    CurveTime {
        /// Exact immutable curve; no nominal year approximation.
        curve: CurveKey,
        /// Native bounded conversion with explicit inverse rounding.
        action: CurveTimeOperation,
    },
    /// Native discount/rate conversions over an explicit actual accrual period.
    RateConversion {
        /// Actual dates/day count or a caller-supplied model year fraction.
        period: RateConversionPeriod,
        /// Original checked compounding conversion.
        action: RateConversionOperation,
    },
    /// Actual bounded dated global money-market fitter.
    CurveFit {
        /// Full native original recipe.
        recipe: CurveRecipe,
        /// Original dated parameter nodes.
        pillars: Vec<Date>,
        /// Original finite-difference/identification controls.
        controls: FitNumericalControls,
    },
    /// Original bounded parametric continuous-zero fitting, never a node-grid fit.
    ParametricFit {
        /// Original complete model/initial parameters and dated observations.
        recipe: CurveRecipe,
        /// Original parameter bounds, native objective, rank and work policy.
        settings: ParametricFitSettings,
    },
    /// Shared anchored schedules and exact irregular references.
    Schedule {
        /// Full original schedule terms.
        terms: ScheduleSpec,
        /// Independent contractual date transformations.
        dates: DateRules,
        /// Exact accrual calendar.
        accrual: convex_rates::ReferencePin,
        /// Exact payment calendar.
        payment: convex_rates::ReferencePin,
        /// Exact independently pinned fixing calendar.
        fixing: convex_rates::ReferencePin,
        /// Exact observation calendar.
        observation: convex_rates::ReferencePin,
        /// Exact ex-coupon calendar.
        ex_coupon: convex_rates::ReferencePin,
    },
    /// Inspect actual eligible fixing revisions without making forecasts.
    Fixings {
        /// Complete original request manifest.
        requirements: Vec<convex_core::fixings::FixingRequirement>,
        /// Actual economic and information cutoff/policy.
        context: FixingContext,
    },
    /// Full fixed, stepped, amortizing, floating or optional bond construction.
    Bond {
        /// Full original contract and versioned calendars.
        bond: Box<BondInput>,
        /// Complete settlement/entitlement/fixing/accrual-prefix state.
        context: BondProjectionContext,
        /// Exact projection role for actual permitted future observations.
        projection: Option<CurveKey>,
        /// One applicable full operation.
        action: BondOperation,
    },
    /// Full term IRS or OIS.
    Swap {
        /// Exact native full trade/leg/collateral/reset terms.
        terms: Box<SwapTerms>,
        /// Actual known-at/history/payment state.
        context: CashflowContext,
        /// One native operation.
        action: SwapOperation,
    },
    /// Full independently dated two-floating-leg basis trade.
    BasisSwap {
        /// Full native terms and explicit side/projection mappings.
        terms: Box<convex_rates::basis::BasisSwapTerms>,
        /// Actual original eligible history/cutoffs.
        context: CashflowContext,
    },
    /// Full single-name CDS.
    Cds {
        /// Complete entity, law, dates, recovery and exact curve versions.
        terms: Box<convex_credit::CdsTerms>,
        /// Surviving state, valuation date and actual initial cash state.
        context: convex_credit::CdsPricingContext,
        /// One applicable native operation.
        action: CdsOperation,
    },
    /// Original complete calibrated credit scenario/CS01/recovery/JTD workflow.
    CreditRisk {
        /// Full original calibration and surviving trade inputs.
        input: Box<CreditRiskInput>,
        /// One original native scenario/risk/default operation.
        action: CreditRiskOperation,
    },
    /// Full product helper descriptions, native quotes and quote conversions.
    Helper {
        /// Original helper terms and actual resources.
        input: Box<HelperInput>,
    },
    /// Actual independently funded nominal-bond repo/coupon carry.
    RepoForward {
        /// Full original helper, funding and calendar assumptions.
        input: Box<RepoForwardInput>,
    },
    /// Full actual zero/bill terms and original quotation.
    Zero {
        /// Full original zero economics with exact decimal strings.
        input: Box<ZeroInput>,
        /// Complete original quote and settlement, or validation only.
        action: ZeroOperation,
    },
    /// Full native option-aware model, price, inverse and resolved factor risk.
    OptionalBond {
        /// Complete original bond and immutable model inputs.
        input: Box<super::products::OptionalInput>,
        /// Original native operation/settings.
        action: super::products::OptionalOperation,
    },
    /// Full dated rate-futures projection and clearing economics.
    RateFuture {
        /// Actual complete stamped exchange inputs.
        input: Box<super::products::RateFutureInput>,
        /// Original native projection/risk/inverse operation.
        action: super::products::RateFutureOperation,
    },
    /// Full original calibration graphs and risk operations.
    Calibration {
        /// Complete original recipes, history policy and helper revisions.
        input: Box<super::curves::CalibrationInput>,
        /// Full build or original graph scenario.
        action: super::curves::CalibrationOperation,
    },
    /// Calculate a complete actual common-factor reference hedge column.
    GraphHedgeRisk {
        /// Complete original graph and native helper/calibration policy.
        graph: Box<super::curves::CalibrationInput>,
        /// Full original product and frozen history/settlement/source context.
        product: Box<super::risk::RiskProductInput>,
        /// Explicit identity, native reference and every required factor.
        input: Box<convex_analytics::workflow::GraphHedgeRiskInput>,
    },
    /// Complete original issuer-basis/survival cash into common-factor hedges.
    CashBasisHedgeRisk {
        /// Original full graph with complete native helpers and policies.
        graph: Box<super::curves::CalibrationInput>,
        /// Actual full security, observation, settlement and calendar revisions.
        security: Box<CashSecurityInput>,
        /// Full model, checked legal mapping and complete factor manifest.
        input: Box<CashBasisHedgeInput>,
    },
    /// Full causal mixed-book lifecycle/cash-event model P&L reconciliation.
    BookPnl {
        /// Complete separately admitted before/after/path and native product data.
        input: Box<super::reconciliation::MixedBookPnlInput>,
    },
    /// Explicit original cohort cash ownership and actual physical handoff P&L.
    CohortBookPnl {
        /// Complete original before/path values, cohort entitlements and assets.
        input: Box<super::reconciliation::CohortBookPnlInput>,
    },
    /// Pure original-state replay and one transition; no ledger commitment.
    Lifecycle {
        /// Full original opening terms and each supplied event/input.
        input: Box<super::events::LifecycleInput>,
    },
    /// Full original government future all-member carry/invoice/CTD and risk.
    BondFuture {
        /// Exact original basket, cash bonds, financing and source stamps.
        input: Box<super::products::BondFutureInput>,
        /// Original native price/risk operation.
        action: super::products::BondFutureOperation,
    },
    /// Expected portfolio manifest, signed quantity and reporting FX aggregation.
    Aggregate {
        /// Exact native supplied expected risk/FX/coverage inputs.
        input: Box<super::risk::AggregateInput>,
    },
    /// Actual stamped cash-hedge prices and frozen-model risk.
    CashHedge {
        /// Full original security, universe, funding and ownership.
        input: Box<CashHedgeInput>,
        /// Original native price or central risk.
        action: CashHedgeOperation,
    },
    /// Original pure futures settlement, tick/risk and indicative roll math.
    FuturesMath {
        /// Full original native inputs for exactly one operation.
        action: Box<FuturesMathOperation>,
    },
    /// Full legal/entity/recovery/basis admission for an indicative bond-CDS hedge.
    BondCdsHedge {
        /// Original actual mapping, credit and liquidity inputs.
        input: Box<BondCdsHedgeInput>,
    },
    /// Exact lattice rounding and residual for an indicative future hedge.
    RoundHedge {
        /// Complete dated native contract, retaining exact multiplier/ticks.
        contract: Box<convex_core::futures::FuturesContract>,
        /// Exact target risk in currency per +1bp.
        target_currency_per_bp: convex_core::contracts::ExactDecimal,
        /// Actual risk per long contract in the same currency/factor basis.
        per_contract_currency_per_bp: convex_core::contracts::ExactDecimal,
        /// Actual explicit rounding.
        rounding: convex_analytics::futures::FuturesRounding,
    },
}
