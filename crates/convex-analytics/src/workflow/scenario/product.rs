//! Closed complete product owners; no externally supplied PV/Greek callback.
use super::*;
use crate::optional_bond as o;
use convex_core::contracts::{FactorTarget, QuoteType};
use convex_credit::ReferencePin;
use convex_curves::contracts::{CurveKey, CurveRole, QuoteUnit};

/// Full native borrowed economics for one position. No storage/provider lookup,
/// price-only optional proxy or frozen baseline CTD is admitted.
pub enum BookScenarioProduct<'a> {
    /// Full original fixed/floating cash, CDS, IRS/OIS or rate-future input.
    Standard(&'a ValuationInputs<'a>),
    /// Full original contextual or context-free observed cash and issuer model.
    CashBasis {
        /// Complete original cash terms, quote and legal identity.
        helper: &'a c::CashBondHelper,
        /// Original independent calendar/contextual reference resources.
        resources: &'a c::CashHelperResources<'a>,
        /// Exact residual-basis graph input.
        basis: &'a CurveKey,
        /// Original promised-cash and separate default/par-recovery model.
        model: &'a c::BondBasisModel,
    },
    /// Admitted original HW1F, full optional cash/notice/reset/history/grids.
    Optional {
        /// Original immutable native preparation.
        prepared: &'a o::PreparedOptionalBond,
        /// Actual full security identity and revision, including OAS scope.
        instrument: &'a ReferencePin,
        /// Original fractional OAS.
        oas_decimal: f64,
    },
    /// Entire original delivery basket and independently stamped cash model.
    Delivery {
        /// Original native contract/carry/quotes/basket/CTD-selection policy.
        inputs: &'a f::BondFuturesInputs<'a>,
        /// Original materialized graph owner; recipe equality alone cannot admit.
        discount: f::FuturesCurve<'a>,
        /// Original explicit all-member work reservation.
        policy: f::BondFuturesGraphPolicy,
    },
}
fn raw_matches(m: &q::QuoteGraphMove, instrument: &ReferencePin) -> ScenarioResult<bool> {
    let d = m.key.definition();
    if !matches!(&d.coordinate, q::RiskCoordinate::Raw { .. }) {
        return Ok(false);
    }
    if !matches!(&d.factor.definition().target, FactorTarget::Instrument { instrument_id } if instrument_id.as_str() == instrument.id.as_str())
    {
        return Ok(false);
    }
    if !matches!(&d.coordinate, q::RiskCoordinate::Raw { input_id, version } if input_id == instrument.id.as_str() && *version == instrument.version)
    {
        return Err(bad(
            "raw security",
            "exact original security observation/revision required",
        ));
    }
    Ok(true)
}
fn validate_standard(
    p: &ValuationInputs<'_>,
    position: &ScenarioPositionInput,
) -> ScenarioResult<()> {
    let current = &position.current;
    let native = match p {
        ValuationInputs::Swap { contract, .. }
            if current.quantity_unit == q::RiskQuantityUnit::Notional =>
        {
            let size = contract
                .terms()
                .fixed
                .leg
                .notionals
                .first()
                .ok_or_else(|| bad("swap reference", "original full notional missing"))?
                .value;
            if current.reference_quantity.decimal() != size {
                return Err(bad(
                    "swap reference",
                    "exact first original fixed-leg notional required",
                ));
            }
            q::RiskPosition {
                quantity: ExactDecimal::new(Decimal::ONE),
                reference_quantity: ExactDecimal::new(Decimal::ONE),
                quantity_unit: q::RiskQuantityUnit::TradeCopies,
                currency: current.currency,
            }
        }
        _ => current.clone(),
    };
    p.validate_position(&native)?;
    let pin = &position.identity.instrument;
    let actual = match p {
        ValuationInputs::Swap { contract, .. } => {
            Some((contract.terms().trade_id.as_str(), contract.terms().version))
        }
        ValuationInputs::Cds { contract, .. } => {
            Some((contract.terms().trade_id.as_str(), contract.terms().version))
        }
        ValuationInputs::RateFuture { inputs, .. } => Some((
            inputs.contract.instrument_id.as_str(),
            inputs.contract.stamp.version,
        )),
        ValuationInputs::OptionalBond { .. } | ValuationInputs::BondFuture(_) => {
            return Err(bad(
                "scenario product",
                "original dedicated optional/all-member owner required",
            ))
        }
        _ => None,
    };
    if actual.is_some_and(|(id, version)| pin.id.as_str() != id || pin.version != version) {
        return Err(bad(
            "instrument",
            "original full product identity/revision differs",
        ));
    }
    Ok(())
}
fn standard_value(
    p: &ValuationInputs<'_>,
    position: &ScenarioPositionInput,
    point: &BookScenarioPoint,
) -> ScenarioResult<(ScenarioValuation, ModelMoney)> {
    let mut spread = 0.0;
    let quote_type = match p {
        ValuationInputs::Bond {
            method: BondPriceMethod::Yield { .. },
            ..
        } => Some(QuoteType::Yield),
        ValuationInputs::Bond {
            method: BondPriceMethod::DiscountSpread { .. },
            ..
        }
        | ValuationInputs::FloatingBond { .. } => Some(QuoteType::ZSpread),
        _ => None,
    };
    for m in &point.moves {
        if Some(m.key.definition().factor.definition().quote_type) == quote_type
            && raw_matches(m, &position.identity.instrument)?
        {
            if m.key.definition().response != q::RiskResponse::FrozenCurves
                || m.key.definition().native_unit != QuoteUnit::DecimalRate
                || m.key.definition().factor.definition().currency != p.currency()
            {
                return Err(bad("raw rate", "same original currency, decimal price-input units and frozen response required"));
            }
            spread += m.delta;
        }
    }
    let value = match p {
        ValuationInputs::Bond {
            cash,
            method:
                BondPriceMethod::Yield {
                    decimal_rate,
                    convention,
                },
            ..
        } => Valuation::Bond(Box::new(
            cash.price_yield(decimal_rate + spread, *convention)?,
        )),
        ValuationInputs::Bond {
            cash,
            method:
                BondPriceMethod::DiscountSpread {
                    curve,
                    role,
                    decimal_spread,
                },
            ..
        } => Valuation::Bond(Box::new(cash.price_spread(
            point.graph.curves.get(curve)?.as_ref(),
            role.clone(),
            decimal_spread + spread,
        )?)),
        ValuationInputs::FloatingBond {
            contract,
            context,
            market,
            projection,
            discount,
            discount_margin,
        } => {
            let changed = ValuationInputs::FloatingBond {
                contract,
                context,
                market: *market,
                projection: projection.clone(),
                discount: discount.clone(),
                discount_margin: discount_margin + spread,
            };
            valuation::price_with_curves(&changed, Some(&point.graph.curves))?
        }
        _ => valuation::price_with_curves(p, Some(&point.graph.curves))?,
    };
    let amount = p.amount(&value)?;
    Ok((ScenarioValuation::Product(value), amount))
}

fn optional_value(
    prepared: &o::PreparedOptionalBond,
    instrument: &ReferencePin,
    oas: f64,
    baseline: &convex_curves::contracts::CurveSet,
    point: &BookScenarioPoint,
) -> ScenarioResult<(ScenarioValuation, ModelMoney)> {
    let (discount, projection) = prepared.curve_recipes();
    let mut shift = o::CurveShift::BASE;
    let mut oas_move = 0.0;
    let mut mean = 0.0;
    let mut sigma = 0.0;
    for m in &point.moves {
        let d = m.key.definition();
        let factor = d.factor.definition();
        match &d.coordinate {
            q::RiskCoordinate::Zero { curve, node: None } if *curve == discount.key || projection.is_some_and(|p| p.key == *curve) => {
                if d.response != q::RiskResponse::FrozenCurves || d.native_unit != QuoteUnit::DecimalRate { return Err(bad("optional curve", "original frozen parallel decimal-zero scenario required")); }
                if *curve == discount.key { shift.discount += m.delta; } else { shift.projection += m.delta; }
            }
            q::RiskCoordinate::Raw { .. } if factor.quote_type == QuoteType::JointZeroRate => {
                let (dkey, pkey) = q::joint_curve_keys(&m.key)?;
                if dkey == discount.key || projection.is_some_and(|p| p.key == pkey) {
                    if dkey != discount.key || !projection.is_some_and(|p| p.key == pkey) { return Err(bad("optional joint", "exact original independent discount/projection pair required")); }
                    shift.discount += m.delta; shift.projection += m.delta;
                }
            }
            q::RiskCoordinate::Raw { input_id, .. } if matches!(factor.quote_type, QuoteType::Hw1fMeanReversion | QuoteType::Hw1fVolatility)
                && matches!(&factor.target, FactorTarget::Model { model_id, .. } if model_id.as_str() == prepared.model().key().id) => {
                let actual = prepared.model().key();
                let name = if factor.quote_type == QuoteType::Hw1fMeanReversion { "mean_reversion" } else { "volatility" };
                if input_id != name || factor.currency != prepared.currency() || !matches!(&factor.target, FactorTarget::Model { model_version, .. } if *model_version == actual.version) { return Err(bad("optional model", "exact original currency, parameter observation/model revision required")); }
                if factor.quote_type == QuoteType::Hw1fMeanReversion { mean += m.delta; } else { sigma += m.delta; }
            }
            q::RiskCoordinate::Raw { .. } if factor.quote_type == QuoteType::OasSpread && raw_matches(m, instrument)? => {
                if factor.currency != prepared.currency() || d.native_unit != QuoteUnit::DecimalRate { return Err(bad("optional OAS", "original currency and fractional OAS input units required")); }
                oas_move += m.delta;
            }
            q::RiskCoordinate::Quote { curve, .. } | q::RiskCoordinate::ParallelQuotes { curve, .. } | q::RiskCoordinate::Zero { curve, .. }
                if *curve == discount.key || projection.is_some_and(|p| p.key == *curve) => return Err(WorkflowError::Unsupported("optional shaped/rebuilt curves require their original full model integration; parallel price_scenario cannot substitute").into()),
            _ => (),
        }
    }
    q::admit_optional_scenario_curves(prepared, baseline, &point.graph.curves, &point.moves)?;
    let value = prepared
        .price_combined_scenario(oas + oas_move, shift, mean, sigma)
        .map_err(WorkflowError::from)?;
    let amount = money(value.present_value, prepared.currency())?;
    Ok((
        ScenarioValuation::Product(Valuation::Optional(Box::new(value))),
        amount,
    ))
}
fn raw_consumption(
    product: &BookScenarioProduct<'_>,
    p: &ScenarioPositionInput,
    moves: &[q::QuoteGraphMove],
) -> ScenarioResult<Vec<bool>> {
    moves.iter().map(|m| {
        let factor = m.key.definition().factor.definition();
        Ok(match product {
            BookScenarioProduct::Standard(v) => (match v {
                ValuationInputs::Bond { method: BondPriceMethod::Yield { .. }, .. } => factor.quote_type == QuoteType::Yield,
                ValuationInputs::Bond { method: BondPriceMethod::DiscountSpread { .. }, .. } | ValuationInputs::FloatingBond { .. } => factor.quote_type == QuoteType::ZSpread,
                _ => false,
            }) && raw_matches(m, &p.identity.instrument)?,
            BookScenarioProduct::CashBasis { .. } => false,
            BookScenarioProduct::Optional { prepared, instrument, .. } =>
                (factor.quote_type == QuoteType::OasSpread && raw_matches(m, instrument)?) ||
                (matches!(factor.quote_type, QuoteType::Hw1fMeanReversion | QuoteType::Hw1fVolatility)
                    && matches!(&factor.target, FactorTarget::Model { model_id, .. } if model_id.as_str() == prepared.model().key().id)),
            BookScenarioProduct::Delivery { inputs, .. } => matches!(m.key.definition().coordinate, q::RiskCoordinate::Raw { .. })
                && matches!(factor.quote_type, QuoteType::RepoRate | QuoteType::CashCleanPrice)
                && matches!(&factor.target, FactorTarget::Instrument { instrument_id } if inputs.deliverables.iter().any(|d| d.security.instrument.id == *instrument_id)),
        })
    }).collect()
}

/// Revalue every baseline/prefix/standalone point through the SAME original
/// complete native pricer. Private results remain bound to this exact path.
pub fn revalue_book_position<'p>(
    path: &'p PreparedBookScenarios,
    index: usize,
    product: &BookScenarioProduct<'_>,
) -> ScenarioResult<ScenarioTradeResult<'p>> {
    let position = path
        .input
        .positions
        .get(index)
        .ok_or_else(|| bad("position", "outside exact original expected book"))?;
    let current = &position.current;
    let pin = &position.identity.instrument;
    let baseline = &path.points[0].graph.curves;
    let basis = match product {
        BookScenarioProduct::Standard(v) => {
            validate_standard(v, position)?;
            v.basis()
        }
        BookScenarioProduct::CashBasis { helper, .. } => {
            if pin.id.as_str() != helper.id
                || current.currency != helper.terms.currency
                || current.quantity_unit != q::RiskQuantityUnit::Notional
                || current.reference_quantity.decimal() != helper.terms.face_value
                || helper.credit_identity.as_ref().is_some_and(|v| {
                    v.instrument_id.as_str() != pin.id.as_str()
                        || v.instrument_version != pin.version
                })
            {
                return Err(bad(
                    "cash position",
                    "original security revision, face and currency required",
                ));
            }
            ValuationBasis::CurrencyPresentValue
        }
        BookScenarioProduct::Optional {
            prepared,
            instrument,
            ..
        } => {
            q::admit_optional_curve_set(prepared, baseline)?;
            prepared
                .admit_scenario_baseline(baseline)
                .map_err(WorkflowError::from)?;
            if *instrument != pin
                || current.currency != prepared.currency()
                || !((current.quantity_unit == q::RiskQuantityUnit::Notional
                    && current.reference_quantity.decimal() == prepared.face_value())
                    || (current.quantity_unit == q::RiskQuantityUnit::TradeCopies
                        && current.reference_quantity.decimal() == Decimal::ONE))
            {
                return Err(bad(
                    "optional position",
                    "original full face/trade copies and security revision required",
                ));
            }
            ValuationBasis::CurrencyPresentValue
        }
        BookScenarioProduct::Delivery { inputs, policy, .. } => {
            policy
                .reserve(inputs.deliverables.len(), path.input.moves.len())
                .map_err(WorkflowError::from)?;
            if current.currency != inputs.contract.currency
                || current.quantity_unit != q::RiskQuantityUnit::Contracts
                || current.reference_quantity.decimal() != Decimal::ONE
                || pin.id.as_str() != inputs.contract.instrument_id.as_str()
                || pin.version != inputs.contract.stamp.version
            {
                return Err(bad(
                    "delivery position",
                    "original one-contract reference, security revision and currency required",
                ));
            }
            ValuationBasis::FuturesQuoteLevel
        }
    };
    let consumed_raw_factors = raw_consumption(product, position, &path.input.moves)?;
    let mut values = Vec::with_capacity(path.points.len());
    let pending = quantity(position, true)?;
    let projected = quantity(position, false)?;
    let rate = fx_rate(&path.input, current.currency)?;
    for point in &path.points {
        let (valuation, reference_amount) = match product {
            BookScenarioProduct::Standard(v) => standard_value(v, position, point)?,
            BookScenarioProduct::Optional {
                prepared,
                instrument,
                oas_decimal,
            } => optional_value(prepared, instrument, *oas_decimal, baseline, point)?,
            BookScenarioProduct::CashBasis {
                helper,
                resources,
                basis,
                model,
            } => {
                let mut changed = (*model).clone();
                if let c::BondBasisModel::CreditParRecovery {
                    survival, recovery, ..
                } = &mut changed
                {
                    let credit = point.graph.curves.get(survival)?;
                    let CurveRole::Credit {
                        recovery: actual, ..
                    } = credit.recipe().role
                    else {
                        return Err(bad("cash recovery", "original actual Credit role required"));
                    };
                    *recovery = actual;
                }
                let value = c::price_basis_bond(
                    helper,
                    resources,
                    &changed,
                    point.graph.curves.get(basis)?.as_ref(),
                    &point.graph.curves,
                )?;
                let amount = money(value.price.present_value, current.currency)?;
                (ScenarioValuation::CashBasis(Box::new(value)), amount)
            }
            BookScenarioProduct::Delivery {
                inputs, discount, ..
            } => {
                let moves = point
                    .moves
                    .iter()
                    .filter(|m| {
                        !matches!(m.key.definition().coordinate, q::RiskCoordinate::Raw { .. })
                            || path
                                .input
                                .moves
                                .iter()
                                .position(|v| v.key == m.key)
                                .is_some_and(|i| consumed_raw_factors[i])
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                let value = f::revalue_bond_futures_basket(
                    inputs,
                    *discount,
                    baseline,
                    &point.graph,
                    &moves,
                )
                .map_err(WorkflowError::from)?;
                let level = value
                    .valuation
                    .fair_price_points
                    .decimal()
                    .checked_mul(inputs.contract.currency_per_price_point.decimal())
                    .and_then(|v| v.to_f64())
                    .ok_or_else(|| {
                        bad(
                            "futures quote level",
                            "original Decimal multiplier overflow",
                        )
                    })?;
                (
                    ScenarioValuation::Delivery(Box::new(value)),
                    money(level, current.currency)?,
                )
            }
        };
        let v = reference_amount.value().get();
        let local = ScenarioAmounts {
            current: money(v * current.scale()?, current.currency)?,
            pending: money(v * pending.scale()?, current.currency)?,
            projected: money(v * projected.scale()?, current.currency)?,
        };
        let reporting = local.map(path.input.reporting_currency, |v| v * rate)?;
        values.push(ScenarioPositionValue {
            valuation,
            reference_amount,
            local,
            reporting,
        });
    }
    if let BookScenarioProduct::Standard(p) = product {
        if p.monetary_value()? != values[0].reference_amount {
            return Err(bad(
                "scenario baseline",
                "original supplied full price differs from actual graph information set",
            ));
        }
    }
    let local = attribution(
        &values.iter().map(|v| v.local.clone()).collect::<Vec<_>>(),
        path.input.moves.len(),
    )?;
    let reporting = attribution(
        &values
            .iter()
            .map(|v| v.reporting.clone())
            .collect::<Vec<_>>(),
        path.input.moves.len(),
    )?;
    let mut ctd_transitions = Vec::new();
    for pair in values[..=path.input.moves.len()].windows(2) {
        if let (ScenarioValuation::Delivery(before), ScenarioValuation::Delivery(after)) =
            (&pair[0].valuation, &pair[1].valuation)
        {
            let before = before.ctd.clone();
            let after = after.ctd.clone();
            ctd_transitions.push(ScenarioCtdTransition {
                selection_changed: before.selection_ties != after.selection_ties,
                fair_deliverable_changed: before.fair_ties != after.fair_ties,
                ties_present: [&before, &after]
                    .iter()
                    .any(|v| v.selection_ties.len() > 1 || v.fair_ties.len() > 1),
                before,
                after,
            });
        }
    }
    Ok(ScenarioTradeResult {
        path,
        input: position.clone(),
        basis,
        values,
        local,
        reporting,
        ctd_transitions,
        consumed_raw_factors,
    })
}
