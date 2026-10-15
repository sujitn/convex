//! Full original cash hedge and futures monetary/roll operations.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::futures as f;
use convex_core::{contracts::*, futures::*};
use serde::{Deserialize, Serialize};

/// Complete-factor supplied-risk operations; the existing scalar advisor is separate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HedgeOperation {
    /// Validate and aggregate all current and pending factors/quantities.
    Aggregate {},
    /// Bounded continuous/lot solve with full original constraint diagnostics.
    Solve {},
}
pub(super) fn hedge(
    problem: &convex_analytics::risk::hedging::constrained::HedgeProblem,
    action: &HedgeOperation,
) -> LibraryResult<Value> {
    use convex_analytics::risk::hedging::constrained as h;
    match action {
        HedgeOperation::Aggregate {} => value(h::aggregate_hedge_risk(problem)?),
        HedgeOperation::Solve {} => value(h::solve_hedge_proposal(problem)?),
    }
}

pub(super) fn directional_hedge(
    problem: &convex_analytics::risk::hedging::constrained::Problem<
        convex_analytics::quote_risk::HedgeSensitivityEvidence,
    >,
    action: &HedgeOperation,
) -> LibraryResult<Value> {
    use convex_analytics::risk::hedging::constrained as h;
    match action {
        HedgeOperation::Aggregate {} => value(h::aggregate_directional_hedge_risk(problem)?),
        HedgeOperation::Solve {} => value(h::solve_directional_hedge_proposal(problem)?),
    }
}

/// Actual checked full security/universe and separate model/execution assumptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashHedgeInput {
    /// Complete original cash universe, side quotes, sizes, fees and eligibility.
    pub universe: CashHedgeUniverse,
    /// Actual selected security revision.
    pub instrument: InstrumentPin,
    /// Actual security source/quality/availability.
    pub security_stamp: ReferenceStamp,
    /// Full original checked bond economics.
    pub bond: BondInput,
    /// Signed side/face/owned inventory and dated holding period.
    pub request: CashHedgeRequest,
    /// Independent original model discount input and source.
    pub discount: StampedCurve,
    /// Actual separate own-cash financing, iff required by original funding.
    pub own_cash: Option<f::FuturesCarryRate>,
    /// Original actual short fee basis, iff actually used.
    pub borrow_fee_basis: Option<f::FuturesBorrowFeeBasis>,
    /// Actual economic/cutoff/quality input decisions.
    pub context: EligibilityContext,
}
/// Original pure model cash hedge pricing/risk; no executable hedge authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CashHedgeOperation {
    /// Complete original model/side/financing/missing-input results.
    Price {},
    /// Actual central same-model rate sensitivity.
    Risk {
        /// Original native decimal continuous-zero bump.
        bump: ExactDecimal,
    },
}
pub(super) fn cash_hedge(
    input: &CashHedgeInput,
    action: &CashHedgeOperation,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let bond = r.bond(&input.bond)?;
    let native = f::CashHedgePricingInputs {
        universe: &input.universe,
        security: f::FuturesBond {
            instrument: &input.instrument,
            stamp: &input.security_stamp,
            bond: &bond,
        },
        request: input.request,
        discount: input.discount.native(r)?,
        own_cash: input.own_cash.as_ref(),
        borrow_fee_basis: input.borrow_fee_basis,
        context: input.context,
    };
    match action {
        CashHedgeOperation::Price {} => value(f::price_cash_hedge(&native)?),
        CashHedgeOperation::Risk { bump } => value(f::cash_hedge_risk(&native, *bump)?),
    }
}
/// Full named front/back roll leg with separately stamped comparable native risk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollLeg {
    /// Complete actual dated original exchange contract.
    pub contract: FuturesContract,
    /// Actual physical basket, absent for rate futures.
    pub basket: Option<DeliveryBasket>,
    /// Actual on-tick execution quote, never a continuous-contract symbol.
    pub quote: f::FuturesMark,
    /// Full actual comparable risk source and exact factor identity.
    pub risk: f::FuturesRollRisk,
    /// Actual nonnegative transaction cost per contract.
    pub costs_per_contract: ExactDecimal,
    /// Actual original cost source/revision; zero is explicit.
    pub costs_stamp: ReferenceStamp,
}
impl RollLeg {
    fn native(&self) -> f::FuturesRollLeg<'_> {
        f::FuturesRollLeg {
            contract: &self.contract,
            basket: self.basket.as_ref(),
            quote: &self.quote,
            risk: &self.risk,
            costs_per_contract: self.costs_per_contract,
            costs_stamp: &self.costs_stamp,
        }
    }
}
/// Exact native futures cash/quote/tick and indicative roll math.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FuturesMathOperation {
    /// Normal clearing rounding and original actually supplied successive marks.
    Settlement {
        /// Full original dated contract.
        contract: FuturesContract,
        /// Exact native monetary clearing rules.
        rules: f::FuturesClearingRules,
        /// Actual independently stamped calendar.
        calendar_stamp: ReferenceStamp,
        /// Actual initial mark.
        previous: f::FuturesMark,
        /// Strictly later supplied official mark.
        current: f::FuturesMark,
        /// Exact signed integer position.
        contracts: i64,
        /// Original time/source/freshness inputs.
        context: EligibilityContext,
    },
    /// Original quote-point/tick risk; not remaining-rate or calibrated PV01.
    QuoteRisk {
        /// Original full dated contract.
        contract: FuturesContract,
        /// Actual lattice business date.
        on: BusinessDate,
        /// Exact signed integer position.
        contracts: i64,
    },
    /// Original dated tick rounding and exact price residual.
    RoundPrice {
        /// Original full dated contract.
        contract: FuturesContract,
        /// Actual tick date.
        on: BusinessDate,
        /// Exact unrounded informational points.
        price: ExactDecimal,
        /// Explicit original tie/side rule.
        rounding: f::FuturesRounding,
    },
    /// Full exact front/back risk-preserving indicative roll.
    Roll {
        /// Full actual closing leg.
        front: RollLeg,
        /// Full actual later opening leg.
        back: Box<RollLeg>,
        /// Exact signed current contracts.
        contracts: i64,
        /// Explicit original rounding.
        rounding: f::FuturesRounding,
        /// Actual source/time/cutoff inputs.
        context: EligibilityContext,
    },
}
pub(super) fn futures_math(
    action: &FuturesMathOperation,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    match action {
        FuturesMathOperation::Settlement {
            contract,
            rules,
            calendar_stamp,
            previous,
            current,
            contracts,
            context,
        } => value(f::futures_settlement_effect(
            contract,
            rules,
            super::products::future_calendar(calendar_stamp, r)?,
            previous,
            current,
            *contracts,
            *context,
        )?),
        FuturesMathOperation::QuoteRisk {
            contract,
            on,
            contracts,
        } => {
            contract
                .validate()
                .map_err(|e| LibraryError::invalid("contract", &e.to_string()))?;
            value(f::futures_quote_risk(contract, *on, *contracts)?)
        }
        FuturesMathOperation::RoundPrice {
            contract,
            on,
            price,
            rounding,
        } => value(f::round_futures_price(contract, *on, *price, *rounding)?),
        FuturesMathOperation::Roll {
            front,
            back,
            contracts,
            rounding,
            context,
        } => value(f::plan_futures_roll(
            &front.native(),
            &back.native(),
            *contracts,
            *rounding,
            *context,
        )?),
    }
}
