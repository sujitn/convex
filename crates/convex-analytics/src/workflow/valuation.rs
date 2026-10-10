//! Closed native product dispatch; full details and valuation meaning retained.
use super::*;
use super::{finite, invalid};
use crate::{futures::*, optional_bond::*, quote_risk::*};
use convex_core::{
    contracts::{ModelMoney, SnapshotId},
    Currency,
};
use convex_curves::contracts::CurveSet;
use rust_decimal::{prelude::ToPrimitive, Decimal};

/// Explicit fixed-cash pricing representation.
#[derive(Debug, Clone)]
pub enum BondPriceMethod {
    /// Full actual dated cash yield.
    Yield {
        /// Fractional annual rate.
        decimal_rate: f64,
        /// Explicit dated quotation.
        convention: DatedYieldConvention,
    },
    /// Complete cash discounted by an exact collateral/government reference.
    DiscountSpread {
        /// Exact supplied curve, never an ID requiring external lookup.
        curve: CurveKey,
        /// Explicit financial role.
        role: BondDiscountRole,
        /// Annual decimal Z-spread.
        decimal_spread: f64,
    },
}
/// Resolved native input bundle. Futures keep their complete existing stamped
/// inputs. Optional models are actually prepared, with original grid guards.
pub enum ValuationInputs<'a> {
    /// Fixed entitled cash and declared yield or model discount representation.
    Bond {
        /// Native full fixed cash.
        cash: &'a PreparedBondCash,
        /// Declared quotation.
        method: BondPriceMethod,
        /// Actual curves/history/publication decisions.
        market: SuppliedMarket<'a>,
    },
    /// Reproject full floating cash on each scenario, freezing eligible history.
    FloatingBond {
        /// Actual native contract.
        contract: &'a convex_bonds::contracts::BondContract,
        /// Actual settlement/history boundary.
        context: &'a convex_bonds::contracts::BondProjectionContext,
        /// Actual immutable market set.
        market: SuppliedMarket<'a>,
        /// Exact source when any forecast is permitted.
        projection: Option<CurveKey>,
        /// Exact same-currency collateral curve.
        discount: CurveKey,
        /// Continuous discount margin, freezing no future forecasts under curve risk.
        discount_margin: f64,
    },
    /// IRS/OIS actual independent leg/reset economics.
    Swap {
        /// Original native contract.
        contract: &'a convex_rates::SwapContract,
        /// Original explicit time/history policy.
        context: convex_rates::CashflowContext,
        /// Actual supplied values.
        market: SuppliedMarket<'a>,
    },
    /// Standard single-name conditional premium/protection and original cash state.
    Cds {
        /// Original checked legal/contractual terms.
        contract: &'a convex_credit::CdsContract,
        /// Explicit outstanding initial-payment state.
        context: &'a convex_credit::CdsPricingContext,
        /// Complete original values.
        curves: &'a CurveSet,
    },
    /// Original HW1F model with full price/risk grid admission.
    OptionalBond {
        /// Immutable original model/cash/fixing preparation.
        prepared: &'a PreparedOptionalBond,
        /// Fractional constant OAS.
        oas_decimal: f64,
    },
    /// Actual partly/fully fixed rate futures and explicit clearing/convexity.
    RateFuture {
        /// Full original native stamped inputs.
        inputs: &'a RateFuturesInputs<'a>,
        /// Immutable publication schedule.
        publications: &'a PublicationSchedule,
    },
    /// Full fixed-date basket/carry and all alternatives; indicative only.
    BondFuture(&'a BondFuturesInputs<'a>),
}
/// Full product results; no flattening away leg/model/source/cash diagnostics.
#[derive(Debug, Clone, serde::Serialize)]
pub enum Valuation {
    /// Full fixed or floating model cash price.
    Bond(Box<BondValue>),
    /// Full IRS/OIS cash, par quotes, PVs and known/projection provenance.
    Swap(Box<convex_rates::pricing::SwapValuation>),
    /// Full conditional credit cash and signed leg/initial PVs.
    Cds(Box<convex_credit::CdsValuation>),
    /// Original price/model/grids/exercise diagnostics.
    Optional(Box<OptionalBondPrice>),
    /// Full native remaining accrual/history/model quote; no discounted PV claim.
    RateFuture(Box<RateFuturesValuation>),
    /// Full native fixed-date carry/invoice/basis/CTD alternatives.
    BondFuture(Box<BondFuturesValuation>),
}
/// Stable distinction between discounted economic PV and a futures mark level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ValuationBasis {
    /// Bond/swap/CDS model PV at the supplied valuation/settlement boundary.
    CurrencyPresentValue,
    /// Futures price points times currency/point, per contract. Changes are
    /// unrounded indicative mark effects, never lifetime PV or posted margin.
    FuturesQuoteLevel,
}
/// Expected cash is never realised or posted by these functions.
#[derive(Debug, Clone)]
pub enum ExpectedCashflows {
    /// Full entitled coupon/principal cash and actual fixing evidence.
    Bond(convex_bonds::contracts::BondProjection),
    /// Independent signed fixed/floating coupon projections.
    Swap(Box<convex_rates::SwapCashflows>),
    /// Conditional scheduled premium, integrated default accrual/protection,
    /// original outstanding initial PV and fair settlement kept in full context.
    Cds(Box<convex_credit::CdsValuation>),
}
/// Conditional optional cash stays available through the actual BondContract
/// projection. Futures cash requires supplied margin/delivery events (CVX-062).
pub fn cashflows(input: &ValuationInputs<'_>) -> WorkflowResult<ExpectedCashflows> {
    match input {
        ValuationInputs::Bond { cash, .. } => {
            Ok(ExpectedCashflows::Bond(cash.cashflows().clone()))
        }
        ValuationInputs::FloatingBond { contract, context, market, projection, .. } => {
            let prepared = PreparedBondCash::floating(contract, context, market, projection.as_ref())?;
            Ok(ExpectedCashflows::Bond(prepared.cashflows().clone()))
        }
        ValuationInputs::Swap { contract, context, market } => {
            let mut projection_error = None;
            let value = with_publications(market.publications, |p| {
                contract.cashflows(*context, market.fixings, p, |r| {
                    match convex_rates::pricing::forecast_from_curve(
                        r, contract.terms().currency, market.curves,
                    ) {
                        Ok((v, _)) => Ok(v),
                        Err(e) => {
                            projection_error = Some(e);
                            Err(CashflowError::InvalidTerms {
                                field: "projection",
                                reason: "native projection query failed".into(),
                            })
                        }
                    }
                })
            });
            if let Some(e) = projection_error {
                return Err(e.into());
            }
            Ok(ExpectedCashflows::Swap(Box::new(value?)))
        }
        ValuationInputs::Cds { contract, context, curves } => {
            Ok(ExpectedCashflows::Cds(Box::new(convex_credit::price_cds(contract, context, curves)?)))
        }
        ValuationInputs::OptionalBond { .. } => Err(WorkflowError::NotApplicable(
            "optional model price is not unconditional payment; use BondContract.project for cash conditional on no exercise and native exercise diagnostics",
        )),
        ValuationInputs::RateFuture { .. } | ValuationInputs::BondFuture(_) => {
            Err(WorkflowError::NotApplicable(
                "futures model quote is not cash; margin/delivery requires explicit economic events",
            ))
        }
    }
}
/// All native fixing requirements before any future forecast or price.
pub fn bond_requirements(
    contract: &convex_bonds::contracts::BondContract,
    context: &convex_bonds::contracts::BondProjectionContext,
    market: &SuppliedMarket<'_>,
) -> WorkflowResult<FixingRequirements> {
    let r = with_publications(market.publications, |p| {
        contract.fixing_requirements(context.settlement, context.on_payment, p)
    })?;
    inspect_fixing_requirements(&r, market.fixings, context.fixing)
}
/// All native IRS/OIS requirements, retaining complete missing due history.
pub fn swap_requirements(
    contract: &convex_rates::SwapContract,
    context: convex_rates::CashflowContext,
    market: &SuppliedMarket<'_>,
) -> WorkflowResult<FixingRequirements> {
    let r = with_publications(market.publications, |p| {
        contract.fixing_requirements(context, p)
    })?;
    inspect_fixing_requirements(&r, market.fixings, context.fixings)
}
/// Existing complete rate-futures requirements, including exact observation windows.
pub fn future_requirements(
    inputs: &RateFuturesInputs<'_>,
    publications: &PublicationSchedule,
) -> WorkflowResult<Vec<convex_core::fixings::FixingRequirement>> {
    with_publications(publications, |p| rate_futures_requirements(inputs, p))
}
/// Execute the existing native product pricer over actual supplied immutable inputs.
pub fn price(input: &ValuationInputs<'_>) -> WorkflowResult<Valuation> {
    price_with_curves(input, None)
}
fn price_with_curves(
    input: &ValuationInputs<'_>,
    scenario: Option<&CurveSet>,
) -> WorkflowResult<Valuation> {
    Ok(match input {
        ValuationInputs::Bond {
            cash,
            method,
            market,
        } => {
            cash.fixed_yield()?;
            let curves = scenario.unwrap_or(market.curves);
            let v = match method {
                BondPriceMethod::Yield {
                    decimal_rate,
                    convention,
                } => cash.price_yield(*decimal_rate, *convention)?,
                BondPriceMethod::DiscountSpread {
                    curve,
                    role,
                    decimal_spread,
                } => {
                    cash.price_spread(curves.get(curve)?.as_ref(), role.clone(), *decimal_spread)?
                }
            };
            Valuation::Bond(Box::new(v))
        }
        ValuationInputs::FloatingBond {
            contract,
            context,
            market,
            projection,
            discount,
            discount_margin,
        } => {
            let m = SuppliedMarket {
                curves: scenario.unwrap_or(market.curves),
                ..*market
            };
            let cash = PreparedBondCash::floating(contract, context, &m, projection.as_ref())?;
            Valuation::Bond(Box::new(cash.price_spread(
                m.curves.get(discount)?.as_ref(),
                BondDiscountRole::Collateral,
                *discount_margin,
            )?))
        }
        ValuationInputs::Swap {
            contract,
            context,
            market,
        } => {
            let inputs = convex_rates::pricing::SwapValuationInputs {
                curves: scenario.unwrap_or(market.curves),
                fixings: market.fixings,
            };
            Valuation::Swap(Box::new(with_publications(market.publications, |p| {
                convex_rates::pricing::price_swap(contract, &inputs, *context, p)
            })?))
        }
        ValuationInputs::Cds {
            contract,
            context,
            curves,
        } => {
            let value = if let Some(scenario) = scenario {
                let credit = scenario.get(&contract.terms().survival_curve)?;
                let convex_curves::contracts::CurveRole::Credit { recovery, .. } =
                    credit.recipe().role
                else {
                    return Err(invalid("CDS scenario", "actual Credit role required"));
                };
                let fraction = convex_core::contracts::FiniteF64::new(recovery)
                    .map_err(|e| invalid("CDS recovery", e.to_string()))?;
                let scenario_contract = contract.with_recovery_fraction(fraction)?;
                convex_credit::price_cds(&scenario_contract, context, scenario)?
            } else {
                convex_credit::price_cds(contract, context, curves)?
            };
            Valuation::Cds(Box::new(value))
        }
        ValuationInputs::OptionalBond {
            prepared,
            oas_decimal,
        } => {
            if scenario.is_some() {
                return Err(WorkflowError::Unsupported("optional model curve scenarios require native price_scenario; rebuilding a price-only model would bypass its admission"));
            }
            Valuation::Optional(Box::new(prepared.price(*oas_decimal)?))
        }
        ValuationInputs::RateFuture {
            inputs,
            publications,
        } => {
            // Scenario curve reference must outlive the temporary native bundle.
            let owned_projection = if let (Some(s), Some(p)) = (scenario, inputs.projection) {
                Some(s.get(&p.curve.recipe().key)?)
            } else {
                None
            };
            let projection = match &owned_projection {
                Some(curve) => Some(FuturesCurve {
                    curve,
                    stamp: inputs
                        .projection
                        .ok_or_else(|| invalid("projection", "missing original source"))?
                        .stamp,
                }),
                None => inputs.projection,
            };
            let native = RateFuturesInputs {
                contract: inputs.contract,
                calendar: FuturesCalendar {
                    stamp: inputs.calendar.stamp,
                    calendar: inputs.calendar.calendar,
                },
                clearing: inputs.clearing,
                fixings: inputs.fixings,
                fixing_stamp: inputs.fixing_stamp,
                projection,
                convexity_model: inputs.convexity_model,
                context: inputs.context,
                unpublished: inputs.unpublished,
                final_only: inputs.final_only,
            };
            Valuation::RateFuture(Box::new(with_publications(publications, |p| {
                price_rate_future(&native, p)
            })?))
        }
        ValuationInputs::BondFuture(inputs) => {
            if scenario.is_some() {
                return Err(WorkflowError::NotApplicable("fixed-date cash futures use explicit cash quotes/repo/carry; apply bond_futures_risk to every native basket member"));
            }
            Valuation::BondFuture(Box::new(price_bond_future(inputs)?))
        }
    })
}
impl ValuationInputs<'_> {
    /// Native settlement currency, never an inferred reporting currency.
    pub fn currency(&self) -> Currency {
        match self {
            Self::Bond { cash, .. } => cash.currency(),
            Self::FloatingBond { contract, .. } => contract.terms().currency,
            Self::Swap { contract, .. } => contract.terms().currency,
            Self::Cds { contract, .. } => contract.terms().currency,
            Self::OptionalBond { prepared, .. } => prepared.currency(),
            Self::RateFuture { inputs, .. } => inputs.contract.currency,
            Self::BondFuture(inputs) => inputs.contract.currency,
        }
    }
    /// Kind of monetary measurement used by repricing and signed risk.
    pub fn basis(&self) -> ValuationBasis {
        if matches!(self, Self::RateFuture { .. } | Self::BondFuture(_)) {
            ValuationBasis::FuturesQuoteLevel
        } else {
            ValuationBasis::CurrencyPresentValue
        }
    }
    /// Enforce native quantity/reference units before any monetary scaling.
    /// Swap/optional contracts carry their own full profiles: use signed copies.
    pub fn validate_position(&self, position: &RiskPosition) -> WorkflowResult<()> {
        let (unit, reference) = match self {
            Self::Bond { cash, .. } => (RiskQuantityUnit::Notional, cash.face_value()),
            Self::FloatingBond { contract, .. } => {
                (RiskQuantityUnit::Notional, contract.terms().face_value)
            }
            Self::Cds { contract, .. } => (
                RiskQuantityUnit::Notional,
                contract.terms().notional.decimal(),
            ),
            Self::Swap { .. } | Self::OptionalBond { .. } => {
                (RiskQuantityUnit::TradeCopies, Decimal::ONE)
            }
            Self::RateFuture { .. } | Self::BondFuture(_) => {
                (RiskQuantityUnit::Contracts, Decimal::ONE)
            }
        };
        if position.currency != self.currency()
            || position.quantity_unit != unit
            || position.reference_quantity.decimal() != reference
        {
            return Err(invalid(
                "position",
                "currency/native quantity unit/original reference size mismatch",
            ));
        }
        position.scale()?;
        Ok(())
    }
    /// Complete reference-unit native measurement; futures quote level is
    /// explicitly labelled separately and never reported as lifetime PV.
    pub fn monetary_value(&self) -> WorkflowResult<ModelMoney> {
        self.amount(&price(self)?)
    }
    fn amount(&self, value: &Valuation) -> WorkflowResult<ModelMoney> {
        let pv = match value {
            Valuation::Bond(v) => return Ok(v.present_value),
            Valuation::Swap(v) => return Ok(v.present_value),
            Valuation::Cds(v) => v.net_pv,
            Valuation::Optional(v) => v.present_value,
            Valuation::RateFuture(v) => {
                let Self::RateFuture { inputs, .. } = self else {
                    return Err(invalid("valuation", "mismatched product"));
                };
                v.model_price_points
                    .decimal()
                    .checked_mul(inputs.contract.currency_per_price_point.decimal())
                    .and_then(|v| v.to_f64())
                    .ok_or_else(|| invalid("quote level", "overflow"))?
            }
            Valuation::BondFuture(v) => {
                let Self::BondFuture(inputs) = self else {
                    return Err(invalid("valuation", "mismatched product"));
                };
                v.fair_price_points
                    .decimal()
                    .checked_mul(inputs.contract.currency_per_price_point.decimal())
                    .and_then(|v| v.to_f64())
                    .ok_or_else(|| invalid("quote level", "overflow"))?
            }
        };
        Ok(risk_reference_pv(pv, self.currency())?)
    }
}
/// Full scenario values; curves and their explicit recovery assumption change.
/// All dated economics, histories, legal identity and cash-state inputs survive.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ValuationScenario {
    /// Original immutable snapshot supplied by the caller.
    pub snapshot_id: SnapshotId,
    /// Actual signed native quantity, applied once.
    pub position: RiskPosition,
    /// Discounted PV or indicative futures quote-level meaning.
    pub basis: ValuationBasis,
    /// Full original product diagnostics.
    pub baseline: Valuation,
    /// Full shocked product diagnostics; no realised cash claim.
    pub shocked: Valuation,
    /// Signed quantity-scaled base currency amount.
    pub baseline_amount: ModelMoney,
    /// Signed quantity-scaled scenario currency amount.
    pub scenario_amount: ModelMoney,
    /// Full revaluation scenario minus baseline, not a sensitivity approximation.
    pub change: ModelMoney,
}
fn scenario_result(
    input: &ValuationInputs<'_>,
    baseline: Valuation,
    shocked: Valuation,
    position: RiskPosition,
    snapshot_id: SnapshotId,
) -> WorkflowResult<ValuationScenario> {
    input.validate_position(&position)?;
    let scale = position.scale()?;
    let base = finite(
        input.amount(&baseline)?.value().get() * scale,
        "scaled base",
    )?;
    let after = finite(
        input.amount(&shocked)?.value().get() * scale,
        "scaled scenario",
    )?;
    Ok(ValuationScenario {
        snapshot_id,
        position,
        basis: input.basis(),
        baseline,
        shocked,
        baseline_amount: risk_reference_pv(base, input.currency())?,
        scenario_amount: risk_reference_pv(after, input.currency())?,
        change: risk_reference_pv(finite(after - base, "scenario change")?, input.currency())?,
    })
}
/// Revalue actual same supplied economics on a complete hypothetical curve set.
/// Scenario curves are not market publications or new live snapshot revisions.
pub fn revalue_curves(
    input: &ValuationInputs<'_>,
    curves: &CurveSet,
    position: RiskPosition,
    snapshot_id: SnapshotId,
) -> WorkflowResult<ValuationScenario> {
    input.validate_position(&position)?;
    if matches!(
        input,
        ValuationInputs::Bond {
            method: BondPriceMethod::Yield { .. },
            ..
        }
    ) {
        return Err(WorkflowError::NotApplicable(
            "dated yield price has no curve valuation input; reprice the explicit yield quote",
        ));
    }
    scenario_result(
        input,
        price(input)?,
        price_with_curves(input, Some(curves))?,
        position,
        snapshot_id,
    )
}
/// Native optional curve revaluation, freezing full model/grid/history admission.
pub fn revalue_optional(
    prepared: &PreparedOptionalBond,
    oas_decimal: f64,
    shift: CurveShift,
    position: RiskPosition,
    snapshot_id: SnapshotId,
) -> WorkflowResult<ValuationScenario> {
    let input = ValuationInputs::OptionalBond {
        prepared,
        oas_decimal,
    };
    input.validate_position(&position)?;
    scenario_result(
        &input,
        price(&input)?,
        Valuation::Optional(Box::new(prepared.price_scenario(oas_decimal, shift)?)),
        position,
        snapshot_id,
    )
}
/// Compose the original full rate/CDS/cash graph with complete native product PV
/// or explicitly labelled futures quote-level risk. All original h/half-step,
/// quote repricing, histories and dependency/model guards remain mandatory.
pub fn curve_quote_risk(
    input: &ValuationInputs<'_>,
    graph: &PreparedQuoteRiskGraph<'_>,
    key: QuoteRiskKey,
    position: RiskPosition,
    snapshot_id: SnapshotId,
    settings: QuoteRiskSettings,
) -> WorkflowResult<QuoteSensitivity> {
    input.validate_position(&position)?;
    if matches!(
        input,
        ValuationInputs::OptionalBond { .. } | ValuationInputs::BondFuture(_)
    ) {
        return Err(WorkflowError::Unsupported("use original optional factor/OAS or all-member bond-futures risk APIs; a price-only generic graph cannot bypass native guards"));
    }
    if capability(input, WorkflowOperation::CurveQuoteRisk).status
        != WorkflowCapabilityStatus::Available
    {
        return Err(WorkflowError::NotApplicable(
            "curve graph risk requires an actual curve-based product representation",
        ));
    }
    let original = input.monetary_value()?;
    let graph_value = input.amount(&price_with_curves(input, Some(&graph.baseline().curves))?)?;
    if original != graph_value {
        return Err(invalid(
            "risk baseline",
            "original supplied price differs from the prepared graph information set",
        ));
    }
    Ok(curve_quote_sensitivity(
        graph,
        key,
        position,
        snapshot_id,
        settings,
        |curves| {
            let value =
                price_with_curves(input, Some(&curves.curves)).and_then(|v| input.amount(&v));
            value.map_err(|e| QuoteRiskError::Invalid {
                field: "full native product".into(),
                reason: e.to_string(),
            })
        },
    )?)
}
/// Existing signed-risk APIs are deliberately reachable here, with their full
/// checked keys, stability, optional risk-resolution and reconciliation guards.
pub use crate::quote_risk::{
    optional_oas_quote_sensitivity, reconcile_curve_node_buckets, reconcile_quote_buckets,
    reference_quote_sensitivity,
};
