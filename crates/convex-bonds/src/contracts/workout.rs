//! Hypothetical fixed-price exercise cash over original checked bond economics.
use super::{
    invalid, unsupported, BondContract, BondContractError, BondCoupon, BondProjection,
    BondProjectionContext, BondProjectionPolicy, CashflowError, Date, Decimal, ExerciseAccrued,
    ExerciseDates, PrincipalCashflow,
};
use crate::types::CalendarId;
use convex_core::{
    calendars::Calendar,
    cashflows::{BoundarySide, PaymentDateEntitlement, PrincipalChange},
    contracts::{IndexId, UtcTimestamp},
    fixings::FixingSeries,
};
use std::collections::{BTreeMap, BTreeSet};

/// Full cash composition for one permitted early exercise. No exercise event,
/// option value, realized receipt or authority is inferred.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkoutProjection {
    /// Complete original unpaid cash and purchase-date accrual/entitlement.
    pub original: BondProjection,
    /// Completed coupon/principal claims and actual early redemption cash.
    pub cash: BondProjection,
    /// Original distinct notice, exercise and redemption settlement dates.
    pub dates: ExerciseDates,
    /// Principal after same-day scheduled reductions, before early redemption.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub outstanding_principal: Decimal,
    /// Unrounded original exercise price times the remaining principal.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub redemption: Decimal,
    /// Separately paid rounded original partial coupon, zero if included in price.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub accrued_cash: Decimal,
    /// Once-rounded combined redemption and partial coupon at cash settlement.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub settlement_cash: Decimal,
}

/// Actual permitted exercise and optional original shifted overnight weight end.
pub struct WorkoutExercise<'a> {
    /// Source index in the complete original exercise array.
    pub provision: usize,
    /// Contractual exercise, distinct from cash settlement.
    pub date: Date,
    /// Exact original exercise calendar identity.
    pub calendar_id: &'a CalendarId,
    /// Actual supplied independent notice/exercise calendar.
    pub calendar: &'a dyn Calendar,
    /// Required only for an actual partial shifted overnight PayAccrued coupon.
    pub weight_end: Option<Date>,
}

/// Checked original floating cash frozen once for every conditional path. Private
/// state cannot be decoded or populated with caller-created cash or rates.
#[derive(Debug, Clone)]
pub struct PreparedFloatingWorkout {
    contract: BondContract,
    settlement: Date,
    original: BondProjection,
}
impl PreparedFloatingWorkout {
    /// Complete original unpaid cash, purchase accrued and known/forecast origins.
    pub fn original(&self) -> &BondProjection {
        &self.original
    }
    /// Compose a legal conditional path over the same frozen observations.
    pub fn exercise(
        &self,
        selection: &WorkoutExercise<'_>,
    ) -> Result<WorkoutProjection, BondContractError> {
        self.contract
            .compose_workout(self.settlement, self.original.clone(), selection)
    }
}

impl BondContract {
    /// Full original fixed/stepped/amortizing exercise cash. Completed amounts
    /// retain their actual delayed payment dates, even after redemption. Scheduled
    /// principal changes on the exercise day precede retirement of the remainder,
    /// matching the native lifecycle order. The original face remains the quote
    /// denominator. Floating/trigger/make-whole and ambiguous ex-coupon ownership
    /// require their own complete contractual quotation path.
    pub fn fixed_workout_projection(
        &self,
        settlement: Date,
        on_payment: PaymentDateEntitlement,
        provision: usize,
        exercise: Date,
        calendar_id: &CalendarId,
        calendar: &dyn Calendar,
    ) -> Result<WorkoutProjection, BondContractError> {
        let original = self.fixed_projection(settlement, on_payment)?;
        self.compose_workout(
            settlement,
            original,
            &WorkoutExercise {
                provision,
                date: exercise,
                calendar_id,
                calendar,
                weight_end: None,
            },
        )
    }
    /// Resolve full original unpaid floating cash exactly once, including all
    /// known revisions and permitted future resets. Optional cash remains
    /// conditional; this is neither a stochastic price nor executed exercise.
    pub fn floating_workouts<P>(
        &self,
        context: &BondProjectionContext,
        history: &FixingSeries,
        publication: P,
        policy: BondProjectionPolicy<'_>,
    ) -> Result<PreparedFloatingWorkout, BondContractError>
    where
        P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
    {
        if matches!(self.terms.coupon, BondCoupon::Fixed) {
            return Err(unsupported(
                "floating workout",
                "actual term or overnight coupon required",
            ));
        }
        Ok(PreparedFloatingWorkout {
            contract: self.clone(),
            settlement: context.settlement,
            original: self.project(context, history, publication, policy)?,
        })
    }
    fn compose_workout(
        &self,
        settlement: Date,
        original: BondProjection,
        selection: &WorkoutExercise<'_>,
    ) -> Result<WorkoutProjection, BondContractError> {
        let WorkoutExercise {
            provision,
            date: exercise,
            calendar_id,
            calendar,
            weight_end,
        } = selection;
        let (provision, exercise) = (*provision, *exercise);
        if *calendar_id != &self.terms.calendars.exercise {
            return Err(invalid(
                "workout.calendar",
                "exact original exercise calendar required",
            ));
        }
        if exercise <= settlement || exercise >= self.terms.schedule.end_date {
            return Err(invalid(
                "workout.date",
                "strictly after purchase settlement and before maturity required",
            ));
        }
        let terms =
            self.terms.exercise.get(provision).ok_or_else(|| {
                invalid("workout.provision", "original exercise provision absent")
            })?;
        let dates = terms.dates(exercise, *calendar)?;
        if dates.notice < self.terms.issue_date || dates.settlement < exercise {
            return Err(invalid(
                "workout.dates",
                "notice after issue and redemption after exercise required",
            ));
        }
        let outstanding = self.outstanding_principal(exercise, BoundarySide::After)?;
        if outstanding <= Decimal::ZERO {
            return Err(invalid(
                "workout.principal",
                "positive remaining exercise principal required",
            ));
        }
        let redemption = outstanding
            .checked_mul(terms.price_per_100)
            .and_then(|v| v.checked_div(Decimal::from(100)))
            .ok_or(CashflowError::Arithmetic("workout redemption"))?;
        let mut cash = original.clone();
        cash.coupons.retain(|c| c.accrual_end <= exercise);
        cash.principal.retain(|p| p.effective_date <= exercise);
        let mut accrued_cash = Decimal::ZERO;
        if terms.accrued == ExerciseAccrued::PayAccrued {
            let plans: Vec<_> = self
                .coupons
                .iter()
                .filter(|c| {
                    let (a, b) = c.period.accrual_dates();
                    a < exercise && exercise < b
                })
                .collect();
            if plans.len() > 1 {
                return Err(unsupported(
                    "workout partial coupon",
                    "overlapping actual accrual intervals require explicit allocation",
                ));
            }
            if let Some(plan) = plans.first() {
                let (start, end) = plan.period.accrual_dates();
                let original_coupon = original
                    .coupons
                    .iter()
                    .find(|c| c.accrual_start == start && c.accrual_end == end)
                    .ok_or_else(|| {
                        unsupported(
                            "workout partial coupon",
                            "previously paid coupon requires explicit exercise ownership",
                        )
                    })?;
                if !original_coupon.entitled {
                    return Err(unsupported(
                        "workout ex-coupon ownership",
                        "reserved coupon ownership needs an explicit early-redemption agreement",
                    ));
                }
                let amount = if let (Some(compound), BondCoupon::Overnight(rate)) =
                    (&original_coupon.overnight, &self.terms.coupon)
                {
                    let cutoff = if rate.convention.observation_shift {
                        weight_end.ok_or_else(|| {
                            invalid(
                                "workout.weight_end",
                                "partial shifted overnight accrued requires original weight cutoff",
                            )
                        })?
                    } else {
                        if weight_end.is_some() {
                            return Err(invalid("workout.weight_end", "unused unshifted cutoff"));
                        }
                        exercise
                    };
                    plan.overnight
                        .as_ref()
                        .ok_or_else(|| {
                            invalid("workout.prefix", "original overnight schedule absent")
                        })?
                        .prefix(exercise, cutoff)?;
                    self.overnight_amount(plan, compound, exercise, Some(cutoff))?
                } else {
                    if weight_end.is_some() {
                        return Err(invalid("workout.weight_end", "unused non-overnight cutoff"));
                    }
                    let rates = original_coupon
                        .rates
                        .iter()
                        .map(|r| {
                            (
                                (
                                    r.requirement.index_id.clone(),
                                    r.requirement.observation_date,
                                ),
                                r.clone(),
                            )
                        })
                        .collect::<BTreeMap<_, _>>();
                    self.term_amount(plan, exercise, &rates)?
                };
                accrued_cash = self.terms.rounding.apply(amount)?;
                if !accrued_cash.is_zero() {
                    let (start, end) = plan.period.accrual_dates();
                    let mut partial = original
                        .coupons
                        .iter()
                        .find(|c| c.accrual_start == start && c.accrual_end == end)
                        .cloned()
                        .ok_or_else(|| {
                            unsupported(
                                "workout partial coupon",
                                "previously paid coupon requires explicit exercise ownership",
                            )
                        })?;
                    if !partial.entitled {
                        return Err(unsupported("workout ex-coupon ownership", "reserved coupon ownership needs an explicit early-redemption agreement"));
                    }
                    partial.accrual_end = exercise;
                    partial.payment_date = dates.settlement;
                    partial.amount = accrued_cash;
                    // Retain only the original observations/reset intervals actually
                    // used by this prefix, without regenerating a shorter lockout.
                    if let (Some(schedule), Some(compound)) =
                        (&plan.overnight, &original_coupon.overnight)
                    {
                        let cutoff = weight_end.unwrap_or(exercise);
                        let prefix = schedule.prefix(exercise, cutoff)?;
                        let dc = self.terms.day_count.to_day_count();
                        let factor = compound.factor_through(cutoff, self.terms.day_count)?;
                        let fraction = dc.year_fraction(start, exercise);
                        let annualized = factor
                            .checked_sub(Decimal::ONE)
                            .and_then(|v| v.checked_div(fraction))
                            .ok_or(CashflowError::Arithmetic("workout prefix rate"))?;
                        let observations = prefix
                            .observations()
                            .iter()
                            .enumerate()
                            .map(|(i, dates)| {
                                let mut o =
                                    compound.observations.get(i).cloned().ok_or_else(|| {
                                        invalid(
                                            "workout.prefix",
                                            "original resolved observation absent",
                                        )
                                    })?;
                                if o.dates.observation_date != dates.observation_date
                                    || o.dates.weight_start != dates.weight_start
                                    || o.dates.weight_end < dates.weight_end
                                {
                                    return Err(invalid(
                                        "workout.prefix",
                                        "original resolved observation identity differs",
                                    ));
                                }
                                o.dates = dates.clone();
                                o.year_fraction =
                                    dc.year_fraction(dates.weight_start, dates.weight_end);
                                Ok(o)
                            })
                            .collect::<Result<Vec<_>, BondContractError>>()?;
                        partial.overnight = Some(convex_core::cashflows::CompoundedCoupon {
                            period_factor: factor,
                            period_year_fraction: fraction,
                            annualized_rate: annualized,
                            observations,
                        });
                    } else {
                        let consumed: BTreeSet<_> = plan
                            .resets
                            .iter()
                            .filter(|reset| reset.start < exercise)
                            .map(|reset| {
                                (
                                    reset.request.index_id.clone(),
                                    reset.request.observation_date,
                                )
                            })
                            .collect();
                        partial.rates.retain(|r| {
                            consumed.contains(&(
                                r.requirement.index_id.clone(),
                                r.requirement.observation_date,
                            ))
                        });
                    }
                    cash.coupons.push(partial);
                }
            }
        }
        let uses_cutoff = terms.accrued == ExerciseAccrued::PayAccrued
            && matches!(&self.terms.coupon, BondCoupon::Overnight(r) if r.convention.observation_shift)
            && self.coupons.iter().any(|p| {
                let (a, b) = p.period.accrual_dates();
                a < exercise && exercise < b
            });
        if weight_end.is_some() && !uses_cutoff {
            return Err(invalid(
                "workout.weight_end",
                "unused partial observation-weight cutoff",
            ));
        }
        let total = self.terms.rounding.apply(
            redemption
                .checked_add(accrued_cash)
                .ok_or(CashflowError::Arithmetic("workout settlement cash"))?,
        )?;
        // Preserve the lifecycle's once-rounded total. Any final rounding
        // allocation stays in redemption; the original partial coupon is intact.
        cash.principal.push(PrincipalCashflow {
            effective_date: exercise,
            payment_date: dates.settlement,
            principal: PrincipalChange {
                before: outstanding,
                after: Decimal::ZERO,
                repayment: outstanding,
            },
            amount: total
                .checked_sub(accrued_cash)
                .ok_or(CashflowError::Arithmetic("workout principal allocation"))?,
            is_redemption: true,
        });
        cash.conditional_on_no_exercise = false;
        Ok(WorkoutProjection {
            original,
            cash,
            dates,
            outstanding_principal: outstanding,
            redemption,
            accrued_cash,
            settlement_cash: total,
        })
    }
}
