//! Exact supplied security cash, fixed-date carry, CTD alternatives and scenarios.

use super::*;
use convex_bonds::contracts::BondCoupon;
use convex_core::{cashflows::PaymentDateEntitlement, daycounts::DayCountConvention};
use convex_curves::contracts::{CurvePoint, CurveQuery, CurveRole};
use convex_math::portable;
use rust_decimal::RoundingStrategy;

/// Explicit simple reinvestment or own-cash opportunity-cost input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FuturesCarryRate {
    /// Original source and immutable assumptions/data revision.
    pub stamp: ReferenceStamp,
    /// Inclusive availability start.
    pub start: BusinessDate,
    /// Inclusive availability end.
    pub end: BusinessDate,
    /// Explicit annualized decimal simple rate; negative rates are allowed.
    pub decimal_rate: ExactDecimal,
    /// Actual ACT/360 or ACT/365F basis.
    pub day_count: DayCountConvention,
}
impl FuturesCarryRate {
    pub(super) fn check(
        &self,
        start: BusinessDate,
        end: BusinessDate,
        report: &mut FuturesInputReport,
    ) -> FuturesResult<()> {
        if self.start > start
            || self.end < end
            || self.start >= self.end
            || !matches!(
                self.day_count,
                DayCountConvention::Act360 | DayCountConvention::Act365Fixed
            )
        {
            return Err(invalid(
                "carry_rate",
                "explicit money-market input must cover the full carry horizon",
            ));
        }
        rate(self.decimal_rate.decimal(), "carry_rate")?;
        report.add(&self.stamp)
    }
    pub(super) fn interest(
        &self,
        start: Date,
        end: Date,
        amount: Decimal,
    ) -> FuturesResult<Decimal> {
        arithmetic(
            amount
                .checked_mul(self.decimal_rate.decimal())
                .and_then(|v| {
                    v.checked_mul(self.day_count.to_day_count().year_fraction(start, end))
                }),
            "carry_interest",
        )
    }
}

/// Coupon treatment is an explicit model choice, separate from repo funding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FuturesCouponCarry {
    /// Coupons are retained as uninvested cash through delivery; no zero-rate guess.
    HeldCash {
        /// Bounded explanation of this exact deterministic assumption.
        evidence: String,
    },
    /// Coupons are reinvested at the supplied simple rate until delivery.
    Reinvested {
        /// Actual available reinvestment source and separate day count.
        rate: FuturesCarryRate,
    },
}

/// Resolved economics of an actual eligible security. The clean quote is supplied
/// at the input bundle's settlement date; the contract computes both accrued dates.
pub struct BondFutureDeliverable<'a> {
    /// Full actual checked security, never synthetic issue/tenor construction.
    pub security: FuturesBond<'a>,
    /// Actual clean cash-market price per100 original face.
    pub clean_price_per100: ExactDecimal,
    /// Original cash quote source, quality and availability at that settlement.
    pub quote_stamp: &'a ReferenceStamp,
    /// Exact per-security repo and haircut; horizon and identity must match.
    pub financing: &'a FinancingInput,
    /// Required for a nonzero haircut, absent for a fully repo-funded purchase.
    pub own_cash: Option<&'a FuturesCarryRate>,
    /// Actual held-cash/reinvestment policy for every entitled interim coupon.
    pub coupon_carry: &'a FuturesCouponCarry,
}

/// Independent deterministic selection criteria. They can disagree when funding,
/// conversion factors or cash timing differ, so no equivalence is assumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FuturesCtdMethod {
    /// Smallest clean net basis at each security's actual supplied funding.
    MinimumNetBasis,
    /// Highest repo return solving cash purchase versus delivery economics.
    MaximumImpliedRepo,
    /// Lowest conversion-factor-adjusted fixed-date forward quote.
    MinimumForwardPrice,
}

/// Complete actual fixed-date cash/carry bundle. Futures-style margin financing,
/// timing/wildcard/end-month options are outside this deterministic model and are
/// always reported. No output from this route is executable advice.
pub struct BondFuturesInputs<'a> {
    /// Exact dated actual physical contract.
    pub contract: &'a FuturesContract,
    /// Exact whole published basket; every member must have supplied economics.
    pub basket: &'a DeliveryBasket,
    /// Exact delivery calendar, independent of security payment calendars.
    pub delivery_calendar: FuturesCalendar<'a>,
    /// Actual quoted purchase settlement, distinct from valuation date.
    pub settlement: BusinessDate,
    /// One explicit admissible business delivery date; never inferred from tenor.
    pub delivery: BusinessDate,
    /// Actual market futures points used for invoice and basis diagnostics.
    pub market: &'a FuturesMark,
    /// Full member economics,1..128, in arbitrary input order.
    pub deliverables: &'a [BondFutureDeliverable<'a>],
    /// Explicit CTD criterion, retained independently from the fair-price minimum.
    pub selection: FuturesCtdMethod,
    /// Economic date/time and information availability cutoff.
    pub context: EligibilityContext,
}

/// Complete per-security carry/invoice diagnostics in explicitly named units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondFuturesAlternative {
    /// Actual immutable deliverable revision.
    pub instrument: InstrumentPin,
    /// Published factor for this contract month, without approximation.
    pub conversion_factor: ExactDecimal,
    /// Original supplied cash clean price.
    pub spot_clean_per100: ExactDecimal,
    /// Dirty purchase value, including actual settlement accrued.
    pub spot_dirty_per100: ExactDecimal,
    /// Actual settlement accrued from the full security contract.
    pub settlement_accrued_per100: ExactDecimal,
    /// Actual delivery accrued from the full security contract.
    pub delivery_accrued_per100: ExactDecimal,
    /// Coupons actually entitled and received after spot, through delivery.
    pub coupons_per100: ExactDecimal,
    /// Those coupons plus explicitly supplied reinvestment income.
    pub coupon_delivery_value_per100: ExactDecimal,
    /// Repo interest on the (1-haircut) dirty purchase balance.
    pub repo_cost_per100: ExactDecimal,
    /// Explicit opportunity cost on the haircut cash portion.
    pub own_cash_cost_per100: ExactDecimal,
    /// Dirty purchase/carry cost less delivery accrued, divided by published CF.
    pub forward_price_points: ExactDecimal,
    /// Original clean cash price minus market futures points times CF.
    pub gross_basis_per100: ExactDecimal,
    /// Clean financed forward value minus market futures points times CF.
    pub net_basis_per100: ExactDecimal,
    /// Simple annualized repo return on the repo-funded balance; reinvestment
    /// and haircut opportunity costs are frozen, invoice cash is unrounded here.
    pub implied_repo_decimal: ExactDecimal,
    /// Actual clean invoice rounding per contract plus actual rounded accrued cash.
    pub delivery_invoice_per_contract: Money,
    /// Negative net basis times face/100, using unrounded invoice economics.
    pub unrounded_basis_profit_per_contract: Money,
    /// Original security-specific repo rate/haircut/source retained in full.
    pub financing: FinancingInput,
    /// Exact haircut rate assumptions, when actually used.
    pub own_cash: Option<FuturesCarryRate>,
    /// Coupon reinvestment assumptions, retained without silent defaults.
    pub coupon_carry: FuturesCouponCarry,
}

/// Indicative deterministic basket result; rankings and fair-price winner remain
/// separate. Exact-score ties are broken by stable instrument ID/version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondFuturesValuation {
    /// Exact actual futures revision.
    pub contract: InstrumentPin,
    /// Exact original basket resource/version.
    pub basket: ResourcePin,
    /// Actual cash settlement date used in every member.
    pub settlement: BusinessDate,
    /// Supplied fixed delivery date.
    pub delivery: BusinessDate,
    /// Original stamped market quote, separate from deterministic model price.
    pub market: FuturesMark,
    /// Chosen selection rule; implied repo and net basis are not interchangeable.
    pub selection: FuturesCtdMethod,
    /// Actual chosen CTD under that rule.
    pub selected: InstrumentPin,
    /// Cheapest fixed-date forward quote, independent of selection rule.
    pub fair_price_points: ExactDecimal,
    /// Security that determines that fixed-date forward quote.
    pub fair_deliverable: InstrumentPin,
    /// Runner-up minus best score; decimal rate for IRR, per100 for net basis,
    /// points for forward price. None means a genuine single-member basket.
    pub selection_gap: Option<ExactDecimal>,
    /// Every actual basket member retained in deterministic identity order.
    pub alternatives: Vec<BondFuturesAlternative>,
    /// Explicit deterministic model limitations, no omitted zero-valued options.
    pub model_limitations: Vec<String>,
    /// All consumed sources and stable execution-data blocks.
    pub inputs: FuturesInputReport,
}

fn bullet(bond: &BondContract, delivery: Date) -> FuturesResult<()> {
    let t = bond.terms();
    if !matches!(t.coupon, BondCoupon::Fixed)
        || t.schedule.end_date <= delivery
        || t.dates.ex_coupon.is_some()
        || !t.principal_prices.is_empty()
        || t.redemption_multiplier != Decimal::ONE
        || t.profiles.notional.iter().any(|v| v.value != t.face_value)
    {
        return Err(invalid("deliverable_terms", "fixed nominal bullet surviving delivery, par principal and supported coupon entitlement required"));
    }
    Ok(())
}
fn score(a: &BondFuturesAlternative, method: FuturesCtdMethod) -> Decimal {
    match method {
        FuturesCtdMethod::MinimumNetBasis => a.net_basis_per100.decimal(),
        FuturesCtdMethod::MaximumImpliedRepo => -a.implied_repo_decimal.decimal(),
        FuturesCtdMethod::MinimumForwardPrice => a.forward_price_points.decimal(),
    }
}

/// Value actual supplied cash/carry and every delivery alternative. All member
/// inputs are mandatory; rejected or missing securities never disappear from CTD.
pub fn price_bond_future(inputs: &BondFuturesInputs<'_>) -> FuturesResult<BondFuturesValuation> {
    price_bond_future_scenario(inputs, &[], Decimal::ZERO)
}

fn price_bond_future_scenario(
    inputs: &BondFuturesInputs<'_>,
    clean_changes: &[(InstrumentPin, Decimal)],
    repo_bump: Decimal,
) -> FuturesResult<BondFuturesValuation> {
    let mut report = contract_report(inputs.contract, Some(inputs.basket), inputs.context)?;
    let FuturesSettlement::Bond { delivery, .. } = &inputs.contract.settlement else {
        return Err(invalid(
            "bond_future",
            "rate futures have no physical delivery economics",
        ));
    };
    inputs
        .delivery_calendar
        .check(&delivery.calendar, &mut report)?;
    if inputs.settlement.date() < date(inputs.context)
        || inputs.delivery <= inputs.settlement
        || inputs.delivery < delivery.first_delivery
        || inputs.delivery > delivery.last_delivery
        || !inputs
            .delivery_calendar
            .calendar
            .is_business_day(inputs.delivery.date())
        || inputs.deliverables.len() != inputs.basket.bonds.len()
    {
        return Err(invalid(
            "delivery",
            "actual future business delivery, purchase settlement and complete basket required",
        ));
    }
    if inputs.market.contract.id != inputs.contract.instrument_id
        || inputs.market.contract.version != inputs.contract.stamp.version
        || inputs.market.date.date() > date(inputs.context)
        || inputs.market.date > inputs.contract.last_trade
        || inputs.market.kind == FuturesMarkKind::FinalSettlement
    {
        return Err(invalid(
            "market_future",
            "actual dated same-revision market quote required",
        ));
    }
    positive(inputs.market.price_points.decimal(), "market_future")?;
    if inputs.market.price_points.decimal() > Decimal::from(1000) {
        return Err(invalid("market_future", "price exceeds1000points"));
    }
    if inputs.market.kind == FuturesMarkKind::Trade {
        inputs
            .contract
            .validate_order_units(inputs.market.date, 1, inputs.market.price_points)?;
    }
    report.add(&inputs.market.stamp)?;
    let mut alternatives = Vec::with_capacity(inputs.deliverables.len());
    for entry in &inputs.basket.bonds {
        let matches: Vec<_> = inputs
            .deliverables
            .iter()
            .filter(|d| d.security.instrument == &entry.instrument)
            .collect();
        if matches.len() != 1 {
            return Err(invalid(
                "basket_member",
                "each exact eligible member needs one complete input",
            ));
        }
        let d = matches[0];
        d.security.check(inputs.contract.currency, &mut report)?;
        bullet(d.security.bond, inputs.delivery.date())?;
        report.add(d.quote_stamp)?;
        d.financing.validate()?;
        if d.financing.instrument != entry.instrument
            || d.financing.currency != inputs.contract.currency
            || d.financing.start != inputs.settlement
            || d.financing.end < inputs.delivery
        {
            return Err(invalid(
                "financing",
                "exact security/currency and full purchase/delivery funding horizon required",
            ));
        }
        report.add(&d.financing.stamp)?;
        let mut financing = d.financing.clone();
        financing.repo_decimal = ExactDecimal::new(arithmetic(
            financing.repo_decimal.decimal().checked_add(repo_bump),
            "repo_bump",
        )?);
        rate(financing.repo_decimal.decimal(), "repo_rate")?;
        let haircut = financing.haircut_fraction.decimal();
        match (haircut.is_zero(), d.own_cash) {
            (false, Some(own)) => own.check(inputs.settlement, inputs.delivery, &mut report)?,
            (true, None) => (),
            _ => return Err(invalid(
                "haircut",
                "nonzero haircut requires actual own-cash rate; zero haircut rejects unused rate",
            )),
        }
        match d.coupon_carry {
            FuturesCouponCarry::HeldCash { evidence }
                if evidence.trim().is_empty()
                    || evidence.len() > 512
                    || evidence.chars().any(char::is_control) =>
            {
                return Err(invalid(
                    "coupon_carry",
                    "bounded explicit held-cash evidence required",
                ))
            }
            FuturesCouponCarry::Reinvested { rate } => {
                rate.check(inputs.settlement, inputs.delivery, &mut report)?
            }
            _ => (),
        }
        let spot = d
            .security
            .bond
            .fixed_projection(inputs.settlement.date(), PaymentDateEntitlement::Exclude)?;
        let at_delivery = d
            .security
            .bond
            .fixed_projection(inputs.delivery.date(), PaymentDateEntitlement::Exclude)?;
        let face = d.security.bond.terms().face_value;
        let normalize = |value: Decimal| {
            arithmetic(
                value
                    .checked_mul(Decimal::from(100))
                    .and_then(|v| v.checked_div(face)),
                "per100_normalization",
            )
        };
        let ai_spot = normalize(spot.accrued)?;
        let ai_delivery = normalize(at_delivery.accrued)?;
        let change = clean_changes
            .iter()
            .find(|(pin, _)| pin == &entry.instrument)
            .map(|(_, v)| *v)
            .unwrap_or(Decimal::ZERO);
        let clean = arithmetic(
            d.clean_price_per100.decimal().checked_add(change),
            "cash_scenario",
        )?;
        positive(clean, "cash_clean")?;
        if clean > Decimal::from(1000) {
            return Err(invalid("cash_clean", "price exceeds1000per100"));
        }
        let dirty = arithmetic(clean.checked_add(ai_spot), "dirty_purchase")?;
        positive(dirty, "dirty_purchase")?;
        let tau = financing
            .day_count
            .to_day_count()
            .year_fraction(inputs.settlement.date(), inputs.delivery.date());
        let repo_balance = arithmetic(dirty.checked_mul(Decimal::ONE - haircut), "repo_balance")?;
        let repo_cost = arithmetic(
            repo_balance
                .checked_mul(financing.repo_decimal.decimal())
                .and_then(|v| v.checked_mul(tau)),
            "repo_cost",
        )?;
        let own_cost = if let Some(own) = d.own_cash {
            own.interest(
                inputs.settlement.date(),
                inputs.delivery.date(),
                arithmetic(dirty.checked_mul(haircut), "haircut_balance")?,
            )?
        } else {
            Decimal::ZERO
        };
        let mut coupons = Decimal::ZERO;
        let mut coupon_value = Decimal::ZERO;
        for coupon in &spot.coupons {
            if coupon.entitled && coupon.payment_date <= inputs.delivery.date() {
                let cash = normalize(coupon.amount)?;
                coupons = arithmetic(coupons.checked_add(cash), "coupon_sum")?;
                let reinvest = match d.coupon_carry {
                    FuturesCouponCarry::HeldCash { .. } => Decimal::ZERO,
                    FuturesCouponCarry::Reinvested { rate } => {
                        rate.interest(coupon.payment_date, inputs.delivery.date(), cash)?
                    }
                };
                coupon_value = arithmetic(
                    cash.checked_add(reinvest)
                        .and_then(|v| coupon_value.checked_add(v)),
                    "coupon_delivery_value",
                )?;
            }
        }
        let cost = arithmetic(
            dirty
                .checked_add(repo_cost)
                .and_then(|v| v.checked_add(own_cost))
                .and_then(|v| v.checked_sub(coupon_value)),
            "forward_dirty",
        )?;
        positive(cost, "forward_dirty")?;
        let forward_clean = arithmetic(cost.checked_sub(ai_delivery), "forward_clean")?;
        positive(forward_clean, "forward_clean")?;
        let cf = entry.conversion_factor.decimal();
        let converted = arithmetic(
            inputs.market.price_points.decimal().checked_mul(cf),
            "converted_market",
        )?;
        let forward = arithmetic(forward_clean.checked_div(cf), "forward_quote")?;
        let gross = arithmetic(clean.checked_sub(converted), "gross_basis")?;
        let net = arithmetic(forward_clean.checked_sub(converted), "net_basis")?;
        let repo_numerator = arithmetic(
            converted
                .checked_add(ai_delivery)
                .and_then(|v| v.checked_add(coupon_value))
                .and_then(|v| v.checked_sub(dirty))
                .and_then(|v| v.checked_sub(own_cost)),
            "implied_repo",
        )?;
        let irr = arithmetic(
            repo_numerator.checked_div(arithmetic(
                repo_balance.checked_mul(tau),
                "repo_denominator",
            )?),
            "implied_repo",
        )?;
        let accrued_cash = arithmetic(
            ai_delivery.checked_mul(inputs.contract.currency_per_price_point.decimal()),
            "invoice_accrued",
        )?
        .round_dp_with_strategy(
            inputs.contract.daily_settlement.cash_decimal_places,
            RoundingStrategy::MidpointAwayFromZero,
        );
        let invoice = bond_delivery_invoice(
            inputs.contract,
            inputs.basket,
            &entry.instrument,
            inputs.delivery,
            inputs.market.price_points,
            ExactDecimal::new(accrued_cash),
        )?;
        alternatives.push(BondFuturesAlternative {
            instrument: entry.instrument.clone(),
            conversion_factor: entry.conversion_factor,
            spot_clean_per100: ExactDecimal::new(clean),
            spot_dirty_per100: ExactDecimal::new(dirty),
            settlement_accrued_per100: ExactDecimal::new(ai_spot),
            delivery_accrued_per100: ExactDecimal::new(ai_delivery),
            coupons_per100: ExactDecimal::new(coupons),
            coupon_delivery_value_per100: ExactDecimal::new(coupon_value),
            repo_cost_per100: ExactDecimal::new(repo_cost),
            own_cash_cost_per100: ExactDecimal::new(own_cost),
            forward_price_points: ExactDecimal::new(forward),
            gross_basis_per100: ExactDecimal::new(gross),
            net_basis_per100: ExactDecimal::new(net),
            implied_repo_decimal: ExactDecimal::new(irr),
            delivery_invoice_per_contract: money(invoice.decimal(), inputs.contract.currency),
            unrounded_basis_profit_per_contract: money(
                arithmetic(
                    (-net).checked_mul(inputs.contract.currency_per_price_point.decimal()),
                    "basis_profit",
                )?,
                inputs.contract.currency,
            ),
            financing,
            own_cash: d.own_cash.cloned(),
            coupon_carry: d.coupon_carry.clone(),
        });
    }
    alternatives.sort_by(|a, b| {
        (a.instrument.id.as_str(), a.instrument.version)
            .cmp(&(b.instrument.id.as_str(), b.instrument.version))
    });
    let mut ranked: Vec<_> = alternatives.iter().collect();
    ranked.sort_by_key(|a| score(a, inputs.selection));
    let selected = ranked
        .first()
        .ok_or_else(|| invalid("basket", "empty alternatives"))?;
    let gap = ranked
        .get(1)
        .map(|a| ExactDecimal::new(score(a, inputs.selection) - score(selected, inputs.selection)));
    let fair = alternatives
        .iter()
        .min_by_key(|a| a.forward_price_points)
        .ok_or_else(|| invalid("basket", "no fair delivery alternative"))?;
    let (selected, fair_price, fair_deliverable) = (
        selected.instrument.clone(),
        fair.forward_price_points,
        fair.instrument.clone(),
    );
    report.finish();
    Ok(BondFuturesValuation {contract:InstrumentPin{id:inputs.contract.instrument_id.clone(),version:inputs.contract.stamp.version},basket:ResourcePin{id:inputs.basket.stamp.id.clone(),version:inputs.basket.stamp.version},
        settlement:inputs.settlement,delivery:inputs.delivery,market:inputs.market.clone(),selection:inputs.selection,selected,fair_price_points:fair_price,fair_deliverable,selection_gap:gap,alternatives,
        model_limitations:vec!["fixed supplied delivery date; timing, wildcard and end-month optionality unvalued".into(),"simple supplied funding and coupon reinvestment; daily futures margin financing unvalued".into()],inputs:report})
}

/// Explicit scenario axis; yield, curve-zero, repo, basis and futures quote risk
/// remain separate. Curve-zero scenarios freeze observed-minus-model clean basis.
pub enum BondFuturesRiskAxis<'a> {
    /// Parallel continuous-zero change at the discount recipe's own time basis.
    DiscountZeros {
        /// Actual currency/collateral discount source used for full security cash.
        discount: FuturesCurve<'a>,
    },
    /// Parallel security repo-rate move, with haircut/reinvestment rates fixed.
    RepoRates,
    /// Clean residual basis move on one exact security, measured per price point.
    SecurityBasis {
        /// Actual basket security revision.
        instrument: &'a InstrumentPin,
    },
}

/// Signed risk with reselected and frozen fair-deliverable measures and both CTD
/// diagnostics. Exact currency scaling uses integer contracts and multiplier once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondFuturesRisk {
    /// Original actual basket price/carry.
    pub base: BondFuturesValuation,
    /// Same model repriced after +bump, all alternatives retained.
    pub up: BondFuturesValuation,
    /// Same model repriced after -bump, all alternatives retained.
    pub down: BondFuturesValuation,
    /// Signed integer position.
    pub contracts: i64,
    /// Actual decimal rate or clean-price-point central bump.
    pub bump: ExactDecimal,
    /// Explicit source/bump semantics, separate from market-quote recalibration.
    pub basis: String,
    /// Unit is currency per bp for rate axes, currency per clean point for basis.
    pub risk_unit: String,
    /// Signed mark change after reselecting cheapest fixed-date forward on each side.
    pub reselected_sensitivity: Money,
    /// Signed mark change holding the base fair deliverable fixed on both sides.
    pub frozen_deliverable_sensitivity: Money,
    /// CTD under the declared method changes in at least one scenario.
    pub selection_switch: bool,
    /// Security determining the fixed-date forward changes in at least one scenario.
    pub fair_deliverable_switch: bool,
    /// Exact per-security observed-minus-model clean basis frozen under zero risk.
    /// Empty for repo or explicitly supplied basis scenarios.
    pub frozen_residual_basis_per100: Vec<(InstrumentPin, ExactDecimal)>,
}

/// Bump/reprice actual cash/carry. A zero scenario uses complete contract cash,
/// with its original market residual held fixed; it is not calibrated quote risk.
pub fn bond_futures_risk(
    inputs: &BondFuturesInputs<'_>,
    axis: BondFuturesRiskAxis<'_>,
    contracts: i64,
    bump: ExactDecimal,
) -> FuturesResult<BondFuturesRisk> {
    let n = count(contracts, true)?;
    let h = bump.decimal();
    let (unit, scale, basis) = match &axis {
        BondFuturesRiskAxis::DiscountZeros { .. } => (
            "currency_per_bp",
            Decimal::new(1, 4),
            "parallel_continuous_discount_zeros_frozen_cash_basis_repo_and_reinvestment",
        ),
        BondFuturesRiskAxis::RepoRates => (
            "currency_per_bp",
            Decimal::new(1, 4),
            "parallel_simple_repo_rates_frozen_cash_quotes_haircut_and_reinvestment",
        ),
        BondFuturesRiskAxis::SecurityBasis { .. } => (
            "currency_per_clean_point",
            Decimal::ONE,
            "security_clean_basis_points_frozen_repo_and_reinvestment",
        ),
    };
    if h < Decimal::new(1, 8)
        || h > if scale == Decimal::ONE {
            Decimal::from(10)
        } else {
            Decimal::new(5, 2)
        }
    {
        return Err(invalid(
            "risk_bump",
            "explicit positive bounded rate or clean-price bump required",
        ));
    }
    let mut base = price_bond_future(inputs)?;
    let mut up_changes = Vec::new();
    let mut down_changes = Vec::new();
    let mut residuals = Vec::new();
    let mut repo = Decimal::ZERO;
    match axis {
        BondFuturesRiskAxis::RepoRates => repo = h,
        BondFuturesRiskAxis::SecurityBasis { instrument } => {
            if !inputs
                .basket
                .bonds
                .iter()
                .any(|b| &b.instrument == instrument)
            {
                return Err(invalid("basis_risk", "exact basket security required"));
            }
            up_changes.push((instrument.clone(), h));
            down_changes.push((instrument.clone(), -h));
        }
        BondFuturesRiskAxis::DiscountZeros { discount } => {
            discount.check(&mut base.inputs)?;
            for d in inputs.deliverables {
                let model = crate::bond_contract::price_fixed(
                    d.security.bond,
                    inputs.settlement.date(),
                    PaymentDateEntitlement::Exclude,
                    discount.curve,
                )?;
                residuals.push((
                    d.security.instrument.clone(),
                    ExactDecimal::new(d.clean_price_per100.decimal() - decimal(model.clean_price)?),
                ));
                let up = shifted_cash_dirty(
                    d.security.bond,
                    inputs.settlement.date(),
                    discount.curve,
                    number(h)?,
                )? - model.dirty_price;
                let down = shifted_cash_dirty(
                    d.security.bond,
                    inputs.settlement.date(),
                    discount.curve,
                    -number(h)?,
                )? - model.dirty_price;
                up_changes.push((d.security.instrument.clone(), decimal(up)?));
                down_changes.push((d.security.instrument.clone(), decimal(down)?));
            }
            base.inputs.finish();
        }
    }
    let mut up = price_bond_future_scenario(inputs, &up_changes, repo)?;
    let mut down = price_bond_future_scenario(inputs, &down_changes, -repo)?;
    // Risk scenarios consume the same additional model source as base.
    for stamp in &base.inputs.stamps {
        up.inputs.add(stamp)?;
        down.inputs.add(stamp)?;
    }
    up.inputs.finish();
    down.inputs.finish();
    let normalize = |difference: Decimal| {
        arithmetic(
            difference
                .checked_mul(inputs.contract.currency_per_price_point.decimal())
                .and_then(|v| v.checked_mul(n))
                .and_then(|v| v.checked_mul(scale))
                .and_then(|v| v.checked_div(Decimal::from(2) * h)),
            "futures_risk",
        )
    };
    let frozen_up = up
        .alternatives
        .iter()
        .find(|a| a.instrument == base.fair_deliverable)
        .ok_or_else(|| invalid("risk", "base fair member missing after up scenario"))?
        .forward_price_points
        .decimal();
    let frozen_down = down
        .alternatives
        .iter()
        .find(|a| a.instrument == base.fair_deliverable)
        .ok_or_else(|| invalid("risk", "base fair member missing after down scenario"))?
        .forward_price_points
        .decimal();
    let reselected = normalize(up.fair_price_points.decimal() - down.fair_price_points.decimal())?;
    let frozen = normalize(frozen_up - frozen_down)?;
    let selection_switch = base.selected != up.selected || base.selected != down.selected;
    let fair_deliverable_switch = base.fair_deliverable != up.fair_deliverable
        || base.fair_deliverable != down.fair_deliverable;
    Ok(BondFuturesRisk {
        base,
        up,
        down,
        contracts,
        bump,
        basis: basis.into(),
        risk_unit: unit.into(),
        reselected_sensitivity: money(reselected, inputs.contract.currency),
        frozen_deliverable_sensitivity: money(frozen, inputs.contract.currency),
        selection_switch,
        fair_deliverable_switch,
        frozen_residual_basis_per100: residuals,
    })
}

pub(super) fn shifted_cash_dirty(
    bond: &BondContract,
    settlement: Date,
    discount: &BuiltCurve,
    bump: f64,
) -> FuturesResult<f64> {
    let model = crate::bond_contract::price_fixed(
        bond,
        settlement,
        PaymentDateEntitlement::Exclude,
        discount,
    )?;
    if !matches!(discount.recipe().role,CurveRole::Discount{collateral_currency} if collateral_currency == bond.terms().currency)
    {
        return Err(invalid(
            "discount",
            "actual collateral/currency discount role required",
        ));
    }
    let df = |at| {
        discount
            .query(&CurveQuery::DiscountFactor {
                point: CurvePoint::Date(at),
            })
            .map(|q| q.value)
    };
    let anchor = df(settlement)?;
    let mut value = 0.0;
    let dc = discount.recipe().time_basis.to_day_count();
    let at_time = number(dc.year_fraction(discount.recipe().reference_date, settlement))?;
    for flow in model.projection.cashflows()? {
        let time = number(dc.year_fraction(discount.recipe().reference_date, flow.date))? - at_time;
        value += number(flow.amount)? * df(flow.date)? / anchor * portable::exp(-bump * time);
        if !value.is_finite() {
            return Err(invalid("risk", "nonfinite zero-shift cash PV"));
        }
    }
    let dirty = value / number(bond.terms().face_value)? * 100.0;
    if !dirty.is_finite() {
        return Err(invalid("risk", "nonfinite shifted dirty price"));
    }
    Ok(dirty)
}
