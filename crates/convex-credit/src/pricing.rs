//! Exact piecewise-exponential integration under explicitly admitted curves.
use crate::{
    checked_date, finite, invalid, CdsContract, CdsError, CdsModel, CdsPricingContext, CdsSide,
    InitialPaymentState, MAX_INTEGRATION_POINTS, STANDARD_MODEL_NOTICE,
};
use convex_core::{daycounts::DayCountConvention, Currency, Date};
use convex_curves::{
    contracts::{
        BuiltCurve, CurveKey, CurvePoint, CurveQuery, CurveRepresentation, CurveRole, CurveSet,
        CurveSource,
    },
    ExtrapolationMethod, InterpolationMethod,
};
use serde::Serialize;

/// Actual credit input used; a conversion never invents a supplied curve version.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreditInput {
    /// Actual checked exact-version survival curve.
    Supplied {
        /// Exact consumed version.
        curve: CurveKey,
    },
    /// Explicit constant intensity under the standard conventional conversion.
    FlatHazard {
        /// Intensity per ACT/365F year, never spread/(1-recovery).
        hazard_per_act365f_year: f64,
    },
}
/// One future scheduled premium and its separately integrated default accrual.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PremiumValue {
    /// Inclusive start.
    pub accrual_start: Date,
    /// Exclusive end, including final unadjusted maturity day.
    pub accrual_end: Date,
    /// Conditional premium cash date.
    pub payment_date: Date,
    /// Actual coupon fraction of a year.
    pub year_fraction: f64,
    /// Survival through the day before accrual end, conditional on valuation.
    pub survival_probability: f64,
    /// Signed promised premium currency amount IF survival condition is met.
    pub conditional_coupon_amount: f64,
    /// Signed scheduled expected premium PV at valuation.
    pub scheduled_pv: f64,
    /// Signed integrated accrued-on-default PV at valuation.
    pub accrual_on_default_pv: f64,
}
/// Signed currency PV and buyer-to-seller fair quote/settlement decomposition.
/// These model results are unrounded; ledger postings require a supplied policy.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CdsValuation {
    /// Common settlement currency.
    pub currency: Currency,
    /// Trade protection perspective used for all PV and premium cash signs.
    pub side: CdsSide,
    /// Explicit mathematical convention/version.
    pub model: CdsModel,
    /// Required model attribution for consumers displaying this route.
    pub model_notice: &'static str,
    /// Exact supplied discount version.
    pub discount_curve: CurveKey,
    /// Actual credit model/version input.
    pub credit_input: CreditInput,
    /// End-of-day conditional-survival valuation boundary.
    pub valuation_date: Date,
    /// Current start-of-day protection eligibility.
    pub step_in_date: Date,
    /// Fair quote cash date, independently of any original recorded payment.
    pub cash_settlement_date: Date,
    /// Signed protection PV at valuation; buyer positive.
    pub protection_leg_pv: f64,
    /// Signed scheduled plus accrued-on-default premium PV; buyer negative.
    pub premium_leg_pv: f64,
    /// Signed outstanding original initial-payment PV; buyer pays positive cash.
    pub initial_payment_pv: f64,
    /// Sum of signed outstanding legs and initial cash.
    pub net_pv: f64,
    /// Unrounded actual accrued-day count through current step-in exclusive.
    pub accrued_days: i64,
    /// Actual premium year fraction through the current step-in boundary.
    pub accrued_year_fraction: f64,
    /// Unsigned currency premium refund to buyer at fair cash settlement.
    pub accrued_premium: f64,
    /// Scheduled and default premium annuity per notional/unit decimal coupon.
    pub dirty_risky_annuity: f64,
    /// Premium annuity less accrued refund discounted to valuation.
    pub clean_risky_annuity: f64,
    /// Par CLEAN running rate from the same supplied curves, where meaningful.
    /// None on maturity/no positive clean annuity; it is never a zero fallback.
    pub par_spread_decimal: Option<f64>,
    /// Fair clean amount from buyer to seller as a fraction of notional.
    pub clean_upfront_fraction: f64,
    /// Fair CASH from buyer to seller, divided by notional; clean minus accrued.
    pub dirty_upfront_fraction: f64,
    /// Fair unrounded currency cash from buyer to seller, independent of side.
    pub cash_settlement_amount: f64,
    /// 100 * (1 - clean upfront fraction); no bond yield semantics.
    pub clean_price_per100: f64,
    /// Signed outstanding conditional premium/default accrual detail.
    pub premiums: Vec<PremiumValue>,
}

pub(crate) struct Components {
    scheduled: f64,
    default_accrual: f64,
    protection: f64,
}
pub(crate) struct Prepared<'a> {
    pub(crate) contract: &'a CdsContract,
    context: &'a CdsPricingContext,
    discount: &'a BuiltCurve,
    points: Vec<f64>,
    discounts: Vec<f64>,
    payment_discounts: Vec<f64>,
    pub(crate) settlement_discount: f64,
    pub(crate) accrued_fraction: f64,
    pub(crate) coupon: f64,
    pub(crate) matured: bool,
    protection_start: f64,
}

fn days(date: Date, valuation: Date) -> f64 {
    valuation.days_between(&date) as f64
}
fn validate_grid(curve: &BuiltCurve, credit: bool) -> Result<(), CdsError> {
    let r = curve.recipe();
    let representation = if credit {
        CurveRepresentation::SurvivalProbabilities
    } else {
        CurveRepresentation::DiscountFactors
    };
    if r.representation != representation
        || r.time_basis != DayCountConvention::Act365Fixed
        || r.interpolation != InterpolationMethod::LogLinear
        || r.extrapolation != ExtrapolationMethod::None
        || !matches!(r.source, CurveSource::Nodes(_))
    {
        return Err(CdsError::Unsupported("standard integration requires supplied direct ACT/365F log-linear DF/survival grids with explicit origin and no extrapolation"));
    }
    Ok(())
}
fn add_knots(
    curve: &BuiltCurve,
    valuation: Date,
    horizon: f64,
    points: &mut Vec<f64>,
) -> Result<(), CdsError> {
    let CurveSource::Nodes(nodes) = &curve.recipe().source else {
        return Err(CdsError::Unsupported("derived or non-node standard curve"));
    };
    let offset = days(curve.recipe().reference_date, valuation);
    for node in nodes {
        let day = match node.pillar {
            CurvePoint::Date(date) => days(date, valuation),
            CurvePoint::Tenor(t) => offset + 365.0 * t,
        };
        if day > 0.0 && day < horizon {
            points.push(day);
        }
    }
    Ok(())
}
fn curve_value(
    curve: &BuiltCurve,
    valuation: Date,
    day: f64,
    credit: bool,
) -> Result<f64, CdsError> {
    let point = CurvePoint::Tenor(curve.date_to_tenor(valuation)? + day / 365.0);
    let query = if credit {
        CurveQuery::SurvivalProbability { point }
    } else {
        CurveQuery::DiscountFactor { point }
    };
    Ok(curve.query(&query)?.value)
}
fn credit_role(contract: &CdsContract, curve: &BuiltCurve) -> Result<(), CdsError> {
    let t = contract.terms();
    let CurveRole::Credit {
        entity_id,
        seniority,
        recovery,
    } = &curve.recipe().role
    else {
        return Err(invalid(
            "survival_curve.role",
            "requires explicit Credit role",
        ));
    };
    if curve.recipe().key != t.survival_curve
        || curve.recipe().currency != t.currency
        || entity_id != t.reference.entity_id.as_str()
        || seniority != t.reference.seniority.as_str()
        || *recovery != t.reference.recovery_fraction.get()
    {
        return Err(invalid(
            "survival_curve.role",
            "exact key/currency/entity/seniority/recovery differs from supplied terms",
        ));
    }
    validate_grid(curve, true)
}
impl<'a> Prepared<'a> {
    pub(crate) fn new(
        contract: &'a CdsContract,
        context: &'a CdsPricingContext,
        discount: &'a BuiltCurve,
        credit: Option<&BuiltCurve>,
    ) -> Result<Self, CdsError> {
        let t = contract.terms();
        for d in [
            context.valuation_date,
            context.step_in_date,
            context.cash_settlement_date,
        ] {
            checked_date(d)?;
        }
        if context.valuation_date < t.trade_date
            || context.step_in_date <= context.valuation_date
            || context.cash_settlement_date < context.valuation_date
            || context.valuation_date.days_between(&context.step_in_date) > 366
            || context
                .valuation_date
                .days_between(&context.cash_settlement_date)
                > 366
        {
            return Err(invalid("pricing_context","requires valuation >= trade, step-in > valuation, settlement >= valuation, bounded 366-day offsets"));
        }
        match (t.initial_clean_upfront_fraction,context.initial_payment_state) {
            (None,InitialPaymentState::NotContracted)=>(),
            (Some(_),InitialPaymentState::Pending) if t.cash_settlement_date>=context.valuation_date=>(),
            (Some(_),InitialPaymentState::Settled) if t.cash_settlement_date<=context.valuation_date=>(),
            _=>return Err(invalid("initial_payment_state","must match contracted cash and actual date; overdue unsettled cash requires explicit lifecycle treatment")),
        }
        let r = discount.recipe();
        if r.key != t.discount_curve
            || r.currency != t.currency
            || r.role
                != (CurveRole::Discount {
                    collateral_currency: t.currency,
                })
            || r.reference_date > context.valuation_date
        {
            return Err(invalid(
                "discount_curve.role",
                "requires exact same-currency/collateral discount key with reference <= valuation",
            ));
        }
        validate_grid(discount, false)?;
        let maturity = t.premium_schedule.end_date;
        let matured = context.valuation_date > maturity || context.step_in_date > maturity;
        if contract.premium_periods().iter().any(|p| {
            p.accrual_end <= context.step_in_date && p.payment_date > context.valuation_date
        }) {
            return Err(CdsError::Unsupported("completed accrual with outstanding delayed premium cash requires explicit lifecycle treatment"));
        }
        let horizon = days(maturity, context.valuation_date).max(0.0);
        let mut points = vec![0.0, horizon];
        add_knots(discount, context.valuation_date, horizon, &mut points)?;
        if let Some(curve) = credit {
            credit_role(contract, curve)?;
            if curve.recipe().reference_date > context.valuation_date {
                return Err(invalid("survival_curve.reference_date", "after valuation"));
            }
            add_knots(curve, context.valuation_date, horizon, &mut points)?;
        }
        let protection_start = days(
            t.protection_start_date.max(context.step_in_date),
            context.valuation_date,
        ) - 1.0;
        if protection_start > 0.0 && protection_start < horizon {
            points.push(protection_start);
        }
        for p in contract.premium_periods() {
            for boundary in [p.accrual_start, p.accrual_end] {
                let day = days(boundary, context.valuation_date) - 1.0;
                if day > 0.0 && day < horizon {
                    points.push(day);
                }
            }
        }
        if points.len() > MAX_INTEGRATION_POINTS {
            return Err(convex_core::cashflows::CashflowError::BoundExceeded(
                "CDS integration points",
            )
            .into());
        }
        points.sort_by(f64::total_cmp);
        points.dedup();
        let base = curve_value(discount, context.valuation_date, 0.0, false)?;
        let mut discounts = Vec::with_capacity(points.len());
        for day in &points {
            discounts.push(finite(
                curve_value(discount, context.valuation_date, *day, false)? / base,
            )?);
        }
        let mut payment_discounts = Vec::with_capacity(contract.premium_periods().len());
        for p in contract.premium_periods() {
            let d = if !matured && p.accrual_end > context.step_in_date {
                finite(
                    curve_value(
                        discount,
                        context.valuation_date,
                        days(p.payment_date, context.valuation_date),
                        false,
                    )? / base,
                )?
            } else {
                0.0
            };
            payment_discounts.push(d);
        }
        let settlement_discount = finite(
            curve_value(
                discount,
                context.valuation_date,
                days(context.cash_settlement_date, context.valuation_date),
                false,
            )? / base,
        )?;
        let accrued_fraction = if matured {
            0.0
        } else {
            contract.accrued_days(context.step_in_date) as f64 / contract.denominator()
        };
        Ok(Self {
            contract,
            context,
            discount,
            points,
            discounts,
            payment_discounts,
            settlement_discount,
            accrued_fraction,
            coupon: t.coupon_decimal.to_model()?.get(),
            matured,
            protection_start,
        })
    }
    pub(crate) fn flat_survival(&self, hazard: f64) -> Result<Vec<f64>, CdsError> {
        if !hazard.is_finite() || !(0.0..=5.0).contains(&hazard) {
            return Err(invalid(
                "hazard",
                "requires finite nonnegative ACT/365F intensity <= 5",
            ));
        }
        self.points
            .iter()
            .map(|d| {
                let q = (-hazard * d / 365.0).exp();
                if q <= 0.0 {
                    Err(CdsError::Numerical("survival underflow"))
                } else {
                    Ok(q)
                }
            })
            .collect()
    }
    fn supplied_survival(&self, curve: &BuiltCurve) -> Result<Vec<f64>, CdsError> {
        let base = curve_value(curve, self.context.valuation_date, 0.0, true)?;
        self.points
            .iter()
            .map(|d| finite(curve_value(curve, self.context.valuation_date, *d, true)? / base))
            .collect()
    }
    // Both log curves are exactly linear on every segment. Integrate with stable
    // exponential moments; no mid-period hazard/discount approximation is used.
    fn integrate(
        &self,
        qs: &[f64],
        start: f64,
        end: f64,
        accrual_origin: Option<f64>,
    ) -> Result<f64, CdsError> {
        if end <= start {
            return Ok(0.0);
        }
        let first = self
            .points
            .binary_search_by(|d| d.total_cmp(&start))
            .map_err(|_| CdsError::Numerical("missing integration start"))?;
        let last = self
            .points
            .binary_search_by(|d| d.total_cmp(&end))
            .map_err(|_| CdsError::Numerical("missing integration end"))?;
        let mut total = 0.0;
        for i in first..last {
            let dt = self.points[i + 1] - self.points[i];
            let hazard_mass = qs[i].ln() - qs[i + 1].ln();
            if hazard_mass < 0.0 {
                return Err(CdsError::Numerical("increasing survival"));
            }
            let exponent = hazard_mass + self.discounts[i].ln() - self.discounts[i + 1].ln();
            let (zero, first) = moments(exponent)?;
            let weight = match accrual_origin {
                None => zero,
                Some(origin) => {
                    ((self.points[i] - origin + 0.5) * zero + dt * first)
                        / self.contract.denominator()
                }
            };
            total += finite(hazard_mass * qs[i] * self.discounts[i] * weight)?;
        }
        finite(total)
    }
    pub(crate) fn components(&self, qs: &[f64]) -> Result<Components, CdsError> {
        let mut c = Components {
            scheduled: 0.0,
            default_accrual: 0.0,
            protection: 0.0,
        };
        if self.matured {
            return Ok(c);
        }
        let val = self.context.valuation_date;
        for (i, p) in self.contract.premium_periods().iter().enumerate() {
            if p.accrual_end <= self.context.step_in_date {
                continue;
            }
            let end = days(p.accrual_end, val) - 1.0;
            let index = self
                .points
                .binary_search_by(|d| d.total_cmp(&end))
                .map_err(|_| CdsError::Numerical("missing coupon survival boundary"))?;
            c.scheduled += p.accrual_start.days_between(&p.accrual_end) as f64
                / self.contract.denominator()
                * qs[index]
                * self.payment_discounts[i];
            if self.contract.terms().pay_accrual_on_default {
                let origin = days(p.accrual_start, val) - 1.0;
                c.default_accrual += self.integrate(
                    qs,
                    origin.max(self.protection_start).max(0.0),
                    end,
                    Some(origin),
                )?;
            }
        }
        c.protection = (1.0 - self.contract.terms().reference.recovery_fraction.get())
            * self.integrate(
                qs,
                self.protection_start.max(0.0),
                *self
                    .points
                    .last()
                    .ok_or(CdsError::Numerical("empty integration timeline"))?,
                None,
            )?;
        finite(c.scheduled)?;
        finite(c.default_accrual)?;
        finite(c.protection)?;
        Ok(c)
    }
    pub(crate) fn par_spread(&self, c: &Components) -> Result<f64, CdsError> {
        let annuity =
            c.scheduled + c.default_accrual - self.accrued_fraction * self.settlement_discount;
        if self.matured || annuity <= 0.0 {
            return Err(CdsError::Unsupported(
                "par/conventional spread with no positive future clean annuity",
            ));
        }
        finite(c.protection / annuity)
    }
    pub(crate) fn clean_fraction(&self, c: &Components) -> Result<f64, CdsError> {
        finite(
            (c.protection - self.coupon * (c.scheduled + c.default_accrual))
                / self.settlement_discount
                + self.coupon * self.accrued_fraction,
        )
    }
    pub(crate) fn result(
        &self,
        qs: &[f64],
        credit_input: CreditInput,
    ) -> Result<CdsValuation, CdsError> {
        let c = self.components(qs)?;
        let t = self.contract.terms();
        let val = self.context.valuation_date;
        let n = t.notional.to_model()?.get();
        let sign = t.side.sign();
        let coupon = self.coupon;
        let clean = self.clean_fraction(&c)?;
        let dirty = finite(clean - coupon * self.accrued_fraction)?;
        let clean_annuity =
            c.scheduled + c.default_accrual - self.accrued_fraction * self.settlement_discount;
        let par = if !self.matured && clean_annuity > 0.0 {
            Some(self.par_spread(&c)?)
        } else {
            None
        };
        let mut premiums = Vec::new();
        for (i, p) in self.contract.premium_periods().iter().enumerate() {
            if self.matured || p.accrual_end <= self.context.step_in_date {
                continue;
            }
            let end = days(p.accrual_end, val) - 1.0;
            let origin = days(p.accrual_start, val) - 1.0;
            let index = self
                .points
                .binary_search_by(|d| d.total_cmp(&end))
                .map_err(|_| CdsError::Numerical("missing premium boundary"))?;
            let yf =
                p.accrual_start.days_between(&p.accrual_end) as f64 / self.contract.denominator();
            let amount = finite(-sign * n * coupon * yf)?;
            let default = if t.pay_accrual_on_default {
                self.integrate(
                    qs,
                    origin.max(self.protection_start).max(0.0),
                    end,
                    Some(origin),
                )?
            } else {
                0.0
            };
            premiums.push(PremiumValue {
                accrual_start: p.accrual_start,
                accrual_end: p.accrual_end,
                payment_date: p.payment_date,
                year_fraction: yf,
                survival_probability: qs[index],
                conditional_coupon_amount: amount,
                scheduled_pv: finite(amount * qs[index] * self.payment_discounts[i])?,
                accrual_on_default_pv: finite(-sign * n * coupon * default)?,
            });
        }
        let initial = if self.context.initial_payment_state == InitialPaymentState::Pending {
            let upfront = t
                .initial_clean_upfront_fraction
                .ok_or(CdsError::Numerical("missing contracted initial cash"))?
                .to_model()?
                .get();
            let trade_accrued =
                self.contract.accrued_days(t.step_in_date) as f64 / self.contract.denominator();
            let df = curve_value(self.discount, val, days(t.cash_settlement_date, val), false)?
                / curve_value(self.discount, val, 0.0, false)?;
            finite(-sign * n * (upfront - coupon * trade_accrued) * df)?
        } else {
            0.0
        };
        let protection = finite(sign * n * c.protection)?;
        let premium = finite(-sign * n * coupon * (c.scheduled + c.default_accrual))?;
        Ok(CdsValuation {
            currency: t.currency,
            side: t.side,
            model: t.model,
            model_notice: STANDARD_MODEL_NOTICE,
            discount_curve: t.discount_curve.clone(),
            credit_input,
            valuation_date: val,
            step_in_date: self.context.step_in_date,
            cash_settlement_date: self.context.cash_settlement_date,
            protection_leg_pv: protection,
            premium_leg_pv: premium,
            initial_payment_pv: initial,
            net_pv: finite(protection + premium + initial)?,
            accrued_days: if self.matured {
                0
            } else {
                self.contract.accrued_days(self.context.step_in_date)
            },
            accrued_year_fraction: self.accrued_fraction,
            accrued_premium: finite(n * coupon * self.accrued_fraction)?,
            dirty_risky_annuity: c.scheduled + c.default_accrual,
            clean_risky_annuity: clean_annuity,
            par_spread_decimal: par,
            clean_upfront_fraction: clean,
            dirty_upfront_fraction: dirty,
            cash_settlement_amount: finite(n * dirty)?,
            clean_price_per100: finite((1.0 - clean) * 100.0)?,
            premiums,
        })
    }
}

// I0(x)=integral exp(-x*u)du; I1(x)=integral u*exp(-x*u)du on [0,1].
// Bounded series removes cancellation at x=0 and admits negative forward rates.
fn moments(x: f64) -> Result<(f64, f64), CdsError> {
    finite(x)?;
    let (zero, first) = if x.abs() < 0.01 {
        let mut zero = 0.0;
        let mut first = 0.0;
        let mut term = 1.0;
        for k in 0..16 {
            zero += term / (k + 1) as f64;
            first += term / (k + 2) as f64;
            term *= -x / (k + 1) as f64;
        }
        (zero, first)
    } else {
        let e = (-x).exp();
        (-(-x).exp_m1() / x, (-(-x).exp_m1() - x * e) / (x * x))
    };
    finite(zero)?;
    finite(first)?;
    Ok((zero, first))
}

/// Value an explicitly surviving single-name contract from checked supplied curves.
pub fn price_cds(
    contract: &CdsContract,
    context: &CdsPricingContext,
    curves: &CurveSet,
) -> Result<CdsValuation, CdsError> {
    let credit = curves.get(&contract.terms().survival_curve)?;
    let prepared = Prepared::new(
        contract,
        context,
        curves.get(&contract.terms().discount_curve)?,
        Some(credit),
    )?;
    let qs = prepared.supplied_survival(credit)?;
    prepared.result(
        &qs,
        CreditInput::Supplied {
            curve: credit.recipe().key.clone(),
        },
    )
}
/// Value under an explicitly supplied constant ACT/365F intensity. This is a
/// conventional flat model, not a calibrated multi-pillar credit curve or CS01.
pub fn price_flat_hazard(
    contract: &CdsContract,
    context: &CdsPricingContext,
    curves: &CurveSet,
    hazard_per_act365f_year: f64,
) -> Result<CdsValuation, CdsError> {
    let prepared = Prepared::new(
        contract,
        context,
        curves.get(&contract.terms().discount_curve)?,
        None,
    )?;
    prepared.result(
        &prepared.flat_survival(hazard_per_act365f_year)?,
        CreditInput::FlatHazard {
            hazard_per_act365f_year,
        },
    )
}
