//! Explicit call/put notice, exercise and cash settlement terms.

use super::{invalid, BondContractError};
use convex_core::{
    calendars::{BusinessDayConvention, Calendar},
    cashflows::{DateOffset, MAX_SCHEDULE_PERIODS},
    Date,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Party holding the redemption right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExerciseRight {
    /// Issuer call.
    Call,
    /// Holder put.
    Put,
}

/// Explicit supported dated exercise styles. Triggered and make-whole
/// contracts require terms/models outside this initial fixed-price schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "style", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExerciseStyle {
    /// One exact date.
    European {
        /// Contractual exercise date.
        date: Date,
    },
    /// Strictly increasing discrete dates.
    Bermudan {
        /// Contractual exercise dates.
        dates: Vec<Date>,
    },
    /// Inclusive continuous window; not collapsed to coupon dates.
    American {
        /// Inclusive first exercise date.
        start: Date,
        /// Inclusive last exercise date.
        end: Date,
    },
}

/// Exercise price convention, independent from accrued cash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExerciseAccrued {
    /// Redemption plus separately accrued coupon.
    PayAccrued,
    /// Redemption includes accrued; do not add a coupon.
    IncludedInPrice,
}

/// Complete fixed-price call/put terms, usable for callable FRNs as well as
/// fixed bonds. Preserving terms is distinct from having an option model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ExerciseTerms {
    /// Issuer call or holder put.
    pub right: ExerciseRight,
    /// Discrete dates or inclusive continuous window.
    pub style: ExerciseStyle,
    /// Per 100 outstanding principal, Decimal contract price.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    pub price_per_100: Decimal,
    /// Nonpositive notice offset from contractual exercise date.
    pub notice_offset: DateOffset,
    /// Nonnegative cash settlement lag after exercise.
    pub settlement_lag: DateOffset,
    /// Independent cash settlement business-day rule.
    pub settlement_adjustment: BusinessDayConvention,
    /// Treatment of separately accrued coupon at redemption.
    pub accrued: ExerciseAccrued,
    /// Contractual protection end, retained independently from exercise windows.
    pub protection_end: Option<Date>,
}

/// Dated consequences of a permitted supplied exercise; it does not assert an
/// exercise occurred, choose exercise optimally or book cash.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExerciseDates {
    /// Last required notice date.
    pub notice: Date,
    /// Exact contractual exercise date.
    pub exercise: Date,
    /// Adjusted cash redemption date.
    pub settlement: Date,
}

impl ExerciseStyle {
    pub(super) fn bounds(&self) -> Result<(Date, Date), BondContractError> {
        match self {
            Self::European { date } => Ok((*date, *date)),
            Self::Bermudan { dates } => {
                if dates.is_empty()
                    || dates.len() > MAX_SCHEDULE_PERIODS
                    || dates.windows(2).any(|p| p[0] >= p[1])
                {
                    return Err(invalid(
                        "exercise.dates",
                        "requires bounded sorted unique dates",
                    ));
                }
                let start = *dates.first().ok_or_else(|| invalid("exercise", "empty"))?;
                let end = *dates.last().ok_or_else(|| invalid("exercise", "empty"))?;
                Ok((start, end))
            }
            Self::American { start, end } if end >= start => Ok((*start, *end)),
            Self::American { .. } => Err(invalid("exercise.window", "end precedes start")),
        }
    }
    /// Whether an exact supplied exercise date is contracted.
    pub fn permits(&self, date: Date) -> bool {
        match self {
            Self::European { date: exercise } => date == *exercise,
            Self::Bermudan { dates } => dates.binary_search(&date).is_ok(),
            Self::American { start, end } => *start <= date && date <= *end,
        }
    }
}

impl ExerciseTerms {
    pub(super) fn validate(
        &self,
        issue: Date,
        maturity: Date,
        calendar: &dyn Calendar,
    ) -> Result<(), BondContractError> {
        let (start, end) = self.style.bounds()?;
        if start <= issue
            || end > maturity
            || self.price_per_100 <= Decimal::ZERO
            || self.price_per_100 > Decimal::from(1000)
        {
            return Err(invalid("exercise", "dates/price outside contract bounds"));
        }
        let offset = |n| match n {
            DateOffset::CalendarDays(n) | DateOffset::BusinessDays(n) => n,
        };
        if offset(self.notice_offset) > 0 || offset(self.settlement_lag) < 0 {
            return Err(invalid(
                "exercise.offset",
                "notice must precede exercise; settlement follows it",
            ));
        }
        if self
            .protection_end
            .is_some_and(|date| date < issue || date > maturity)
        {
            return Err(invalid("exercise.protection", "outside contract life"));
        }
        for date in [start, end] {
            let notice = self.notice_offset.apply(date, calendar)?;
            let settlement = calendar
                .adjust(
                    self.settlement_lag.apply(date, calendar)?,
                    self.settlement_adjustment,
                )
                .map_err(|e| invalid("exercise.settlement", e.to_string()))?;
            if notice < issue || settlement < date {
                return Err(invalid(
                    "exercise",
                    "notice precedes issue or settlement precedes exercise",
                ));
            }
        }
        Ok(())
    }

    /// Preserve separate notice/exercise/settlement dates under actual calendar.
    pub fn dates(
        &self,
        date: Date,
        calendar: &dyn Calendar,
    ) -> Result<ExerciseDates, BondContractError> {
        if !self.style.permits(date) || self.protection_end.is_some_and(|end| date < end) {
            return Err(invalid(
                "exercise.date",
                "outside contracted exercise or within protection",
            ));
        }
        Ok(ExerciseDates {
            notice: self.notice_offset.apply(date, calendar)?,
            exercise: date,
            settlement: calendar
                .adjust(
                    self.settlement_lag.apply(date, calendar)?,
                    self.settlement_adjustment,
                )
                .map_err(|e| invalid("exercise.settlement", e.to_string()))?,
        })
    }
}

/// Convert supplied existing call terms without ignoring unsupported provisions.
pub(crate) fn call_terms(
    schedule: &crate::types::CallSchedule,
    maturity: Date,
) -> Result<Vec<ExerciseTerms>, BondContractError> {
    use crate::types::CallType;
    if schedule.entries.is_empty()
        || schedule.entries.len() > MAX_SCHEDULE_PERIODS
        || schedule
            .entries
            .windows(2)
            .any(|p| p[0].start_date >= p[1].start_date)
    {
        return Err(invalid(
            "call_schedule",
            "requires bounded sorted unique entries",
        ));
    }
    if schedule.make_whole_spread.is_some()
        || matches!(
            schedule.call_type,
            CallType::MakeWhole | CallType::Mandatory
        )
    {
        return Err(super::unsupported(
            "call provision",
            "make-whole/triggered call needs complete separate terms/model",
        ));
    }
    schedule
        .entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let end = if let Some(end) = entry.end_date {
                end
            } else if let Some(next) = schedule.entries.get(i + 1) {
                next.start_date
                    .checked_add_days(-1)
                    .map_err(|e| invalid("call.end_date", e.to_string()))?
            } else {
                maturity
            };
            let style = match schedule.call_type {
                CallType::American | CallType::ParCall => ExerciseStyle::American {
                    start: entry.start_date,
                    end,
                },
                CallType::European => {
                    if entry.end_date.is_some_and(|end| end != entry.start_date) {
                        return Err(invalid(
                            "call.end_date",
                            "discrete call cannot carry an exercise window",
                        ));
                    }
                    ExerciseStyle::European {
                        date: entry.start_date,
                    }
                }
                CallType::Bermudan => {
                    if entry.end_date.is_some_and(|end| end != entry.start_date) {
                        return Err(invalid(
                            "call.end_date",
                            "discrete call cannot carry an exercise window",
                        ));
                    }
                    ExerciseStyle::Bermudan {
                        dates: vec![entry.start_date],
                    }
                }
                _ => return Err(super::unsupported("call", "unsupported call provision")),
            };
            let price = Decimal::try_from(entry.call_price)
                .map_err(|_| invalid("call.price", "must be finite Decimal"))?;
            let notice = entry.notice_days.unwrap_or(0);
            if notice > 366 {
                return Err(invalid("call.notice", "exceeds 366 days"));
            }
            Ok(ExerciseTerms {
                right: ExerciseRight::Call,
                style,
                price_per_100: price,
                notice_offset: DateOffset::CalendarDays(-(notice as i32)),
                settlement_lag: DateOffset::CalendarDays(0),
                settlement_adjustment: BusinessDayConvention::Unadjusted,
                accrued: ExerciseAccrued::PayAccrued,
                protection_end: schedule.protection_end,
            })
        })
        .collect()
}

/// Convert put terms with the same strict dated-price semantics.
pub(crate) fn put_terms(
    schedule: &crate::types::PutSchedule,
    maturity: Date,
) -> Result<Vec<ExerciseTerms>, BondContractError> {
    use crate::types::{CallEntry, CallSchedule, CallType, PutType};
    let call_type = match schedule.put_type {
        PutType::American => CallType::American,
        PutType::European => CallType::European,
        PutType::Bermudan => CallType::Bermudan,
        _ => {
            return Err(super::unsupported(
                "put trigger",
                "change-of-control/death terms require explicit trigger state",
            ))
        }
    };
    let calls = CallSchedule {
        call_type,
        protection_end: None,
        make_whole_spread: None,
        entries: schedule
            .entries
            .iter()
            .map(|entry| CallEntry {
                start_date: entry.start_date,
                end_date: entry.end_date,
                call_price: entry.put_price,
                notice_days: entry.notice_days,
            })
            .collect(),
    };
    let mut terms = call_terms(&calls, maturity)?;
    for term in &mut terms {
        term.right = ExerciseRight::Put;
    }
    Ok(terms)
}
