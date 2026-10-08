//! Thin full-term product dispatch over the existing native algorithms.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::{futures as f, optional_bond as o, workflow as w};
use convex_bonds::contracts::{BondCoupon, BondProjectionContext};
use convex_core::{contracts::*, fixings::*, futures::*};
use convex_rates::{pricing::SwapValuationInputs, ReferencePin};
use serde::{Deserialize, Serialize};

/// An exact curve value plus its original source/quality revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct StampedCurve {
    /// Exact immutable curve key.
    pub curve: CurveKey,
    /// Native source quality, validity, availability and actual numeric version.
    pub stamp: ReferenceStamp,
}
impl StampedCurve {
    pub(super) fn native<'a>(
        &'a self,
        r: &'a ResolvedInputs<'_>,
    ) -> LibraryResult<f::FuturesCurve<'a>> {
        Ok(f::FuturesCurve {
            curve: r.curves.get(&self.curve)?.as_ref(),
            stamp: &self.stamp,
        })
    }
}
/// Complete original HW1F source; an assumption remains labelled as such.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OptionalModelInput {
    /// Explicit immutable exogenous parameter assumption.
    Assumed {
        /// Actual caller-assigned model identity/version.
        key: CurveKey,
        /// Original checked mean reversion and diffusion.
        parameters: convex_bonds::options::hw1f::Hw1fParameters,
        /// Original rationale, retained in every native result.
        reason: String,
    },
    /// Full dated original native swaption volatility calibration.
    Swaptions {
        /// Actual caller-assigned output revision.
        key: CurveKey,
        /// Actual model root date.
        valuation_date: Date,
        /// Actual calibration currency.
        currency: Currency,
        /// Original exact discount input.
        discount: CurveKey,
        /// Original fixed-leg calendar revision.
        calendar: ReferencePin,
        /// Explicit exogenous mean reversion.
        mean_reversion: f64,
        /// Full dated normal/shifted-lognormal quotes.
        quotes: Vec<o::SwaptionCalibrationQuote>,
        /// Original quote/rank/domain/work controls.
        settings: o::SigmaCalibrationSettings,
    },
}
pub(super) fn model(
    input: &OptionalModelInput,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<o::OptionalBondModel> {
    Ok(match input {
        OptionalModelInput::Assumed {
            key,
            parameters,
            reason,
        } => o::OptionalBondModel::assumed(key.clone(), *parameters, reason.clone())?,
        OptionalModelInput::Swaptions {
            key,
            valuation_date,
            currency,
            discount,
            calendar,
            mean_reversion,
            quotes,
            settings,
        } => o::calibrate_sigma(o::SigmaCalibrationInputs {
            model_key: key.clone(),
            valuation_date: *valuation_date,
            currency: *currency,
            discount: r.curves.get(discount)?.as_ref(),
            calendar: r.calendar(calendar)?,
            mean_reversion: *mean_reversion,
            quotes,
            settings: *settings,
        })?,
    })
}
/// Complete optional fixed/term-floating inputs, with native model/grid guards.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OptionalInput {
    /// Original full bond economics and exact calendars.
    pub bond: BondInput,
    /// Original model assumptions or full dated calibration observations.
    pub model: OptionalModelInput,
    /// Exact discount dependency.
    pub discount: CurveKey,
    /// Independent term-FRN projection role, absent for fixed cash.
    pub projection: Option<CurveKey>,
    /// Original settlement/known-at/fixing/accrual prefix state.
    pub projection_context: BondProjectionContext,
    /// Original exercise-state and payment ownership policy.
    pub context: o::OptionalBondContext,
    /// Original price convergence and simultaneous exercise policy.
    pub settings: o::OptionalBondSettings,
}
/// Original guarded optional model operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OptionalOperation {
    /// Full model price and original refinement/exercise/fixing diagnostics.
    Price {
        /// Original annualized decimal OAS.
        oas_decimal: f64,
    },
    /// Frozen-model full curve scenario.
    Scenario {
        /// Original decimal OAS.
        oas_decimal: f64,
        /// Explicit separate discount/projection/OAS axes.
        shift: o::CurveShift,
    },
    /// Same native model closed-domain inverse with final price residual.
    Oas {
        /// Dirty price per100 original face.
        dirty_price: f64,
        /// Original bracket/work/price bound.
        settings: o::OasSolveSettings,
    },
    /// Resolved native sensitivity, independently refined from price.
    Risk {
        /// Original decimal OAS.
        oas_decimal: f64,
        /// Explicit native risk axis.
        axis: o::RiskAxis,
        /// Actual decimal bump.
        bump: f64,
        /// Original separate duration/convexity refinement bounds.
        settings: o::RiskSettings,
    },
    /// Canonical actual h/half-step OAS risk with both native refinement guards.
    QuoteRisk {
        /// Original decimal OAS.
        oas_decimal: f64,
        /// Full original raw OAS factor and units.
        key: Box<convex_analytics::quote_risk::QuoteRiskKey>,
        /// Signed actual quantity/reference basis, scaled once.
        position: convex_analytics::quote_risk::RiskPosition,
        /// Actual immutable snapshot identity.
        snapshot_id: convex_core::contracts::SnapshotId,
        /// Original reference h/half-step stability/work bounds.
        settings: convex_analytics::quote_risk::QuoteRiskSettings,
        /// Original independent native duration/convexity grid bounds.
        native_settings: o::RiskSettings,
    },
}
pub(super) fn prepare_optional(
    input: &OptionalInput,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<o::PreparedOptionalBond> {
    let bond = r.bond(&input.bond)?;
    let model = model(&input.model, r)?;
    let projection = input
        .projection
        .as_ref()
        .map(|k| r.curves.get(k))
        .transpose()?;
    let inputs = o::OptionalBondInputs {
        discount: r.curves.get(&input.discount)?.as_ref(),
        projection: projection.map(|c| c.as_ref()),
        model: &model,
        exercise_calendar: r.calendar(&input.bond.calendars.exercise)?,
    };
    Ok(match bond.terms().coupon {
        BondCoupon::Fixed => o::prepare_fixed(&bond, input.context, inputs, input.settings)?,
        _ => o::prepare_floating(
            &bond,
            input.context,
            &input.projection_context,
            &r.history,
            r.publications.callback(),
            inputs,
            input.settings,
        )?,
    })
}
/// Actual full rate future, calendar/clearing/history/model/source decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RateFutureInput {
    /// Actual dated exchange contract and disjoint observation mechanics.
    pub contract: FuturesContract,
    /// Actual observation/value-calendar source revision.
    pub calendar_stamp: ReferenceStamp,
    /// Explicit original monetary/final-rate/revision-selection rules.
    pub clearing: f::FuturesClearingRules,
    /// Actual fixing-manifest source quality and availability.
    pub fixing_stamp: ReferenceStamp,
    /// Explicit revision selection, which native clearing must agree with.
    pub fixing_selection: f::FuturesFixingSelection,
    /// Exact separate projection curve/source, absent when fully fixed.
    pub projection: Option<StampedCurve>,
    /// Original convexity-model source where supplied convexity requires it.
    pub convexity_model: Option<ReferenceStamp>,
    /// Supplied economic and known-at/freshness/execution meaning.
    pub context: EligibilityContext,
    /// Contractual unpublished observation policy.
    pub unpublished: UnpublishedFixingPolicy,
    /// Actual preliminary/final eligibility.
    pub final_only: bool,
}
/// Actual native rate futures requirements/projection/risk/inverse operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RateFutureOperation {
    /// Native complete fixing requirements.
    Requirements {},
    /// Original full remaining-rate valuation and final settlement precision.
    Price {},
    /// Signed quantity and all unfixed holiday weights with native model guards.
    Risk {
        /// Signed integer contract count.
        contracts: i64,
        /// Original annual decimal projection bump.
        bump: ExactDecimal,
    },
    /// Native remaining-rate inverse after computing and validating full state.
    RemainingRate {
        /// Original futures points.
        price: ExactDecimal,
    },
}
pub(super) fn future_calendar<'a>(
    stamp: &'a ReferenceStamp,
    r: &'a ResolvedInputs<'_>,
) -> LibraryResult<f::FuturesCalendar<'a>> {
    let pin = ReferencePin {
        id: stamp.id.clone(),
        version: stamp.version,
    };
    Ok(f::FuturesCalendar {
        stamp,
        calendar: r.calendar(&pin)?,
    })
}
pub(super) fn rate_future<'a>(
    input: &'a RateFutureInput,
    r: &'a ResolvedInputs<'_>,
    history: &'a f::FuturesFixings,
) -> LibraryResult<f::RateFuturesInputs<'a>> {
    Ok(f::RateFuturesInputs {
        contract: &input.contract,
        calendar: future_calendar(&input.calendar_stamp, r)?,
        clearing: &input.clearing,
        fixings: history,
        fixing_stamp: &input.fixing_stamp,
        projection: input.projection.as_ref().map(|p| p.native(r)).transpose()?,
        convexity_model: input.convexity_model.as_ref(),
        context: input.context,
        unpublished: input.unpublished,
        final_only: input.final_only,
    })
}
/// One exact deliverable with original cash, quote and financing economics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondFutureMember {
    /// Exact original security revision.
    pub instrument: InstrumentPin,
    /// Actual security source/quality.
    pub stamp: ReferenceStamp,
    /// Full checked cash bond economics and exact independent calendars.
    pub bond: BondInput,
    /// Actual quoted clean price per100 original face.
    pub clean_price_per100: ExactDecimal,
    /// Original cash quote source.
    pub quote_stamp: ReferenceStamp,
    /// Actual per-security repo/haircut/horizon.
    pub financing: FinancingInput,
    /// Original own cash opportunity cost when a haircut exists.
    pub own_cash: Option<f::FuturesCarryRate>,
    /// Explicit original coupon reinvestment/held-cash policy.
    pub coupon_carry: f::FuturesCouponCarry,
}
/// Complete physical future with every basket member and actual delivery dates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondFutureInput {
    /// Actual dated exchange contract.
    pub contract: FuturesContract,
    /// Whole actual basket and original reciprocal factors.
    pub basket: DeliveryBasket,
    /// Actual delivery-calendar source revision.
    pub calendar_stamp: ReferenceStamp,
    /// Original purchase settlement.
    pub settlement: BusinessDate,
    /// Explicit actual delivery date; no maturity-tenor guess.
    pub delivery: BusinessDate,
    /// Original futures mark and source.
    pub market: f::FuturesMark,
    /// Actual complete economics for every original member.
    pub members: Vec<BondFutureMember>,
    /// Explicit CTD criterion.
    pub selection: f::FuturesCtdMethod,
    /// Actual economic/information/quality/freshness policy.
    pub context: EligibilityContext,
}
/// Full native all-member bond-futures factor axis.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BondFutureAxis {
    /// Original full-security discount-zero bump, retaining cash basis.
    DiscountZeros {
        /// Actual exact curve and source.
        discount: StampedCurve,
    },
    /// Actual per-security repo rates with original carry preserved.
    RepoRates {},
    /// One exact original member's residual clean-price basis.
    SecurityBasis {
        /// Exact member revision.
        instrument: InstrumentPin,
    },
}
/// Full native price or guarded all-member risk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BondFutureOperation {
    /// All original alternatives and carry/invoice/CTD diagnostics.
    Price {},
    /// Original model/all-member risk, with CTD switches retained.
    Risk {
        /// Native axis.
        axis: BondFutureAxis,
        /// Signed actual integer contracts.
        contracts: i64,
        /// Original bump in the named axis unit.
        bump: ExactDecimal,
    },
}
pub(super) fn bond_future(
    input: &BondFutureInput,
    action: &BondFutureOperation,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    if input.members.is_empty() || input.members.len() > 128 {
        return Err(LibraryError::limit(
            "1..128 complete futures members required",
        ));
    }
    let bonds = input
        .members
        .iter()
        .map(|m| r.bond(&m.bond))
        .collect::<LibraryResult<Vec<_>>>()?;
    let deliverables = input
        .members
        .iter()
        .zip(&bonds)
        .map(|(m, b)| f::BondFutureDeliverable {
            security: f::FuturesBond {
                instrument: &m.instrument,
                stamp: &m.stamp,
                bond: b,
            },
            clean_price_per100: m.clean_price_per100,
            quote_stamp: &m.quote_stamp,
            financing: &m.financing,
            own_cash: m.own_cash.as_ref(),
            coupon_carry: &m.coupon_carry,
        })
        .collect::<Vec<_>>();
    let inputs = f::BondFuturesInputs {
        contract: &input.contract,
        basket: &input.basket,
        delivery_calendar: future_calendar(&input.calendar_stamp, r)?,
        settlement: input.settlement,
        delivery: input.delivery,
        market: &input.market,
        deliverables: &deliverables,
        selection: input.selection,
        context: input.context,
    };
    match action {
        BondFutureOperation::Price {} => value(f::price_bond_future(&inputs)?),
        BondFutureOperation::Risk {
            axis,
            contracts,
            bump,
        } => {
            let axis = match axis {
                BondFutureAxis::RepoRates {} => f::BondFuturesRiskAxis::RepoRates,
                BondFutureAxis::DiscountZeros { discount } => {
                    f::BondFuturesRiskAxis::DiscountZeros {
                        discount: discount.native(r)?,
                    }
                }
                BondFutureAxis::SecurityBasis { instrument } => {
                    f::BondFuturesRiskAxis::SecurityBasis { instrument }
                }
            };
            value(f::bond_futures_risk(&inputs, axis, *contracts, *bump)?)
        }
    }
}
pub(super) fn prepare_bond(
    input: &BondInput,
    context: &BondProjectionContext,
    projection: Option<&CurveKey>,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<w::PreparedBondCash> {
    let bond = r.bond(input)?;
    Ok(match bond.terms().coupon {
        BondCoupon::Fixed => {
            if projection.is_some() {
                return Err(LibraryError::invalid(
                    "projection",
                    "fixed cash cannot consume projection",
                ));
            }
            w::PreparedBondCash::fixed(&bond, context.settlement, context.on_payment)?
        }
        _ => w::PreparedBondCash::floating(&bond, context, &r.market(), projection)?,
    })
}
pub(super) fn price_bond(
    cash: &w::PreparedBondCash,
    method: &BondMethod,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<w::BondValue> {
    Ok(match method {
        BondMethod::Yield {
            decimal_rate,
            convention,
        } => cash.price_yield(*decimal_rate, *convention)?,
        BondMethod::DiscountSpread {
            curve,
            role,
            decimal_spread,
        } => cash.price_spread(
            r.curves.get(curve)?.as_ref(),
            role.native(),
            *decimal_spread,
        )?,
    })
}
pub(super) fn execute(
    r: &ResolvedInputs<'_>,
    operation: &LibraryOperation,
) -> LibraryResult<Value> {
    match operation {
        LibraryOperation::Workout {
            input,
            context,
            convention,
            action,
        } => super::workout::execute(input, context, *convention, action, r),
        LibraryOperation::CashSecurity {
            input,
            reference_date,
            convention,
            settings,
            action,
        } => super::cash::security(input, *reference_date, *convention, *settings, action, r),
        LibraryOperation::GovernmentSpread {
            target,
            benchmarks,
            reference_date,
            settings,
        } => super::cash::government(target, benchmarks, *reference_date, settings, r),
        LibraryOperation::Capabilities {} => Ok(super::capabilities()),
        LibraryOperation::Roundtrip {} => value(r.input()),
        LibraryOperation::Curves {} => super::curves::describe_curves(&r.curves),
        LibraryOperation::CurveQuery { curve, query } => value(r.curves.get(curve)?.query(query)?),
        LibraryOperation::CurveTime { curve, action } => {
            super::curves::curve_time(r.curves.get(curve)?, action)
        }
        LibraryOperation::RateConversion { period, action } => {
            super::curves::rate_conversion(period, action)
        }
        LibraryOperation::CurveFit {
            recipe,
            pillars,
            controls,
        } => super::curves::fit_curve(recipe, pillars, *controls),
        LibraryOperation::Fixings {
            requirements,
            context,
        } => {
            let out = w::inspect_fixing_requirements(requirements, &r.history, *context)?;
            Ok(
                serde_json::json!({"required":out.required,"missing":out.missing,"projected":out.projected}),
            )
        }
        LibraryOperation::Schedule {
            terms,
            dates,
            accrual,
            payment,
            fixing,
            observation,
            ex_coupon,
        } => {
            let accrual = r.calendar(accrual)?;
            let payment = r.calendar(payment)?;
            let fixing = r.calendar(fixing)?;
            let observation = r.calendar(observation)?;
            let ex_coupon = r.calendar(ex_coupon)?;
            let schedule = convex_core::cashflows::CouponSchedule::generate_with_calendars(
                terms,
                dates,
                &convex_core::cashflows::ScheduleCalendars {
                    accrual,
                    payment,
                    fixing,
                    observation,
                    ex_coupon,
                },
            )?;
            value(schedule.periods())
        }
        LibraryOperation::Bond {
            bond,
            context,
            projection,
            action,
        } => {
            let contract = r.bond(bond)?;
            match action {
                BondOperation::Construct {} => Ok(
                    serde_json::json!({"terms":contract.terms(),"calendar_pins":bond.calendars,
                    "periods":contract.periods().collect::<Vec<_>>(),"principal":contract.principal_cashflows()}),
                ),
                BondOperation::Requirements {} => {
                    let out = w::bond_requirements(&contract, context, &r.market())?;
                    Ok(
                        serde_json::json!({"required":out.required,"missing":out.missing,"projected":out.projected}),
                    )
                }
                action => {
                    let cash = prepare_bond(bond, context, projection.as_ref(), r)?;
                    match action {
                        BondOperation::Cashflows {} => value(cash.cashflows()),
                        BondOperation::Price { method } => value(price_bond(&cash, method, r)?),
                        BondOperation::QuoteRisk {
                            method,
                            key,
                            position,
                            snapshot_id,
                            settings,
                        } => super::risk::bond_quote_risk(
                            &cash,
                            method,
                            key,
                            position,
                            snapshot_id,
                            settings,
                            r,
                        ),
                        BondOperation::YieldInverse {
                            target,
                            convention,
                            settings,
                        } => value(w::solve_bond_yield(&cash, *convention, *target, *settings)?),
                        BondOperation::SpreadInverse {
                            target,
                            curve,
                            role,
                            settings,
                        } => value(w::solve_bond_spread(
                            &cash,
                            r.curves.get(curve)?.as_ref(),
                            role.native(),
                            *target,
                            *settings,
                        )?),
                        _ => Err(LibraryError::invalid("bond action", "unreachable action")),
                    }
                }
            }
        }
        LibraryOperation::Swap {
            terms,
            context,
            action,
        } => {
            let contract = terms.as_ref().clone().build(&r.swap_calendars(terms)?)?;
            let inputs = SwapValuationInputs {
                curves: &r.curves,
                fixings: &r.history,
            };
            match action {
                SwapOperation::Construct {} => Ok(
                    serde_json::json!({"terms":contract.terms(),"fixed_periods":contract.fixed_schedule().periods(),"floating_periods":contract.floating_coupons().iter().map(|p|p.period()).collect::<Vec<_>>()}),
                ),
                SwapOperation::Requirements {} => {
                    let out = w::swap_requirements(&contract, *context, &r.market())?;
                    Ok(
                        serde_json::json!({"required":out.required,"missing":out.missing,"projected":out.projected}),
                    )
                }
                SwapOperation::Price {} => value(convex_rates::pricing::price_swap(
                    &contract,
                    &inputs,
                    *context,
                    r.publications.callback(),
                )?),
                SwapOperation::Cashflows {} => {
                    let input = w::ValuationInputs::Swap {
                        contract: &contract,
                        context: *context,
                        market: r.market(),
                    };
                    let price = w::price(&input)?;
                    match price {
                        w::Valuation::Swap(v) => value(v.cashflows),
                        _ => Err(LibraryError::invalid("swap", "native family mismatch")),
                    }
                }
            }
        }
        LibraryOperation::BasisSwap { terms, context } => {
            let contract = terms.as_ref().clone().build(&r.basis_calendars(terms)?)?;
            value(convex_rates::basis::price_basis_swap(
                &contract,
                &SwapValuationInputs {
                    curves: &r.curves,
                    fixings: &r.history,
                },
                *context,
                r.publications.callback(),
            )?)
        }
        LibraryOperation::Cds {
            terms,
            context,
            action,
        } => {
            let contract = terms.as_ref().clone().build(&r.credit_calendars(terms)?)?;
            match action {
                CdsOperation::Construct {} => Ok(
                    serde_json::json!({"terms":contract.terms(),"premium_periods":contract.premium_periods()}),
                ),
                CdsOperation::Price {} => {
                    value(convex_credit::price_cds(&contract, context, &r.curves)?)
                }
                CdsOperation::Convert { quote, settings } => value(convex_credit::convert_cds(
                    &contract, context, &r.curves, *quote, *settings,
                )?),
            }
        }
        LibraryOperation::OptionalBond { input, action } => {
            let prepared = prepare_optional(input, r)?;
            match action {
                OptionalOperation::Price { oas_decimal } => value(prepared.price(*oas_decimal)?),
                OptionalOperation::Scenario { oas_decimal, shift } => {
                    value(prepared.price_scenario(*oas_decimal, *shift)?)
                }
                OptionalOperation::Oas {
                    dirty_price,
                    settings,
                } => value(prepared.solve_oas(*dirty_price, *settings)?),
                OptionalOperation::Risk {
                    oas_decimal,
                    axis,
                    bump,
                    settings,
                } => value(prepared.risk(*oas_decimal, *axis, *bump, *settings)?),
                OptionalOperation::QuoteRisk {
                    oas_decimal,
                    key,
                    position,
                    snapshot_id,
                    settings,
                    native_settings,
                } => {
                    let out = convex_analytics::quote_risk::optional_oas_quote_sensitivity(
                        &prepared,
                        key.as_ref().clone(),
                        position.clone(),
                        snapshot_id.clone(),
                        settings.clone(),
                        *oas_decimal,
                        *native_settings,
                    )?;
                    Ok(
                        serde_json::json!({"sensitivity":out.sensitivity,"native":out.native,"half":out.half}),
                    )
                }
            }
        }
        LibraryOperation::RateFuture { input, action } => {
            let history =
                f::FuturesFixings::new(r.fixing_observations().to_vec(), input.fixing_selection)?;
            let inputs = rate_future(input, r, &history)?;
            match action {
                RateFutureOperation::Requirements {} => {
                    value(w::future_requirements(&inputs, &r.publications)?)
                }
                RateFutureOperation::Price {} => {
                    value(f::price_rate_future(&inputs, r.publications.callback())?)
                }
                RateFutureOperation::Risk { contracts, bump } => value(f::rate_futures_risk(
                    &inputs,
                    |i, d| r.publications.callback()(i, d),
                    *contracts,
                    *bump,
                )?),
                RateFutureOperation::RemainingRate { price } => {
                    let valuation = f::price_rate_future(&inputs, r.publications.callback())?;
                    value(f::remaining_rate_from_price(
                        &input.contract,
                        &valuation,
                        *price,
                    )?)
                }
            }
        }
        LibraryOperation::BondFuture { input, action } => bond_future(input, action, r),
        LibraryOperation::Calibration { input, action } => {
            super::curves::calibration(input, action, r)
        }
        LibraryOperation::Lifecycle { input } => super::events::replay(input, r),
        LibraryOperation::Aggregate { input } => super::risk::aggregate(input),
        LibraryOperation::CreditRisk { input, action } => {
            super::credit::credit_risk(input, action, r)
        }
        LibraryOperation::Helper { input } => super::helpers::helper(input, r),
        LibraryOperation::RepoForward { input } => super::helpers::repo_forward(input, r),
        LibraryOperation::Zero { input, action } => super::zero::zero(input, action, r),
        LibraryOperation::CashHedge { input, action } => {
            super::hedges::cash_hedge(input, action, r)
        }
        LibraryOperation::FuturesMath { action } => super::hedges::futures_math(action, r),
        LibraryOperation::BondCdsHedge { input } => super::credit::bond_cds_hedge(input, r),
        LibraryOperation::RoundHedge {
            contract,
            target_currency_per_bp,
            per_contract_currency_per_bp,
            rounding,
        } => {
            contract
                .validate()
                .map_err(|e| LibraryError::invalid("future", &e.to_string()))?;
            value(f::round_futures_hedge(
                *target_currency_per_bp,
                *per_contract_currency_per_bp,
                *rounding,
            )?)
        }
    }
}
