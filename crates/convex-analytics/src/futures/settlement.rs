//! Exact ticks, normal futures variation, integer residuals and supplied rolls.

use super::*;
use convex_core::contracts::RiskFactorKey;
use rust_decimal::RoundingStrategy;

/// Explicit nearest/directional rounding, independently chosen for the operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FuturesRounding {
    /// Half ties increase magnitude, including negative numbers.
    NearestAwayFromZero,
    /// Half ties go to the greater number, including negative rates.
    NearestUp,
    /// Greatest lattice value no greater than input.
    Down,
    /// Least lattice value no smaller than input.
    Up,
    /// Reduce magnitude to the nearest lattice value.
    TowardZero,
}

fn rounded(value: Decimal, increment: Decimal, rule: FuturesRounding) -> FuturesResult<Decimal> {
    positive(increment, "rounding_increment")?;
    let q = arithmetic(value.checked_div(increment), "rounding")?;
    let whole = match rule {
        FuturesRounding::NearestAwayFromZero => {
            q.round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        }
        FuturesRounding::NearestUp => {
            let floor = q.floor();
            if q - floor >= Decimal::new(5, 1) {
                floor + Decimal::ONE
            } else {
                floor
            }
        }
        FuturesRounding::Down => q.floor(),
        FuturesRounding::Up => q.ceil(),
        FuturesRounding::TowardZero => q.trunc(),
    };
    arithmetic(whole.checked_mul(increment), "rounding")
}

/// Actual clearing conventions for one exact contract revision. The supported
/// monetary method is normal futures rounding: round each contract value before
/// subtracting and multiplying quantity. Other clearing methods need new terms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FuturesClearingRules {
    /// Exact owner; a front/back or correction mismatch fails.
    pub contract: InstrumentPin,
    /// Actual clearing-rules source/version/quality.
    pub stamp: ReferenceStamp,
    /// Explicit normal-futures money method identifier, exactly `rounded_contract_values`.
    pub money_method: String,
    /// Decimal annualized rate increment for final settlement; None for bonds.
    pub final_rate_increment: Option<ExactDecimal>,
    /// Explicit rate rounding, mandatory for rates and absent for bonds.
    pub final_rate_rounding: Option<FuturesRounding>,
    /// Exact revision-selection rule for rate settlement, absent for bonds.
    pub fixing_selection: Option<FuturesFixingSelection>,
}
impl FuturesClearingRules {
    pub(super) fn check(
        &self,
        contract: &FuturesContract,
        report: &mut FuturesInputReport,
    ) -> FuturesResult<()> {
        if self.contract.id != contract.instrument_id
            || self.contract.version != contract.stamp.version
            || self.money_method != "rounded_contract_values"
        {
            return Err(invalid(
                "clearing_rules",
                "exact contract and supported monetary method required",
            ));
        }
        match &contract.settlement {
            FuturesSettlement::Bond { .. }
                if self.final_rate_increment.is_some()
                    || self.final_rate_rounding.is_some()
                    || self.fixing_selection.is_some() =>
            {
                return Err(invalid(
                    "clearing_rules",
                    "bond futures cannot consume final-rate rounding",
                ))
            }
            FuturesSettlement::Rate { .. } => {
                let increment = self
                    .final_rate_increment
                    .ok_or_else(|| invalid("clearing_rules", "missing final-rate precision"))?
                    .decimal();
                if increment <= Decimal::ZERO
                    || increment > Decimal::new(1, 3)
                    || self.final_rate_rounding.is_none()
                    || self.fixing_selection.is_none()
                {
                    return Err(invalid(
                        "clearing_rules",
                        "positive rate increment <=0.001 and explicit rounding required",
                    ));
                }
            }
            _ => (),
        }
        report.add(&self.stamp)
    }
    pub(super) fn final_price(&self, decimal_rate: Decimal) -> FuturesResult<Decimal> {
        let increment = self
            .final_rate_increment
            .ok_or_else(|| invalid("final_rate", "missing precision"))?
            .decimal();
        let rule = self
            .final_rate_rounding
            .ok_or_else(|| invalid("final_rate", "missing rounding rule"))?;
        arithmetic(
            rounded(decimal_rate, increment, rule)?
                .checked_mul(Decimal::from(100))
                .and_then(|r| Decimal::from(100).checked_sub(r)),
            "final_rate",
        )
    }
}

/// Meaning of a supplied price; settlement precision is distinct from order ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FuturesMarkKind {
    /// Actual execution price; exact dated outright tick required.
    Trade,
    /// Official daily settlement; may have different precision from order ticks.
    DailySettlement,
    /// Official final cash-settlement price, with the separate rate lattice.
    FinalSettlement,
}

/// Actual dated price for an exact contract, never a continuous/root symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FuturesMark {
    /// Actual contract and revision.
    pub contract: InstrumentPin,
    /// Original price source and supplied freshness/availability.
    pub stamp: ReferenceStamp,
    /// Actual exchange business date.
    pub date: BusinessDate,
    /// Quoted futures/index points.
    pub price_points: ExactDecimal,
    /// Trade versus daily/final settlement.
    pub kind: FuturesMarkKind,
}
impl FuturesMark {
    fn check(
        &self,
        contract: &FuturesContract,
        rules: &FuturesClearingRules,
        report: &mut FuturesInputReport,
    ) -> FuturesResult<()> {
        if self.contract.id != contract.instrument_id
            || self.contract.version != contract.stamp.version
            || self.date < contract.listed_from
            || self.date.date() > date(report.context)
        {
            return Err(invalid(
                "mark",
                "exact contract and nonfuture dated mark required",
            ));
        }
        let p = self.price_points.decimal();
        let ceiling = if matches!(contract.settlement, FuturesSettlement::Rate { .. }) {
            200
        } else {
            1000
        };
        if p < Decimal::ZERO
            || p > Decimal::from(ceiling)
            || (p.is_zero() && matches!(contract.settlement, FuturesSettlement::Bond { .. }))
        {
            return Err(invalid("mark", "price outside contracted family bounds"));
        }
        match self.kind {
            FuturesMarkKind::Trade => {
                contract.validate_order_units(self.date, 1, self.price_points)?
            }
            FuturesMarkKind::DailySettlement if self.date > contract.last_trade => {
                return Err(invalid("mark", "daily settlement after last trading date"))
            }
            FuturesMarkKind::FinalSettlement => {
                let FuturesSettlement::Rate {
                    final_settlement, ..
                } = &contract.settlement
                else {
                    return Err(invalid(
                        "mark",
                        "physical bond future has no final cash settlement",
                    ));
                };
                if &self.date != final_settlement
                    || rules.final_price(contract.rate_from_price(self.price_points)?.decimal())?
                        != p
                {
                    return Err(invalid(
                        "mark",
                        "exact final date and final-rate lattice required",
                    ));
                }
            }
            _ => (),
        }
        report.add(&self.stamp)
    }
}

fn monetary_value(contract: &FuturesContract, price: ExactDecimal) -> FuturesResult<Decimal> {
    Ok(arithmetic(
        price
            .decimal()
            .checked_mul(contract.currency_per_price_point.decimal()),
        "contract_value",
    )?
    .round_dp_with_strategy(
        contract.daily_settlement.cash_decimal_places,
        RoundingStrategy::MidpointAwayFromZero,
    ))
}

/// Proposed daily/final variation cash, not realized cash or an authoritative state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FuturesSettlementEffect {
    /// Original supplied mark; the application retains it as the prior state.
    pub previous: FuturesMark,
    /// New supplied official mark; no price is inferred from today's clock.
    pub current: FuturesMark,
    /// Signed integer position, positive long.
    pub contracts: i64,
    /// Rounded per-contract monetary value at the prior mark.
    pub previous_contract_value: Money,
    /// Rounded per-contract monetary value at the current mark.
    pub current_contract_value: Money,
    /// Difference of those values, scaled by signed quantity.
    pub proposed_variation: Money,
    /// Actual consumed versions and data blocks. No posting authority.
    pub inputs: FuturesInputReport,
}

/// Apply normal futures rounding to a supplied prior mark and strictly later
/// official daily/final settlement. Same-day Trade-to-settlement is valid.
/// Replaying a daily settlement as its own successor is rejected. Authoritative
/// deduplication and fills/position amendments belong to the application/ledger.
pub fn futures_settlement_effect(
    contract: &FuturesContract,
    rules: &FuturesClearingRules,
    calendar: FuturesCalendar<'_>,
    previous: &FuturesMark,
    current: &FuturesMark,
    contracts: i64,
    context: EligibilityContext,
) -> FuturesResult<FuturesSettlementEffect> {
    contract.validate()?;
    let n = count(contracts, false)?;
    let mut report = FuturesInputReport::new(
        context,
        HedgeInputEligibility {
            eligible: false,
            issues: Vec::new(),
        },
    );
    report.add(&contract.stamp)?;
    rules.check(contract, &mut report)?;
    calendar.check(&contract.daily_settlement.calendar, &mut report)?;
    if previous.kind == FuturesMarkKind::FinalSettlement
        || current.kind == FuturesMarkKind::Trade
        || previous.date > current.date
        || (previous.date == current.date && previous.kind != FuturesMarkKind::Trade)
        || !calendar.calendar.is_business_day(current.date.date())
    {
        return Err(invalid(
            "settlement",
            "ordered mark transition and actual settlement business date required",
        ));
    }
    previous.check(contract, rules, &mut report)?;
    current.check(contract, rules, &mut report)?;
    let before = monetary_value(contract, previous.price_points)?;
    let after = monetary_value(contract, current.price_points)?;
    let variation = arithmetic(
        after.checked_sub(before).and_then(|v| v.checked_mul(n)),
        "variation",
    )?;
    report.finish();
    Ok(FuturesSettlementEffect {
        previous: previous.clone(),
        current: current.clone(),
        contracts,
        previous_contract_value: money(before, contract.currency),
        current_contract_value: money(after, contract.currency),
        proposed_variation: money(variation, contract.currency),
        inputs: report,
    })
}

/// Exact linear quote delta and dated tick economics; these are not rate PV01.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FuturesQuoteRisk {
    /// Exact dated contract revision.
    pub contract: InstrumentPin,
    /// Signed currency change per +1 quoted price/index point.
    pub currency_per_price_point: Money,
    /// Unrounded currency for one tick of one contract.
    pub tick_currency_per_contract: Money,
    /// Unrounded signed quote change for one tick of this position.
    pub position_tick_currency: Money,
    /// Informational units never constitute executable advice.
    pub execution_status: HedgeExecutionStatus,
}
/// Quote delta uses the multiplier once; rate bp risk is a separate computation.
pub fn futures_quote_risk(
    contract: &FuturesContract,
    on: BusinessDate,
    contracts: i64,
) -> FuturesResult<FuturesQuoteRisk> {
    let n = count(contracts, true)?;
    let tick = contract.tick_at(on)?.decimal();
    let multiplier = contract.currency_per_price_point.decimal();
    let one_tick = arithmetic(multiplier.checked_mul(tick), "tick_value")?;
    Ok(FuturesQuoteRisk {
        contract: InstrumentPin {
            id: contract.instrument_id.clone(),
            version: contract.stamp.version,
        },
        currency_per_price_point: money(
            arithmetic(multiplier.checked_mul(n), "quote_delta")?,
            contract.currency,
        ),
        tick_currency_per_contract: money(one_tick, contract.currency),
        position_tick_currency: money(
            arithmetic(one_tick.checked_mul(n), "tick_delta")?,
            contract.currency,
        ),
        execution_status: HedgeExecutionStatus::IndicativeOnly,
    })
}

/// Rounded price and retained exact residual, without an implicit limit-side choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FuturesRoundedPrice {
    /// Exact original quoted points.
    pub original: ExactDecimal,
    /// Exact dated outright lattice value.
    pub rounded: ExactDecimal,
    /// Rounded minus original, in quoted points.
    pub residual_points: ExactDecimal,
}
/// Round an informational model price; an order still requires explicit admission.
pub fn round_futures_price(
    contract: &FuturesContract,
    on: BusinessDate,
    price: ExactDecimal,
    rule: FuturesRounding,
) -> FuturesResult<FuturesRoundedPrice> {
    let value = rounded(price.decimal(), contract.tick_at(on)?.decimal(), rule)?;
    contract.validate_order_units(on, 1, ExactDecimal::new(value))?;
    Ok(FuturesRoundedPrice {
        original: price,
        rounded: ExactDecimal::new(value),
        residual_points: ExactDecimal::new(value - price.decimal()),
    })
}

/// Integer hedge solution in one explicitly supplied signed risk unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FuturesRoundedHedge {
    /// Continuous signed contracts before rounding.
    pub continuous_contracts: ExactDecimal,
    /// Rounded integer contracts, including a meaningful zero.
    pub contracts: i64,
    /// Original target risk plus rounded hedge risk, in the same currency/risk unit.
    pub residual_currency_per_bp: ExactDecimal,
    /// Original decision retained in the result.
    pub rounding: FuturesRounding,
}
/// Missing or zero candidate risk is an error, never an invented hedge count.
pub fn round_futures_hedge(
    target_currency_per_bp: ExactDecimal,
    per_contract_currency_per_bp: ExactDecimal,
    rule: FuturesRounding,
) -> FuturesResult<FuturesRoundedHedge> {
    let r = per_contract_currency_per_bp.decimal();
    if r.is_zero()
        || r.abs() > Decimal::from(1_000_000_000_000_000i64)
        || target_currency_per_bp.decimal().abs() > Decimal::from(1_000_000_000_000_000i64)
    {
        return Err(invalid(
            "hedge_risk",
            "nonzero candidate and bounded target risk required",
        ));
    }
    let continuous = arithmetic(
        (-target_currency_per_bp.decimal()).checked_div(r),
        "hedge_count",
    )?;
    if continuous.abs() > Decimal::from(1_000_000_000i64) {
        return Err(invalid(
            "hedge_count",
            "continuous contracts exceed1billion",
        ));
    }
    let integer = rounded(continuous, Decimal::ONE, rule)?;
    let contracts = integer
        .to_i64()
        .ok_or_else(|| invalid("hedge_count", "integer conversion overflow"))?;
    let residual = arithmetic(
        integer
            .checked_mul(r)
            .and_then(|v| v.checked_add(target_currency_per_bp.decimal())),
        "hedge_residual",
    )?;
    Ok(FuturesRoundedHedge {
        continuous_contracts: ExactDecimal::new(continuous),
        contracts,
        residual_currency_per_bp: ExactDecimal::new(residual),
        rounding: rule,
    })
}

/// Supplied, dated per-contract risk used only to compare matching factor bases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FuturesRollRisk {
    /// Exact owner revision.
    pub contract: InstrumentPin,
    /// Actual factor/bump/model convention; both roll legs must match exactly.
    pub factor: RiskFactorKey,
    /// Signed currency per +1bp, for one long contract.
    pub per_contract_currency_per_bp: Money,
    /// Actual risk calculation/version quality and availability.
    pub stamp: ReferenceStamp,
}
/// Complete supplied front/back leg; no continuous-contract symbol lookup.
pub struct FuturesRollLeg<'a> {
    /// Actual dated contract.
    pub contract: &'a FuturesContract,
    /// Exact actual basket for physical futures, absent for rates.
    pub basket: Option<&'a DeliveryBasket>,
    /// Actual on-tick execution quote for the supplied roll business date.
    pub quote: &'a FuturesMark,
    /// Actual comparable risk, with distinct source lineage.
    pub risk: &'a FuturesRollRisk,
    /// Explicit nonnegative currency transaction cost per contract.
    pub costs_per_contract: ExactDecimal,
    /// Actual cost source; zero is a supplied fact, never a default.
    pub costs_stamp: &'a ReferenceStamp,
}
/// Indicative supplied roll, preserving exact quantities, costs and risk residual.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FuturesRollPlan {
    /// Signed closing trade on the current contract.
    pub close_front_contracts: i64,
    /// Signed rounded opening trade on the supplied next contract.
    pub open_back: FuturesRoundedHedge,
    /// Back minus front quoted points; not an upfront cash payment or P&L.
    pub calendar_spread_points: ExactDecimal,
    /// Positive transaction cost at both exact integer quantities.
    pub transaction_costs: Money,
    /// All source gates, keeping indicative/stale/synthetic blocks visible.
    pub inputs: FuturesInputReport,
}
/// Preserve the current position's supplied factor risk on a later named month,
/// then expose the rounding residual. Quotes/costs do not imply fills; variation
/// on closing fills is a separate supplied settlement effect.
pub fn plan_futures_roll(
    front: &FuturesRollLeg<'_>,
    back: &FuturesRollLeg<'_>,
    current_contracts: i64,
    rule: FuturesRounding,
    context: EligibilityContext,
) -> FuturesResult<FuturesRollPlan> {
    let n = count(current_contracts, false)?;
    let compatible = match (&front.contract.settlement, &back.contract.settlement) {
        (FuturesSettlement::Bond { .. }, FuturesSettlement::Bond { .. }) => true,
        (
            FuturesSettlement::Rate { accrual: a, .. },
            FuturesSettlement::Rate { accrual: b, .. },
        ) => match (a, b) {
            (
                RateAccrual::Overnight {
                    index: ai,
                    averaging: aa,
                    day_count: ad,
                    ..
                },
                RateAccrual::Overnight {
                    index: bi,
                    averaging: ba,
                    day_count: bd,
                    ..
                },
            ) => ai == bi && aa == ba && ad == bd,
            (
                RateAccrual::Term {
                    index: ai,
                    day_count: ad,
                    ..
                },
                RateAccrual::Term {
                    index: bi,
                    day_count: bd,
                    ..
                },
            ) => ai == bi && ad == bd,
            _ => false,
        },
        _ => false,
    };
    if front.contract.instrument_id == back.contract.instrument_id
        || !compatible
        || front.contract.product_code != back.contract.product_code
        || front.contract.exchange != back.contract.exchange
        || front.contract.currency != back.contract.currency
        || (
            back.contract.contract_month.year,
            back.contract.contract_month.month,
        ) <= (
            front.contract.contract_month.year,
            front.contract.contract_month.month,
        )
        || front.risk.factor != back.risk.factor
    {
        return Err(invalid("roll", "distinct later contract of the same product/currency and exact common risk factor required"));
    }
    let mut report = contract_report(front.contract, front.basket, context)?;
    let other = contract_report(back.contract, back.basket, context)?;
    report.assessment.issues.extend(other.assessment.issues);
    for s in &other.stamps {
        report.add(s)?;
    }
    for leg in [front, back] {
        if leg.quote.kind != FuturesMarkKind::Trade
            || leg.quote.date.date() != date(context)
            || leg.quote.contract.id != leg.contract.instrument_id
            || leg.quote.contract.version != leg.contract.stamp.version
            || leg.risk.contract != leg.quote.contract
            || leg.risk.per_contract_currency_per_bp.currency() != leg.contract.currency
            || leg.risk.factor.definition().currency != leg.contract.currency
            || leg.risk.factor.definition().family
                != convex_core::contracts::FactorFamily::InterestRate
            || leg.costs_per_contract.decimal() < Decimal::ZERO
            || leg.costs_per_contract.decimal() > Decimal::from(1_000_000_000i64)
        {
            return Err(invalid(
                "roll_leg",
                "exact dated quote/risk/cost inputs required",
            ));
        }
        leg.contract
            .validate_order_units(leg.quote.date, 1, leg.quote.price_points)?;
        report.add(&leg.quote.stamp)?;
        report.add(&leg.risk.stamp)?;
        report.add(leg.costs_stamp)?;
    }
    let original = arithmetic(
        front
            .risk
            .per_contract_currency_per_bp
            .value()
            .decimal()
            .checked_mul(n),
        "roll_risk",
    )?;
    let open_back = round_futures_hedge(
        ExactDecimal::new(-original),
        back.risk.per_contract_currency_per_bp.value(),
        rule,
    )?;
    let cost = arithmetic(
        front
            .costs_per_contract
            .decimal()
            .checked_mul(n.abs())
            .and_then(|v| {
                back.costs_per_contract
                    .decimal()
                    .checked_mul(Decimal::from(open_back.contracts).abs())
                    .and_then(|b| v.checked_add(b))
            }),
        "roll_cost",
    )?;
    report.finish();
    Ok(FuturesRollPlan {
        close_front_contracts: -current_contracts,
        open_back,
        calendar_spread_points: ExactDecimal::new(
            back.quote.price_points.decimal() - front.quote.price_points.decimal(),
        ),
        transaction_costs: money(cost, front.contract.currency),
        inputs: report,
    })
}
