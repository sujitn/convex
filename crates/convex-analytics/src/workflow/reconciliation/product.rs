//! Full native valuation and exact replayed-claim composition.
use super::*;
use crate::{cash_calibration as c, futures as f, optional_bond as o};
use convex_bonds::contracts::{BondContract, BondCoupon, BondProjectionContext};
use convex_core::cashflows::PaymentDateEntitlement;
use convex_curves::contracts::{BuiltCurve, CurvePoint, CurveQuery, CurveRole, CurveSet};

/// Closed full original economics. Optional preparations are rebuilt here from
/// the actual checked contract, preventing a foreign prepared-price substitution.
pub enum CausalPnlProduct<'a> {
    /// Full original fixed/floating/CDS/IRS/OIS/rate-future native input.
    Standard(&'a ValuationInputs<'a>),
    /// Full original context/reference/issuer-basis cash helper.
    CashBasis {
        /// Complete actual security quote/economics.
        helper: &'a c::CashBondHelper,
        /// Original independent calendar resources.
        resources: &'a c::CashHelperResources<'a>,
        /// Original promised/recovery model.
        model: &'a c::BondBasisModel,
        /// Exact actual residual-basis curve.
        basis: &'a CurveKey,
        /// Full actual materialized graph.
        curves: &'a CurveSet,
    },
    /// Full original optional-model preparation, cash/notice/history and policy.
    Optional {
        /// Actual full native contract.
        contract: &'a BondContract,
        /// Actual economic state/root.
        context: o::OptionalBondContext,
        /// Original full term/overnight context.
        projection_context: &'a BondProjectionContext,
        /// Exact original market/history.
        market: SuppliedMarket<'a>,
        /// Exact original model/curves/exercise calendar.
        inputs: o::OptionalBondInputs<'a>,
        /// Original bounded price convergence policy.
        settings: o::OptionalBondSettings,
        /// Original fractional OAS.
        oas_decimal: f64,
    },
    /// Complete actual delivery basket and original all-member model owner.
    Delivery {
        /// Full original basket/carry/observations.
        inputs: &'a f::BondFuturesInputs<'a>,
        /// Actual source/materialized discount.
        discount: f::FuturesCurve<'a>,
        /// Full native original graph at this point.
        graph: &'a q::QuoteGraphScenario,
        /// Explicit all-member work ceiling.
        policy: f::BondFuturesGraphPolicy,
    },
}
fn same_bond(state: &l::LifecycleState, contract: &BondContract) -> ReconciliationResult<()> {
    match state.instrument() {
        l::LifecycleInstrument::Bond {
            contract: actual, ..
        } if actual.terms() == contract.terms() => Ok(()),
        _ => Err(bad(
            "lifecycle product",
            "exact same full native bond economics required",
        )),
    }
}
fn unit(state: &l::LifecycleState) -> Decimal {
    match state.view().position {
        l::LifecyclePosition::BondFace(q)
        | l::LifecyclePosition::TradeCopies(q)
        | l::LifecyclePosition::CdsNotional(q) => q.decimal(),
        l::LifecyclePosition::FuturesContracts(q) => Decimal::from(q),
    }
}
fn futures_fills(
    contract: &convex_core::futures::FuturesContract,
    point: &PnlPointInput,
    pos: &PnlPositionInput,
) -> ReconciliationResult<()> {
    for fill in point.fills.iter().filter(|f| f.trade_id == pos.identity.id) {
        let quantity = fill.quantity.decimal();
        let count = quantity
            .to_i64()
            .filter(|_| quantity.fract().is_zero())
            .ok_or_else(|| {
                bad(
                    "fill quantity",
                    "exact signed integer futures contracts required",
                )
            })?;
        let price = dec(fill
            .consideration
            .value()
            .decimal()
            .checked_div(quantity)
            .and_then(|v| v.checked_div(contract.currency_per_price_point.decimal())))?;
        contract
            .validate_order_units(
                fill.date
                    .try_into()
                    .map_err(|e| WorkflowError::from(f::FuturesError::from(e)))?,
                count,
                ExactDecimal::new(price),
            )
            .map_err(|e| WorkflowError::from(f::FuturesError::from(e)))?;
    }
    Ok(())
}
fn curve_ratio(curve: &BuiltCurve, date: Date, due: Date) -> ReconciliationResult<f64> {
    let query = |d| {
        curve
            .query(&CurveQuery::DiscountFactor {
                point: CurvePoint::Date(d),
            })
            .map(|q| q.value)
            .map_err(WorkflowError::from)
            .map_err(ReconciliationError::from)
    };
    let ratio = query(due)? / query(date)?;
    if !ratio.is_finite() || ratio <= 0.0 {
        return Err(bad(
            "claim discount",
            "positive finite actual DF ratio required",
        ));
    }
    Ok(ratio)
}
fn bond_removed(
    v: &BondValue,
    contract: &BondContract,
    key: &l::ObligationKey,
    scale: f64,
    curves: &CurveSet,
    date: Date,
) -> ReconciliationResult<f64> {
    let cash = match key.kind {
        l::CashKind::BondCoupon => contract.periods().nth(key.ordinal as usize).and_then(|p| {
            v.cash
                .coupons
                .iter()
                .find(|c| {
                    c.accrual_end == key.effective_date
                        && c.accrual_start == p.accrual_dates().0
                        && c.entitled
                })
                .map(|c| (c.amount, c.payment_date))
        }),
        l::CashKind::Principal => contract
            .principal_cashflows()
            .get(key.ordinal as usize)
            .and_then(|p| {
                v.cash
                    .principal
                    .iter()
                    .find(|c| {
                        c.effective_date == key.effective_date && c.payment_date == p.payment_date
                    })
                    .map(|c| (c.amount, c.payment_date))
            }),
        _ => None,
    };
    let Some((amount, due)) = cash else {
        return Ok(0.0);
    };
    let ratio=match &v.basis{
        BondValueBasis::DatedYield{decimal_rate,convention}=>convex_curves::contracts::discount_from_rate(*decimal_rate,convex_curves::contracts::year_fraction(date,due,convention.day_count).map_err(WorkflowError::from)?,convention.compounding).map_err(WorkflowError::from)?,
        BondValueBasis::DiscountSpread{curve,decimal_spread,..}=>curve_ratio(curves.get(curve).map_err(WorkflowError::from)?.as_ref(),date,due)?*(-decimal_spread*num(Decimal::from(date.days_between(&due)))?/365.0).exp(),
        BondValueBasis::ContextualIcma{..}=>return Err(WorkflowError::Unsupported("contextual ICMA embedded future earned claims require the exact original contextual time model").into()),
    };
    Ok(num(amount)? * scale * ratio)
}
fn standard<'a>(
    p: &'a ValuationInputs<'a>,
    state: &l::LifecycleState,
    point: &PnlPointInput,
    pos: &PnlPositionInput,
) -> ReconciliationResult<()> {
    let mut native = pos.current.clone();
    if let ValuationInputs::Swap { contract, .. } = p {
        if native.quantity_unit == q::RiskQuantityUnit::Notional {
            if native.reference_quantity.decimal()
                != contract
                    .terms()
                    .fixed
                    .leg
                    .notionals
                    .first()
                    .ok_or_else(|| bad("notional", "original reference absent"))?
                    .value
            {
                return Err(bad(
                    "notional",
                    "exact first fixed-leg native reference required",
                ));
            }
            native.quantity = ExactDecimal::new(Decimal::ONE);
            native.reference_quantity = native.quantity;
            native.quantity_unit = q::RiskQuantityUnit::TradeCopies;
        }
    }
    p.validate_position(&native)?;
    match p {
        ValuationInputs::Bond { cash, .. } => {
            same_bond(state, cash.contract())?;
            if cash.settlement() != point.valuation_date {
                return Err(bad("time", "actual prepared cash root differs"));
            }
        }
        ValuationInputs::FloatingBond {
            contract, context, ..
        } => {
            same_bond(state, contract)?;
            if context.settlement != point.valuation_date
                || context.fixing.known_at != point.known_at
                || context.on_payment != PaymentDateEntitlement::Exclude
            {
                return Err(bad(
                    "context",
                    "exact point date/cutoff and excluded boundary cash required",
                ));
            }
        }
        ValuationInputs::Swap {
            contract, context, ..
        } => {
            if !matches!(state.instrument(),l::LifecycleInstrument::Swap(c) if c.terms()==contract.terms())
                || context.fixings.valuation_date != point.valuation_date
                || context.fixings.known_at != point.known_at
                || context.on_payment != PaymentDateEntitlement::Exclude
            {
                return Err(bad(
                    "swap",
                    "exact full state/economics/date/cutoff and excluded boundary cash required",
                ));
            }
        }
        ValuationInputs::Cds {
            contract, context, ..
        } => {
            if !matches!(state.instrument(),l::LifecycleInstrument::Cds(c) if c.terms()==contract.terms())
                || context.valuation_date != point.valuation_date
            {
                return Err(bad(
                    "CDS",
                    "exact full original legal economics/date required",
                ));
            }
            let initial = state
                .view()
                .obligations
                .into_iter()
                .find(|o| o.key.kind == l::CashKind::InitialCreditCash);
            let expected = if contract.terms().initial_clean_upfront_fraction.is_none() {
                convex_credit::InitialPaymentState::NotContracted
            } else if initial.as_ref().is_some_and(|o| o.settled) {
                convex_credit::InitialPaymentState::Settled
            } else {
                convex_credit::InitialPaymentState::Pending
            };
            if context.initial_payment_state != expected {
                return Err(bad("CDS initial cash","initial-payment state must match actual recorded settlement; false paid state rejected"));
            }
        }
        ValuationInputs::RateFuture { inputs, .. } => {
            futures_fills(inputs.contract, point, pos)?;
            if !matches!(state.instrument(),l::LifecycleInstrument::Futures{contract,..} if contract.as_ref()==inputs.contract)
                || inputs.context.as_of.datetime().date_naive()
                    != point.valuation_date.as_naive_date()
                || inputs.context.known_at != point.known_at
            {
                return Err(bad(
                    "rate future",
                    "exact original dated contract/cutoff required",
                ));
            }
        }
        _ => {
            return Err(bad(
                "product",
                "dedicated full optional/delivery owner required",
            ))
        }
    }
    Ok(())
}
fn optional_notice(
    contract: &BondContract,
    context: o::OptionalBondContext,
    inputs: &o::OptionalBondInputs<'_>,
    oas: f64,
    state: &l::LifecycleState,
) -> ReconciliationResult<Option<(BondValue, WorkoutCashDetails)>> {
    let view = state.view();
    let Some((ordinal, _, date, _)) = view.exercise_notices.first().copied() else {
        return Ok(None);
    };
    if view.exercise_notices.len() != 1 || !matches!(contract.terms().coupon, BondCoupon::Fixed) {
        return Err(WorkflowError::Unsupported("multiple or floating pending notices require complete forced-path projection/state integration").into());
    }
    let notice = state
        .exercise_notice()
        .ok_or(ReconciliationError::Incomplete(
            "actual applied notice provenance missing",
        ))?;
    if !matches!(notice.action, l::LifecycleAction::ExerciseNotice { provision, exercise_date } if provision == ordinal && exercise_date == date)
    {
        return Err(bad(
            "notice",
            "actual applied notice must match the original outstanding exercise",
        ));
    }
    let context = WorkoutContext {
        reference_date: context.settlement,
        trade_date: context.settlement,
        settlement: context.settlement,
        on_payment: context.on_payment,
        notice: WorkoutNotice::Notified {
            provision: ordinal,
            exercise: date,
            notice_date: notice.effective_date,
        },
    };
    let prepared = workout::prepare(
        contract,
        &context,
        &WorkoutSelection::Exercise {
            provision: ordinal,
            date,
        },
        &contract.terms().calendars.exercise,
        inputs.exercise_calendar,
    )?;
    let value = prepared
        .cash
        .price_spread(inputs.discount, BondDiscountRole::Collateral, oas)?;
    let details = prepared
        .exercise
        .ok_or_else(|| bad("notice", "actual original exercise cash absent"))?;
    Ok(Some((value, details)))
}
/// Price every original product, then compose its exact replayed obligations and
/// receipts. All-ended zero exposure is a native state conclusion, never a
/// missing-value fallback. Cash received is not counted again as a claim.
pub fn mark_causal_pnl_position<'p>(
    path: &'p PreparedCausalPnl,
    point_index: usize,
    index: usize,
    state: &l::LifecycleState,
    product: &CausalPnlProduct<'_>,
    cash_discount: &BuiltCurve,
) -> ReconciliationResult<CausalPositionMark<'p>> {
    let point = path
        .input
        .points
        .get(point_index)
        .ok_or_else(|| bad("point", "outside original path"))?;
    let pos = point
        .positions
        .get(index)
        .ok_or_else(|| bad("holding", "outside expected coverage"))?;
    let view = state.view();
    let currency = pos.current.currency;
    if let Some(pins) = &path.cohort_state_pins {
        if cohorts::state_pin(state)? != pins[point_index][index] {
            return Err(bad(
                "cohort state",
                "same complete admitted original state and receipt provenance required",
            ));
        }
    }
    let mut original = path.input.points[0].positions[index].current.clone();
    let cohort = path.cohorts.as_ref().map(|owners| &owners[index]);
    if let Some(owner) = cohort {
        original.quantity = owner.quantity;
    }
    let opening = if matches!(state.instrument(), l::LifecycleInstrument::Swap(_))
        && original.quantity_unit == q::RiskQuantityUnit::Notional
    {
        dec(original
            .quantity
            .decimal()
            .checked_div(original.reference_quantity.decimal()))?
    } else {
        original.quantity.decimal()
    };
    if unit(state) != opening {
        return Err(bad("opening position","actual native lifecycle quantity must equal the same original holding quantity in native units"));
    }
    if view.trade_id != pos.identity.id
        || view.known_at > point.known_at
        || view.effective_date > point.valuation_date
        || view
            .pending_boundaries
            .iter()
            .any(|(d, _)| *d <= point.valuation_date)
    {
        return Err(bad(
            "state",
            "same exact trade, available effects and all due original boundaries replayed required",
        ));
    }
    if view.status == l::EconomicStatus::DefaultPending {
        return Err(ReconciliationError::Incomplete(
            "actual default recovery/auction terms absent",
        ));
    }
    let physical_future = path.handoffs.get(point_index).is_some_and(|row| {
        row.iter()
            .flatten()
            .any(|h| h.future == pos.identity.id && state.delivery_event() == Some(&h.delivery))
    });
    if view.delivery_pending.is_some() && !physical_future {
        return Err(ReconciliationError::Incomplete(
            "actual delivery security receipt and a separate asset holding valuation required",
        ));
    }
    if matches!(state.instrument(),l::LifecycleInstrument::Futures{contract,..} if matches!(contract.settlement,convex_core::futures::FuturesSettlement::Bond{..}))
        && view.status != l::EconomicStatus::Active
        && !physical_future
    {
        return Err(ReconciliationError::Incomplete("physical delivery requires an explicit separate security holding and complete asset/invoice reconciliation"));
    }
    if cash_discount.recipe().key != pos.cash_discount
        || cash_discount.recipe().currency != currency
        || !matches!(cash_discount.recipe().role,CurveRole::Discount{collateral_currency} if collateral_currency==currency)
    {
        return Err(bad(
            "cash discount",
            "exact same-currency collateral claim owner required",
        ));
    }
    let mut scale = pos.current.scale().map_err(WorkflowError::from)?;
    let mut physical = path
        .handoffs
        .get(point_index)
        .and_then(|h| h[index].clone());
    if let Some(h) = &physical {
        scale +=
            num(h.outstanding_face.decimal())? / num(pos.current.reference_quantity.decimal())?;
        if !matches!(
            product,
            CausalPnlProduct::Standard(ValuationInputs::Bond { .. })
        ) {
            return Err(WorkflowError::Unsupported("physical handoff requires the complete original plain fixed-cash bond valuation owner").into());
        }
    }
    let fraction = num(view.remaining_fraction.decimal())?;
    scale *= fraction;
    let active = view.status == l::EconomicStatus::Active;
    if cohort.is_none()
        && (!view.obligations.is_empty() || fraction != 1.0 || !active)
        && unit(state)
            != if matches!(state.instrument(), l::LifecycleInstrument::Swap(_))
                && pos.current.quantity_unit == q::RiskQuantityUnit::Notional
            {
                dec(pos
                    .current
                    .quantity
                    .decimal()
                    .checked_div(pos.current.reference_quantity.decimal()))?
            } else {
                pos.current.quantity.decimal()
            }
    {
        return Err(WorkflowError::Unsupported("fills across prior cash/termination/default require separately identified original cohorts and their actual lifecycle quantities").into());
    }
    if !active && !pos.pending_quantity.decimal().is_zero() {
        return Err(bad(
            "pending",
            "ended economic instrument cannot retain hypothetical pending exposure",
        ));
    }
    let (valuation, reference, basis, ctd, curves) = match product {
        CausalPnlProduct::Standard(p) => {
            standard(p, state, point, pos)?;
            let curves = match p {
                ValuationInputs::Bond { market, .. }
                | ValuationInputs::FloatingBond { market, .. }
                | ValuationInputs::Swap { market, .. } => Some(market.curves),
                ValuationInputs::Cds { curves, .. } => Some(*curves),
                _ => None,
            };
            if active {
                let v = if cohort.is_some() {
                    if let ValuationInputs::Cds {
                        contract,
                        context,
                        curves,
                    } = p
                    {
                        Valuation::Cds(Box::new(
                            convex_credit::price_cds_remaining_exposure(contract, context, curves)
                                .map_err(WorkflowError::from)?,
                        ))
                    } else {
                        price(p)?
                    }
                } else {
                    price(p)?
                };
                let amount = p.amount(&v)?.value().get();
                (
                    LifecycleModelValue::Active(Box::new(ScenarioValuation::Product(v))),
                    amount,
                    p.basis(),
                    None,
                    curves,
                )
            } else {
                (
                    LifecycleModelValue::Ended(view.status),
                    0.0,
                    p.basis(),
                    None,
                    curves,
                )
            }
        }
        CausalPnlProduct::Optional {
            contract,
            context,
            projection_context,
            market,
            inputs,
            settings,
            oas_decimal,
        } => {
            same_bond(state, contract)?;
            if context.settlement != point.valuation_date
                || context.on_payment != PaymentDateEntitlement::Exclude
                || projection_context.settlement != point.valuation_date
                || projection_context.fixing.known_at != point.known_at
                || currency != contract.terms().currency
                || pos.current.quantity_unit != q::RiskQuantityUnit::Notional
                || pos.current.reference_quantity.decimal() != contract.terms().face_value
            {
                return Err(bad(
                    "optional",
                    "full original face/date/cutoff and actual contract required",
                ));
            }
            if active {
                if let Some((v, e)) =
                    optional_notice(contract, *context, inputs, *oas_decimal, state)?
                {
                    let amount = v.present_value.value().get();
                    (
                        LifecycleModelValue::Notified {
                            notice: Box::new(
                                state
                                    .exercise_notice()
                                    .ok_or(ReconciliationError::Incomplete(
                                        "actual applied notice missing",
                                    ))?
                                    .clone(),
                            ),
                            valuation: Box::new(v),
                            exercise: e,
                        },
                        amount,
                        ValuationBasis::CurrencyPresentValue,
                        None,
                        Some(market.curves),
                    )
                } else {
                    let prepared = if matches!(contract.terms().coupon, BondCoupon::Fixed) {
                        o::prepare_fixed(
                            contract,
                            *context,
                            o::OptionalBondInputs {
                                discount: inputs.discount,
                                projection: inputs.projection,
                                model: inputs.model,
                                exercise_calendar: inputs.exercise_calendar,
                            },
                            *settings,
                        )
                    } else {
                        o::prepare_floating(
                            contract,
                            *context,
                            projection_context,
                            market.fixings,
                            market.publications.callback(),
                            o::OptionalBondInputs {
                                discount: inputs.discount,
                                projection: inputs.projection,
                                model: inputs.model,
                                exercise_calendar: inputs.exercise_calendar,
                            },
                            *settings,
                        )
                    }
                    .map_err(WorkflowError::from)?;
                    let v = prepared.price(*oas_decimal).map_err(WorkflowError::from)?;
                    let amount = v.present_value;
                    (
                        LifecycleModelValue::Active(Box::new(ScenarioValuation::Product(
                            Valuation::Optional(Box::new(v)),
                        ))),
                        amount,
                        ValuationBasis::CurrencyPresentValue,
                        None,
                        Some(market.curves),
                    )
                }
            } else {
                (
                    LifecycleModelValue::Ended(view.status),
                    0.0,
                    ValuationBasis::CurrencyPresentValue,
                    None,
                    Some(market.curves),
                )
            }
        }
        CausalPnlProduct::CashBasis {
            helper,
            resources,
            model,
            basis,
            curves,
        } => {
            if !matches!(state.instrument(),l::LifecycleInstrument::Bond{contract,..} if contract.terms()==&helper.terms)
                || helper.id != pos.identity.instrument.id.as_str()
                || helper.settlement != point.valuation_date
                || helper.on_payment != PaymentDateEntitlement::Exclude
                || pos.current.reference_quantity.decimal() != helper.terms.face_value
                || pos.current.quantity_unit != q::RiskQuantityUnit::Notional
                || currency != helper.terms.currency
            {
                return Err(bad(
                    "cash basis",
                    "same full original native security/context/units required",
                ));
            }
            if active {
                let v = c::price_basis_bond(
                    helper,
                    resources,
                    model,
                    curves.get(basis).map_err(WorkflowError::from)?.as_ref(),
                    curves,
                )?;
                let amount = v.price.present_value;
                (
                    LifecycleModelValue::Active(Box::new(ScenarioValuation::CashBasis(Box::new(
                        v,
                    )))),
                    amount,
                    ValuationBasis::CurrencyPresentValue,
                    None,
                    Some(*curves),
                )
            } else {
                (
                    LifecycleModelValue::Ended(view.status),
                    0.0,
                    ValuationBasis::CurrencyPresentValue,
                    None,
                    Some(*curves),
                )
            }
        }
        CausalPnlProduct::Delivery {
            inputs,
            discount,
            graph,
            policy,
        } => {
            futures_fills(inputs.contract, point, pos)?;
            policy
                .reserve(inputs.deliverables.len(), 1)
                .map_err(WorkflowError::from)?;
            if !matches!(state.instrument(),l::LifecycleInstrument::Futures{contract,..} if contract.as_ref()==inputs.contract)
                || inputs.settlement.date() != point.valuation_date
                || inputs.context.known_at != point.known_at
                || pos.current.quantity_unit != q::RiskQuantityUnit::Contracts
                || pos.current.reference_quantity.decimal() != Decimal::ONE
                || currency != inputs.contract.currency
            {
                return Err(bad(
                    "delivery",
                    "original contract/basket/context/one-contract units required",
                ));
            }
            if active {
                let v =
                    f::revalue_bond_futures_basket(inputs, *discount, &graph.curves, graph, &[])
                        .map_err(WorkflowError::from)?;
                let amount =
                    num(dec(v.valuation.fair_price_points.decimal().checked_mul(
                        inputs.contract.currency_per_price_point.decimal(),
                    ))?)?;
                let ctd = v.ctd.clone();
                (
                    LifecycleModelValue::Active(Box::new(ScenarioValuation::Delivery(Box::new(v)))),
                    amount,
                    ValuationBasis::FuturesQuoteLevel,
                    Some(ctd),
                    Some(&graph.curves),
                )
            } else {
                (
                    LifecycleModelValue::Ended(view.status),
                    0.0,
                    ValuationBasis::FuturesQuoteLevel,
                    None,
                    Some(&graph.curves),
                )
            }
        }
    };
    let mut claims = Vec::new();
    let mut outstanding_pv = 0.0;
    let mut removed_pv = 0.0;
    let mut recorded = Decimal::ZERO;
    let mut proposed = Decimal::ZERO;
    for o in &view.obligations {
        if o.amount.currency() != currency || o.recorded.currency() != currency {
            return Err(bad("cash", "native obligation currency mismatch"));
        }
        let entitled = cohorts::entitlement(path, point_index, index, &o.key);
        let residual_ownership = cohorts::residual_value(path, point_index, index, state, o)?;
        let owned_recorded = residual_ownership
            .as_ref()
            .map_or(o.recorded.value().decimal(), |r| {
                r.owned_recorded.value().decimal()
            });
        let owned_proposed = residual_ownership
            .as_ref()
            .map_or(o.amount.value().decimal(), |r| {
                r.owned_proposed.value().decimal()
            });
        let transferred_invoice = cohort.is_some() && o.key.kind == l::CashKind::DeliveryInvoice;
        if entitled && !transferred_invoice {
            recorded = dec(recorded.checked_add(owned_recorded))?;
            proposed = dec(proposed.checked_add(owned_proposed))?;
        }
        let outstanding = dec(o
            .amount
            .value()
            .decimal()
            .checked_sub(o.recorded.value().decimal()))?;
        let owned_outstanding = residual_ownership
            .as_ref()
            .map_or(outstanding, |r| r.owned_outstanding.value().decimal());
        let ratio = if basis == ValuationBasis::FuturesQuoteLevel
            || owned_outstanding.is_zero()
            || !entitled
        {
            1.0
        } else if o.due_date <= point.valuation_date {
            if pos.overdue == OverdueClaimPolicy::Reject {
                return Err(ReconciliationError::Incomplete(
                    "explicit complete overdue claim model required",
                ));
            }
            1.0
        } else {
            curve_ratio(cash_discount, point.valuation_date, o.due_date)?
        };
        let mut removed = 0.0;
        if active {
            match &valuation {
                LifecycleModelValue::Active(v) => match v.as_ref() {
                    ScenarioValuation::Product(Valuation::Bond(v)) => {
                        if let l::LifecycleInstrument::Bond { contract, .. } = state.instrument() {
                            removed = bond_removed(
                                v,
                                contract,
                                &o.key,
                                scale,
                                curves
                                    .ok_or_else(|| bad("curve", "actual cash curve set absent"))?,
                                point.valuation_date,
                            )?;
                        }
                    }
                    ScenarioValuation::Product(Valuation::Cds(v)) => {
                        if o.key.kind == l::CashKind::InitialCreditCash {
                            removed = v.initial_payment_pv * scale;
                        } else if o.key.kind == l::CashKind::Premium
                            && o.due_date > point.valuation_date
                        {
                            return Err(WorkflowError::Unsupported("earned delayed credit coupons require a complete original conditional-cash decomposition").into());
                        }
                    }
                    ScenarioValuation::Product(Valuation::Swap(v))
                        if matches!(
                            o.key.kind,
                            l::CashKind::FixedCoupon | l::CashKind::FloatingCoupon
                        ) =>
                    {
                        let rows = if o.key.kind == l::CashKind::FixedCoupon {
                            &v.cashflows.fixed
                        } else {
                            &v.cashflows.floating
                        };
                        let pvs = if o.key.kind == l::CashKind::FixedCoupon {
                            &v.fixed
                        } else {
                            &v.floating
                        };
                        for (f, pv) in rows.iter().zip(pvs) {
                            if f.period.accrual_dates().1 == o.key.effective_date
                                && f.period.payment_date() == o.due_date
                            {
                                removed += pv.present_value.value().get() * scale;
                            }
                        }
                    }
                    ScenarioValuation::Product(Valuation::Optional(_))
                    | ScenarioValuation::CashBasis(_)
                        if o.due_date > point.valuation_date && !o.settled =>
                    {
                        return Err(WorkflowError::Unsupported("embedded optional/issuer future earned claims require full model claim decomposition").into());
                    }
                    _ => {}
                },
                LifecycleModelValue::Notified { valuation: v, .. } => {
                    if let l::LifecycleInstrument::Bond { contract, .. } = state.instrument() {
                        removed = bond_removed(
                            v,
                            contract,
                            &o.key,
                            scale,
                            curves.ok_or_else(|| bad("curve", "actual notice curve set absent"))?,
                            point.valuation_date,
                        )?;
                    }
                }
                LifecycleModelValue::Ended(_) => {}
            }
        }
        let pv = if basis == ValuationBasis::FuturesQuoteLevel || !entitled {
            0.0
        } else {
            num(owned_outstanding)? * ratio
        };
        outstanding_pv += pv;
        removed_pv += removed;
        let cash_basis = basis == ValuationBasis::CurrencyPresentValue;
        claims.push(PnlClaimValue {
            obligation: o.clone(),
            outstanding: Money::new(ExactDecimal::new(outstanding), currency),
            discount_ratio: cash_basis.then_some(ratio),
            present_value: if cash_basis {
                Some(money(pv, currency)?)
            } else {
                None
            },
            removed_from_active_model: money(removed, currency)?,
            entitled: cohort.map(|_| entitled),
            residual_ownership,
        });
    }
    if let Some(h) = physical.as_mut() {
        let outstanding = h.outstanding_invoice.value().decimal();
        h.invoice_discount_ratio = if outstanding.is_zero() {
            1.0
        } else if h.invoice.due_date <= point.valuation_date {
            if pos.overdue == OverdueClaimPolicy::Reject {
                return Err(ReconciliationError::Incomplete(
                    "explicit complete overdue physical invoice model required",
                ));
            }
            1.0
        } else {
            curve_ratio(cash_discount, point.valuation_date, h.invoice.due_date)?
        };
        let pv = num(outstanding)? * h.invoice_discount_ratio;
        h.invoice_present_value = money(pv, currency)?;
        h.outstanding_asset_present_value = Some(money(
            reference * num(h.outstanding_face.decimal())?
                / num(pos.current.reference_quantity.decimal())?
                * fraction,
            currency,
        )?);
        outstanding_pv += pv;
        recorded = dec(recorded.checked_add(h.invoice.recorded.value().decimal()))?;
        proposed = dec(proposed.checked_add(h.invoice.amount.value().decimal()))?;
    }
    let mut pending = pos.current.clone();
    pending.quantity = pos.pending_quantity;
    let pending_scale = pending.scale().map_err(WorkflowError::from)? * fraction;
    let mut projected = pos.current.clone();
    projected.quantity = ExactDecimal::new(dec(pos
        .current
        .quantity
        .decimal()
        .checked_add(pos.pending_quantity.decimal()))?);
    let projected_scale = projected.scale().map_err(WorkflowError::from)? * fraction
        + physical.as_ref().map_or(Ok(0.0), |h| {
            Ok::<_, ReconciliationError>(
                num(h.outstanding_face.decimal())? / num(pos.current.reference_quantity.decimal())?
                    * fraction,
            )
        })?;
    let local = amounts(
        [
            reference * scale + outstanding_pv - removed_pv,
            reference * pending_scale,
            reference * projected_scale + outstanding_pv - removed_pv,
        ],
        currency,
    )?;
    let rate = fx(path, point_index, currency)?;
    let reporting = amounts(
        values(&local).map(|v| v * rate),
        path.input.reporting_currency,
    )?;
    let fills = point
        .fills
        .iter()
        .filter(|x| x.trade_id == pos.identity.id)
        .collect::<Vec<_>>();
    let sum = |f: fn(&PnlFill) -> Decimal| {
        fills
            .iter()
            .try_fold(Decimal::ZERO, |a, x| dec(a.checked_add(f(x))))
    };
    let executed = sum(|x| x.quantity.decimal())?;
    let consideration = sum(|x| x.consideration.value().decimal())?;
    let fees = sum(|x| x.fees.value().decimal())?;
    let mut executed_position = pos.current.clone();
    executed_position.quantity = ExactDecimal::new(executed);
    let terminal_quote = if basis == ValuationBasis::FuturesQuoteLevel && !active {
        let l::LifecycleInstrument::Futures { contract, .. } = state.instrument() else {
            return Err(bad(
                "terminal quote",
                "actual native futures contract absent",
            ));
        };
        let mark = view
            .futures_mark
            .as_ref()
            .filter(|m| {
                m.kind == f::FuturesMarkKind::FinalSettlement
                    || physical_future && m.kind == f::FuturesMarkKind::DailySettlement
            })
            .ok_or(ReconciliationError::Incomplete(
                "actual final rate-future settlement price absent",
            ))?;
        Some(money(
            num(dec(mark
                .price_points
                .decimal()
                .checked_mul(contract.currency_per_price_point.decimal())
                .and_then(|v| {
                    v.checked_mul(pos.current.quantity.decimal())
                }))?)?,
            currency,
        )?)
    } else {
        None
    };
    Ok(CausalPositionMark {
        path,
        point: point_index,
        index,
        state: view,
        valuation,
        basis,
        reference_amount: money(reference, currency)?,
        local,
        reporting,
        claims,
        recorded_cash: Money::new(ExactDecimal::new(recorded), currency),
        proposed_cash: Money::new(ExactDecimal::new(proposed), currency),
        executed_quantity: ExactDecimal::new(executed),
        executed_marking: money(
            reference * executed_position.scale().map_err(WorkflowError::from)? * fraction,
            currency,
        )?,
        fill_consideration: Money::new(ExactDecimal::new(consideration), currency),
        fill_fees: Money::new(ExactDecimal::new(fees), currency),
        ctd,
        terminal_quote,
        cohort: cohort.cloned(),
        physical_delivery: physical,
    })
}
