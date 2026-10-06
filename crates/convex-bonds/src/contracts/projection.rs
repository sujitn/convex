//! Publication-aware cashflow projection with all-history preflight.

use super::{
    invalid, unsupported, BondContract, BondContractError, BondCoupon, CouponPlan, MAX_BOND_TERMS,
};
use convex_core::{
    cashflows::{
        BoundarySide, CashflowError, CouponCalculation, NegativeCouponPolicy,
        PaymentDateEntitlement, PrincipalCashflow,
    },
    contracts::{IndexId, UtcTimestamp},
    daycounts::DayCountConvention,
    fixings::{
        FixingContext, FixingRequirement, FixingSeries, ResolvedRate, MAX_FIXING_REQUIREMENTS,
    },
    Date,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Exact interval passed to the supplied projection policy. Observation,
/// rate-interval and settlement dates are not interchangeable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionRequest {
    /// Exact index; no discount/projection role substitution.
    pub index_id: IndexId,
    /// Fixing observation date.
    pub observation_date: Date,
    /// Start of actual reset/overnight rate interval.
    pub rate_start: Date,
    /// End of actual rate interval, including overnight weekend length.
    pub rate_end: Date,
    /// Annualized simple forward day count expected from the policy.
    pub day_count: DayCountConvention,
}

/// Forecast policy independent of the fixing publication policy.
pub enum BondProjectionPolicy<'a> {
    /// Any unpublished/future rate remains a missing input.
    RequirePublished,
    /// Supplied pure callback; failures propagate.
    Supplied(&'a mut dyn FnMut(&ProjectionRequest) -> Result<Decimal, CashflowError>),
}

/// Explicit accrual prefix end for a shifted overnight coupon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OvernightAccrualCutoff {
    /// Actual unshifted coupon accrual start.
    pub period_start: Date,
    /// Prefix end in the prepared observation-weight grid.
    pub weight_end: Date,
}

/// Supplied payment/settlement and cutoff state; no system clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BondProjectionContext {
    /// Settlement, distinct from valuation/information date.
    pub settlement: Date,
    /// Explicit pre/post payment-date state.
    pub on_payment: PaymentDateEntitlement,
    /// Publication/revision/preliminary policy and cutoff.
    pub fixing: FixingContext,
    /// Required for partial shifted overnight accrual. Unused entries fail.
    pub overnight_accrual_cutoffs: Vec<OvernightAccrualCutoff>,
}

/// Expected full coupon, entitlement and accrued cash.
#[derive(Debug, Clone)]
pub struct CouponProjection {
    /// Unadjusted start.
    pub contractual_start: Date,
    /// Unadjusted end.
    pub contractual_end: Date,
    /// Actual accrual start.
    pub accrual_start: Date,
    /// Actual accrual end.
    pub accrual_end: Date,
    /// Adjusted/delayed cash date.
    pub payment_date: Date,
    /// Full rounded absolute cash.
    pub amount: Decimal,
    /// Coupon entitlement independent of principal/rebate.
    pub entitled: bool,
    /// Rounded absolute accrued/rebate.
    pub accrued: Decimal,
    /// Exact rates/revisions and forecast reasons for term resets.
    pub rates: Vec<ResolvedRate>,
    /// Actual daily factors, weights and provenance for overnight coupons.
    pub overnight: Option<convex_core::cashflows::CompoundedCoupon>,
}

/// Expected contractual cash, not realized payment or an optional price.
#[derive(Debug, Clone)]
pub struct BondProjection {
    /// Every unpaid coupon, including ex-coupon nonentitled amounts.
    pub coupons: Vec<CouponProjection>,
    /// Entitled principal with separate effective/cash dates.
    pub principal: Vec<PrincipalCashflow>,
    /// Absolute coupon accrual/rebate sum.
    pub accrued: Decimal,
    /// Principal after settlement-date effective changes.
    pub outstanding_principal: Decimal,
    /// Conditional straight cash when early exercise is contracted.
    pub conditional_on_no_exercise: bool,
}

impl BondProjection {
    /// Entitled absolute cash in dated order. This retains actual separate coupon
    /// and principal payments and never folds repayments into averaged factors.
    pub fn cashflows(&self) -> Result<Vec<crate::traits::BondCashFlow>, BondContractError> {
        let mut flows = Vec::with_capacity(self.coupons.len() + self.principal.len());
        for coupon in &self.coupons {
            if coupon.entitled {
                flows.push(
                    crate::traits::BondCashFlow::coupon(coupon.payment_date, coupon.amount)
                        .with_accrual(coupon.accrual_start, coupon.accrual_end),
                );
            }
        }
        for principal in &self.principal {
            flows.push(crate::traits::BondCashFlow::principal(
                principal.payment_date,
                principal.amount,
            ));
        }
        flows.sort_by_key(|flow| flow.date);
        Ok(flows)
    }
}

impl BondContract {
    fn check_settlement(&self, settlement: Date) -> Result<(), BondContractError> {
        if !(1..=9999).contains(&settlement.year()) || settlement < self.terms.issue_date {
            return Err(invalid("settlement", "invalid date or before issue"));
        }
        Ok(())
    }
    fn active(
        &self,
        plan: &CouponPlan,
        settlement: Date,
        on_payment: PaymentDateEntitlement,
    ) -> bool {
        plan.period.payment_date() > settlement
            || (plan.period.payment_date() == settlement
                && on_payment == PaymentDateEntitlement::Include)
    }
    fn requests(
        &self,
        settlement: Date,
        on_payment: PaymentDateEntitlement,
    ) -> Result<BTreeMap<(IndexId, Date), ProjectionRequest>, BondContractError> {
        self.check_settlement(settlement)?;
        let mut requests = BTreeMap::new();
        for plan in &self.coupons {
            if !self.active(plan, settlement, on_payment) {
                continue;
            }
            let mut insert = |request: ProjectionRequest| -> Result<(), BondContractError> {
                let key = (request.index_id.clone(), request.observation_date);
                if requests.get(&key).is_some_and(|old| *old != request) {
                    return Err(invalid(
                        "fixings",
                        "one observation maps to conflicting projection intervals",
                    ));
                }
                requests.insert(key, request);
                if requests.len() > MAX_BOND_TERMS {
                    return Err(CashflowError::BoundExceeded("bond requirements").into());
                }
                Ok(())
            };
            for reset in &plan.resets {
                insert(reset.request.clone())?;
            }
            if let (Some(schedule), BondCoupon::Overnight(rate)) =
                (&plan.overnight, &self.terms.coupon)
            {
                for observation in schedule.observations() {
                    insert(ProjectionRequest {
                        index_id: rate.index_id.clone(),
                        observation_date: observation.observation_date,
                        rate_start: observation.observation_date,
                        rate_end: observation.rate_end,
                        day_count: self.terms.day_count,
                    })?;
                }
            }
        }
        Ok(requests)
    }
    /// All actual unpaid fixing requirements; expected publication comes from
    /// the supplied administrator schedule. Post-payment matured lists are empty.
    pub fn fixing_requirements<P>(
        &self,
        settlement: Date,
        on_payment: PaymentDateEntitlement,
        mut publication: P,
    ) -> Result<Vec<FixingRequirement>, BondContractError>
    where
        P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
    {
        self.requests(settlement, on_payment)?
            .into_values()
            .map(|request| {
                Ok(FixingRequirement {
                    expected_publication: publication(&request.index_id, request.observation_date)?,
                    index_id: request.index_id,
                    observation_date: request.observation_date,
                })
            })
            .collect()
    }
    /// Fixed/stepped/amortizing cash. Floating coupons need actual fixing inputs.
    pub fn fixed_projection(
        &self,
        settlement: Date,
        on_payment: PaymentDateEntitlement,
    ) -> Result<BondProjection, BondContractError> {
        if !matches!(self.terms.coupon, BondCoupon::Fixed) {
            return Err(unsupported(
                "floating coupon",
                "requires publication-aware history/projection",
            ));
        }
        self.check_settlement(settlement)?;
        self.assemble(settlement, on_payment, &[], &BTreeMap::new(), None)
    }
    /// Preflight the ENTIRE missing history list before any forecast. Core
    /// resolution uses bounded batches and retains actual revision origins.
    pub fn project<P>(
        &self,
        context: &BondProjectionContext,
        history: &FixingSeries,
        mut publication: P,
        mut policy: BondProjectionPolicy<'_>,
    ) -> Result<BondProjection, BondContractError>
    where
        P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
    {
        let requests = self.requests(context.settlement, context.on_payment)?;
        let requirements: Vec<_> = requests
            .values()
            .map(|request| {
                Ok(FixingRequirement {
                    expected_publication: publication(&request.index_id, request.observation_date)?,
                    index_id: request.index_id.clone(),
                    observation_date: request.observation_date,
                })
            })
            .collect::<Result<_, CashflowError>>()?;
        let mut resolutions = Vec::new();
        let mut missing = Vec::new();
        for batch in requirements.chunks(MAX_FIXING_REQUIREMENTS) {
            let resolution = history.resolve(batch, context.fixing)?;
            missing.extend_from_slice(resolution.missing());
            resolutions.push(resolution);
        }
        if !missing.is_empty() {
            return Err(CashflowError::MissingFixings(missing).into());
        }
        let mut rates = BTreeMap::new();
        for resolution in resolutions {
            for rate in resolution.project(|requirement| {
                let request = requests
                    .get(&(requirement.index_id.clone(), requirement.observation_date))
                    .ok_or_else(|| CashflowError::InvalidTerms {
                        field: "projection",
                        reason: "missing rate interval".into(),
                    })?;
                match &mut policy {
                    BondProjectionPolicy::RequirePublished => {
                        Err(CashflowError::MissingFixings(vec![requirement.clone()]))
                    }
                    BondProjectionPolicy::Supplied(forecast) => forecast(request),
                }
            })? {
                rates.insert(
                    (
                        rate.requirement.index_id.clone(),
                        rate.requirement.observation_date,
                    ),
                    rate,
                );
            }
        }
        self.assemble(
            context.settlement,
            context.on_payment,
            &context.overnight_accrual_cutoffs,
            &rates,
            Some((history, context.fixing)),
        )
    }
    fn calculation(&self, index_rate: Decimal) -> CouponCalculation {
        CouponCalculation {
            index_rate,
            day_count: self.terms.day_count,
            principal: self.terms.coupon_principal,
            rounding: self.terms.rounding,
            negative_coupons: self.terms.negative_coupons,
        }
    }
    fn term_amount(
        &self,
        plan: &CouponPlan,
        end: Date,
        rates: &BTreeMap<(IndexId, Date), ResolvedRate>,
    ) -> Result<Decimal, BondContractError> {
        if matches!(self.terms.coupon, BondCoupon::Fixed) {
            return Ok(self.profiles.segment_amount(
                &plan.period,
                plan.period.accrual_dates().0,
                end,
                self.calculation(Decimal::ZERO),
            )?);
        }
        let mut amount = Decimal::ZERO;
        for reset in &plan.resets {
            let right = reset.end.min(end);
            if right <= reset.start {
                continue;
            }
            let rate = rates
                .get(&(
                    reset.request.index_id.clone(),
                    reset.request.observation_date,
                ))
                .ok_or_else(|| invalid("rate", "unresolved term reset"))?;
            let segment = self.profiles.segment_amount(
                &plan.period,
                reset.start,
                right,
                self.calculation(rate.rate),
            )?;
            amount = amount
                .checked_add(segment)
                .ok_or(CashflowError::Arithmetic("reset coupon sum"))?;
        }
        Ok(amount)
    }
    fn assemble(
        &self,
        settlement: Date,
        on_payment: PaymentDateEntitlement,
        cutoffs: &[OvernightAccrualCutoff],
        rates: &BTreeMap<(IndexId, Date), ResolvedRate>,
        fixing: Option<(&FixingSeries, FixingContext)>,
    ) -> Result<BondProjection, BondContractError> {
        if cutoffs.len() > self.coupons.len()
            || cutoffs
                .windows(2)
                .any(|p| p[0].period_start >= p[1].period_start)
        {
            return Err(invalid(
                "overnight_accrual_cutoffs",
                "requires bounded sorted unique coupon starts",
            ));
        }
        let mut used_cutoffs = 0;
        let mut coupons = Vec::new();
        let mut accrued = Decimal::ZERO;
        for plan in &self.coupons {
            if !self.active(plan, settlement, on_payment) {
                continue;
            }
            let (start, end) = plan.period.accrual_dates();
            let mut coupon_rates = Vec::new();
            for reset in &plan.resets {
                let rate = rates
                    .get(&(
                        reset.request.index_id.clone(),
                        reset.request.observation_date,
                    ))
                    .ok_or_else(|| invalid("rate", "unresolved reset"))?;
                if !coupon_rates
                    .iter()
                    .any(|r: &ResolvedRate| r.requirement == rate.requirement)
                {
                    coupon_rates.push(rate.clone());
                }
            }
            let overnight = match (&plan.overnight, &self.terms.coupon) {
                (Some(schedule), BondCoupon::Overnight(rate)) => {
                    let (history, context) = fixing
                        .ok_or_else(|| invalid("fixings", "required for overnight coupon"))?;
                    Some(schedule.compound(
                        &rate.index_id,
                        history,
                        context,
                        |date| {
                            rates
                                .get(&(rate.index_id.clone(), date))
                                .map(|r| r.requirement.expected_publication)
                                .ok_or_else(|| CashflowError::InvalidTerms {
                                    field: "publication",
                                    reason: "unresolved date".into(),
                                })
                        },
                        |date, _| {
                            rates
                                .get(&(rate.index_id.clone(), date))
                                .map(|r| r.rate)
                                .ok_or_else(|| CashflowError::InvalidTerms {
                                    field: "projection",
                                    reason: "unresolved date".into(),
                                })
                        },
                    )?)
                }
                _ => None,
            };
            let (full, partial) = if let Some(compound) = &overnight {
                let partial_end = settlement.max(start).min(end);
                let weight_end = if partial_end == end {
                    None
                } else if partial_end == start {
                    Some(
                        compound
                            .observations
                            .first()
                            .ok_or_else(|| invalid("overnight", "empty"))?
                            .dates
                            .weight_start,
                    )
                } else if matches!(&self.terms.coupon, BondCoupon::Overnight(rate) if rate.convention.observation_shift)
                {
                    let cutoff = cutoffs
                        .iter()
                        .find(|c| c.period_start == start)
                        .ok_or_else(|| {
                            unsupported(
                                "shifted overnight accrued",
                                "supply explicit observation-weight prefix end",
                            )
                        })?;
                    used_cutoffs += 1;
                    Some(cutoff.weight_end)
                } else {
                    Some(partial_end)
                };
                (
                    self.overnight_amount(plan, compound, end, None)?,
                    self.overnight_amount(plan, compound, partial_end, weight_end)?,
                )
            } else {
                (
                    self.term_amount(plan, end, rates)?,
                    self.term_amount(plan, settlement.max(start).min(end), rates)?,
                )
            };
            let mut partial = if settlement <= start || settlement >= plan.period.payment_date() {
                Decimal::ZERO
            } else {
                partial
            };
            if settlement > start
                && settlement < plan.period.payment_date()
                && plan.period.is_ex_coupon(settlement)
            {
                partial = match self.terms.ex_coupon_accrual {
                    convex_core::cashflows::ExCouponAccrual::SubtractCoupon => partial
                        .checked_sub(full)
                        .ok_or(CashflowError::Arithmetic("ex-coupon rebate"))?,
                    convex_core::cashflows::ExCouponAccrual::Zero => Decimal::ZERO,
                };
            }
            let partial = self.terms.rounding.apply(partial)?;
            accrued = accrued
                .checked_add(partial)
                .ok_or(CashflowError::Arithmetic("bond accrued sum"))?;
            coupons.push(CouponProjection {
                contractual_start: plan.period.contractual_dates().0,
                contractual_end: plan.period.contractual_dates().1,
                accrual_start: start,
                accrual_end: end,
                payment_date: plan.period.payment_date(),
                amount: self.terms.rounding.apply(full)?,
                entitled: plan.period.is_entitled(settlement, on_payment),
                accrued: partial,
                rates: coupon_rates,
                overnight,
            });
        }
        if used_cutoffs != cutoffs.len() {
            return Err(invalid(
                "overnight_accrual_cutoffs",
                "unused cutoff hides mismatched coupon terms",
            ));
        }
        Ok(BondProjection {
            coupons,
            principal: self
                .principal
                .iter()
                .filter(|p| p.is_entitled(settlement, on_payment))
                .cloned()
                .collect(),
            accrued,
            outstanding_principal: self.outstanding_principal(settlement, BoundarySide::After)?,
            conditional_on_no_exercise: !self.terms.exercise.is_empty(),
        })
    }
    fn overnight_amount(
        &self,
        plan: &CouponPlan,
        compound: &convex_core::cashflows::CompoundedCoupon,
        accrual_end: Date,
        weight_end: Option<Date>,
    ) -> Result<Decimal, BondContractError> {
        let (start, _) = plan.period.accrual_dates();
        // Validate effective negative-coupon policy; actual index cash uses the
        // compound product, never an averaged daily rate.
        self.profiles.segment_amount(
            &plan.period,
            start,
            accrual_end,
            self.calculation(compound.annualized_rate),
        )?;
        let principal = self
            .profiles
            .notional()
            .value_at(start, BoundarySide::After)?;
        let gearing = self
            .terms
            .profiles
            .gearing
            .iter()
            .find(|p| p.start <= start && p.end > start)
            .ok_or_else(|| invalid("gearing", "missing coupon coverage"))?
            .value;
        let mut factor = Decimal::ONE;
        if let Some(cutoff) = weight_end {
            let first = compound
                .observations
                .first()
                .ok_or_else(|| invalid("overnight", "empty"))?
                .dates
                .weight_start;
            let last = compound
                .observations
                .last()
                .ok_or_else(|| invalid("overnight", "empty"))?
                .dates
                .weight_end;
            if cutoff < first || cutoff > last {
                return Err(invalid(
                    "overnight_accrual_cutoff",
                    "outside prepared weights",
                ));
            }
            for observation in &compound.observations {
                let end = observation.dates.weight_end.min(cutoff);
                if end <= observation.dates.weight_start {
                    break;
                }
                let fraction = self
                    .terms
                    .day_count
                    .to_day_count()
                    .year_fraction(observation.dates.weight_start, end);
                let term = observation
                    .rate
                    .checked_mul(fraction)
                    .and_then(|x| x.checked_add(Decimal::ONE))
                    .ok_or(CashflowError::Arithmetic("overnight accrued factor"))?;
                if term <= Decimal::ZERO {
                    return Err(invalid("overnight_factor", "nonpositive prefix term"));
                }
                factor = factor
                    .checked_mul(term)
                    .ok_or(CashflowError::Arithmetic("overnight accrued product"))?;
            }
        } else {
            factor = compound.period_factor;
        }
        let index_cash = factor
            .checked_sub(Decimal::ONE)
            .and_then(|x| x.checked_mul(gearing))
            .and_then(|x| x.checked_mul(principal))
            .ok_or(CashflowError::Arithmetic("overnight coupon"))?;
        let mut simple = self.calculation(Decimal::ZERO);
        simple.negative_coupons = NegativeCouponPolicy::Allow;
        let simple_cash = self
            .profiles
            .segment_amount(&plan.period, start, accrual_end, simple)?;
        let total = index_cash
            .checked_add(simple_cash)
            .ok_or(CashflowError::Arithmetic("overnight spread coupon"))?;
        if self.terms.negative_coupons == NegativeCouponPolicy::Reject && total < Decimal::ZERO {
            return Err(invalid(
                "coupon",
                "negative overnight amount rejected by contract",
            ));
        }
        Ok(total)
    }
}
