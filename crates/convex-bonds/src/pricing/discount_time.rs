//! Checked settlement-to-payment time over explicitly supplied economic context.
//! These inputs are already entitled cash. Calendar adjustment, schedules and
//! ownership belong to the original contract and are never inferred here.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::yield_cash::{self, CashYieldCompounding};
use crate::{traits::BondCashFlow, BondError, BondResult};
use convex_core::{
    cashflows::{
        act365l_year_fraction, icma_year_fraction, validate_icma_reference_periods,
        ReferencePeriod, ScheduleSpec, MAX_CONTRACT_DAYS, MAX_SCHEDULE_PERIODS,
    },
    contracts::ExactDecimal,
    daycounts::{DayCountConvention, Thirty360EIsda},
    types::{Date, Frequency},
};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde::{Deserialize, Serialize};

/// One actual entitled payment in unchanged original cash units.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DatedYieldCash {
    /// Actual payment date; no business-day adjustment is performed.
    pub date: Date,
    /// Original signed cash, without face or coupon-frequency substitution.
    pub amount: ExactDecimal,
}

/// Numerical cash prepared from actual sorted, strictly future payment dates.
/// Zero discount time never changes the original payment or entitlement state.
/// Construction requires the full original discount context. This value cannot
/// be deserialized or constructed from arbitrary supplied numerical times.
#[derive(Debug, Clone)]
pub struct PreparedDatedCash {
    cash: Vec<(f64, f64)>,
}

impl PreparedDatedCash {
    /// Build from actual dates and the complete supplied discount context.
    pub fn new(
        cash: &[DatedYieldCash],
        settlement: Date,
        basis: &DiscountTimeBasis,
    ) -> BondResult<Self> {
        prepare_dated_cash(cash, settlement, basis).map(Self::from_validated_dates)
    }

    pub(crate) fn from_validated_dates(cash: Vec<(f64, f64)>) -> Self {
        Self { cash }
    }

    /// Read-only original cash order and settlement-to-payment discount years.
    #[must_use]
    pub fn discount_cash(&self) -> &[(f64, f64)] {
        &self.cash
    }

    /// Price the same original cash, including strictly future zero-time cash.
    pub fn price(&self, annual_yield: f64, compounding: CashYieldCompounding) -> BondResult<f64> {
        yield_cash::value_dated_cash(&self.cash, annual_yield, compounding).map(|value| value.0)
    }
}

/// Complete discount-time input, separate from coupon accrual and compounding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiscountTimeBasis {
    /// A genuinely date-only convention, evaluated from settlement to payment.
    Calendar {
        /// ICMA, ACT365L and 30E/360 ISDA are rejected in this variant.
        day_count: DayCountConvention,
    },
    /// Actual anchored nominal mesh, including explicit delayed-payment tails.
    Icma {
        /// Original schedule anchor, roll, stubs, dates and coupon frequency.
        schedule: ScheduleSpec,
        /// Original contiguous nominal periods; never inferred from cash order.
        reference_periods: Vec<ReferencePeriod>,
    },
    /// ACT365L split over actual coupon periods, independent of cash omissions.
    Act365L {
        /// Actual coupon determination frequency, separate from compounding.
        frequency: Frequency,
        /// Complete actual coupon periods covering settlement through payments.
        coupon_periods: Vec<ReferencePeriod>,
    },
    /// Actual contractual termination for the February maturity exception.
    Thirty360EIsda {
        /// Original termination, never guessed from the last payment date.
        termination_date: Date,
    },
}

fn invalid(message: &str) -> BondError {
    BondError::invalid_spec(message)
}
fn number(value: Decimal) -> BondResult<f64> {
    value
        .to_f64()
        .filter(|n| n.is_finite() && (value.is_zero() || *n != 0.0))
        .ok_or_else(|| {
            BondError::pricing_failed("discount time/cash conversion overflow or underflow")
        })
}
fn interval(start: Date, end: Date, allow_equal: bool) -> BondResult<()> {
    if !(1..=9999).contains(&start.year())
        || !(1..=9999).contains(&end.year())
        || end < start
        || (!allow_equal && end == start)
        || start.days_between(&end) > MAX_CONTRACT_DAYS
    {
        return Err(invalid("invalid or excessive discount-time interval"));
    }
    Ok(())
}

impl DiscountTimeBasis {
    /// Exact discount role consumed by this context.
    pub fn day_count(&self) -> DayCountConvention {
        match self {
            Self::Calendar { day_count } => *day_count,
            Self::Icma { .. } => DayCountConvention::ActActIcma,
            Self::Act365L { .. } => DayCountConvention::Act365Leap,
            Self::Thirty360EIsda { .. } => DayCountConvention::Thirty360EIsda,
        }
    }

    fn validate(&self) -> BondResult<()> {
        match self {
            Self::Calendar {
                day_count:
                    DayCountConvention::ActActIcma
                    | DayCountConvention::Act365Leap
                    | DayCountConvention::Thirty360EIsda,
            } => Err(invalid(
                "discount day count requires explicit reference/coupon/termination context",
            )),
            Self::Calendar { .. } => Ok(()),
            Self::Icma {
                schedule,
                reference_periods,
            } => {
                interval(schedule.start_date, schedule.end_date, false)?;
                validate_icma_reference_periods(schedule, reference_periods)
                    .map_err(|e| invalid(&e.to_string()))?;
                if reference_periods
                    .first()
                    .is_none_or(|p| p.start > schedule.start_date)
                    || reference_periods
                        .last()
                        .is_none_or(|p| p.end < schedule.end_date)
                {
                    return Err(invalid("ICMA mesh must cover the original schedule"));
                }
                Ok(())
            }
            Self::Act365L {
                frequency,
                coupon_periods,
            } => {
                if frequency.is_zero()
                    || coupon_periods.is_empty()
                    || coupon_periods.len() > MAX_SCHEDULE_PERIODS
                {
                    return Err(invalid(
                        "positive ACT365L frequency and bounded actual coupon mesh required",
                    ));
                }
                for (i, p) in coupon_periods.iter().enumerate() {
                    interval(p.start, p.end, false)?;
                    if i > 0 && coupon_periods[i - 1].end != p.start {
                        return Err(invalid("ordered contiguous actual coupon periods required"));
                    }
                }
                interval(
                    coupon_periods[0].start,
                    coupon_periods[coupon_periods.len() - 1].end,
                    false,
                )
            }
            Self::Thirty360EIsda { termination_date } => {
                interval(*termination_date, *termination_date, true)
            }
        }
    }

    fn fraction(&self, start: Date, end: Date) -> BondResult<Decimal> {
        match self {
            Self::Calendar { day_count } => Ok(day_count.to_day_count().year_fraction(start, end)),
            Self::Icma {
                schedule,
                reference_periods,
            } => icma_year_fraction(start, end, schedule.frequency, reference_periods)
                .map_err(|e| invalid(&e.to_string())),
            Self::Thirty360EIsda { termination_date } => {
                Thirty360EIsda::new(Some(*termination_date))
                    .checked_year_fraction(start, end)
                    .map_err(|e| invalid(&e.to_string()))
            }
            Self::Act365L {
                frequency,
                coupon_periods,
            } => {
                let mut total = Decimal::ZERO;
                let mut covered = 0_i64;
                for p in coupon_periods {
                    let a = start.max(p.start);
                    let b = end.min(p.end);
                    if b > a {
                        let value = act365l_year_fraction(a, b, p.start, p.end, *frequency)
                            .map_err(|e| invalid(&e.to_string()))?;
                        total = total.checked_add(value).ok_or_else(|| {
                            BondError::pricing_failed("ACT365L time sum overflow")
                        })?;
                        covered += a.days_between(&b);
                    }
                }
                if covered != start.days_between(&end) {
                    return Err(invalid("incomplete actual ACT365L coupon coverage"));
                }
                Ok(total)
            }
        }
    }
}

/// Prepare 1..4096 sorted, strictly future original cash entries exactly once.
/// The mesh may contain periods with no cash, and multiple amounts may share a
/// payment date. Full ICMA/ACT365L coverage is mandatory, including payment tails.
/// A strictly future payment may have zero discount years under 30/360.
/// Use PreparedDatedCash for pricing; raw supplied-time pricing remains positive-only.
/// Signed cash is admitted for pricing; inverse owners enforce root selection.
pub fn prepare_dated_cash(
    cash: &[DatedYieldCash],
    settlement: Date,
    basis: &DiscountTimeBasis,
) -> BondResult<Vec<(f64, f64)>> {
    interval(settlement, settlement, true)?;
    if cash.is_empty() || cash.len() > MAX_SCHEDULE_PERIODS {
        return Err(invalid("1..4096 dated cash entries required"));
    }
    basis.validate()?;
    let mut previous = settlement;
    cash.iter()
        .map(|c| {
            interval(settlement, c.date, false)?;
            if c.date < previous {
                return Err(invalid("dated cash must retain sorted payment order"));
            }
            previous = c.date;
            let time = number(basis.fraction(settlement, c.date)?)?;
            if time < 0.0 {
                return Err(invalid("negative discount years are not supported"));
            }
            Ok((time, number(c.amount.decimal())?))
        })
        .collect()
}

pub(crate) fn prepare_bond_cash(
    cash: &[BondCashFlow],
    settlement: Date,
    day_count: DayCountConvention,
    basis: Option<&DiscountTimeBasis>,
) -> BondResult<Vec<(f64, f64)>> {
    if cash.len() > MAX_SCHEDULE_PERIODS {
        return Err(invalid("cash-flow capacity exceeded"));
    }
    let fallback = DiscountTimeBasis::Calendar { day_count };
    let basis = basis.unwrap_or(&fallback);
    if basis.day_count() != day_count {
        return Err(invalid(
            "explicit context does not match the supplied discount day count",
        ));
    }
    let original: Vec<_> = cash.iter().map(DatedYieldCash::from).collect();
    prepare_dated_cash(&original, settlement, basis)
}

impl From<&BondCashFlow> for DatedYieldCash {
    fn from(cash: &BondCashFlow) -> Self {
        Self {
            date: cash.date,
            amount: ExactDecimal::new(cash.amount),
        }
    }
}
