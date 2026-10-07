//! Two actual independently projected floating legs, using the shared leg engine.
//! No index/discount substitution, principal exchange or spread compounding.

use std::collections::BTreeMap;

use convex_core::{
    cashflows::{CashflowError, DatedValue, MAX_SCHEDULE_PERIODS},
    contracts::{IndexId, ModelDecimalRate, ModelMoney, Money, TradeId, UtcTimestamp, Version},
    fixings::{FixingRequirement, MAX_FIXING_REQUIREMENTS},
    Currency, Date,
};
use convex_curves::contracts::{CurveQueryValue, CurveRecipe};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    invalid,
    pricing::{price_swap, DiscountedCoupon, SwapParQuotes, SwapValuationInputs},
    CalendarBinding, CashflowContext, CollateralTerms, CouponCashflow, FixedLegTerms,
    FloatingLegTerms, LegCalendars, RateUse, SwapCalendars, SwapContract, SwapError, SwapSide,
    SwapTerms,
};

/// Native absolute-spread quotation leg. Both leg names retain their economic sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum BasisQuotedLeg {
    /// Spread on the paid floating leg.
    Pay,
    /// Spread on the received floating leg.
    Receive,
}

/// Complete same-currency basis trade; no manufactured fixed leg is an input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BasisSwapTerms {
    /// Economic schema version, exactly 1.
    pub schema_version: u32,
    /// Actual caller-assigned trade identity.
    pub trade_id: TradeId,
    /// Immutable economic revision.
    pub version: Version,
    /// Both legs' cash currency.
    pub currency: Currency,
    /// Shared unadjusted effective date.
    pub effective_date: Date,
    /// Shared unadjusted maturity; payment/index horizons can be later.
    pub maturity_date: Date,
    /// Independently dated paid floating leg and projection mapping.
    pub pay: FloatingLegTerms,
    /// Independently dated received floating leg and projection mapping.
    pub receive: FloatingLegTerms,
    /// Exact same-currency agreement and discount mapping.
    pub collateral: CollateralTerms,
}

/// Actual supplied resources for one floating leg.
#[derive(Clone, Copy)]
pub struct BasisLegCalendars<'a> {
    /// Independent accrual/payment bindings.
    pub leg: LegCalendars<'a>,
    /// Term reset/value calendar, absent for overnight.
    pub reset: Option<CalendarBinding<'a>>,
    /// Term fixing calendar, absent for overnight.
    pub fixing: Option<CalendarBinding<'a>>,
    /// Overnight observation calendar, absent for term.
    pub observation: Option<CalendarBinding<'a>>,
}

/// Independent exact calendar versions for both basis legs.
#[derive(Clone, Copy)]
pub struct BasisSwapCalendars<'a> {
    /// Paid leg resources.
    pub pay: BasisLegCalendars<'a>,
    /// Received leg resources.
    pub receive: BasisLegCalendars<'a>,
}

/// Checked immutable two-leg contract. Shared floating coupon caches are reused.
#[derive(Debug, Clone)]
pub struct BasisSwapContract {
    terms: BasisSwapTerms,
    pay: SwapContract,
    receive: SwapContract,
}

impl BasisSwapTerms {
    /// Build both actual floating schedules/profiles and reset/observation plans.
    pub fn build(self, calendars: &BasisSwapCalendars<'_>) -> Result<BasisSwapContract, SwapError> {
        // Native callers also receive the shared ingress bound before the
        // validation adapter copies any profile; strict JSON already enforces it.
        for leg in [&self.pay, &self.receive] {
            for entries in [
                &leg.leg().notionals,
                &leg.profiles().spreads,
                &leg.profiles().gearings,
            ] {
                if entries.len() > MAX_SCHEDULE_PERIODS {
                    return Err(CashflowError::BoundExceeded("basis profile ingress").into());
                }
            }
        }
        let pay = floating_adapter(&self, &self.pay, calendars.pay)?;
        let receive = floating_adapter(&self, &self.receive, calendars.receive)?;
        Ok(BasisSwapContract {
            terms: self,
            pay,
            receive,
        })
    }
}

// The existing leg engine is housed in SwapContract. A private zero fixed coupon
// on that same leg's actual coverage provides its validation/annuity machinery.
// It is never a market input, actual basis cashflow or reported pricing leg.
// All economically nonzero cash comes unchanged from the shared floating engine.
fn floating_adapter(
    terms: &BasisSwapTerms,
    leg: &FloatingLegTerms,
    calendars: BasisLegCalendars<'_>,
) -> Result<SwapContract, SwapError> {
    let first = leg
        .leg()
        .notionals
        .first()
        .ok_or_else(|| invalid("notional", "missing leg profile"))?;
    let last = leg
        .leg()
        .notionals
        .last()
        .ok_or_else(|| invalid("notional", "missing leg profile"))?;
    SwapTerms {
        schema_version: terms.schema_version,
        trade_id: terms.trade_id.clone(),
        version: terms.version,
        currency: terms.currency,
        side: SwapSide::PayFixed,
        effective_date: terms.effective_date,
        maturity_date: terms.maturity_date,
        fixed: FixedLegTerms {
            leg: leg.leg().clone(),
            rates: vec![DatedValue {
                start: first.start,
                end: last.end,
                value: Decimal::ZERO,
            }],
        },
        floating: leg.clone(),
        collateral: terms.collateral.clone(),
    }
    .build(&SwapCalendars {
        fixed: calendars.leg,
        floating: calendars.leg,
        reset: calendars.reset,
        fixing: calendars.fixing,
        observation: calendars.observation,
    })
}

impl BasisSwapContract {
    /// Original exact two-leg economics; no validation-only fixed leg is exposed.
    pub fn terms(&self) -> &BasisSwapTerms {
        &self.terms
    }

    /// Both complete fixing requirement sets, before any curve is consulted.
    pub fn fixing_requirements<P>(
        &self,
        context: CashflowContext,
        mut publication: P,
    ) -> Result<Vec<FixingRequirement>, SwapError>
    where
        P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
    {
        let mut requirements = BTreeMap::new();
        for leg in [&self.pay, &self.receive] {
            for r in leg.fixing_requirements(context, &mut publication)? {
                let key = (r.index_id.clone(), r.observation_date);
                if requirements.len() >= crate::MAX_SWAP_TERMS && !requirements.contains_key(&key) {
                    return Err(CashflowError::BoundExceeded("basis fixing requirements").into());
                }
                if let Some(old) = requirements.insert(key, r.clone()) {
                    if old != r {
                        return Err(invalid(
                            "publication",
                            "conflicting basis publication times",
                        ));
                    }
                }
            }
        }
        Ok(requirements.into_values().collect())
    }
}

/// Signed basis leg result with original coupons and actual consumed rates/curves.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BasisLegValuation {
    /// Signed leg PV in absolute currency.
    pub present_value: ModelMoney,
    /// Signed expected accrued, never a recorded cash payment.
    pub accrued: Money,
    /// Exact signed original expected coupon cash.
    pub cashflows: Vec<CouponCashflow>,
    /// Exact reset or daily observations and their origins.
    pub rates: Vec<RateUse>,
    /// Actual signed discounted coupons.
    pub discounted: Vec<DiscountedCoupon>,
    /// Actually consumed forward queries.
    pub projection_queries: Vec<CurveQueryValue>,
    /// Valuation-date discount, absent when all cash is paid.
    pub valuation_discount: Option<CurveQueryValue>,
    /// Actual immutable consumed recipes including transitive dependencies.
    pub consumed_curves: Vec<CurveRecipe>,
}

/// Both uniform replacement spreads and parallel shifts are explicit.
#[derive(Debug, Clone, serde::Serialize)]
pub enum BasisParQuotes {
    /// Full unrounded linear cash with positive remaining spread exposure.
    Available {
        /// Uniform replacement paid-leg annualized decimal spread.
        pay_spread: ModelDecimalRate,
        /// Uniform replacement received-leg annualized decimal spread.
        receive_spread: ModelDecimalRate,
        /// Additive shift to every existing paid spread step.
        pay_spread_shift: ModelDecimalRate,
        /// Additive shift to every existing received spread step.
        receive_spread_shift: ModelDecimalRate,
        /// Positive paid spread PV per unit decimal rate.
        pay_annuity: ModelMoney,
        /// Positive received spread PV per unit decimal rate.
        receive_annuity: ModelMoney,
    },
    /// No fabricated zero inverse for expired/rounded/clipped cash.
    Unavailable {
        /// Stable reason analytic inversion does not apply.
        reason: &'static str,
    },
}

/// Actual two-leg basis valuation. Positive PV is value to this pay/receive trade.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BasisSwapValuation {
    /// Original complete basis terms.
    pub terms: BasisSwapTerms,
    /// Supplied cash/history context.
    pub context: CashflowContext,
    /// Net signed currency PV.
    pub present_value: ModelMoney,
    /// Paid leg has negative cash/PV; negative coupons retain their actual signs.
    pub pay: BasisLegValuation,
    /// Received leg has positive cash/PV.
    pub receive: BasisLegValuation,
    /// Absolute replacement versus additive spread inverses.
    pub par: BasisParQuotes,
}

/// Price the full two-leg trade. Preflight both histories before either forecast.
/// Administrator publication callback is frozen once per unique index/date.
pub fn price_basis_swap<P>(
    contract: &BasisSwapContract,
    inputs: &SwapValuationInputs<'_>,
    context: CashflowContext,
    mut publication: P,
) -> Result<BasisSwapValuation, SwapError>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    let mut publications = BTreeMap::new();
    let requirements = contract.fixing_requirements(context, |index, at| {
        if let Some(t) = publications.get(&(index.clone(), at)) {
            return Ok(*t);
        }
        let t = publication(index, at)?;
        publications.insert((index.clone(), at), t);
        Ok(t)
    })?;
    let mut missing = Vec::new();
    for chunk in requirements.chunks(MAX_FIXING_REQUIREMENTS) {
        missing.extend(
            inputs
                .fixings
                .resolve(chunk, context.fixings)?
                .missing()
                .iter()
                .cloned(),
        );
    }
    if !missing.is_empty() {
        return Err(CashflowError::MissingFixings(missing).into());
    }
    let frozen = |index: &IndexId, at: Date| {
        publications
            .get(&(index.clone(), at))
            .copied()
            .ok_or_else(|| CashflowError::InvalidTerms {
                field: "publication",
                reason: "missing frozen basis publication".into(),
            })
    };
    let pay = price_swap(&contract.pay, inputs, context, frozen)?;
    let receive = price_swap(&contract.receive, inputs, context, frozen)?;
    let currency = contract.terms.currency;
    let p = pay.floating_present_value.value().get();
    let r = receive.floating_present_value.value().get();
    let net = r - p;
    let model_rate = |x| {
        Ok::<_, SwapError>(ModelDecimalRate::new(
            convex_core::contracts::FiniteF64::new(x)
                .map_err(|_| invalid("basis par", "nonfinite rate"))?,
        ))
    };
    let par = match (&pay.par, &receive.par) {
        (SwapParQuotes::Available { floating_spread: ps, floating_annuity: pa, .. },
         SwapParQuotes::Available { floating_spread: rs, floating_annuity: ra, .. }) =>
            BasisParQuotes::Available {
                pay_spread: model_rate(ps.value().get() + r / pa.value().get())?,
                receive_spread: model_rate(rs.value().get() + p / ra.value().get())?,
                pay_spread_shift: model_rate(net / pa.value().get())?,
                receive_spread_shift: model_rate(-net / ra.value().get())?,
                pay_annuity: *pa,
                receive_annuity: *ra,
            },
        _ => BasisParQuotes::Unavailable { reason: "requires unrounded Allow-negative cash and positive remaining exposure on both legs" },
    };
    let signed =
        |mut v: crate::pricing::SwapValuation, sign: f64| -> Result<BasisLegValuation, SwapError> {
            let finite = |x| {
                convex_core::contracts::FiniteF64::new(x)
                    .map_err(|_| invalid("basis PV", "nonfinite result"))
            };
            let exact_sign = if sign < 0.0 {
                -Decimal::ONE
            } else {
                Decimal::ONE
            };
            for c in &mut v.cashflows.floating {
                c.signed_amount = c
                    .signed_amount
                    .checked_mul(exact_sign)
                    .ok_or(CashflowError::Arithmetic("basis signed cash"))?;
            }
            for c in &mut v.floating {
                c.present_value =
                    ModelMoney::new(finite(c.present_value.value().get() * sign)?, currency);
            }
            let accrued = v
                .floating_accrued
                .value()
                .decimal()
                .checked_mul(exact_sign)
                .ok_or(CashflowError::Arithmetic("basis signed accrued"))?;
            Ok(BasisLegValuation {
                present_value: ModelMoney::new(
                    finite(v.floating_present_value.value().get() * sign)?,
                    currency,
                ),
                accrued: Money::new(convex_core::contracts::ExactDecimal::new(accrued), currency),
                cashflows: v.cashflows.floating,
                rates: v.cashflows.rates,
                discounted: v.floating,
                projection_queries: v.projection_queries,
                valuation_discount: v.valuation_discount,
                consumed_curves: v.consumed_curves,
            })
        };
    Ok(BasisSwapValuation {
        terms: contract.terms.clone(),
        context,
        present_value: ModelMoney::new(
            convex_core::contracts::FiniteF64::new(net)
                .map_err(|_| invalid("basis PV", "nonfinite result"))?,
            currency,
        ),
        pay: signed(pay, -1.0)?,
        receive: signed(receive, 1.0)?,
        par,
    })
}
