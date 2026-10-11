//! Full basket graph revaluation. Cash, carry and CTD mathematics stay with their
//! original owners; no baseline winner is substituted for an entire basket.

use super::*;
use crate::{bond_contract::ContractPrice, quote_risk as q};
use convex_core::{cashflows::PaymentDateEntitlement, contracts::SnapshotId};
use convex_curves::contracts::{CurveKey, CurveRecipe, CurveSet, QuoteRepricing, QuoteUnit};

/// Explicit full-member work reservation, additional to all original product and
/// calibration bounds. One factor reserves twelve member price/carry visits.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondFuturesGraphPolicy {
    /// 1..24,576 visits; a sixteen-factor, 128-member book reserves the ceiling.
    pub max_member_evaluations: usize,
}
impl BondFuturesGraphPolicy {
    /// Admit the entire reservation before pricing, including zero exposures.
    pub fn reserve(&self, members: usize, factors: usize) -> FuturesResult<usize> {
        if !(1..=128).contains(&members) || !(1..=16).contains(&factors) {
            return Err(invalid(
                "basket risk work",
                "1..128 members and 1..16 factors required",
            ));
        }
        let visits = 12 * members * factors;
        if self.max_member_evaluations == 0
            || self.max_member_evaluations > 24_576
            || visits > self.max_member_evaluations
        {
            return Err(invalid(
                "basket risk work",
                "complete twelve-visit member reservation exhausted",
            ));
        }
        Ok(visits)
    }
}
/// Complete common coordinate, signed original contracts and numerical policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondFuturesGraphRequest {
    /// Full original factor, including actual source revision and monetary unit.
    pub key: q::QuoteRiskKey,
    /// Signed integer contracts; the native reference is exactly one contract.
    pub position: q::RiskPosition,
    /// Original immutable information set, never a new market publication.
    pub snapshot_id: SnapshotId,
    /// Mandatory actual central and half-step stability policy.
    pub settings: q::QuoteRiskSettings,
    /// Explicit full-member work bound.
    pub policy: BondFuturesGraphPolicy,
}
/// Exact tie sets in original identity order. No proximity epsilon replaces the
/// original Decimal rankings; a near tie remains a positive selection gap.
#[derive(Debug, Clone, Serialize)]
pub struct BondFuturesCtdState {
    /// Every exact tied winner under the independently declared selection rule.
    pub selection_ties: Vec<InstrumentPin>,
    /// Every exact tied minimum conversion-factor-adjusted forward price.
    pub fair_ties: Vec<InstrumentPin>,
}
fn ctd(value: &BondFuturesValuation) -> FuturesResult<BondFuturesCtdState> {
    let selection_score = |a: &BondFuturesAlternative| match value.selection {
        FuturesCtdMethod::MinimumNetBasis => a.net_basis_per100.decimal(),
        FuturesCtdMethod::MaximumImpliedRepo => -a.implied_repo_decimal.decimal(),
        FuturesCtdMethod::MinimumForwardPrice => a.forward_price_points.decimal(),
    };
    let best = value
        .alternatives
        .iter()
        .find(|a| a.instrument == value.selected)
        .ok_or_else(|| invalid("CTD", "selected original member absent"))?;
    Ok(BondFuturesCtdState {
        selection_ties: value
            .alternatives
            .iter()
            .filter(|a| selection_score(a) == selection_score(best))
            .map(|a| a.instrument.clone())
            .collect(),
        fair_ties: value
            .alternatives
            .iter()
            .filter(|a| a.forward_price_points == value.fair_price_points)
            .map(|a| a.instrument.clone())
            .collect(),
    })
}
/// Both full original and scenario cash valuations for one actual member.
#[derive(Debug, Clone, Serialize)]
pub struct BondFuturesMemberRevaluation {
    /// Exact security revision; complete cash/profile/calendar data stays below.
    pub instrument: InstrumentPin,
    /// Actual observed cash quotation, separate from either model quotation.
    pub observed_clean_per100: ExactDecimal,
    /// Full original discount-model cash, original face, accrued and dates.
    pub baseline_model: ContractPrice,
    /// Full actually repriced scenario cash and curve identity.
    pub scenario_model: ContractPrice,
    /// Original observed minus model clean quote, held under curve moves.
    pub frozen_residual_basis_per100: ExactDecimal,
    /// Actual changed clean quote supplied to the SAME full carry pricer.
    pub scenario_clean_per100: ExactDecimal,
}
/// Lossless actual graph source recipes and original fit/repricing evidence.
#[derive(Debug, Clone, Serialize)]
pub struct BondFuturesGraphDiagnostics {
    /// Every actually supplied/evaluated scenario curve recipe, in key order.
    pub curves: Vec<CurveRecipe>,
    /// Original dependency-first full recipe rebuild order.
    pub build_order: Vec<CurveKey>,
    /// Actual native-unit helper repricing; not a synthetic zero residual.
    pub repricing: Vec<QuoteRepricing>,
    /// Measured original calibration work.
    pub price_evaluations: usize,
    /// Original frozen calibration publication decisions.
    pub publications: Vec<convex_core::fixings::FixingRequirement>,
}
fn diagnostics(graph: &q::QuoteGraphScenario) -> FuturesResult<BondFuturesGraphDiagnostics> {
    Ok(BondFuturesGraphDiagnostics {
        curves: graph
            .curves
            .keys()
            .map(|key| graph.curves.get(key).map(|c| c.recipe().clone()))
            .collect::<Result<_, _>>()?,
        build_order: graph.build_order.clone(),
        repricing: graph.repricing.clone(),
        price_evaluations: graph.price_evaluations,
        publications: graph.publications.clone(),
    })
}
/// One complete actual basket evaluation; futures money is unrounded model
/// points times currency per point per contract, never discounted lifetime PV.
#[derive(Debug, Clone, Serialize)]
pub struct BondFuturesGraphEvaluation {
    /// Actual canonical move; original stamps remain hypothetical within snapshot.
    pub delta: f64,
    /// Every original member, full funding/carry/invoice and independent CTD choice.
    pub valuation: BondFuturesValuation,
    /// Both complete native cash valuations and original residual for every member.
    pub members: Vec<BondFuturesMemberRevaluation>,
    /// Exact tie sets for this particular scenario, independent from switches.
    pub ctd: BondFuturesCtdState,
    /// Original full graph and actually rebuilt native quote diagnostics.
    pub graph: BondFuturesGraphDiagnostics,
}
/// Complete five-basket risk. Central and one-sided values are finite-bump
/// measurements; a CTD switch/tie never asserts an infinitesimal smooth Greek.
#[derive(Debug, Clone, Serialize)]
pub struct BondFuturesGraphRisk {
    /// Original complete coordinate/position/snapshot/work policy.
    pub request: BondFuturesGraphRequest,
    /// Actual original stamped discount model, with full construction semantics.
    pub discount: CurveRecipe,
    /// Exact original discount source/quality/availability.
    pub discount_stamp: ReferenceStamp,
    /// Twelve visits per member, reserved before any valuation.
    pub reserved_member_evaluations: usize,
    /// Actual five-basket reselected common-factor monetary measurement.
    pub sensitivity: q::QuoteSensitivity,
    /// Exactly [0,+h,-h,+h/2,-h/2], with every basket alternative retained.
    pub evaluations: Vec<BondFuturesGraphEvaluation>,
    /// Separate diagnostic holding the ORIGINAL fair-deliverable fixed at +/-h.
    pub frozen_deliverable_sensitivity: f64,
    /// Same separate diagnostic at +/-h/2.
    pub frozen_deliverable_half_sensitivity: f64,
    /// Actual normalized plus-side secant from base to +h.
    pub one_sided_plus: f64,
    /// Actual normalized minus-side secant from -h to base.
    pub one_sided_minus: f64,
    /// Declared selection winner changes among any of the five valuations.
    pub selection_switch: bool,
    /// Fixed-date fair-price winner changes among any of the five valuations.
    pub fair_deliverable_switch: bool,
    /// Any scenario contains multiple exact selection/fair winners.
    pub ties_present: bool,
    /// False for a switch or tie; never a universal model/convergence certificate.
    pub ctd_stable: bool,
}

fn risk_error(error: impl std::fmt::Display) -> q::QuoteRiskError {
    q::QuoteRiskError::Invalid {
        field: "full delivery-futures basket".into(),
        reason: error.to_string(),
    }
}
fn finite(value: f64, field: &str) -> q::QuoteRiskResult<f64> {
    convex_core::contracts::FiniteF64::new(value)
        .map(|v| v.get())
        .map_err(|e| risk_error(format!("{field}: {e}")))
}
/// Bind the EXACT original complete discount recipe to the prepared information
/// set. Borrow its original materialized curve from `graph.baseline().curves`;
/// helper/derived recipes alone do not contain all resolved values/dependencies.
pub fn admit_bond_futures_graph(
    inputs: &BondFuturesInputs<'_>,
    discount: FuturesCurve<'_>,
    baseline: &CurveSet,
) -> FuturesResult<()> {
    let curve = baseline.get(&discount.curve.recipe().key)?;
    if curve.recipe() != discount.curve.recipe() {
        return Err(invalid(
            "basket risk baseline",
            "entire original discount recipe differs from prepared graph",
        ));
    }
    if !std::ptr::eq(curve.as_ref(), discount.curve) {
        return Err(invalid(
            "basket risk baseline",
            "borrow the original immutable discount curve from the prepared graph; a recipe match cannot admit a different materialized fit or dependency",
        ));
    }
    let mut base = price_bond_future(inputs)?;
    discount.check(&mut base.inputs)?;
    Ok(())
}
fn position(inputs: &BondFuturesInputs<'_>, position: &q::RiskPosition) -> q::QuoteRiskResult<()> {
    position.scale()?;
    let quantity = position.quantity.decimal();
    if position.currency != inputs.contract.currency
        || position.quantity_unit != q::RiskQuantityUnit::Contracts
        || position.reference_quantity.decimal() != Decimal::ONE
        || quantity.abs() > Decimal::from(1_000_000_000i64)
    {
        return Err(risk_error(
            "exact native currency, integer contracts and one-contract reference required",
        ));
    }
    Ok(())
}

// Strict raw factors bind only one actual original financing or cash observation.
// Parallel repo remains available through the original native bond_futures_risk;
// no fictitious aggregate source revision is created here.
fn raw_member<'a>(
    inputs: &'a BondFuturesInputs<'_>,
    key: &q::QuoteRiskKey,
) -> q::QuoteRiskResult<Option<(&'a InstrumentPin, bool)>> {
    let d = key.definition();
    let q::RiskCoordinate::Raw { input_id, version } = &d.coordinate else {
        return Ok(None);
    };
    let convex_core::contracts::FactorTarget::Instrument { instrument_id } =
        &d.factor.definition().target
    else {
        return Err(risk_error(
            "raw delivery factor requires exact original basket security",
        ));
    };
    let member = inputs
        .deliverables
        .iter()
        .find(|m| m.security.instrument.id == *instrument_id)
        .ok_or_else(|| risk_error("raw factor names no original basket security"))?;
    let repo = match d.factor.definition().quote_type {
        convex_core::contracts::QuoteType::RepoRate if d.native_unit == QuoteUnit::DecimalRate => {
            true
        }
        convex_core::contracts::QuoteType::CashCleanPrice
            if d.native_unit == QuoteUnit::PricePer100 =>
        {
            false
        }
        _ => {
            return Err(risk_error(
                "actual member decimal repo or clean price-point basis required",
            ))
        }
    };
    let stamp = if repo {
        &member.financing.stamp
    } else {
        member.quote_stamp
    };
    if input_id != stamp.id.as_str()
        || *version != stamp.version
        || d.factor.definition().tenor.as_str() != "spot"
    {
        return Err(risk_error(
            "original member quote/source revision or tenor mismatch",
        ));
    }
    Ok(Some((member.security.instrument, repo)))
}

fn evaluate(
    inputs: &BondFuturesInputs<'_>,
    discount: FuturesCurve<'_>,
    originals: &[(InstrumentPin, ContractPrice)],
    graph: &q::QuoteGraphScenario,
    key: &q::QuoteRiskKey,
    delta: f64,
) -> FuturesResult<BondFuturesGraphEvaluation> {
    evaluate_many(
        inputs,
        discount,
        originals,
        graph,
        &[q::QuoteGraphMove {
            key: key.clone(),
            delta,
        }],
        delta,
    )
}
fn evaluate_many(
    inputs: &BondFuturesInputs<'_>,
    discount: FuturesCurve<'_>,
    originals: &[(InstrumentPin, ContractPrice)],
    graph: &q::QuoteGraphScenario,
    moves: &[q::QuoteGraphMove],
    delta: f64,
) -> FuturesResult<BondFuturesGraphEvaluation> {
    let raw = moves
        .iter()
        .map(|m| {
            Ok((
                raw_member(inputs, &m.key)
                    .map_err(|e| invalid("raw basket factor", e.to_string()))?,
                decimal(m.delta)?,
            ))
        })
        .collect::<FuturesResult<Vec<_>>>()?;
    let changed_discount = graph.curves.get(&discount.curve.recipe().key)?;
    let mut members = Vec::with_capacity(inputs.deliverables.len());
    let mut financing = Vec::with_capacity(inputs.deliverables.len());
    for d in inputs.deliverables {
        let model = originals
            .iter()
            .find(|(pin, _)| pin == d.security.instrument)
            .ok_or_else(|| invalid("basket model", "original complete member price absent"))?
            .1
            .clone();
        let scenario = crate::bond_contract::price_fixed(
            d.security.bond,
            inputs.settlement.date(),
            PaymentDateEntitlement::Exclude,
            changed_discount,
        )?;
        let residual = arithmetic(
            d.clean_price_per100
                .decimal()
                .checked_sub(decimal(model.clean_price)?),
            "cash basis",
        )?;
        // Accrued/cash entitlement never changes under a financial factor move.
        let move_cash = decimal(scenario.clean_price - model.clean_price)?;
        let mut basis_move = Decimal::ZERO;
        let mut f = d.financing.clone();
        for (member, bump) in &raw {
            if member.is_some_and(|(pin, _)| pin == d.security.instrument) {
                if member.is_some_and(|(_, repo)| repo) {
                    f.repo_decimal = ExactDecimal::new(arithmetic(
                        f.repo_decimal.decimal().checked_add(*bump),
                        "member repo",
                    )?);
                } else {
                    basis_move = arithmetic(basis_move.checked_add(*bump), "member basis")?;
                }
            }
        }
        let clean = arithmetic(
            d.clean_price_per100
                .decimal()
                .checked_add(move_cash)
                .and_then(|v| v.checked_add(basis_move)),
            "scenario clean",
        )?;
        financing.push(f);
        members.push(BondFuturesMemberRevaluation {
            instrument: d.security.instrument.clone(),
            observed_clean_per100: d.clean_price_per100,
            baseline_model: model,
            scenario_model: scenario,
            frozen_residual_basis_per100: ExactDecimal::new(residual),
            scenario_clean_per100: ExactDecimal::new(clean),
        });
    }
    let deliverables: Vec<_> = inputs
        .deliverables
        .iter()
        .zip(&members)
        .zip(&financing)
        .map(|((d, m), f)| BondFutureDeliverable {
            security: d.security,
            clean_price_per100: m.scenario_clean_per100,
            quote_stamp: d.quote_stamp,
            financing: f,
            own_cash: d.own_cash,
            coupon_carry: d.coupon_carry,
        })
        .collect();
    let native = BondFuturesInputs {
        contract: inputs.contract,
        basket: inputs.basket,
        delivery_calendar: inputs.delivery_calendar,
        settlement: inputs.settlement,
        delivery: inputs.delivery,
        market: inputs.market,
        deliverables: &deliverables,
        selection: inputs.selection,
        context: inputs.context,
    };
    let mut valuation = price_bond_future(&native)?;
    discount.check(&mut valuation.inputs)?;
    valuation.inputs.finish();
    members.sort_by(|a, b| {
        (&a.instrument.id, a.instrument.version).cmp(&(&b.instrument.id, b.instrument.version))
    });
    Ok(BondFuturesGraphEvaluation {
        delta,
        ctd: ctd(&valuation)?,
        valuation,
        members,
        graph: diagnostics(graph)?,
    })
}
fn level(value: &BondFuturesValuation, inputs: &BondFuturesInputs<'_>) -> FuturesResult<f64> {
    number(arithmetic(
        value
            .fair_price_points
            .decimal()
            .checked_mul(inputs.contract.currency_per_price_point.decimal()),
        "futures quote level",
    )?)
}

/// Reprice every member at every canonical move through the original full graph,
/// cash and carry owners. Every half-step must satisfy the caller's unchanged
/// monetary policy; failed/non-smooth finite differences never become zero risk.
pub fn bond_futures_graph_risk(
    inputs: &BondFuturesInputs<'_>,
    discount: FuturesCurve<'_>,
    graph: &q::PreparedQuoteRiskGraph<'_>,
    request: &BondFuturesGraphRequest,
) -> q::QuoteRiskResult<BondFuturesGraphRisk> {
    request.settings.validate_for(&request.key)?;
    position(inputs, &request.position)?;
    if request.key.definition().factor.definition().currency != inputs.contract.currency {
        return Err(risk_error("actual native factor currency required"));
    }
    let reservation = request
        .policy
        .reserve(inputs.deliverables.len(), 1)
        .map_err(risk_error)?;
    raw_member(inputs, &request.key)?;
    admit_bond_futures_graph(inputs, discount, &graph.baseline().curves)?;
    let originals = inputs
        .deliverables
        .iter()
        .map(|d| {
            crate::bond_contract::price_fixed(
                d.security.bond,
                inputs.settlement.date(),
                PaymentDateEntitlement::Exclude,
                discount.curve,
            )
            .map(|v| (d.security.instrument.clone(), v))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| q::QuoteRiskError::Futures(FuturesError::Bond(e)))?;
    let mut evaluations = Vec::with_capacity(5);
    let sensitivity = q::reference_quote_sensitivity(
        request.key.clone(),
        request.position.clone(),
        request.snapshot_id.clone(),
        request.settings.clone(),
        |delta| {
            let changed;
            let scenario = if delta == 0.0
                || matches!(
                    request.key.definition().coordinate,
                    q::RiskCoordinate::Raw { .. }
                ) {
                graph.baseline()
            } else {
                changed = q::evaluate_quote_graph(graph, &request.key, delta)?;
                &changed
            };
            let evaluation = evaluate(inputs, discount, &originals, scenario, &request.key, delta)?;
            let money = q::risk_reference_pv(
                level(&evaluation.valuation, inputs).map_err(risk_error)?,
                inputs.contract.currency,
            )?;
            evaluations.push(evaluation);
            Ok(money)
        },
    )?;
    let baseline = &evaluations[0].valuation;
    let scale = request.position.scale()?;
    let unit = request.key.definition().unit.coordinate_size();
    let frozen = |i: usize| -> q::QuoteRiskResult<f64> {
        let a = evaluations[i]
            .valuation
            .alternatives
            .iter()
            .find(|a| a.instrument == baseline.fair_deliverable)
            .ok_or_else(|| risk_error("original fair member missing after full scenario"))?;
        let points = arithmetic(
            a.forward_price_points
                .decimal()
                .checked_mul(inputs.contract.currency_per_price_point.decimal()),
            "frozen quote level",
        )
        .map_err(risk_error)?;
        Ok(number(points).map_err(risk_error)? * scale)
    };
    let selection_switch = evaluations
        .iter()
        .any(|e| e.valuation.selected != baseline.selected);
    let fair_deliverable_switch = evaluations
        .iter()
        .any(|e| e.valuation.fair_deliverable != baseline.fair_deliverable);
    let ties_present = evaluations
        .iter()
        .any(|e| e.ctd.selection_ties.len() > 1 || e.ctd.fair_ties.len() > 1);
    Ok(BondFuturesGraphRisk {
        request: request.clone(),
        discount: discount.curve.recipe().clone(),
        discount_stamp: discount.stamp.clone(),
        reserved_member_evaluations: reservation,
        frozen_deliverable_sensitivity: finite(
            (frozen(1)? - frozen(2)?) / (2.0 * request.settings.bump) * unit,
            "frozen risk",
        )?,
        frozen_deliverable_half_sensitivity: finite(
            (frozen(3)? - frozen(4)?) / request.settings.bump * unit,
            "frozen half risk",
        )?,
        one_sided_plus: finite(
            (sensitivity.plus_pv - sensitivity.base_pv) / request.settings.bump * unit,
            "plus secant",
        )?,
        one_sided_minus: finite(
            (sensitivity.base_pv - sensitivity.minus_pv) / request.settings.bump * unit,
            "minus secant",
        )?,
        sensitivity,
        evaluations,
        selection_switch,
        fair_deliverable_switch,
        ties_present,
        ctd_stable: !selection_switch && !fair_deliverable_switch && !ties_present,
    })
}

/// Full actual single-shock scenario with signed unrounded futures quote-level money.
#[derive(Debug, Clone, Serialize)]
pub struct BondFuturesGraphScenario {
    /// Complete original checked coordinate/position/snapshot/work request.
    pub request: BondFuturesScenarioRequest,
    /// Entire original basket, not a baseline winning-bond proxy.
    pub baseline: BondFuturesGraphEvaluation,
    /// Entire repriced basket with reselected and tied CTD choices.
    pub shocked: BondFuturesGraphEvaluation,
    /// Signed original fair model quote-level currency amount.
    pub baseline_amount: convex_core::contracts::ModelMoney,
    /// Signed actual shocked fair model quote-level currency amount.
    pub scenario_amount: convex_core::contracts::ModelMoney,
    /// Actual signed scenario minus base quote-level change; no realized cash claim.
    pub change: convex_core::contracts::ModelMoney,
}
/// Full revaluation request, without inventing a derivative bump/stability policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondFuturesScenarioRequest {
    /// Complete actual coordinate and original source pins.
    pub key: q::QuoteRiskKey,
    /// Actual signed integer contracts relative to exactly one native contract.
    pub position: q::RiskPosition,
    /// Original immutable information set.
    pub snapshot_id: SnapshotId,
    /// Explicit all-member work policy, conservative for this two-price operation.
    pub policy: BondFuturesGraphPolicy,
}
/// Full all-member revaluation, including arbitrary admitted positive/negative raw
/// repo/basis or graph moves. No derivative stability certificate is inferred.
pub fn bond_futures_graph_scenario(
    inputs: &BondFuturesInputs<'_>,
    discount: FuturesCurve<'_>,
    graph: &q::PreparedQuoteRiskGraph<'_>,
    request: &BondFuturesScenarioRequest,
    delta: f64,
) -> q::QuoteRiskResult<BondFuturesGraphScenario> {
    finite(delta, "scenario")?;
    position(inputs, &request.position)?;
    request
        .policy
        .reserve(inputs.deliverables.len(), 1)
        .map_err(risk_error)?;
    if request.key.definition().factor.definition().currency != inputs.contract.currency {
        return Err(risk_error("actual native factor currency required"));
    }
    raw_member(inputs, &request.key)?;
    admit_bond_futures_graph(inputs, discount, &graph.baseline().curves)?;
    let originals = inputs
        .deliverables
        .iter()
        .map(|d| {
            crate::bond_contract::price_fixed(
                d.security.bond,
                inputs.settlement.date(),
                PaymentDateEntitlement::Exclude,
                discount.curve,
            )
            .map(|v| (d.security.instrument.clone(), v))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| q::QuoteRiskError::Futures(FuturesError::Bond(e)))?;
    let changed;
    let scenario = if delta == 0.0
        || matches!(
            request.key.definition().coordinate,
            q::RiskCoordinate::Raw { .. }
        ) {
        graph.baseline()
    } else {
        changed = q::evaluate_quote_graph(graph, &request.key, delta)?;
        &changed
    };
    let baseline = evaluate(
        inputs,
        discount,
        &originals,
        graph.baseline(),
        &request.key,
        0.0,
    )?;
    let shocked = evaluate(inputs, discount, &originals, scenario, &request.key, delta)?;
    let scale = request.position.scale()?;
    let a = level(&baseline.valuation, inputs).map_err(risk_error)? * scale;
    let b = level(&shocked.valuation, inputs).map_err(risk_error)? * scale;
    Ok(BondFuturesGraphScenario {
        request: request.clone(),
        baseline,
        shocked,
        baseline_amount: q::risk_reference_pv(a, inputs.contract.currency)?,
        scenario_amount: q::risk_reference_pv(b, inputs.contract.currency)?,
        change: q::risk_reference_pv(b - a, inputs.contract.currency)?,
    })
}

/// All original member valuations and independently reselected CTD/tie sets.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BondFuturesBasketValue {
    /// Full native delivery/carry/invoice alternatives.
    pub valuation: BondFuturesValuation,
    /// Full original and scenario cash-model prices and residual basis.
    pub members: Vec<BondFuturesMemberRevaluation>,
    /// Separate selection and fair-value winner/tie sets.
    pub ctd: BondFuturesCtdState,
}
/// Reprice all original members for a simultaneous complete graph plus exact
/// raw member financing/clean-basis moves. Original graph-owner affinity applies.
/// The enclosing book reserves the full original basket policy before this call.
pub fn revalue_bond_futures_basket(
    inputs: &BondFuturesInputs<'_>,
    discount: FuturesCurve<'_>,
    baseline: &CurveSet,
    scenario: &q::QuoteGraphScenario,
    moves: &[q::QuoteGraphMove],
) -> FuturesResult<BondFuturesBasketValue> {
    q::validate_quote_graph_moves(moves).map_err(|e| invalid("basket moves", e.to_string()))?;
    admit_bond_futures_graph(inputs, discount, baseline)?;
    let originals = inputs
        .deliverables
        .iter()
        .map(|d| {
            crate::bond_contract::price_fixed(
                d.security.bond,
                inputs.settlement.date(),
                PaymentDateEntitlement::Exclude,
                discount.curve,
            )
            .map(|v| (d.security.instrument.clone(), v))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let value = evaluate_many(inputs, discount, &originals, scenario, moves, 0.0)?;
    Ok(BondFuturesBasketValue {
        valuation: value.valuation,
        members: value.members,
        ctd: value.ctd,
    })
}
