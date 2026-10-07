//! Expected signed cash over immutable supplied history and explicit forecasts.

use convex_core::{
    cashflows::{CashflowError, CompoundedCoupon, CouponPeriod, PaymentDateEntitlement},
    contracts::{IndexId, UtcTimestamp},
    daycounts::DayCountConvention,
    fixings::{
        FixingContext, FixingRequirement, FixingSeries, RateOrigin, ResolvedRate,
        MAX_FIXING_REQUIREMENTS,
    },
    Currency, Date,
};
use convex_curves::contracts::CurveKey;
use rust_decimal::Decimal;
use std::collections::BTreeMap;

use crate::{
    contract::calculation, date, invalid, CollateralTerms, ResetSlice, SwapContract, SwapError,
    SwapTerms, MAX_SWAP_TERMS,
};

/// Supplied time, eligibility and same-day payment ownership. Cash before the
/// valuation date is excluded; this projection does not assert it was paid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashflowContext {
    /// Complete valuation/known-at/history policy; must match contract policy.
    pub fixings: FixingContext,
    /// Include/exclude expected coupons on valuation date explicitly.
    pub on_payment: PaymentDateEntitlement,
}

/// Actual pure forecast request, with no tenor-to-date or index-name inference.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProjectionRequest {
    /// Exact index/date/publication requirement, distinct from rate interval.
    pub requirement: FixingRequirement,
    /// Full index value-date interval start.
    pub rate_start: Date,
    /// Full index value-date interval end, including a holiday span.
    pub rate_end: Date,
    /// Simple annualized decimal quotation day count.
    pub day_count: DayCountConvention,
    /// Term index tenor in months; absent for overnight.
    pub tenor_months: Option<u32>,
    /// Exact required index projection dependency.
    pub projection_curve: CurveKey,
}

/// Supplied annualized decimal forecast and exact source identity/version.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ForecastRate {
    /// Annualized decimal rate, checked in ±10 including negatives.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub rate: Decimal,
    /// Caller-bound exact curve version; must match the requested dependency.
    pub projection_curve: CurveKey,
}

/// Every actually consumed unique rate, with exact published revision or forecast
/// reason. Published rates never acquire a projection curve lineage.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RateUse {
    /// Original actual interval and exact dependency/publication requirement.
    pub request: ProjectionRequest,
    /// Annualized decimal value and publication/revision or projection reason.
    pub resolved: ResolvedRate,
    /// Exact supplied forecast source; absent for published observations.
    pub projection_source: Option<CurveKey>,
}

/// Term observation consumed by one coupon slice, preserving actual accrual DC.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ResolvedResetSlice {
    /// Actual independent reset/value/observation and coupon-slice dates.
    pub dates: ResetSlice,
    /// Coupon slice year fraction, distinct from the index quotation fraction.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub year_fraction: Decimal,
    /// Exact resolved fixing/projection rate.
    pub rate: ResolvedRate,
}

/// Cash calculation evidence; this is not an option valuation or realized cash.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum CouponEvidence {
    /// Deterministic independently dated fixed rate/notional profile.
    Fixed,
    /// Arithmetic simple-accrual reset slices, not compounding.
    Term(Vec<ResolvedResetSlice>),
    /// Every actual weighted overnight observation and compound factor.
    Overnight(CompoundedCoupon),
}

/// One expected coupon in absolute contract-currency units. Negative rates may
/// reverse payment direction; no implicit floor or notional exchange occurs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CouponCashflow {
    /// Distinct contractual/accrual/payment/reference dates.
    pub period: CouponPeriod,
    /// Full unshifted coupon year fraction.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub year_fraction: Decimal,
    /// Unrounded cash before applying trade direction.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub unrounded_amount: Decimal,
    /// Contract-rounded cash before applying trade direction.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub amount: Decimal,
    /// Rounded signed cash from this payer/receiver trade's perspective.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub signed_amount: Decimal,
    /// Actual rate/weight/reset calculation evidence.
    pub evidence: CouponEvidence,
}

/// Complete expected cash projection. It contains no discounted PV, fair rate,
/// risk, recorded payment, amendment or termination effect.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SwapCashflows {
    /// Exact economic input version and all immutable calendar/curve pins.
    pub terms: SwapTerms,
    /// Supplied historical cutoff and payment-date state.
    pub context: CashflowContext,
    /// Absolute cash currency.
    pub currency: Currency,
    /// Agreement and discount dependency required by the subsequent pricer.
    pub collateral: CollateralTerms,
    /// Expected fixed coupons in chronological period order.
    pub fixed: Vec<CouponCashflow>,
    /// Expected floating coupons in chronological period order.
    pub floating: Vec<CouponCashflow>,
    /// Unique consumed rates in observation-date order; future history never leaks.
    pub rates: Vec<RateUse>,
}

impl SwapContract {
    /// Crystallize only coupons whose original accrual ends on this day. Delayed
    /// payment dates and full native history/rounding evidence remain unchanged.
    pub fn completed_cashflows<P>(
        &self,
        context: CashflowContext,
        history: &FixingSeries,
        publication: P,
    ) -> Result<SwapCashflows, SwapError>
    where
        P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
    {
        if context.on_payment != PaymentDateEntitlement::Include {
            return Err(invalid("completed_cash", "include same-day cash"));
        }
        let mut completed = self.clone();
        completed
            .floating
            .retain(|c| c.period.accrual_dates().1 == context.fixings.valuation_date);
        let mut result = completed.cashflows(context, history, publication, |r| {
            Err(CashflowError::MissingFixings(vec![r.requirement.clone()]))
        })?;
        result
            .fixed
            .retain(|c| c.period.accrual_dates().1 == context.fixings.valuation_date);
        Ok(result)
    }

    /// Original signed cash due on exactly the supplied valuation day. Only due
    /// floating coupons consume history; unpublished rates cannot become payable
    /// forecast cash. This neither discounts nor records any payment.
    pub fn due_cashflows<P>(
        &self,
        context: CashflowContext,
        history: &FixingSeries,
        publication: P,
    ) -> Result<SwapCashflows, SwapError>
    where
        P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
    {
        if context.on_payment != PaymentDateEntitlement::Include {
            return Err(invalid("due_cash", "include same-day cash explicitly"));
        }
        let mut due = self.clone();
        due.floating
            .retain(|c| c.period.payment_date() == context.fixings.valuation_date);
        let mut result = due.cashflows(context, history, publication, |r| {
            Err(CashflowError::MissingFixings(vec![r.requirement.clone()]))
        })?;
        result
            .fixed
            .retain(|c| c.period.payment_date() == context.fixings.valuation_date);
        Ok(result)
    }

    /// Discover all unique outstanding floating requirements before valuation.
    /// Publication times are supplied from the actual administrator schedule;
    /// each unique observation calls the publication function once.
    pub fn fixing_requirements<P>(
        &self,
        context: CashflowContext,
        publication: P,
    ) -> Result<Vec<FixingRequirement>, SwapError>
    where
        P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
    {
        Ok(self
            .requests(context, publication)?
            .into_values()
            .map(|r| r.requirement)
            .collect())
    }

    /// Generate validated expected leg cashflows. Preflight the entire leg's
    /// required history before invoking ANY forecast. Forecast each permitted
    /// unique observation once; published revisions are copied from history.
    /// Custom callbacks must terminate without panic and have no hidden clock.
    pub fn cashflows<P, F>(
        &self,
        context: CashflowContext,
        history: &FixingSeries,
        publication: P,
        forecast: F,
    ) -> Result<SwapCashflows, SwapError>
    where
        P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
        F: FnMut(&ProjectionRequest) -> Result<ForecastRate, CashflowError>,
    {
        let requests = self.requests(context, publication)?;
        let uses = resolve_rates(&requests, context, history, forecast)?;
        let fixed_sign = self.terms.side.fixed_sign();
        let mut fixed = Vec::new();
        for period in self
            .fixed_schedule
            .periods()
            .iter()
            .filter(|p| outstanding(p, context))
        {
            let mut c = calculation(&self.terms.fixed.leg, Decimal::ZERO);
            c.rounding = convex_core::cashflows::CashRounding::Unrounded;
            let unrounded = self.fixed_profiles.coupon_amount(period, c)?;
            fixed.push(cashflow(
                period,
                unrounded,
                fixed_sign,
                &self.terms.fixed.leg,
                CouponEvidence::Fixed,
            )?);
        }
        let mut floating = Vec::new();
        for plan in self
            .floating
            .iter()
            .filter(|p| outstanding(&p.period, context))
        {
            floating.push(self.floating_coupon(plan, context, history, &uses, -fixed_sign)?);
        }
        Ok(SwapCashflows {
            terms: self.terms.clone(),
            context,
            currency: self.terms.currency,
            collateral: self.terms.collateral.clone(),
            fixed,
            floating,
            rates: uses.into_values().collect(),
        })
    }

    fn floating_coupon(
        &self,
        plan: &crate::FloatingCouponPlan,
        context: CashflowContext,
        history: &FixingSeries,
        uses: &BTreeMap<Date, RateUse>,
        sign: Decimal,
    ) -> Result<CouponCashflow, SwapError> {
        let leg = self.terms.floating.leg();
        let mut amount = Decimal::ZERO;
        let evidence = if let Some(schedule) = &plan.overnight {
            // Reuse the single core compounding implementation with the
            // already frozen eligible resolutions. This invokes no new forecast.
            let coupon = schedule.compound(
                self.terms.floating.index_id(),
                history,
                context.fixings,
                |at| {
                    Ok(uses
                        .get(&at)
                        .ok_or_else(|| cf_invalid("observation", "missing publication"))?
                        .request
                        .requirement
                        .expected_publication)
                },
                |at, _| {
                    Ok(uses
                        .get(&at)
                        .ok_or_else(|| cf_invalid("observation", "missing resolved rate"))?
                        .resolved
                        .rate)
                },
            )?;
            let mut c = calculation(leg, coupon.annualized_rate);
            c.rounding = convex_core::cashflows::CashRounding::Unrounded;
            amount = self.floating_profiles.coupon_amount(&plan.period, c)?;
            CouponEvidence::Overnight(coupon)
        } else {
            let mut slices = Vec::with_capacity(plan.resets.len());
            for reset in &plan.resets {
                let rate = uses
                    .get(&reset.observation_date)
                    .ok_or_else(|| invalid("fixing", "missing resolved reset"))?;
                let slice = self.floating_profiles.segment_amount(
                    &plan.period,
                    reset.accrual_start,
                    reset.accrual_end,
                    calculation(leg, rate.resolved.rate),
                )?;
                amount = amount
                    .checked_add(slice)
                    .ok_or(CashflowError::Arithmetic("term coupon sum"))?;
                slices.push(ResolvedResetSlice {
                    dates: reset.clone(),
                    year_fraction: plan.period.year_fraction(
                        reset.accrual_start,
                        reset.accrual_end,
                        leg.day_count,
                    )?,
                    rate: rate.resolved.clone(),
                });
            }
            CouponEvidence::Term(slices)
        };
        cashflow(&plan.period, amount, sign, leg, evidence)
    }

    fn requests<P>(
        &self,
        context: CashflowContext,
        mut publication: P,
    ) -> Result<BTreeMap<Date, ProjectionRequest>, SwapError>
    where
        P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
    {
        date(context.fixings.valuation_date)?;
        let policy = self.terms.floating.fixing_policy();
        if policy.final_only != context.fixings.final_only
            || policy.unpublished != context.fixings.unpublished
        {
            return Err(invalid(
                "fixing_policy",
                "context must match exact contract policy",
            ));
        }
        let mut requests = BTreeMap::<Date, ProjectionRequest>::new();
        for plan in self
            .floating
            .iter()
            .filter(|p| outstanding(&p.period, context))
        {
            let mut intervals = Vec::new();
            for r in &plan.resets {
                intervals.push((
                    r.observation_date,
                    r.rate_start,
                    r.rate_end,
                    r.projection_day_count,
                    Some(r.tenor_months),
                ));
            }
            if let Some(s) = &plan.overnight {
                for o in s.observations() {
                    intervals.push((
                        o.observation_date,
                        o.observation_date,
                        o.rate_end,
                        self.terms.floating.leg().day_count,
                        None,
                    ));
                }
            }
            for (at, start, end, day_count, tenor_months) in intervals {
                if let Some(old) = requests.get(&at) {
                    if (
                        old.rate_start,
                        old.rate_end,
                        old.day_count,
                        old.tenor_months,
                    ) != (start, end, day_count, tenor_months)
                    {
                        return Err(invalid(
                            "observation",
                            "one observation has conflicting quotation intervals",
                        ));
                    }
                    continue;
                }
                if requests.len() == MAX_SWAP_TERMS {
                    return Err(CashflowError::BoundExceeded("swap fixing requirements").into());
                }
                let index_id = self.terms.floating.index_id();
                let expected_publication = publication(index_id, at)?;
                if expected_publication.datetime().date_naive() < at.as_naive_date() {
                    return Err(invalid(
                        "publication",
                        "expected publication precedes observation",
                    ));
                }
                requests.insert(
                    at,
                    ProjectionRequest {
                        requirement: FixingRequirement {
                            index_id: index_id.clone(),
                            observation_date: at,
                            expected_publication,
                        },
                        rate_start: start,
                        rate_end: end,
                        day_count,
                        tenor_months,
                        projection_curve: self.terms.floating.projection_curve().clone(),
                    },
                );
            }
        }
        Ok(requests)
    }
}

fn resolve_rates<F>(
    requests: &BTreeMap<Date, ProjectionRequest>,
    context: CashflowContext,
    history: &FixingSeries,
    mut forecast: F,
) -> Result<BTreeMap<Date, RateUse>, SwapError>
where
    F: FnMut(&ProjectionRequest) -> Result<ForecastRate, CashflowError>,
{
    let requirements: Vec<_> = requests.values().map(|r| r.requirement.clone()).collect();
    let mut resolutions = Vec::new();
    let mut missing = Vec::new();
    for chunk in requirements.chunks(MAX_FIXING_REQUIREMENTS) {
        let resolution = history.resolve(chunk, context.fixings)?;
        missing.extend_from_slice(resolution.missing());
        resolutions.push(resolution);
    }
    if !missing.is_empty() {
        return Err(CashflowError::MissingFixings(missing).into());
    }
    let mut uses = BTreeMap::new();
    for resolution in resolutions {
        let rates = resolution.project(|r| {
            let request = requests
                .get(&r.observation_date)
                .ok_or_else(|| cf_invalid("observation", "missing prepared request"))?;
            let projected = forecast(request)?;
            if projected.projection_curve != request.projection_curve {
                return Err(cf_invalid(
                    "projection_curve",
                    "exact source identity/version mismatch",
                ));
            }
            Ok(projected.rate)
        })?;
        for resolved in rates {
            let request = requests
                .get(&resolved.requirement.observation_date)
                .ok_or_else(|| invalid("observation", "missing request"))?
                .clone();
            let projection_source = match resolved.origin {
                RateOrigin::Projected(_) => Some(request.projection_curve.clone()),
                RateOrigin::Published(_) => None,
            };
            uses.insert(
                resolved.requirement.observation_date,
                RateUse {
                    request,
                    resolved,
                    projection_source,
                },
            );
        }
    }
    Ok(uses)
}

fn outstanding(period: &CouponPeriod, context: CashflowContext) -> bool {
    period.payment_date() > context.fixings.valuation_date
        || (period.payment_date() == context.fixings.valuation_date
            && context.on_payment == PaymentDateEntitlement::Include)
}
fn cashflow(
    period: &CouponPeriod,
    unrounded_amount: Decimal,
    sign: Decimal,
    leg: &crate::LegTerms,
    evidence: CouponEvidence,
) -> Result<CouponCashflow, SwapError> {
    let amount = leg.rounding.apply(unrounded_amount)?;
    Ok(CouponCashflow {
        period: period.clone(),
        year_fraction: period.year_fraction(
            period.accrual_dates().0,
            period.accrual_dates().1,
            leg.day_count,
        )?,
        unrounded_amount,
        amount,
        signed_amount: amount
            .checked_mul(sign)
            .ok_or(CashflowError::Arithmetic("signed cash"))?,
        evidence,
    })
}
fn cf_invalid(field: &'static str, reason: &str) -> CashflowError {
    CashflowError::InvalidTerms {
        field,
        reason: reason.to_owned(),
    }
}
