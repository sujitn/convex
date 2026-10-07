//! Dated financing, haircut funding and coupon reinvestment remain independent.

use super::*;
use convex_core::{
    cashflows::PaymentDateEntitlement, contracts::ExactDecimal, daycounts::DayCountConvention,
};
use convex_rates::{CalendarBinding, ReferencePin};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Exact collateral definition attached to an actual repo/funding curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoFundingCurve {
    /// Immutable original curve version.
    pub curve: CurveKey,
    /// Exact repo collateral/own-cash funding definition.
    pub collateral_id: String,
}
/// Coupon cash treatment distinct from principal financing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum RepoCouponCarry {
    /// Explicitly retain coupons uninvested until delivery.
    HeldCash {
        /// Actual declared assumption, bounded and nonempty.
        evidence: String,
    },
    /// Reinvest each actual received coupon over its own cash-to-delivery dates.
    Reinvested {
        /// Exact separately supplied reinvestment curve and collateral.
        funding: RepoFundingCurve,
    },
}
/// Full supplied cash purchase and delivery/funding assumptions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoBondForwardTerms {
    /// Original complete cash security and actual clean/dirty/yield observation.
    pub spot: CashBondHelper,
    /// Actual fixed delivery date, distinct from security maturity.
    pub delivery: Date,
    /// Exact actual delivery calendar resource.
    pub delivery_calendar: ReferencePin,
    /// Exact independently supplied repo funding curve and collateral.
    pub repo: RepoFundingCurve,
    /// Exact haircut fraction in[0,1); funding rate cannot manufacture a haircut.
    pub haircut_fraction: ExactDecimal,
    /// Mandatory only for a nonzero haircut; actual separate own-cash funding.
    pub own_cash: Option<RepoFundingCurve>,
    /// Exact declared coupon handling policy.
    pub coupons: RepoCouponCarry,
}
/// All resolved immutable resources; no provider or registry lookup.
pub struct RepoBondForwardInputs<'a> {
    /// Complete original economic assumptions.
    pub terms: &'a RepoBondForwardTerms,
    /// Actual security calendars with matching identities/versions.
    pub resources: &'a CashHelperResources<'a>,
    /// Actual delivery-calendar resource.
    pub delivery_calendar: CalendarBinding<'a>,
    /// Actual repo, own-cash and reinvestment curves.
    pub curves: &'a CurveSet,
}
/// Actual repo factor/rate for an independently supplied dated interest period.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoPeriodRate {
    /// Exact funding identity/collateral.
    pub funding: RepoFundingCurve,
    /// Actual cash start.
    pub start: Date,
    /// Actual cash end.
    pub end: Date,
    /// Annualized simple rate's actual quotation basis.
    pub day_count: DayCountConvention,
    /// Actual cash accrual fraction.
    pub accrual_fraction: f64,
    /// P(start)/P(end), positive even for negative repo rates.
    pub growth: f64,
    /// Exact period-equivalent simple rate; caller may bind this plus the full
    /// curve provenance into CVX-023 FinancingInput. This is model funding data.
    pub simple_decimal_rate: f64,
}
/// Query a true Repo role for a dated period; discount/government substitution
/// and wrong collateral/currency fail even if their numbers happen to coincide.
pub fn repo_rate_over_period(
    curves: &CurveSet,
    funding: &RepoFundingCurve,
    currency: convex_core::Currency,
    start: Date,
    end: Date,
    day_count: DayCountConvention,
) -> CashCalibrationResult<RepoPeriodRate> {
    let c = curves.get(&funding.curve)?;
    if !label(&funding.collateral_id)
        || c.recipe().currency != currency
        || !matches!(&c.recipe().role, CurveRole::Repo { collateral_id } if collateral_id == &funding.collateral_id)
    {
        return Err(invalid(
            "repo role",
            "exact same-currency Repo/collateral role required",
        ));
    }
    let tau = year_fraction(start, end, day_count)?;
    let ratio = c
        .query(&CurveQuery::DiscountRatio {
            start: CurvePoint::Date(start),
            end: CurvePoint::Date(end),
        })?
        .value;
    let rate = rate_from_discount(ratio, tau, convex_core::types::Compounding::Simple)?;
    let growth = finite(1.0 / ratio, "repo growth")?;
    Ok(RepoPeriodRate {
        funding: funding.clone(),
        start,
        end,
        day_count,
        accrual_fraction: tau,
        growth,
        simple_decimal_rate: rate,
    })
}
/// One original entitled coupon and its distinct reinvestment horizon.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoCouponValue {
    /// Actual coupon payment date, including contractual delay/adjustment.
    pub payment_date: Date,
    /// Actual received cash per100 original face.
    pub cash_per100: f64,
    /// Explicit coupon funding growth, one only for HeldCash or payment at delivery.
    pub growth: f64,
    /// Actual coupon value at delivery.
    pub delivery_value_per100: f64,
}
/// Deterministic fixed-date carry; no delivery options or daily-margin financing.
#[derive(Debug, Clone)]
pub struct RepoBondForwardValuation {
    /// Full original economic assumptions and exact pins, retained verbatim.
    pub terms: RepoBondForwardTerms,
    /// Actual consistent settlement clean/dirty/accrued conversion.
    pub spot: CashQuoteConversion,
    /// Actual full-contract delivery accrued per100 face.
    pub delivery_accrued_per100: f64,
    /// Exact repo period factor with separate actual start/end dates.
    pub repo: RepoPeriodRate,
    /// Actual own-cash factor only when the haircut is nonzero.
    pub own_cash: Option<RepoPeriodRate>,
    /// Repo cost on(1-haircut) dirty purchase value, signed for negative rates.
    pub repo_cost_per100: f64,
    /// Separate cost on haircut cash; absent funding never means a zero-rate guess.
    pub own_cash_cost_per100: f64,
    /// All actually received coupon cash and distinct delivery values.
    pub coupons: Vec<RepoCouponValue>,
    /// Unrounded modeled dirty forward per100 original face.
    pub dirty_forward_per100: f64,
    /// Unrounded modeled clean forward per100 original face.
    pub clean_forward_per100: f64,
}
impl RepoBondForwardValuation {
    /// Fixed-date unrounded futures points at an actually supplied conversion
    /// factor. This supplies carry economics, never a basket/CTD/executable quote.
    pub fn futures_price_points(
        &self,
        conversion_factor: ExactDecimal,
    ) -> CashCalibrationResult<f64> {
        let cf = pricing::number(conversion_factor.decimal())?;
        if cf <= 0.0 || cf > 10.0 {
            return Err(invalid(
                "conversion factor",
                "positive bounded supplied factor required",
            ));
        }
        finite(self.clean_forward_per100 / cf, "futures points")
    }
}

/// Full supplied nominal-bullet carry, including actual stepped coupon cash.
/// Money/quantity execution, delivery optionality and realized margin cash are
/// separate workflows; this result is an unrounded deterministic model value.
pub fn price_repo_bond_forward(
    input: &RepoBondForwardInputs<'_>,
) -> CashCalibrationResult<RepoBondForwardValuation> {
    let t = input.terms;
    let repo_curve = input.curves.get(&t.repo.curve)?;
    let p = pricing::prepare(&t.spot, input.resources, repo_curve.recipe().reference_date)?;
    if input.delivery_calendar.pin != &t.delivery_calendar
        || !input.delivery_calendar.calendar.is_business_day(t.delivery)
        || t.delivery <= t.spot.settlement
        || t.delivery >= t.spot.terms.schedule.end_date
        || t.spot.on_payment != PaymentDateEntitlement::Exclude
        || t.spot.quote_kind == CashQuoteKind::ParCouponRate
        || t.spot.terms.dates.ex_coupon.is_some()
        || !t.spot.terms.principal_prices.is_empty()
        || t.spot.terms.redemption_multiplier != Decimal::ONE
        || t.spot
            .terms
            .profiles
            .notional
            .iter()
            .any(|n| n.value != t.spot.terms.face_value)
    {
        return Err(invalid("forward terms", "actual future business delivery, post-payment nominal bullet purchase and full compatible pins required"));
    }
    let haircut = pricing::number(t.haircut_fraction.decimal())?;
    if !(0.0..1.0).contains(&haircut) || t.own_cash.is_some() != (haircut != 0.0) {
        return Err(invalid(
            "haircut funding",
            "fraction in[0,1), exact separate own-cash funding iff nonzero required",
        ));
    }
    let currency = t.spot.terms.currency;
    let start = t.spot.settlement;
    let repo = repo_rate_over_period(
        input.curves,
        &t.repo,
        currency,
        start,
        t.delivery,
        DayCountConvention::Act360,
    )?;
    let own_cash = t
        .own_cash
        .as_ref()
        .map(|f| {
            repo_rate_over_period(
                input.curves,
                f,
                currency,
                start,
                t.delivery,
                DayCountConvention::Act360,
            )
        })
        .transpose()?;
    match &t.coupons {
        RepoCouponCarry::HeldCash { evidence }
            if evidence.is_empty()
                || evidence.trim() != evidence
                || evidence.len() > 512
                || evidence.chars().any(char::is_control) =>
        {
            return Err(invalid(
                "coupon policy",
                "bounded actual held-cash assumption required",
            ))
        }
        RepoCouponCarry::Reinvested { funding } => {
            repo_rate_over_period(
                input.curves,
                funding,
                currency,
                start,
                t.delivery,
                DayCountConvention::Act360,
            )?;
        }
        _ => (),
    }
    let delivery = p
        .bond
        .fixed_projection(t.delivery, PaymentDateEntitlement::Exclude)?;
    let face = pricing::number(t.spot.terms.face_value)?;
    let delivery_accrued = pricing::number(delivery.accrued)? / face * 100.0;
    let repo_cost = p.observed.dirty_per100 * (1.0 - haircut) * (repo.growth - 1.0);
    let own_cost = own_cash
        .as_ref()
        .map(|r| p.observed.dirty_per100 * haircut * (r.growth - 1.0))
        .unwrap_or(0.0);
    let mut coupons = Vec::new();
    for c in &p.projection.coupons {
        if !c.entitled || c.payment_date <= start || c.payment_date > t.delivery {
            continue;
        }
        let growth = match &t.coupons {
            RepoCouponCarry::HeldCash { .. } => 1.0,
            RepoCouponCarry::Reinvested { funding } if c.payment_date < t.delivery => {
                repo_rate_over_period(
                    input.curves,
                    funding,
                    currency,
                    c.payment_date,
                    t.delivery,
                    DayCountConvention::Act360,
                )?
                .growth
            }
            RepoCouponCarry::Reinvested { .. } => 1.0,
        };
        let cash = pricing::number(c.amount)? / face * 100.0;
        coupons.push(RepoCouponValue {
            payment_date: c.payment_date,
            cash_per100: cash,
            growth,
            delivery_value_per100: finite(cash * growth, "coupon carry")?,
        });
    }
    let coupon_value: f64 = coupons.iter().map(|c| c.delivery_value_per100).sum();
    let dirty = finite(
        p.observed.dirty_per100 + repo_cost + own_cost - coupon_value,
        "dirty forward",
    )?;
    let clean = finite(dirty - delivery_accrued, "clean forward")?;
    if dirty <= 0.0 || clean <= 0.0 {
        return Err(invalid(
            "forward price",
            "positive model clean/dirty forward required",
        ));
    }
    Ok(RepoBondForwardValuation {
        terms: t.clone(),
        spot: p.observed,
        delivery_accrued_per100: delivery_accrued,
        repo,
        own_cash,
        repo_cost_per100: finite(repo_cost, "repo cost")?,
        own_cash_cost_per100: finite(own_cost, "own cash cost")?,
        coupons,
        dirty_forward_per100: dirty,
        clean_forward_per100: clean,
    })
}
