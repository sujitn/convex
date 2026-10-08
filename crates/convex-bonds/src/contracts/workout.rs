//! Hypothetical fixed-price exercise cash over original checked bond economics.
use super::{
    invalid, unsupported, BondContract, BondContractError, BondProjection, CashflowError, Date,
    Decimal, ExerciseAccrued, ExerciseDates, PrincipalCashflow,
};
use crate::types::CalendarId;
use convex_core::{
    calendars::Calendar,
    cashflows::{BoundarySide, PaymentDateEntitlement, PrincipalChange},
};
use std::collections::BTreeMap;

/// Full cash composition for one permitted early exercise. No exercise event,
/// option value, realized receipt or authority is inferred.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FixedWorkoutProjection {
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
    ) -> Result<FixedWorkoutProjection, BondContractError> {
        if calendar_id != &self.terms.calendars.exercise {
            return Err(invalid(
                "workout.calendar",
                "exact original exercise calendar required",
            ));
        }
        let original = self.fixed_projection(settlement, on_payment)?;
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
        let dates = terms.dates(exercise, calendar)?;
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
                accrued_cash = self.terms.rounding.apply(self.term_amount(
                    plan,
                    exercise,
                    &BTreeMap::new(),
                )?)?;
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
                    cash.coupons.push(partial);
                }
            }
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
        Ok(FixedWorkoutProjection {
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
