//! Actual cash hedge model values, side quotes, costs and signed model risk.

use super::*;
use convex_core::cashflows::PaymentDateEntitlement;
use convex_curves::contracts::CurveKey;

/// Explicit base for an annualized stock-loan fee; an index/currency cannot choose it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FuturesBorrowFeeBasis {
    /// Borrowed original face, independently of cash price.
    Face,
    /// Original observed dirty market value at settlement, held constant for estimate.
    DirtyMarketValue,
}

/// Complete resolved cash hedge model and execution inputs. A missing side quote
/// still permits a complete model value, with absent execution cash and data blocks.
pub struct CashHedgePricingInputs<'a> {
    /// Actual immutable cash universe, quotes, available sizes and eligibility.
    pub universe: &'a CashHedgeUniverse,
    /// Exact actual full checked security; must be a candidate of this universe.
    pub security: FuturesBond<'a>,
    /// Actual proposed face/side/owned inventory/holding horizon.
    pub request: CashHedgeRequest,
    /// Separate actual currency/collateral model discount source.
    pub discount: FuturesCurve<'a>,
    /// Required for nonzero funding haircut when funding is supplied.
    pub own_cash: Option<&'a FuturesCarryRate>,
    /// Required when an actual short-loan fee is estimated, otherwise absent.
    pub borrow_fee_basis: Option<FuturesBorrowFeeBasis>,
    /// Economic instant and information availability cutoff.
    pub context: EligibilityContext,
}

/// Actual model and side-quote values; expected estimates never mean realized cash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashHedgeValuation {
    /// Exact original actual security revision.
    pub instrument: InstrumentPin,
    /// Buy versus owned sale versus eligible short.
    pub side: CashHedgeSide,
    /// Strictly positive original face; direction remains explicit.
    pub face: ExactDecimal,
    /// Actual quoted security settlement.
    pub settlement: BusinessDate,
    /// Full-contract model clean per100 original face.
    pub model_clean_per100: ExactDecimal,
    /// Full-contract model dirty per100 original face.
    pub model_dirty_per100: ExactDecimal,
    /// Actual contract accrued per100 original face.
    pub accrued_per100: ExactDecimal,
    /// Signed model dirty value, positive purchase and negative sale/short.
    pub signed_model_value: Money,
    /// Original actual ask for buy or bid for sale/short; None remains missing.
    pub side_clean_per100: Option<ExactDecimal>,
    /// Actual side dirty price, distinct from model fair value.
    pub side_dirty_per100: Option<ExactDecimal>,
    /// Signed proposed settlement cash including costs, negative for purchase.
    /// Absent unless both side price and separate costs are present.
    pub proposed_settlement_cash: Option<Money>,
    /// Explicit positive transaction cost for the proposed face; None is missing.
    pub transaction_costs: Option<Money>,
    /// Gross fixed-initial-balance repo plus haircut opportunity cost estimate.
    /// No coupon reinvestment/principal paydown is inferred; None is missing funding.
    pub initial_balance_financing_estimate: Option<Money>,
    /// Explicit fee estimate for an actual supplied short loan; None means unused
    /// or missing loan data, as diagnosed in the input assessment.
    pub borrow_fee_estimate: Option<Money>,
    /// Original loan fee basis, when actually used.
    pub borrow_fee_basis: Option<FuturesBorrowFeeBasis>,
    /// Side quote minus model clean value; absent with missing quote.
    pub residual_basis_per100: Option<ExactDecimal>,
    /// Exact actually consumed discount key.
    pub discount_curve: CurveKey,
    /// Every actual consumed source and execution-input rejection reason.
    pub inputs: FuturesInputReport,
}

/// Price actual fixed/stepped/amortizing plain security cash and the proposed
/// hedge at its actual side quote. Optional/floating terms require their owning
/// complete model/history APIs and fail this fixed cash route explicitly.
pub fn price_cash_hedge(inputs: &CashHedgePricingInputs<'_>) -> FuturesResult<CashHedgeValuation> {
    let assessment = assess_cash_hedge(
        inputs.universe,
        inputs.security.instrument,
        inputs.request,
        inputs.context,
    )?;
    let mut report = FuturesInputReport::new(inputs.context, assessment);
    report.add(&inputs.universe.stamp)?;
    inputs
        .security
        .check(inputs.universe.currency, &mut report)?;
    inputs.discount.check(&mut report)?;
    let candidate = inputs
        .universe
        .candidates
        .iter()
        .find(|c| &c.instrument == inputs.security.instrument)
        .ok_or_else(|| {
            invalid(
                "cash_hedge",
                "exact security revision is missing from actual universe",
            )
        })?;
    report.add(&candidate.quote)?;
    let model = crate::bond_contract::price_fixed(
        inputs.security.bond,
        candidate.settlement.date(),
        PaymentDateEntitlement::Exclude,
        inputs.discount.curve,
    )?;
    let clean = decimal(model.clean_price)?;
    let dirty = decimal(model.dirty_price)?;
    let accrued = decimal(model.accrued)?;
    let face = inputs.request.face.decimal();
    let factor = arithmetic(face.checked_div(Decimal::from(100)), "face_scaling")?;
    let sign = if inputs.request.side == CashHedgeSide::Buy {
        Decimal::ONE
    } else {
        -Decimal::ONE
    };
    let side_clean = if inputs.request.side == CashHedgeSide::Buy {
        candidate.ask_per100
    } else {
        candidate.bid_per100
    };
    let side_dirty = side_clean
        .map(|c| arithmetic(c.decimal().checked_add(accrued), "side_dirty").map(ExactDecimal::new))
        .transpose()?;
    let costs = candidate
        .costs
        .as_ref()
        .map(|c| -> FuturesResult<Decimal> {
            report.add(&c.stamp)?;
            arithmetic(
                c.currency_per100_face.decimal().checked_mul(factor),
                "transaction_costs",
            )
        })
        .transpose()?;
    let settlement_cash = match (side_dirty, costs) {
        (Some(price), Some(cost)) => Some(money(
            arithmetic(
                price
                    .decimal()
                    .checked_mul(-sign)
                    .and_then(|v| v.checked_mul(factor))
                    .and_then(|v| v.checked_sub(cost)),
                "settlement_cash",
            )?,
            candidate.currency,
        )),
        _ => None,
    };
    let funding_cost = if let Some(funding) = &candidate.financing {
        report.add(&funding.stamp)?;
        let h = funding.haircut_fraction.decimal();
        match (h.is_zero(), inputs.own_cash) {
            (false, Some(own)) => {
                own.check(candidate.settlement, inputs.request.horizon, &mut report)?
            }
            (true, None) => (),
            _ => {
                return Err(invalid(
                    "cash_haircut",
                    "nonzero haircut needs own-cash rate; zero haircut refuses unused rate",
                ))
            }
        }
        if let Some(side) = side_dirty.filter(|_| funding.end >= inputs.request.horizon) {
            let balance = arithmetic(side.decimal().checked_mul(factor), "funding_balance")?;
            let tau = funding
                .day_count
                .to_day_count()
                .year_fraction(candidate.settlement.date(), inputs.request.horizon.date());
            let repo = arithmetic(
                balance
                    .checked_mul(Decimal::ONE - h)
                    .and_then(|v| v.checked_mul(funding.repo_decimal.decimal()))
                    .and_then(|v| v.checked_mul(tau)),
                "cash_repo_cost",
            )?;
            let own = if let Some(own) = inputs.own_cash {
                own.interest(
                    candidate.settlement.date(),
                    inputs.request.horizon.date(),
                    arithmetic(balance.checked_mul(h), "cash_haircut_balance")?,
                )?
            } else {
                Decimal::ZERO
            };
            Some(money(
                arithmetic(repo.checked_add(own), "cash_funding_cost")?,
                candidate.currency,
            ))
        } else {
            None
        }
    } else {
        if inputs.own_cash.is_some() {
            return Err(invalid(
                "cash_haircut",
                "own-cash rate without actual financing",
            ));
        }
        None
    };
    let borrow = if inputs.request.side == CashHedgeSide::Short {
        if let Some(loan) = &candidate.borrow {
            report.add(&loan.stamp)?;
            let basis = inputs
                .borrow_fee_basis
                .ok_or_else(|| invalid("borrow_fee", "actual fee-base policy is required"))?;
            let balance = match basis {
                FuturesBorrowFeeBasis::Face => Some(face),
                FuturesBorrowFeeBasis::DirtyMarketValue => side_dirty
                    .map(|p| arithmetic(p.decimal().checked_mul(factor), "borrow_balance"))
                    .transpose()?,
            };
            balance
                .filter(|_| loan.until >= inputs.request.horizon)
                .map(|b| {
                    arithmetic(
                        b.checked_mul(loan.fee_decimal.decimal()).and_then(|v| {
                            v.checked_mul(loan.day_count.to_day_count().year_fraction(
                                candidate.settlement.date(),
                                inputs.request.horizon.date(),
                            ))
                        }),
                        "borrow_fee",
                    )
                    .map(|v| money(v, candidate.currency))
                })
                .transpose()?
        } else {
            if inputs.borrow_fee_basis.is_some() {
                return Err(invalid(
                    "borrow_fee",
                    "unused fee policy without actual loan",
                ));
            }
            None
        }
    } else {
        if inputs.borrow_fee_basis.is_some() {
            return Err(invalid(
                "borrow_fee",
                "unused short-loan policy for buy/owned sale",
            ));
        }
        None
    };
    report.finish();
    Ok(CashHedgeValuation {
        instrument: inputs.security.instrument.clone(),
        side: inputs.request.side,
        face: inputs.request.face,
        settlement: candidate.settlement,
        model_clean_per100: ExactDecimal::new(clean),
        model_dirty_per100: ExactDecimal::new(dirty),
        accrued_per100: ExactDecimal::new(accrued),
        signed_model_value: money(
            arithmetic(
                dirty.checked_mul(factor).and_then(|v| v.checked_mul(sign)),
                "cash_model_value",
            )?,
            candidate.currency,
        ),
        side_clean_per100: side_clean,
        side_dirty_per100: side_dirty,
        proposed_settlement_cash: settlement_cash,
        transaction_costs: costs.map(|v| money(v, candidate.currency)),
        initial_balance_financing_estimate: funding_cost,
        borrow_fee_estimate: borrow,
        borrow_fee_basis: inputs.borrow_fee_basis,
        residual_basis_per100: side_clean.map(|v| ExactDecimal::new(v.decimal() - clean)),
        discount_curve: model.discount_curve,
        inputs: report,
    })
}

/// Signed fixed-model discount-zero risk of the actual proposed cash security.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashHedgeRisk {
    /// Complete base model/side economics and all source blocks.
    pub base: CashHedgeValuation,
    /// Actual central parallel continuous-zero bump in decimal rate.
    pub bump_decimal_rate: ExactDecimal,
    /// Signed currency change normalized to +1bp at the recipe's time basis.
    pub currency_per_discount_zero_bp: Money,
    /// Actual risk basis, distinct from raw yield or recalibrated input-quote PV01.
    pub basis: String,
}
/// Reprice the complete actual projected contract cash. Quotes, financing and
/// borrowed availability are frozen and do not disappear from risk admission.
pub fn cash_hedge_risk(
    inputs: &CashHedgePricingInputs<'_>,
    bump_decimal_rate: ExactDecimal,
) -> FuturesResult<CashHedgeRisk> {
    let bump = bump_decimal_rate.decimal();
    if bump < Decimal::new(1, 8) || bump > Decimal::new(5, 2) {
        return Err(invalid("risk_bump", "decimal bump must be in[1e-8,0.05]"));
    }
    let base = price_cash_hedge(inputs)?;
    let up = super::bond::shifted_cash_dirty(
        inputs.security.bond,
        base.settlement.date(),
        inputs.discount.curve,
        number(bump)?,
    )?;
    let down = super::bond::shifted_cash_dirty(
        inputs.security.bond,
        base.settlement.date(),
        inputs.discount.curve,
        -number(bump)?,
    )?;
    let sign = if inputs.request.side == CashHedgeSide::Buy {
        Decimal::ONE
    } else {
        -Decimal::ONE
    };
    let value = arithmetic(
        decimal(up - down)?
            .checked_mul(inputs.request.face.decimal())
            .and_then(|v| v.checked_div(Decimal::from(100)))
            .and_then(|v| v.checked_mul(sign))
            .and_then(|v| v.checked_mul(Decimal::new(1, 4)))
            .and_then(|v| v.checked_div(Decimal::from(2) * bump)),
        "cash_risk",
    )?;
    Ok(CashHedgeRisk {
        base,
        bump_decimal_rate,
        currency_per_discount_zero_bp: money(value, inputs.universe.currency),
        basis: "parallel_continuous_discount_zeros_frozen_cash_quotes_funding_and_borrow".into(),
    })
}
