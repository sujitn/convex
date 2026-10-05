//! Checked decimal coupon, accrual and expected principal effects.

use super::{
    BoundarySide, CashflowError, CouponPeriod, DateRules, DatedProfile, PrincipalChange,
    ProfileKind,
};
use crate::{calendars::Calendar, daycounts::DayCountConvention, fixings::validate_rate, Date};
use rust_decimal::{Decimal, RoundingStrategy};
use std::collections::BTreeSet;

/// Contractual cash rounding, applied once after summing coupon segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CashRounding {
    /// Preserve all available Decimal precision (maximum scale 28).
    Unrounded,
    /// Round ties to the nearest even digit at the supplied scale.
    NearestEven(u32),
    /// Round ties away from zero at the supplied scale.
    MidpointAwayFromZero(u32),
    /// Truncate toward zero at the supplied scale.
    TowardZero(u32),
}

impl CashRounding {
    /// Round a checked Decimal value; reject an unsupported scale.
    pub fn apply(self, amount: Decimal) -> Result<Decimal, CashflowError> {
        let (scale, strategy) = match self {
            Self::Unrounded => return Ok(amount),
            Self::NearestEven(n) => (n, RoundingStrategy::MidpointNearestEven),
            Self::MidpointAwayFromZero(n) => (n, RoundingStrategy::MidpointAwayFromZero),
            Self::TowardZero(n) => (n, RoundingStrategy::ToZero),
        };
        if scale > 28 {
            return Err(CashflowError::terms("rounding", "Decimal scale exceeds 28"));
        }
        Ok(amount.round_dp_with_strategy(scale, strategy))
    }
}

/// Principal convention for the amount due at a contractual coupon boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CouponPrincipal {
    /// Accrue each segment on its independently dated outstanding principal.
    AccrualProfile,
    /// Use principal immediately before the contractual coupon-end boundary.
    BeforeCouponBoundary,
    /// Use principal after that boundary (zero after final redemption).
    AfterCouponBoundary,
}

/// Negative coupon handling is a contract choice; there is no implicit floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NegativeCouponPolicy {
    /// Preserve signed negative amounts.
    Allow,
    /// Reject a contract whose effective rate is negative in any segment.
    Reject,
}

/// Explicit coupon inputs in absolute principal and annualized decimal rates.
/// An index rate must already have been resolved under the fixing policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CouponCalculation {
    /// Resolved term index rate; zero only for an explicitly fixed coupon.
    pub index_rate: Decimal,
    /// Contractual day-count convention.
    pub day_count: DayCountConvention,
    /// Principal convention at coupon/notional boundaries.
    pub principal: CouponPrincipal,
    /// Final cash rounding policy.
    pub rounding: CashRounding,
    /// Explicit contract treatment of negative effective rates.
    pub negative_coupons: NegativeCouponPolicy,
}

/// Ex-coupon accrued convention, independent of cash entitlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExCouponAccrual {
    /// Accrued-to-settlement minus the full unrounded coupon, then round once.
    SubtractCoupon,
    /// Contract specifies zero accrued in the ex-coupon interval.
    Zero,
}

/// Four independently dated profiles. Matching coverage is required; their
/// internal step dates may differ. Effective rate = coupon + gearing*index + spread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouponProfiles {
    coupon: DatedProfile,
    spread: DatedProfile,
    gearing: DatedProfile,
    notional: DatedProfile,
}

impl CouponProfiles {
    /// Validate roles and full equal coverage without truncating any profile.
    pub fn new(
        coupon: DatedProfile,
        spread: DatedProfile,
        gearing: DatedProfile,
        notional: DatedProfile,
    ) -> Result<Self, CashflowError> {
        for (profile, kind) in [
            (&coupon, ProfileKind::Coupon),
            (&spread, ProfileKind::Spread),
            (&gearing, ProfileKind::Gearing),
            (&notional, ProfileKind::Notional),
        ] {
            if profile.kind() != kind || profile.coverage() != coupon.coverage() {
                return Err(CashflowError::terms("profiles", "roles or coverage differ"));
            }
        }
        Ok(Self {
            coupon,
            spread,
            gearing,
            notional,
        })
    }
    /// Independently validated outstanding-principal schedule.
    pub fn notional(&self) -> &DatedProfile {
        &self.notional
    }
    /// Expected full coupon amount. Payment dates do not select profile values.
    pub fn coupon_amount(
        &self,
        period: &CouponPeriod,
        calculation: CouponCalculation,
    ) -> Result<Decimal, CashflowError> {
        calculation.rounding.apply(self.accumulated(
            period,
            period.accrual_dates().1,
            calculation,
        )?)
    }
    /// Accrued for this still-unpaid period. Before accrual and on/after payment
    /// it is zero. During payment delay it is the fully accrued coupon; a leg
    /// caller sums applicable unpaid periods. This does not assert cash was paid.
    pub fn accrued(
        &self,
        period: &CouponPeriod,
        settlement: Date,
        calculation: CouponCalculation,
        ex_accrual: ExCouponAccrual,
    ) -> Result<Decimal, CashflowError> {
        super::validate_date(settlement)?;
        // Validate full economic inputs even when the requested accrued amount is zero.
        let full = self.accumulated(period, period.accrual_dates().1, calculation)?;
        if settlement <= period.accrual_dates().0 || settlement >= period.payment_date() {
            return Ok(Decimal::ZERO);
        }
        let mut amount = self.accumulated(
            period,
            settlement.min(period.accrual_dates().1),
            calculation,
        )?;
        if period.is_ex_coupon(settlement) {
            amount = match ex_accrual {
                ExCouponAccrual::SubtractCoupon => amount
                    .checked_sub(full)
                    .ok_or(CashflowError::Arithmetic("ex-coupon subtraction"))?,
                ExCouponAccrual::Zero => Decimal::ZERO,
            };
        }
        calculation.rounding.apply(amount)
    }

    fn accumulated(
        &self,
        period: &CouponPeriod,
        end: Date,
        calculation: CouponCalculation,
    ) -> Result<Decimal, CashflowError> {
        validate_rate(calculation.index_rate)?;
        calculation.rounding.apply(Decimal::ZERO)?;
        let (start, accrual_end) = period.accrual_dates();
        let coverage = self.coupon.coverage();
        if start < coverage.0 || accrual_end > coverage.1 || end < start || end > accrual_end {
            return Err(CashflowError::terms(
                "profiles",
                "coupon accrual not fully covered",
            ));
        }
        let boundary_notional = match calculation.principal {
            CouponPrincipal::AccrualProfile => None,
            CouponPrincipal::BeforeCouponBoundary => Some(
                self.notional
                    .principal_change(period.contractual_dates().1)?
                    .before,
            ),
            CouponPrincipal::AfterCouponBoundary => Some(
                self.notional
                    .principal_change(period.contractual_dates().1)?
                    .after,
            ),
        };
        let mut cuts = BTreeSet::from([start, end]);
        for profile in [&self.coupon, &self.spread, &self.gearing, &self.notional] {
            for entry in profile.entries() {
                if entry.start > start && entry.start < end {
                    cuts.insert(entry.start);
                }
            }
        }
        let cuts: Vec<_> = cuts.into_iter().collect();
        let mut total = Decimal::ZERO;
        for pair in cuts.windows(2) {
            let coupon = self.coupon.value_at(pair[0], BoundarySide::After)?;
            let spread = self.spread.value_at(pair[0], BoundarySide::After)?;
            let gearing = self.gearing.value_at(pair[0], BoundarySide::After)?;
            let principal = match boundary_notional {
                Some(value) => value,
                None => self.notional.value_at(pair[0], BoundarySide::After)?,
            };
            let rate = gearing
                .checked_mul(calculation.index_rate)
                .and_then(|x| x.checked_add(coupon))
                .and_then(|x| x.checked_add(spread))
                .ok_or(CashflowError::Arithmetic("effective coupon rate"))?;
            if calculation.negative_coupons == NegativeCouponPolicy::Reject && rate < Decimal::ZERO
            {
                return Err(CashflowError::terms(
                    "coupon",
                    "negative effective rate is unsupported by this contract",
                ));
            }
            let fraction = period.year_fraction(pair[0], pair[1], calculation.day_count)?;
            let amount = principal
                .checked_mul(rate)
                .and_then(|x| x.checked_mul(fraction))
                .ok_or(CashflowError::Arithmetic("coupon amount"))?;
            total = total
                .checked_add(amount)
                .ok_or(CashflowError::Arithmetic("coupon sum"))?;
        }
        Ok(total)
    }
}

/// Expected principal cash, with economic and payment dates kept distinct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrincipalCashflow {
    /// Effective notional change date.
    pub effective_date: Date,
    /// Delayed/adjusted payment date.
    pub payment_date: Date,
    /// Explicit principal before/after the change.
    pub principal: PrincipalChange,
    /// Signed absolute cash, rounded under the supplied policy.
    pub amount: Decimal,
    /// True for the final residual principal redemption.
    pub is_redemption: bool,
}

impl PrincipalCashflow {
    /// Cash entitlement is separate from coupon ex dates and realized payment state.
    pub fn is_entitled(&self, settlement: Date, on_payment: super::PaymentDateEntitlement) -> bool {
        settlement < self.payment_date
            || (settlement == self.payment_date
                && on_payment == super::PaymentDateEntitlement::Include)
    }
}

/// Project all independently dated principal changes plus final redemption.
/// Internal increases create negative cash advances. The redemption multiplier
/// applies only to final residual principal; zero is permitted if contractual.
pub fn principal_cashflows(
    notional: &DatedProfile,
    rules: &DateRules,
    calendar: &dyn Calendar,
    redemption_multiplier: Decimal,
    rounding: CashRounding,
) -> Result<Vec<PrincipalCashflow>, CashflowError> {
    if notional.kind() != ProfileKind::Notional
        || redemption_multiplier < Decimal::ZERO
        || redemption_multiplier > Decimal::from(10)
    {
        return Err(CashflowError::terms(
            "redemption",
            "requires Notional profile and multiplier in 0..10",
        ));
    }
    rounding.apply(Decimal::ZERO)?;
    let mut cashflows = Vec::with_capacity(notional.entries().len());
    for entry in notional.entries() {
        let date = entry.end;
        let principal = notional.principal_change(date)?;
        let is_redemption = date == notional.coverage().1;
        let amount = if is_redemption {
            principal
                .repayment
                .checked_mul(redemption_multiplier)
                .ok_or(CashflowError::Arithmetic("redemption"))?
        } else {
            principal.repayment
        };
        if amount != Decimal::ZERO {
            cashflows.push(PrincipalCashflow {
                effective_date: date,
                payment_date: rules.payment_date(date, calendar)?,
                principal,
                amount: rounding.apply(amount)?,
                is_redemption,
            });
        }
    }
    Ok(cashflows)
}
